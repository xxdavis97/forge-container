use std::fs;
use std::process::Command;

fn copy_file(src: &str, dst: &str) {
    if let Some(parent) = std::path::Path::new(dst).parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Err(e) = fs::copy(src, dst) {
        eprintln!("Warning: Failed to copy {} to {}: {}", src, dst, e);
    } else {
        println!("Copied {}", src);
    }
}

fn copy_shared_libraries(root: &str, binary: &str) {
    println!("Copying shared libraries for {}...", binary);
    
    let output = Command::new("ldd")
        .arg(binary)
        .output()
        .expect("Failed to run ldd");
    
    let output_str = String::from_utf8_lossy(&output.stdout);
    
    for line in output_str.lines() {
        if line.contains("=>") {
            // Lines like: "libc.so.6 => /lib/x86_64-linux-gnu/libc.so.6 (0x...)"
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                let lib_path = parts[2];
                if lib_path.starts_with('/') {
                    let dst = format!("{}{}", root, lib_path);
                    copy_file(lib_path, &dst);
                }
            }
        } else if line.trim().starts_with('/') {
            // Handle dynamic linker like "/lib64/ld-linux-x86-64.so.2 (0x...)"
            if let Some(lib_path) = line.trim().split_whitespace().next() {
                let dst = format!("{}{}", root, lib_path);
                copy_file(lib_path, &dst);
            }
        }
    }
}

fn copy_directory_recursive(src: &str, dst: &str) {
    if let Err(e) = Command::new("cp")
        .args(&["-r", src, dst])
        .output()
    {
        eprintln!("Warning: Failed to copy directory {} to {}: {}", src, dst, e);
    } else {
        println!("Copied directory {}", src);
    }
}

fn copy_terminfo(root: &str) {
    println!("Copying terminfo database...");
    
    // Create usr/share directory
    let share_dir = format!("{}/usr/share", root);
    fs::create_dir_all(&share_dir).ok();
    
    // Copy terminfo database
    copy_directory_recursive("/usr/share/terminfo", &format!("{}/usr/share/terminfo", root));
    
    // Also copy /lib/terminfo if it exists (some systems use this)
    if std::path::Path::new("/lib/terminfo").exists() {
        let lib_dir = format!("{}/lib", root);
        fs::create_dir_all(&lib_dir).ok();
        copy_directory_recursive("/lib/terminfo", &format!("{}/lib/terminfo", root));
    }
}

pub fn copy_bash_and_dependencies(root: &str) {
    println!("Copying bash and dependencies...");
    
    // Copy bash binary
    copy_file("/bin/bash", &format!("{}/bin/bash", root));

    let binaries = vec![
        // Shell and basics
        "/bin/bash",
        "/bin/sh",
        
        // File operations
        "/bin/ls",
        "/bin/cat",
        "/bin/touch",
        "/bin/cp",
        "/bin/mv",
        "/bin/rm",
        "/bin/mkdir",
        "/bin/rmdir",
        
        // Text editors
        "/bin/nano",
        "/usr/bin/vi", 
        
        // System utilities
        "/bin/ps",
        "/bin/pwd",
        "/usr/bin/top",  
        "/bin/kill",
        "/usr/bin/dd",
        
        // Text processing
        "/bin/grep",
        "/usr/bin/find",
        "/usr/bin/wc",
        "/usr/bin/head",
        "/usr/bin/tail",
        
        // Network
        "/bin/ip",
        "/sbin/ip",
        "/sbin/iptables",
        "/bin/ping",
        "/usr/bin/curl",
    ];
    
    // Copy each binary
    for binary in &binaries {
        let filename = std::path::Path::new(binary)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap();
        let dst = format!("{}/bin/{}", root, filename);
        copy_file(binary, &dst);
        
        // Copy libraries for this binary
        copy_shared_libraries(root, binary);
        
    }
    // Copy terminfo for nano and other terminal programs
    copy_terminfo(root);
    
    println!("Bash and dependencies copied");
}
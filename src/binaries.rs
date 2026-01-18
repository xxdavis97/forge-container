use std::fs;
use std::process::Command;
use std::collections::HashSet;
use log::debug;

fn copy_file(src: &str, dst: &str) -> bool {
    if let Some(parent) = std::path::Path::new(dst).parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Err(_) = fs::copy(src, dst) {
        false
    } else {
        true
    }
}

fn collect_shared_libraries(binary: &str, libs: &mut HashSet<String>) {
    let output = match Command::new("ldd").arg(binary).output() {
        Ok(o) => o,
        Err(_) => return,
    };

    let output_str = String::from_utf8_lossy(&output.stdout);

    for line in output_str.lines() {
        if line.contains("=>") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                let lib_path = parts[2];
                if lib_path.starts_with('/') {
                    libs.insert(lib_path.to_string());
                }
            }
        } else if line.trim().starts_with('/') {
            if let Some(lib_path) = line.trim().split_whitespace().next() {
                libs.insert(lib_path.to_string());
            }
        }
    }
}

fn copy_directory_recursive(src: &str, dst: &str) {
    let _ = Command::new("cp")
        .args(&["-r", src, dst])
        .output();
}

fn copy_terminfo(root: &str) {
    let share_dir = format!("{}/usr/share", root);
    fs::create_dir_all(&share_dir).ok();
    copy_directory_recursive("/usr/share/terminfo", &format!("{}/usr/share/terminfo", root));

    if std::path::Path::new("/lib/terminfo").exists() {
        let lib_dir = format!("{}/lib", root);
        fs::create_dir_all(&lib_dir).ok();
        copy_directory_recursive("/lib/terminfo", &format!("{}/lib/terminfo", root));
    }
}

pub fn copy_bash_and_dependencies(root: &str) {
    debug!("Copying binaries and libraries...");

    let binaries = vec![
        "/bin/bash",
        "/bin/sh",
        "/bin/ls",
        "/bin/cat",
        "/bin/touch",
        "/bin/cp",
        "/bin/mv",
        "/bin/rm",
        "/bin/mkdir",
        "/bin/rmdir",
        "/bin/nano",
        "/usr/bin/vi",
        "/bin/ps",
        "/bin/pwd",
        "/usr/bin/top",
        "/bin/kill",
        "/usr/bin/dd",
        "/bin/grep",
        "/usr/bin/find",
        "/usr/bin/wc",
        "/usr/bin/head",
        "/usr/bin/tail",
        "/bin/ip",
        "/sbin/ip",
        "/sbin/iptables",
        "/bin/ping",
        "/usr/bin/curl",
    ];

    // Collect all unique libraries first
    let mut all_libs: HashSet<String> = HashSet::new();
    for binary in &binaries {
        collect_shared_libraries(binary, &mut all_libs);
    }

    // Copy binaries
    let mut bin_count = 0;
    for binary in &binaries {
        let filename = std::path::Path::new(binary)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap();
        let dst = format!("{}/bin/{}", root, filename);
        if copy_file(binary, &dst) {
            bin_count += 1;
        }
    }

    // Copy libraries (deduplicated)
    let mut lib_count = 0;
    for lib_path in &all_libs {
        let dst = format!("{}{}", root, lib_path);
        if copy_file(lib_path, &dst) {
            lib_count += 1;
        }
    }

    copy_terminfo(root);

    debug!("Copied {} binaries, {} libraries", bin_count, lib_count);
}
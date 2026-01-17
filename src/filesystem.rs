use nix::mount::{mount, umount2, MsFlags, MntFlags};
use nix::unistd::{chdir, pivot_root};
use std::fs;
use std::process;

use crate::binaries::copy_bash_and_dependencies; 

fn create_container_dirs(new_root: &str) {
    println!("Creating container directory structure...");
    // Create root directory if it doesn't exist
    fs::create_dir_all(new_root).expect("Failed to create container root");

    let dirs = vec![
        "bin", "sbin", "lib", "lib64",  // Binaries and libraries
        "usr/bin", "usr/sbin", "usr/lib",
        "etc", "root", "home",           // Config and user dirs
        "proc", "sys", "dev", "tmp",     // Virtual filesystems
        "var", "run",                    // Runtime data
        "old_root",                      // For pivot_root
    ];
    for dir in dirs {
        let path = format!("{}/{}", new_root, dir);
        if let Err(e) = fs::create_dir_all(&path) {
            eprintln!("Warning: Failed to create {}: {}", path, e);
        }
    }
    
    println!("Container directories created");
}

fn pivot_to_new_root(new_root: &str) {
    // Change to new root directory
    if let Err(e) = chdir(new_root) {
        eprintln!("Failed to chdir to new root: {}", e);
        process::exit(1);
    }

        let old_root_path = "./old_root";
    if let Err(e) = fs::create_dir_all(old_root_path) {
        eprintln!("Failed to create old_root: {}", e);
    }

    if let Err(e) = pivot_root(".", "./old_root") {
        eprintln!("Failed to pivot_root: {}", e);
        eprintln!("Trying alternative method with MS_MOVE...");
        
        // Alternative: Use MS_MOVE mount instead of pivot_root
        use_mount_instead_of_pivot(new_root);
        return;
    }
    
    // Change to the new root (which is now /)
    if let Err(e) = chdir("/") {
        eprintln!("Failed to chdir to /: {}", e);
        process::exit(1);
    }
    
    // Unmount the old root
    if let Err(e) = umount2("/old_root", MntFlags::MNT_DETACH) {
        eprintln!("Warning: Failed to unmount old root: {}", e);
    }
    
    // Remove the old_root directory
    if let Err(e) = fs::remove_dir("/old_root") {
        eprintln!("Warning: Failed to remove /old_root: {}", e);
    }
    
    println!("Successfully pivoted to new root!");
}

fn use_mount_instead_of_pivot(new_root: &str) {
    println!("Using chroot as fallback...");
    
    use nix::unistd::chroot;
    
    // Simple chroot as fallback
    if let Err(e) = chroot(new_root) {
        eprintln!("Failed to chroot: {}", e);
        process::exit(1);
    }
    
    if let Err(e) = chdir("/") {
        eprintln!("Failed to chdir after chroot: {}", e);
        process::exit(1);
    }
    
    println!("Successfully chrooted to new root");
}

fn mount_proc() {
    mount(
        Some("proc"),
        "/proc",
        Some("proc"),
        MsFlags::empty(),
        None::<&str>,
    ).expect("Failed to mount /proc");
}

fn mount_sys() {
    mount(
        Some("sysfs"),
        "/sys",
        Some("sysfs"),
        MsFlags::empty(),
        None::<&str>,
    ).expect("Failed to mount /sys");
}

fn mount_dev() {
    if let Err(e) = mount(
        Some("devtmpfs"),
        "/dev",
        Some("devtmpfs"),
        MsFlags::MS_NOSUID | MsFlags::MS_STRICTATIME,
        None::<&str>,
    ) {
        eprintln!("Warning: Failed to mount devtmpfs: {}", e);
        eprintln!("Trying tmpfs fallback...");
        
        // Fallback to tmpfs
        if let Err(e2) = mount(
            Some("tmpfs"),
            "/dev",
            Some("tmpfs"),
            MsFlags::MS_NOSUID,
            Some("mode=755"),
        ) {
            eprintln!("Warning: Failed to mount /dev: {}", e2);
        }
    } else {
        println!("Mounted /dev with devtmpfs");
    }
}

fn mount_tmp() {
    // Mount /tmp as tmpfs
    mount(
        Some("tmpfs"),
        "/tmp",
        Some("tmpfs"),
        MsFlags::empty(),
        None::<&str>,
    ).expect("Failed to mount /tmp");
}

fn make_mount_point(new_root: &str) {
    println!("Making {} a mount point...", new_root);
    
    // Bind mount the directory to itself to make it a mount point
    // This is required for pivot_root to work
    if let Err(e) = mount(
        Some(new_root),
        new_root,
        None::<&str>,  // No filesystem type (it's a bind mount)
        MsFlags::MS_BIND | MsFlags::MS_REC,  // Recursive bind mount
        None::<&str>,
    ) {
        eprintln!("Failed to bind mount new root: {}", e);
        process::exit(1);
    }
    
    println!("New root is now a mount point");
}

fn mount_essential_filesystems() {
    mount_proc();
    mount_sys();
    mount_dev();
    mount_tmp();
    println!("Essential filesystems mounted");
}

pub fn setup_root_filesystem(new_root: &str) {
    println!("Setting up isolated root filesystem at {}...", new_root);

    // Create the new root directory structure
    create_container_dirs(new_root);

    copy_bash_and_dependencies(new_root);

    make_mount_point(new_root);
    
    // Pivot to the new root
    pivot_to_new_root(new_root);
    
    // Mount essential filesystems in the new root
    mount_essential_filesystems();
}
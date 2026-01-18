use nix::mount::{mount, umount2, MsFlags, MntFlags};
use nix::unistd::{chdir, pivot_root};
use std::fs;
use std::process;
use log::{debug, warn, error};

use crate::binaries::copy_bash_and_dependencies;

fn create_container_dirs(new_root: &str) {
    debug!("Creating container directory structure...");
    fs::create_dir_all(new_root).expect("Failed to create container root");

    let dirs = vec![
        "bin", "sbin", "lib", "lib64",
        "usr/bin", "usr/sbin", "usr/lib",
        "etc", "root", "home",
        "proc", "sys", "dev", "tmp",
        "var", "run",
        "old_root",
    ];
    for dir in dirs {
        let path = format!("{}/{}", new_root, dir);
        if let Err(e) = fs::create_dir_all(&path) {
            warn!("Failed to create {}: {}", path, e);
        }
    }

    debug!("Container directories created");
}

fn pivot_to_new_root(new_root: &str) {
    if let Err(e) = chdir(new_root) {
        error!("Failed to chdir to new root: {}", e);
        process::exit(1);
    }

    let old_root_path = "./old_root";
    if let Err(e) = fs::create_dir_all(old_root_path) {
        warn!("Failed to create old_root: {}", e);
    }

    if let Err(e) = pivot_root(".", "./old_root") {
        debug!("pivot_root failed: {}, trying chroot fallback...", e);
        use_mount_instead_of_pivot(new_root);
        return;
    }

    if let Err(e) = chdir("/") {
        error!("Failed to chdir to /: {}", e);
        process::exit(1);
    }

    if let Err(e) = umount2("/old_root", MntFlags::MNT_DETACH) {
        debug!("Failed to unmount old root: {}", e);
    }

    if let Err(e) = fs::remove_dir("/old_root") {
        debug!("Failed to remove /old_root: {}", e);
    }

    debug!("Pivoted to new root");
}

fn use_mount_instead_of_pivot(new_root: &str) {
    debug!("Using chroot as fallback...");

    use nix::unistd::chroot;

    if let Err(e) = chroot(new_root) {
        error!("Failed to chroot: {}", e);
        process::exit(1);
    }

    if let Err(e) = chdir("/") {
        error!("Failed to chdir after chroot: {}", e);
        process::exit(1);
    }

    debug!("Chrooted to new root");
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
    if let Err(_) = mount(
        Some("devtmpfs"),
        "/dev",
        Some("devtmpfs"),
        MsFlags::MS_NOSUID | MsFlags::MS_STRICTATIME,
        None::<&str>,
    ) {
        debug!("devtmpfs failed, trying tmpfs fallback...");
        if let Err(e) = mount(
            Some("tmpfs"),
            "/dev",
            Some("tmpfs"),
            MsFlags::MS_NOSUID,
            Some("mode=755"),
        ) {
            warn!("Failed to mount /dev: {}", e);
        }
    }
}

fn mount_tmp() {
    mount(
        Some("tmpfs"),
        "/tmp",
        Some("tmpfs"),
        MsFlags::empty(),
        None::<&str>,
    ).expect("Failed to mount /tmp");
}

fn make_mount_point(new_root: &str) {
    debug!("Making {} a mount point...", new_root);

    if let Err(e) = mount(
        Some(new_root),
        new_root,
        None::<&str>,
        MsFlags::MS_BIND | MsFlags::MS_REC,
        None::<&str>,
    ) {
        error!("Failed to bind mount new root: {}", e);
        process::exit(1);
    }

    debug!("New root is now a mount point");
}

fn mount_essential_filesystems() {
    mount_proc();
    mount_sys();
    mount_dev();
    mount_tmp();
    debug!("Essential filesystems mounted");
}

pub fn setup_root_filesystem(new_root: &str) {
    debug!("Setting up isolated root filesystem at {}...", new_root);

    create_container_dirs(new_root);
    copy_bash_and_dependencies(new_root);
    make_mount_point(new_root);
    pivot_to_new_root(new_root);
    mount_essential_filesystems();
}

use std::fs;
use std::process;
use nix::libc;
use log::{debug, warn};

const CGROUP_ROOT: &str = "/sys/fs/cgroup";

fn is_cgroup_v2() -> bool {
    std::path::Path::new("/sys/fs/cgroup/cgroup.controllers").exists()
}

pub fn setup_cgroups(container_name: &str) {
    debug!("Setting up cgroups for {}...", container_name);

    create_cgroup_hierarchy(container_name);
    set_resource_limits(container_name);
    add_process_to_cgroup(container_name);

    debug!("Cgroups configured");
}

fn create_cgroup_hierarchy(name: &str) {
    debug!("Creating cgroup hierarchy...");
    if is_cgroup_v2() {
        let path = format!("{}/{}", CGROUP_ROOT, name);
        if let Err(e) = fs::create_dir_all(&path) {
            warn!("Failed to create cgroup {}: {}", path, e);
        }
        enable_controllers_v2();
    } else {
        let controllers = vec!["cpu", "memory", "pids"];
        for controller in controllers {
            let path = format!("{}/{}/{}", CGROUP_ROOT, controller, name);
            if let Err(e) = fs::create_dir_all(&path) {
                warn!("Failed to create cgroup {}: {}", path, e);
            }
        }
    }
}

fn enable_controllers_v2() {
    let controllers_file = format!("{}/cgroups.controllers", CGROUP_ROOT);
    if let Ok(_controllers) = fs::read_to_string(&controllers_file) {
        let subtree_file = format!("{}/cgroup.subtree_control", CGROUP_ROOT);
        let enable = "+cpu +memory +pids".to_string();

        if let Err(e) = fs::write(&subtree_file, &enable) {
            debug!("Failed to enable controllers: {}", e);
        }
    }
}

fn set_resource_limits(name: &str) {
    debug!("Setting resource limits...");

    if is_cgroup_v2() {
        set_limits_v2(name);
    } else {
        set_limits_v1(name);
    }
}

fn set_limits_v1(name: &str) {
    write_cgroup_file(&format!("cpu/{}/cpu.cfs_quota_us", name), "50000");
    write_cgroup_file(&format!("cpu/{}/cpu.cfs_period_us", name), "100000");
    write_cgroup_file(&format!("memory/{}/memory.limit_in_bytes", name), "536870912");
    write_cgroup_file(&format!("pids/{}/pids.max", name), "100");
    debug!("Resource limits set (v1): CPU 50%, Memory 512MB, PIDs 100");
}

fn set_limits_v2(name: &str) {
    write_cgroup_file(&format!("{}/cpu.max", name), "50000 100000");
    write_cgroup_file(&format!("{}/memory.max", name), "536870912");
    write_cgroup_file(&format!("{}/pids.max", name), "100");
    debug!("Resource limits set (v2): CPU 50%, Memory 512MB, PIDs 100");
}

pub fn add_process_to_cgroup(name: &str) {
    let pid = process::id().to_string();

    if is_cgroup_v2() {
        write_cgroup_file(&format!("{}/cgroup.procs", name), &pid);
    } else {
        let controllers = vec!["cpu", "memory", "pids"];
        for controller in controllers {
            write_cgroup_file(&format!("{}/{}/cgroup.procs", controller, name), &pid);
        }
    }
}

fn write_cgroup_file(path: &str, content: &str) {
    let full_path = format!("{}/{}", CGROUP_ROOT, path);
    if let Err(e) = fs::write(&full_path, content) {
        debug!("Failed to write to {}: {}", full_path, e);
    }
}

pub fn cleanup_cgroup(name: &str) {
    debug!("Cleaning up cgroups...");

    std::thread::sleep(std::time::Duration::from_millis(100));

    if is_cgroup_v2() {
        cleanup_cgroup_v2(name);
    } else {
        cleanup_cgroup_v1(name);
    }
}

fn cleanup_cgroup_v2(name: &str) {
    let path = format!("{}/{}", CGROUP_ROOT, name);
    let procs_file = format!("{}/cgroup.procs", path);

    if let Ok(pids) = fs::read_to_string(&procs_file) {
        for pid_str in pids.lines() {
            if let Ok(pid) = pid_str.trim().parse::<i32>() {
                if pid > 0 {
                    unsafe {
                        libc::kill(pid, libc::SIGKILL);
                    }
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    match fs::remove_dir(&path) {
        Ok(_) => debug!("Cgroup removed: {}", name),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => {}
    }
}

fn cleanup_cgroup_v1(name: &str) {
    let controllers = vec!["cpu", "memory", "pids"];

    for controller in &controllers {
        let path = format!("{}/{}/{}", CGROUP_ROOT, controller, name);
        let procs_file = format!("{}/cgroup.procs", path);

        if let Ok(pids) = fs::read_to_string(&procs_file) {
            for pid_str in pids.lines() {
                if let Ok(pid) = pid_str.trim().parse::<i32>() {
                    if pid > 0 {
                        unsafe {
                            libc::kill(pid, libc::SIGKILL);
                        }
                    }
                }
            }
        }
    }

    std::thread::sleep(std::time::Duration::from_millis(50));

    for controller in &controllers {
        let path = format!("{}/{}/{}", CGROUP_ROOT, controller, name);
        let _ = fs::remove_dir(&path);
    }
}

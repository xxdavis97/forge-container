use std::fs;
use std::process;

const CGROUP_ROOT: &str = "/sys/fs/cgroup";

fn is_cgroup_v2() -> bool {
    // Check if using unified hierarchy (cgroup v2)
    std::path::Path::new("/sys/fs/cgroup/cgroup.controllers").exists()
}

pub fn setup_cgroups(container_name: &str) {
    println!("Setting up cgroups for {}...", container_name);
    
    create_cgroup_hierarchy(container_name);
    set_resource_limits(container_name);
    add_process_to_cgroup(container_name);
    
    println!("Cgroups configured successfully");
}

fn create_cgroup_hierarchy(name: &str) {
    println!("Creating cgroup hierarchy...");
    if is_cgroup_v2() {
        let path = format!("{}/{}", CGROUP_ROOT, name);
        match fs::create_dir_all(&path) {
            Ok(_) => println!("✓ Created cgroup: {}", path),
            Err(e) => eprintln!("✗ FAILED to create cgroup {}: {}", path, e),
        }
        enable_controllers_v2();
    } else {
        // Create directories for each controller
        let controllers = vec!["cpu", "memory", "pids"];
        for controller in controllers {
            let path = format!("{}/{}/{}", CGROUP_ROOT, controller, name);
            if let Err(e) = fs::create_dir_all(&path) {
                eprintln!("Warning: Failed to create cgroup {}: {}", path, e);
            } else {
                println!("Created cgroup: {}", path);
            }
        }
    }
}

fn enable_controllers_v2() {
    let controllers_file = format!("{}/cgroups.controllers", CGROUP_ROOT);
    if let Ok(_controllers) = fs::read_to_string(&controllers_file) {
        let subtree_file = format!("{}/cgroup.subtree_control", CGROUP_ROOT);
        let enable = format!("+cpu +memory +pids");
        
        if let Err(e) = fs::write(&subtree_file, &enable) {
            eprintln!("Warning: Failed to enable controllers: {}", e);
        }
    }
}

fn set_resource_limits(name: &str) {
    println!("Setting resource limits...");
    
    if is_cgroup_v2() {
        println!("Detected cgroup v2");
        set_limits_v2(name);
    } else {
        println!("Detected cgroup v1");
        set_limits_v1(name);
    }
}

fn set_limits_v1(name: &str) {
    // Your current code
    write_cgroup_file(&format!("cpu/{}/cpu.cfs_quota_us", name), "50000");
    write_cgroup_file(&format!("cpu/{}/cpu.cfs_period_us", name), "100000");
    println!("  CPU: Limited to 50% of one core");
    
    write_cgroup_file(&format!("memory/{}/memory.limit_in_bytes", name), "536870912");
    println!("  Memory: Limited to 512MB");
    
    write_cgroup_file(&format!("pids/{}/pids.max", name), "100");
    println!("  PIDs: Limited to 100 processes");
}

fn set_limits_v2(name: &str) {
    // Cgroup v2 uses different files
    write_cgroup_file(&format!("{}/cpu.max", name), "50000 100000");
    println!("  CPU: Limited to 50% of one core");
    
    write_cgroup_file(&format!("{}/memory.max", name), "536870912");
    println!("  Memory: Limited to 512MB");
    
    write_cgroup_file(&format!("{}/pids.max", name), "100");
    println!("  PIDs: Limited to 100 processes");
}

pub fn add_process_to_cgroup(name: &str) {
    let pid = process::id().to_string();

    if is_cgroup_v2() {
        // Cgroup v2: single unified hierarchy
        write_cgroup_file(&format!("{}/cgroup.procs", name), &pid);
    } else {
        // Cgroup v1: separate hierarchies
        let controllers = vec!["cpu", "memory", "pids"];
        
        for controller in controllers {
            write_cgroup_file(&format!("{}/{}/cgroup.procs", controller, name), &pid);
        }
    }
}

fn write_cgroup_file(path: &str, content: &str) {
    let full_path = format!("{}/{}", CGROUP_ROOT, path);

    if let Err(e) = fs::write(&full_path, content) {
        eprintln!("Warning: Failed to write to {}: {}", full_path, e);
    }
}

pub fn cleanup_cgroup(name: &str) {
    println!("Cleaning up cgroups...");
    if is_cgroup_v2() {
        let path = format!("{}/{}", CGROUP_ROOT, name);
        if let Err(e) = fs::remove_dir(&path) {
            eprintln!("Warning: Failed to remove cgroup {}: {}", path, e);
        }
    } else {
        let controllers = vec!["cpu", "memory", "pids"];
        
        for controller in controllers {
            let path = format!("{}/{}/{}", CGROUP_ROOT, controller, name);
            
            // Remove directory (will only work if empty/no processes)
            if let Err(e) = fs::remove_dir(&path) {
                eprintln!("Warning: Failed to remove cgroup {}: {}", path, e);
            }
        }
    }

}
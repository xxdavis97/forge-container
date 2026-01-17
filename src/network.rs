use std::process::Command;

pub fn get_default_interface_public() -> String {
    get_default_interface()
}

pub fn setup_veth_pair_with_iface(container_pid: u32, default_iface: &str) {
    println!("=== Setting up network ===");
    let veth_host = format!("veth-{}", container_pid);
    let veth_container = format!("veth-c-{}", container_pid);
    
    // 1. Create veth pair
    create_veth_pair(&veth_host, &veth_container);
    
    // 2. Move container end to namespace
    move_to_netns(&veth_container, container_pid);
    
    // 3. Configure host end
    configure_host_veth(&veth_host);
    
    // 4. Configure container end (from host, using netns)
    configure_container_veth(&veth_container, container_pid);
    
    // 5. Enable NAT
    enable_nat(&veth_host, &default_iface);
    
    println!("=== Network setup complete ===");
}

fn create_veth_pair(veth_host: &str, veth_container: &str) {
    run_ip(&["link", "add", veth_host, "type", "veth", "peer", "name", veth_container]);
}

fn move_to_netns(veth_container: &str, container_pid: u32) {
    let netns_path = format!("/proc/{}/ns/net", container_pid);
    let netns_name = format!("cnt-{}", container_pid);
    
    println!("Moving {} to namespace PID {}", veth_container, container_pid);
    println!("Netns path: {}", netns_path);
    
    // Check if netns path exists
    if !std::path::Path::new(&netns_path).exists() {
        eprintln!("ERROR: Netns path doesn't exist: {}", netns_path);
        return;
    }
    
    std::fs::create_dir_all("/var/run/netns").ok();
    let netns_link = format!("/var/run/netns/{}", netns_name);
    
    let _ = std::fs::remove_file(&netns_link);
    
    if let Err(e) = std::os::unix::fs::symlink(&netns_path, &netns_link) {
        eprintln!("ERROR: Failed to create symlink: {}", e);
        return;
    }
    
    println!("Created symlink: {} -> {}", netns_link, netns_path);
    
    run_ip(&["link", "set", veth_container, "netns", &netns_name]);
    
    // Verify it worked
    let check = Command::new("ip")
        .args(&["link", "show", veth_container])
        .output();
    
    if let Ok(output) = check {
        if output.status.success() {
            println!("WARNING: {} still visible on host after move!", veth_container);
        } else {
            println!("✓ {} successfully moved to namespace", veth_container);
        }
    }
    
    std::fs::remove_file(&netns_link).ok();
}

fn configure_host_veth(veth_host: &str) {
    run_ip(&["addr", "add", "10.0.0.1/24", "dev", veth_host]);
    run_ip(&["link", "set", veth_host, "up"]);
}

fn configure_container_veth(veth_container: &str, container_pid: u32) {
    let netns_path = format!("/proc/{}/ns/net", container_pid);
    let netns_name = format!("cnt-{}", container_pid);
    
    std::fs::create_dir_all("/var/run/netns").ok();
    let netns_link = format!("/var/run/netns/{}", netns_name);
    
    let _ = std::fs::remove_file(&netns_link);
    std::os::unix::fs::symlink(&netns_path, &netns_link).ok();
    
    // Configure inside namespace
    run_ip(&["netns", "exec", &netns_name, "ip", "addr", "add", "10.0.0.2/24", "dev", veth_container]);
    run_ip(&["netns", "exec", &netns_name, "ip", "link", "set", veth_container, "up"]);
    run_ip(&["netns", "exec", &netns_name, "ip", "link", "set", "lo", "up"]);
    run_ip(&["netns", "exec", &netns_name, "ip", "route", "add", "default", "via", "10.0.0.1"]);
    
    std::fs::remove_file(&netns_link).ok();
}

fn enable_nat(veth_host: &str, default_iface: &str) {
    println!("Enabling NAT via {}", default_iface);
    
    run_iptables(&["-t", "nat", "-A", "POSTROUTING", "-s", "10.0.0.0/24", "-o", &default_iface, "-j", "MASQUERADE"]);
    run_iptables(&["-A", "FORWARD", "-i", veth_host, "-o", &default_iface, "-j", "ACCEPT"]);
    run_iptables(&["-A", "FORWARD", "-i", &default_iface, "-o", veth_host, "-j", "ACCEPT"]);
}

fn get_default_interface() -> String {
    let output = Command::new("ip")
        .args(&["route", "show", "default"])
        .output()
        .expect("Failed to get route");
    
    let route = String::from_utf8_lossy(&output.stdout);
    println!("Default route output: {}", route);  // Debug
    
    // Parse "default via X.X.X.X dev INTERFACE"
    for part in route.split_whitespace() {
        // Look for the word after "dev"
    }
    
    let parts: Vec<&str> = route.split_whitespace().collect();
    if let Some(dev_pos) = parts.iter().position(|&x| x == "dev") {
        if dev_pos + 1 < parts.len() {
            let iface = parts[dev_pos + 1].to_string();
            println!("Detected interface: {}", iface);
            return iface;
        }
    }
    
    println!("WARNING: Falling back to enp0s1");
    "enp0s1".to_string()
}

fn run_ip(args: &[&str]) {
    let output = Command::new("ip").args(args).output().expect("ip failed");
    if !output.status.success() {
        eprintln!("ip {} failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr));
    }
}

fn run_iptables(args: &[&str]) {
    println!("Running: iptables {}", args.join(" "));
    
    let output = Command::new("iptables")
        .args(args)
        .output()
        .expect("iptables command failed to execute");
    
    println!("Exit status: {:?}", output.status);
    println!("Stdout: {}", String::from_utf8_lossy(&output.stdout));
    println!("Stderr: {}", String::from_utf8_lossy(&output.stderr));
    
    if !output.status.success() {
        eprintln!("ERROR: iptables {} failed!", args.join(" "));
        std::process::exit(1);
    }
}
use nix::sched::{unshare, CloneFlags};
use std::process;

pub fn create_namespaces_without_network() {
    println!("Creating namespaces (without network)...");
    
    let flags = CloneFlags::CLONE_NEWPID |
                CloneFlags::CLONE_NEWNS |
                CloneFlags::CLONE_NEWUTS;
    // NO CLONE_NEWNET yet!
    
    if let Err(e) = unshare(flags) {
        eprintln!("Failed to create namespaces: {}", e);
        process::exit(1);
    }
    
    println!("Namespaces created (PID, Mount, UTS)");
}

pub fn create_network_namespace() {
    println!("Creating network namespace...");
    
    let flags = CloneFlags::CLONE_NEWNET;
    
    if let Err(e) = unshare(flags) {
        eprintln!("Failed to create network namespace: {}", e);
        process::exit(1);
    }
    
    println!("Network namespace created");
}
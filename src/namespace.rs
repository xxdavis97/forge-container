use nix::sched::{unshare, CloneFlags};
use std::process;
use log::{debug, error};

pub fn create_namespaces_without_network() {
    debug!("Creating namespaces (PID, Mount, UTS)...");

    let flags = CloneFlags::CLONE_NEWPID |
                CloneFlags::CLONE_NEWNS |
                CloneFlags::CLONE_NEWUTS;

    if let Err(e) = unshare(flags) {
        error!("Failed to create namespaces: {}", e);
        process::exit(1);
    }

    debug!("Namespaces created");
}

pub fn create_network_namespace() {
    debug!("Creating network namespace...");

    let flags = CloneFlags::CLONE_NEWNET;

    if let Err(e) = unshare(flags) {
        error!("Failed to create network namespace: {}", e);
        process::exit(1);
    }

    debug!("Network namespace created");
}

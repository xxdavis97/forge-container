mod namespace;
mod filesystem;
mod container;
mod binaries; 
mod cgroups;
mod network;

use nix::unistd::{fork, ForkResult};
use nix::sys::wait::waitpid;
use std::process;

use container::run_container;
use cgroups::cleanup_cgroup;

fn main() {
    println!("Starting container runtime (PID: {})...", process::id());
    
    match unsafe { fork() } {
        Ok(ForkResult::Parent {child}) => {
            println!("Waiting for container process: {}", child);
            let _ = waitpid(child, None);
            println!("Container exited");
            cleanup_cgroup("my_container");
            process::exit(0);
        }
        Ok(ForkResult::Child) => {
            run_container();
        }
        Err(e) => {
            eprintln!("Fork failed: {}", e);
            process::exit(1);
        }
    }
}
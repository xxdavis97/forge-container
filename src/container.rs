use nix::unistd::{execvp, fork, ForkResult};
use nix::sys::wait::waitpid;
use std::ffi::CString;
use std::process;

use crate::namespace;
use crate::filesystem::setup_root_filesystem;
use crate::cgroups;
use crate::network;

const CONTAINER_ROOT: &str = "/tmp/container-root";
const CONTAINER_NAME: &str = "my_container";  

pub fn run_container() -> ! {
    println!("Setting up container (PID: {})...", process::id());
    
    cgroups::setup_cgroups(CONTAINER_NAME);
    let _ = std::fs::write("/proc/sys/net/ipv4/ip_forward", "1");
    let default_iface = network::get_default_interface_public();
    
    // DON'T create network namespace yet!
    // Only create PID, Mount, UTS namespaces
    namespace::create_namespaces_without_network();
    let netns_pid = process::id();
    
    println!("Forking to become PID 1...");
    
    match unsafe { fork() } {
        Ok(ForkResult::Parent {child}) => {
            println!("Spawned PID 1 process");
            
            network::setup_veth_pair_with_iface(child.as_raw() as u32, &default_iface);
            
            let _ = waitpid(child, None);
            process::exit(0);
        }
        Ok(ForkResult::Child) => {
            namespace::create_network_namespace();
            cgroups::add_process_to_cgroup(CONTAINER_NAME);
            setup_root_filesystem(CONTAINER_ROOT);
            
            start_shell();
        }
        Err(e) => {
            eprintln!("Second fork failed: {}", e);
            process::exit(1);
        }
    }
}

#[allow(unreachable_code)]
fn start_shell() -> !{
    println!("Starting shell...");
    let shell = if std::path::Path::new("/bin/bash").exists() {
        "/bin/bash"
    } else {
        "/bin/sh"
    };
    let shell = CString::new(shell).unwrap();
    let args = vec![shell.clone()];
    
    execvp(&shell, &args).expect("Failed to exec shell");
    unreachable!("execvp should never return");
}
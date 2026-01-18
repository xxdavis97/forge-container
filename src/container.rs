use nix::unistd::{execvp, fork, ForkResult};
use nix::sys::wait::waitpid;
use std::ffi::CString;
use std::process;
use log::{debug, info, warn, error};

use crate::namespace;
use crate::filesystem::setup_root_filesystem;
use crate::cgroups;
use crate::network;
use crate::image::ImageConfig;

const CONTAINER_ROOT: &str = "/tmp/container-root";
const CONTAINER_NAME: &str = "my_container";  

pub fn run_container() -> ! {
    debug!("Setting up container (PID: {})...", process::id());

    cgroups::setup_cgroups(CONTAINER_NAME);
    let _ = std::fs::write("/proc/sys/net/ipv4/ip_forward", "1");
    let default_iface = network::get_default_interface_public();

    namespace::create_namespaces_without_network();
    let _netns_pid = process::id();

    debug!("Forking to become PID 1...");

    match unsafe { fork() } {
        Ok(ForkResult::Parent {child}) => {
            debug!("Spawned PID 1 process: {}", child);

            network::setup_veth_pair_with_iface(child.as_raw() as u32, &default_iface);

            let _ = waitpid(child, None);
            let _ = std::fs::remove_dir_all(CONTAINER_ROOT);

            process::exit(0);
        }
        Ok(ForkResult::Child) => {
            namespace::create_network_namespace();
            cgroups::add_process_to_cgroup(CONTAINER_NAME);
            setup_root_filesystem(CONTAINER_ROOT);

            start_shell();
        }
        Err(e) => {
            error!("Fork failed: {}", e);
            process::exit(1);
        }
    }
}

pub fn run_container_from_image(rootfs_path: &str, config: &ImageConfig, container_name: &str) -> ! {
    debug!("Setting up container from image (PID: {})...", process::id());

    cgroups::setup_cgroups(container_name);
    let _ = std::fs::write("/proc/sys/net/ipv4/ip_forward", "1");
    let default_iface = network::get_default_interface_public();

    namespace::create_namespaces_without_network();

    debug!("Forking to become PID 1...");

    match unsafe { fork() } {
        Ok(ForkResult::Parent {child}) => {
            debug!("Spawned PID 1 process: {}", child);

            network::setup_veth_pair_with_iface(child.as_raw() as u32, &default_iface);

            let _ = waitpid(child, None);

            cgroups::cleanup_cgroup(container_name);
            let _ = std::fs::remove_dir_all(rootfs_path);
            info!("Container exited");

            process::exit(0);
        }
        Ok(ForkResult::Child) => {
            namespace::create_network_namespace();
            cgroups::add_process_to_cgroup(container_name);
            setup_root_filesystem(rootfs_path);

            for env_var in &config.env {
                if let Some(pos) = env_var.find('=') {
                    let key = &env_var[..pos];
                    let value = &env_var[pos + 1..];
                    std::env::set_var(key, value);
                }
            }

            if let Err(e) = std::env::set_current_dir(&config.working_dir) {
                warn!("Failed to change directory to {}: {}", config.working_dir, e);
            }

            if !config.entrypoint.is_empty() {
                start_entrypoint(&config.entrypoint);
            } else {
                start_shell();
            }
        }
        Err(e) => {
            error!("Fork failed: {}", e);
            process::exit(1);
        }
    }
}

fn start_entrypoint(entrypoint: &[String]) -> ! {
    debug!("Starting entrypoint: {:?}", entrypoint);

    let program = CString::new(entrypoint[0].as_str()).unwrap();
    let args: Vec<CString> = entrypoint.iter()
        .map(|s| CString::new(s.as_str()).unwrap())
        .collect();

    match execvp(&program, &args) {
        Ok(_) => unreachable!(),
        Err(e) => panic!("Failed to exec entrypoint: {}", e),
    }
}

fn start_shell() -> ! {
    debug!("Starting shell...");
    let shell = if std::path::Path::new("/bin/bash").exists() {
        "/bin/bash"
    } else {
        "/bin/sh"
    };
    let shell = CString::new(shell).unwrap();
    let args = vec![shell.clone()];

    match execvp(&shell, &args) {
        Ok(_) => unreachable!(),
        Err(e) => panic!("Failed to exec shell: {}", e),
    }
}
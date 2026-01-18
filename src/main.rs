mod namespace;
mod filesystem;
mod container;
mod binaries;
mod cgroups;
mod network;
mod image;
mod forgefile;
mod imagebuilder;

use nix::unistd::{fork, ForkResult};
use nix::sys::wait::waitpid;
use std::process;
use std::env;
use log::{debug, info, error};

use container::run_container;
use cgroups::cleanup_cgroup;
use image::{build_image, run_image};

fn main() {
    // Initialize logger - defaults to "info", use RUST_LOG=debug for verbose
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info")
    ).format_timestamp(None).init();

    debug!("Starting container runtime (PID: {})...", process::id());

    let args: Vec<String> = env::args().collect();

    if args.len() > 1 && args[1] == "build" {
        if let Err(e) = build_image(&args) {
            error!("Build failed: {}", e);
            process::exit(1);
        }
        return;
    }

    if args.len() > 1 && args[1] == "run" {
        if args.len() < 3 {
            error!("Usage: container-runtime run IMAGE:TAG");
            process::exit(1);
        }
        if let Err(e) = run_image(&args[2]) {
            error!("Run failed: {}", e);
            process::exit(1);
        }
        return;
    }

    // Default: run interactive container
    match unsafe { fork() } {
        Ok(ForkResult::Parent { child }) => {
            debug!("Waiting for container process: {}", child);
            let _ = waitpid(child, None);
            info!("Container exited");
            cleanup_cgroup("my_container");
            process::exit(0);
        }
        Ok(ForkResult::Child) => {
            run_container();
        }
        Err(e) => {
            error!("Fork failed: {}", e);
            process::exit(1);
        }
    }
}

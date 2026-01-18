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
use std::path::PathBuf;
use log::{debug, info, error};

use container::run_container;
use cgroups::cleanup_cgroup;

fn main() {
    // Initialize logger - defaults to "info", use RUST_LOG=debug for verbose
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info")
    ).format_timestamp(None).init();

    debug!("Starting container runtime (PID: {})...", process::id());

    let args: Vec<String> = env::args().collect();

    if args.len() > 1 && args[1] == "build" {
        if let Err(e) = build_image(&args) {
            eprintln!("Build failed: {}", e);
            process::exit(1);
        }
        return;
    }
    if args.len() > 1 && args[1] == "run" {
        if args.len() < 3 {
            eprintln!("Usage: container-runtime run IMAGE:TAG");
            process::exit(1);
        }
        if let Err(e) = run_image(&args[2]) {
            eprintln!("Run failed: {}", e);
            process::exit(1);
        }
        return;
    }
    
    match unsafe { fork() } {
        Ok(ForkResult::Parent {child}) => {
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

fn build_image(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use image::ImageStore;
    use imagebuilder::ImageBuilder;
    
    // Parse args: build -f Containerfile -t myapp:v1.0
    let mut containerfile_path = PathBuf::from("ForgeFile");
    let mut image_name = "app";
    let mut image_tag = "latest";
    
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "-f" | "--file" => {
                containerfile_path = PathBuf::from(&args[i + 1]);
                i += 2;
            }
            "-t" | "--tag" => {
                let parts: Vec<&str> = args[i + 1].split(':').collect();
                image_name = parts[0];
                image_tag = parts.get(1).unwrap_or(&"latest");
                i += 2;
            }
            _ => i += 1,
        }
    }
    
    // Create image store
    let store_path = PathBuf::from(std::env::var("HOME")?)
        .join(".container-runtime/images");
    let store = ImageStore::new(store_path)?;
    
    // Build the image
    let builder = ImageBuilder::new(store);
    builder.build(&containerfile_path, image_name, image_tag)?;
    
    Ok(())
}

fn run_image(image_ref: &str) -> Result<(), Box<dyn std::error::Error>> {
    use image::{ImageStore, ImageConfig};
    use std::path::PathBuf;
    use container::run_container_from_image;
    
    println!("🚀 Running container from image: {}", image_ref);
    
    // Parse image reference (e.g., "myapp:v1.0")
    let parts: Vec<&str> = image_ref.split(':').collect();
    let name = parts[0];
    let tag = parts.get(1).unwrap_or(&"latest");
    
    // Load image from store
    let store_path = PathBuf::from(std::env::var("HOME")?)
        .join(".container-runtime/images");
    let store = ImageStore::new(store_path)?;
    
    println!("📖 Loading image {}:{}...", name, tag);
    let manifest = store.load_manifest(name, tag)?;
    
    // Load config
    let config_path = store.root.join("manifests")
        .join(name)
        .join(format!("{}.config", tag));
    let config_json = std::fs::read_to_string(config_path)?;
    let config: ImageConfig = serde_json::from_str(&config_json)?;
    
    // Create temporary rootfs and extract layers
    let container_id = uuid::Uuid::new_v4();
    let rootfs = PathBuf::from(format!("/tmp/container-{}", container_id));
    std::fs::create_dir_all(&rootfs)?;
    
    println!("📦 Extracting {} layers...", manifest.layers.len());
    for (i, layer_digest) in manifest.layers.iter().enumerate() {
        println!("  [{}/{}] Extracting layer {}...", 
            i + 1, manifest.layers.len(), &layer_digest[..16]);
        
        let layer_path = store.get_layer_path(layer_digest);
        std::process::Command::new("tar")
            .args(&["-xzf", layer_path.to_str().unwrap(), "-C", rootfs.to_str().unwrap()])
            .status()?;
    }
    
    println!("✅ Rootfs ready at {:?}", rootfs);
    println!("🎯 Starting container with YOUR runtime...\n");
    
    println!("Container Configuration:");
    println!("  Working Dir: {}", config.working_dir);
    println!("  Environment: {:?}", config.env);
    println!("  Entrypoint: {:?}", config.entrypoint);
    println!();
    
    // Run container using your existing runtime!
    let container_name = format!("img-{}", container_id);
    run_container_from_image(rootfs.to_str().unwrap(), &config, &container_name);
    
    // Never reaches here because run_container_from_image never returns
}

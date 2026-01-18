use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::fs;
use std::io::Write;
use std::collections::HashMap;
use log::{debug, info};

use crate::imagebuilder::ImageBuilder;
use crate::container::run_container_from_image;

const LAYERS: &str = "layers";
const MANIFESTS: &str = "manifests";
const CACHE_INDEX: &str = "cache_index.json";

// This represents ONE image (like "myapp:v1.0")
#[derive(Serialize, Deserialize, Debug)]
pub struct ImageManifest {
    pub name: String,           // "myapp"
    pub tag: String,            // "v1.0"
    pub layers: Vec<String>,    // ["sha256:abc...", "sha256:def..."]
}

// This is the configuration for HOW to run the container
#[derive(Serialize, Deserialize, Debug)]
pub struct ImageConfig {
    pub entrypoint: Vec<String>,  // ["python3", "app.py"]
    pub env: Vec<String>,         // ["PATH=/usr/bin", "PYTHONUNBUFFERED=1"]
    pub working_dir: String,      // "/app"
}

pub struct ImageStore {
    pub root: PathBuf,  // Like ~/.container-runtime/images
}

impl ImageStore {
    pub fn new(root: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let _ = fs::create_dir_all(&root)?;
        let _ = fs::create_dir_all(root.join(LAYERS));
        let _ = fs::create_dir_all(root.join(MANIFESTS));

        Ok(Self { root })
    }

    pub fn save_manifest(&self, manifest: &ImageManifest) -> Result<(), Box<dyn std::error::Error>> {
        let dir = self.root.join(MANIFESTS).join(&manifest.name);
        fs::create_dir_all(&dir)?;

        let file_path = dir.join(&manifest.tag);
        let json = serde_json::to_string_pretty(manifest)?;
        let file = fs::File::create(file_path);
        let _ = file?.write_all(json.as_bytes());

        debug!("Saved manifest: {}:{}", manifest.name, manifest.tag);
        Ok(())
    }

    pub fn load_manifest(&self, name: &str, tag: &str) -> Result<ImageManifest, Box<dyn std::error::Error>> {
        let file_path = self.root.join(MANIFESTS).join(name).join(tag);
        let json = fs::read_to_string(file_path)?;
        let manifest: ImageManifest = serde_json::from_str(&json)?;
        Ok(manifest)
    }

    pub fn save_layer(&self, tarball_path: &Path) -> Result<String, Box<dyn std::error::Error>> {
        use sha2::{Sha256, Digest};
    
        let data = fs::read(tarball_path)?;
        let digest = format!("sha256:{}", hex::encode(Sha256::digest(&data)));
    
        let dest = self.root.join("layers").join(&digest);
        fs::copy(tarball_path, dest)?;
        
        Ok(digest)
    }

    pub fn get_layer_path(&self, digest: &str) -> PathBuf {
        self.root.join("layers").join(digest)
    }

    /// Load the cache index (cache_key -> layer_digest mapping)
    pub fn load_cache_index(&self) -> HashMap<String, String> {
        let path = self.root.join(CACHE_INDEX);
        if let Ok(json) = fs::read_to_string(&path) {
            serde_json::from_str(&json).unwrap_or_default()
        } else {
            HashMap::new()
        }
    }

    /// Save the cache index
    pub fn save_cache_index(&self, index: &HashMap<String, String>) -> Result<(), Box<dyn std::error::Error>> {
        let path = self.root.join(CACHE_INDEX);
        let json = serde_json::to_string_pretty(index)?;
        fs::write(path, json)?;
        Ok(())
    }

    /// Check if a layer exists for the given cache key
    pub fn get_cached_layer(&self, cache_key: &str) -> Option<String> {
        let index = self.load_cache_index();
        index.get(cache_key).cloned()
    }

    /// Store a cache key -> layer digest mapping
    pub fn cache_layer(&self, cache_key: &str, layer_digest: &str) -> Result<(), Box<dyn std::error::Error>> {
        let mut index = self.load_cache_index();
        index.insert(cache_key.to_string(), layer_digest.to_string());
        self.save_cache_index(&index)?;
        Ok(())
    }

    /// Check if a layer file exists
    pub fn layer_exists(&self, digest: &str) -> bool {
        self.get_layer_path(digest).exists()
    }

    /// Load image configuration
    pub fn load_config(&self, name: &str, tag: &str) -> Result<ImageConfig, Box<dyn std::error::Error>> {
        let config_path = self.root.join(MANIFESTS)
            .join(name)
            .join(format!("{}.config", tag));
        let config_json = fs::read_to_string(config_path)?;
        let config: ImageConfig = serde_json::from_str(&config_json)?;
        Ok(config)
    }
}

/// Build an image from a Forgefile
pub fn build_image(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
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
    info!("Building image {}:{}", image_name, image_tag);
    let builder = ImageBuilder::new(store);
    builder.build(&containerfile_path, image_name, image_tag)?;

    Ok(())
}

/// Run a container from an image
pub fn run_image(image_ref: &str) -> Result<(), Box<dyn std::error::Error>> {
    info!("Running container from image: {}", image_ref);

    // Parse image reference (e.g., "myapp:v1.0")
    let parts: Vec<&str> = image_ref.split(':').collect();
    let name = parts[0];
    let tag = parts.get(1).unwrap_or(&"latest");

    // Load image from store
    let store_path = PathBuf::from(std::env::var("HOME")?)
        .join(".container-runtime/images");
    let store = ImageStore::new(store_path)?;

    debug!("Loading image {}:{}...", name, tag);
    let manifest = store.load_manifest(name, tag)?;

    // Load config
    let config = store.load_config(name, tag)?;

    // Create temporary rootfs and extract layers
    let container_id = uuid::Uuid::new_v4();
    let rootfs = PathBuf::from(format!("/tmp/container-{}", container_id));
    fs::create_dir_all(&rootfs)?;

    info!("Extracting {} layers...", manifest.layers.len());
    for (i, layer_digest) in manifest.layers.iter().enumerate() {
        debug!("  [{}/{}] Extracting layer {}...",
            i + 1, manifest.layers.len(), &layer_digest[..16]);

        let layer_path = store.get_layer_path(layer_digest);
        std::process::Command::new("tar")
            .args(&["-xzf", layer_path.to_str().unwrap(), "-C", rootfs.to_str().unwrap()])
            .status()?;
    }

    debug!("Rootfs ready at {:?}", rootfs);
    debug!("Container config - workdir: {}, env: {:?}, entrypoint: {:?}",
        config.working_dir, config.env, config.entrypoint);

    // Run container using the container runtime
    let container_name = format!("img-{}", container_id);
    run_container_from_image(rootfs.to_str().unwrap(), &config, &container_name);

    // Never reaches here because run_container_from_image never returns
}

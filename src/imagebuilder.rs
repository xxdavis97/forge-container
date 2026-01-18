use crate::forgefile::{Forgefile, Instruction};
use crate::image::{ImageStore, ImageManifest, ImageConfig};
use std::path::{Path, PathBuf};
use std::fs;
use std::process::Command;
use sha2::{Sha256, Digest};
use log::info;

pub struct ImageBuilder {
    store: ImageStore,
}

impl ImageBuilder {
    pub fn new(store: ImageStore) -> Self {
        Self { store }
    }

    pub fn build(&self, forgefile_path: &Path, name: &str, tag: &str) -> Result<(), Box<dyn std::error::Error>> {
        let forgefile = Forgefile::parse(forgefile_path)?;

        let build_dir = PathBuf::from("/tmp/container-build");
        if build_dir.exists() {
            fs::remove_dir_all(&build_dir)?;
        }
        fs::create_dir_all(&build_dir)?;

        let rootfs = build_dir.join("rootfs");
        fs::create_dir_all(&rootfs)?;

        let mut config = ImageConfig {
            entrypoint: Vec::new(),
            env: vec!["PATH=/usr/local/bin:/usr/bin:/bin".to_string()],
            working_dir: "/".to_string(),
        };

        let mut layers: Vec<String> = Vec::new();
        let mut prev_cache_key = String::from("base");
        let mut cache_valid = true;

        for instruction in forgefile.instructions.iter() {
            match instruction {
                Instruction::From { image } => {
                    let cache_key = self.compute_cache_key(&prev_cache_key, &format!("FROM:{}", image));

                    if cache_valid {
                        if let Some(layer_digest) = self.store.get_cached_layer(&cache_key) {
                            if self.store.layer_exists(&layer_digest) {
                                info!("  📦 FROM {} (cached)", image);
                                self.extract_layer(&layer_digest, &rootfs)?;
                                layers.push(layer_digest);
                                prev_cache_key = cache_key;
                                continue;
                            }
                        }
                    }

                    // Cache miss - execute instruction
                    cache_valid = false;
                    info!("  📥 FROM {} (downloading...)", image);
                    self.pull_base_image(image, &rootfs)?;

                    let layer_digest = self.create_layer(&rootfs)?;
                    self.store.cache_layer(&cache_key, &layer_digest)?;
                    layers.push(layer_digest);
                    prev_cache_key = cache_key;
                }

                Instruction::Copy { src, dest } => {
                    // For COPY, cache key includes hash of source file contents
                    let src_path = forgefile.context_dir.join(src);
                    let content_hash = self.hash_path(&src_path)?;
                    let cache_key = self.compute_cache_key(&prev_cache_key, &format!("COPY:{}:{}:{}", src, dest, content_hash));

                    if cache_valid {
                        if let Some(layer_digest) = self.store.get_cached_layer(&cache_key) {
                            if self.store.layer_exists(&layer_digest) {
                                info!("  📄 COPY {} -> {} (cached)", src, dest);
                                self.extract_layer(&layer_digest, &rootfs)?;
                                layers.push(layer_digest);
                                prev_cache_key = cache_key;
                                continue;
                            }
                        }
                    }

                    // Cache miss
                    cache_valid = false;
                    info!("  📄 COPY {} -> {}", src, dest);
                    let dest_path = rootfs.join(dest.trim_start_matches("/"));

                    if let Some(parent) = dest_path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    if src_path.is_dir() {
                        copy_dir(&src_path, &dest_path)?;
                    } else {
                        fs::copy(&src_path, &dest_path)?;
                    }

                    let layer_digest = self.create_layer(&rootfs)?;
                    self.store.cache_layer(&cache_key, &layer_digest)?;
                    layers.push(layer_digest);
                    prev_cache_key = cache_key;
                }

                Instruction::Run { command } => {
                    let cache_key = self.compute_cache_key(&prev_cache_key, &format!("RUN:{}", command));

                    if cache_valid {
                        if let Some(layer_digest) = self.store.get_cached_layer(&cache_key) {
                            if self.store.layer_exists(&layer_digest) {
                                info!("  ⚙️  RUN {} (cached)", command);
                                self.extract_layer(&layer_digest, &rootfs)?;
                                layers.push(layer_digest);
                                prev_cache_key = cache_key;
                                continue;
                            }
                        }
                    }

                    // Cache miss
                    cache_valid = false;
                    info!("  ⚙️  RUN {}", command);
                    self.run_in_chroot(&rootfs, command)?;

                    let layer_digest = self.create_layer(&rootfs)?;
                    self.store.cache_layer(&cache_key, &layer_digest)?;
                    layers.push(layer_digest);
                    prev_cache_key = cache_key;
                }

                Instruction::Workdir { path } => {
                    config.working_dir = path.clone();
                    // No layer, but update cache key for chain
                    prev_cache_key = self.compute_cache_key(&prev_cache_key, &format!("WORKDIR:{}", path));
                }

                Instruction::Env { key, value } => {
                    config.env.push(format!("{}={}", key, value));
                    prev_cache_key = self.compute_cache_key(&prev_cache_key, &format!("ENV:{}={}", key, value));
                }

                Instruction::Entrypoint { args } => {
                    config.entrypoint = args.clone();
                    prev_cache_key = self.compute_cache_key(&prev_cache_key, &format!("ENTRYPOINT:{:?}", args));
                }
            }
        }

        let manifest = ImageManifest {
            name: name.to_string(),
            tag: tag.to_string(),
            layers,
        };
        self.store.save_manifest(&manifest)?;

        let config_json = serde_json::to_string_pretty(&config)?;
        let config_path = self.store.root.join("manifests")
            .join(name)
            .join(format!("{}.config", tag));
        fs::write(config_path, config_json)?;

        // Cleanup build directory
        let _ = fs::remove_dir_all(&build_dir);

        info!("  ✅ Build complete: {}:{}", name, tag);
        Ok(())
    }

    fn compute_cache_key(&self, prev_key: &str, instruction: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(prev_key.as_bytes());
        hasher.update(instruction.as_bytes());
        format!("cache:{}", hex::encode(hasher.finalize()))
    }

    fn hash_path(&self, path: &Path) -> Result<String, Box<dyn std::error::Error>> {
        let mut hasher = Sha256::new();

        if path.is_file() {
            hasher.update(&fs::read(path)?);
        } else if path.is_dir() {
            self.hash_dir_recursive(path, &mut hasher)?;
        }

        Ok(hex::encode(hasher.finalize()))
    }

    fn hash_dir_recursive(&self, dir: &Path, hasher: &mut Sha256) -> Result<(), Box<dyn std::error::Error>> {
        let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|e| e.path());

        for entry in entries {
            let path = entry.path();
            hasher.update(path.file_name().unwrap().to_string_lossy().as_bytes());

            if path.is_file() {
                hasher.update(&fs::read(&path)?);
            } else if path.is_dir() {
                self.hash_dir_recursive(&path, hasher)?;
            }
        }
        Ok(())
    }

    fn extract_layer(&self, digest: &str, rootfs: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let layer_path = self.store.get_layer_path(digest);
        Command::new("tar")
            .args(&["-xzf", layer_path.to_str().unwrap(), "-C", rootfs.to_str().unwrap()])
            .status()?;
        Ok(())
    }

    fn pull_base_image(&self, image: &str, dest: &Path) -> Result<(), Box<dyn std::error::Error>> {
        if image.starts_with("alpine") {
            let arch = std::env::consts::ARCH;
            let alpine_arch = match arch {
                "x86_64" => "x86_64",
                "aarch64" => "aarch64",
                _ => return Err(format!("Unsupported architecture: {}", arch).into()),
            };

            // Check if we have a cached alpine download
            let alpine_cache = PathBuf::from(format!(
                "{}/alpine-{}.tar.gz",
                self.store.root.to_str().unwrap(),
                alpine_arch
            ));

            if !alpine_cache.exists() {
                let url = format!(
                    "https://dl-cdn.alpinelinux.org/alpine/v3.19/releases/{}/alpine-minirootfs-3.19.1-{}.tar.gz",
                    alpine_arch, alpine_arch
                );

                info!("    Downloading Alpine for {}...", alpine_arch);
                let output = Command::new("curl")
                    .args(&["-L", "-o", alpine_cache.to_str().unwrap(), &url])
                    .output()?;

                if !output.status.success() {
                    return Err("Failed to download base image".into());
                }
            }

            Command::new("tar")
                .args(&["-xzf", alpine_cache.to_str().unwrap(), "-C", dest.to_str().unwrap()])
                .status()?;
        } else {
            return Err(format!("Unsupported base image: {}. Only 'alpine:*' is supported.", image).into());
        }
        Ok(())
    }

    fn run_in_chroot(&self, rootfs: &Path, command: &str) -> Result<(), Box<dyn std::error::Error>> {
        let resolv_conf = rootfs.join("etc/resolv.conf");

        if let Some(parent) = resolv_conf.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::copy("/etc/resolv.conf", &resolv_conf)?;

        let status = Command::new("chroot")
            .arg(rootfs)
            .arg("/bin/sh")
            .arg("-c")
            .arg(command)
            .status()?;

        if !status.success() {
            return Err(format!("RUN command failed: {}", command).into());
        }
        Ok(())
    }

    fn create_layer(&self, rootfs: &Path) -> Result<String, Box<dyn std::error::Error>> {
        let layer_id = uuid::Uuid::new_v4();
        let tarball_path = PathBuf::from(format!("/tmp/layer-{}.tar.gz", layer_id));

        Command::new("tar")
            .args(&["-czf", tarball_path.to_str().unwrap(), "-C", rootfs.to_str().unwrap(), "."])
            .status()?;

        let digest = self.store.save_layer(&tarball_path)?;
        fs::remove_file(&tarball_path)?;

        Ok(digest)
    }
}

fn copy_dir(src: &Path, dest: &Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let dest_path = dest.join(entry.file_name());

        if file_type.is_dir() {
            copy_dir(&entry.path(), &dest_path)?;
        } else {
            fs::copy(&entry.path(), &dest_path)?;
        }
    }
    Ok(())
}

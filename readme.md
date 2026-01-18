# Forge Container

## Project Summary

This is my attempt of implementing a virtualization container, similar to Docker. I decided to implement this in Rust as I wanted to gain familiarity with a common new low level programming language that I have not used before. The key things that had to be tackled including creating a run-time container by isolating namespaces.

I spawn an ubuntu VM using multipass due to my reliance on specific linux commands. I then worked to setup a filesystem, utilize cgroups to limit the resources consumed by my container, and copy over binaries to allow my container to run commands such as ls and ip. I also needed to setup network access for the container through network interfacing.

The last step was to be able to handling imaging so users could leverage the container to run code inside it, the real power of containers.

### Why Rust?

Rust is particularly well-suited for building container runtimes and other systems-level software:

- **Memory Safety Without Garbage Collection** - Rust's ownership model guarantees memory safety at compile time without the runtime overhead of a garbage collector. This is critical for container runtimes where performance and predictable latency matter.

- **Zero-Cost Abstractions** - High-level constructs compile down to efficient machine code comparable to C/C++, allowing expressive code without sacrificing performance.

- **Direct System Call Access** - The `nix` crate provides safe Rust bindings to Linux system calls like `fork()`, `unshare()`, and `pivot_root()`, making it straightforward to interact with kernel interfaces.

- **Strong Type System** - Many bugs are caught at compile time rather than runtime. The compiler enforces proper error handling, preventing the "forgot to check return value" bugs common in C.

- **Concurrency** - The ownership model prevents data races, which is valuable when managing container processes and network namespaces.

- **Industry Adoption** - Rust is increasingly used in infrastructure software. Projects like `youki` (an OCI container runtime) and parts of AWS's Firecracker are written in Rust.

### Why Learn Rust?

- **Growing Demand** - Major companies (AWS, Microsoft, Google, Meta, Cloudflare, Discord) are adopting Rust for performance-critical infrastructure.

- **Modern Tooling** - Cargo provides dependency management, building, testing, and documentation in one tool. Clippy and rustfmt ensure consistent, idiomatic code.

- **Versatility** - Rust excels at CLI tools, web services, embedded systems, and WebAssembly, making it applicable across many domains.

- **Systems Understanding** - Writing systems code in Rust forces you to understand memory layouts, lifetimes, and low-level concepts while the compiler guides you toward correct code.

- **Community** - Consistently rated among the most loved programming languages, with excellent documentation and a welcoming community.

---

## Phase 1 - Isolating Processes

The first goal was to be able to run my container, and spawn a process with a pid of 1, without showing the pids of the host. This would show that I have correctly isolated processes within the container. To do this we use `fork()` to copy the process and `unshare()` to be able to create new linux namespaces for the process. We need to use new namespaces as it is the Linux kernel's way of providing processes with isolated views of system resources. When initializing the name spaces we first strive to isolate process tree, mounted filesystems, and hostname. We leverage the namespace flags `CLONE_NEWPID`, `CLONE_NEWNS`, and `CLONE_NEWUTS` for this combined with a pipe operator.

### Testing Phase 1

In the container shell we execute the following command:
```bash
echo $$
```
Which yields the pid of 1.

We then run the same command not within the container but simply within the VM shell, we show a far higher pid.

### Key Aspects For Phase 1

We leverage several system calls to enable phase 1 to be possible, these include:

| Command | Purpose |
|---------|---------|
| `fork()` | Create copy of the current process |
| `unshare()` | Create new namespaces for the process |
| `execvp()` | Replace the current process with bash |
| `waitpid()` | Wait for child process to finish in parent |

One key thing to note is the use of 2 `fork()` commands. The reason for this is we have our main running on the host which has its own pid (say 123) which then spawns a child on the host (say pid 124). We then call `unshare()` to create the new namespace for pid 124 and `fork()` again to create a grandchild with pid 1 in the new namespace. `exec bash` then becomes pid 1 shell.

### Code - Creating Namespaces

```rust
// src/namespace.rs
pub fn create_namespaces_without_network() {
    let flags = CloneFlags::CLONE_NEWPID |
                CloneFlags::CLONE_NEWNS |
                CloneFlags::CLONE_NEWUTS;

    if let Err(e) = unshare(flags) {
        error!("Failed to create namespaces: {}", e);
        process::exit(1);
    }
}
```

### Code - Fork and Execute Shell

```rust
// src/container.rs
match unsafe { fork() } {
    Ok(ForkResult::Parent {child}) => {
        // Parent waits for child to exit
        let _ = waitpid(child, None);
        process::exit(0);
    }
    Ok(ForkResult::Child) => {
        // Child becomes PID 1 in new namespace
        start_shell();
    }
    Err(e) => {
        error!("Fork failed: {}", e);
        process::exit(1);
    }
}
```

---

## Phase 2 - Isolating Filesystem

The goal here is to mount a new root filesystem for the container. Here we need to mount folders such as `/proc` so we can leverage the `ps` command. The objective is to create a root for the container and pivot the root to change the container's view of the filesystem. We will also set up other directories such as `/bin`, `/lib` and `/etc`. In addition to setting up basic file system directories we had to copy over binaries from the host such as `/bin/bash` for the shell and utilities such as `/bin/ls` as once we isolate the filesystem we would no longer to be able to run these pivotal commands from the container.

### Testing Phase 2 - Further Process Isolation

To begin we test more process isolation (we couldn't do this before as `/proc` was not mounted) the container shell we execute the following command `ps aux`, which only shows 2 processes.

We then run the same command not within the container but simply within the VM shell, showing far more processes, proving we are isolated.

### Testing Phase 2 - Filesystem Isolation

Prior to setting up our filesystem namespace we were able from the container execute commands such as `ls /home` which would then list the files and directories in the home folder on the host VM. After making the changes when running the same command `ls /home` we return empty as the container no longer has visibility into the host.

### Use of pivot_root vs. chroot

| Feature | chroot | pivot_root |
|---------|--------|------------|
| Security | Could escape with sufficient privilege | Can't escape |
| Root Mount | Changes apparent root | Changes actual root mount |
| Old root | Still accessible | Removed completely |
| Use Case | Dev/Testing | Production container |

Docker utilizes `pivot_root` as well.

### Key Aspects For Phase 2

- Learning virtual filesystems in linux
- Dealing with shared libraries (`.so` files)
- Dynamic linking with `ldd`
- Mount points and binding

### Code - Setting Up Root Filesystem

```rust
// src/filesystem.rs
pub fn setup_root_filesystem(new_root: &str) {
    create_container_dirs(new_root);
    copy_bash_and_dependencies(new_root);
    make_mount_point(new_root);
    pivot_to_new_root(new_root);
    mount_essential_filesystems();
}
```

### Code - Creating Container Directories

```rust
// src/filesystem.rs
fn create_container_dirs(new_root: &str) {
    fs::create_dir_all(new_root).expect("Failed to create container root");

    let dirs = vec![
        "bin", "sbin", "lib", "lib64",
        "usr/bin", "usr/sbin", "usr/lib",
        "etc", "root", "home",
        "proc", "sys", "dev", "tmp",
        "var", "run", "old_root",
    ];
    for dir in dirs {
        let path = format!("{}/{}", new_root, dir);
        fs::create_dir_all(&path).ok();
    }
}
```

### Code - Pivoting to New Root

```rust
// src/filesystem.rs
fn pivot_to_new_root(new_root: &str) {
    chdir(new_root).expect("Failed to chdir");
    fs::create_dir_all("./old_root").ok();

    pivot_root(".", "./old_root").expect("pivot_root failed");

    chdir("/").expect("Failed to chdir to /");
    umount2("/old_root", MntFlags::MNT_DETACH).ok();
    fs::remove_dir("/old_root").ok();
}
```

### Code - Copying Binaries and Dependencies

```rust
// src/binaries.rs
pub fn copy_bash_and_dependencies(root: &str) {
    let binaries = vec![
        "/bin/bash", "/bin/sh", "/bin/ls", "/bin/cat",
        "/bin/ps", "/bin/ip", "/sbin/iptables", "/usr/bin/curl",
        // ... more binaries
    ];

    // Collect all unique libraries using ldd
    let mut all_libs: HashSet<String> = HashSet::new();
    for binary in &binaries {
        collect_shared_libraries(binary, &mut all_libs);
    }

    // Copy binaries and their dependencies
    for binary in &binaries {
        let filename = Path::new(binary).file_name().unwrap();
        let dst = format!("{}/bin/{}", root, filename.to_str().unwrap());
        fs::copy(binary, &dst).ok();
    }

    for lib_path in &all_libs {
        let dst = format!("{}{}", root, lib_path);
        fs::copy(lib_path, &dst).ok();
    }
}
```

---

## Phase 3 - Employing Resource Limits

The current structure allows the container to consume an unlimited amount of resources from the host system. This is an issue as it can consume too much CPU, starving other processes, consume too much RAM, crashing the system, consume too much storage, filling disk space, or spawn unlimited processes, fork bomb. We solve this with linux cgroups.

### Testing Phase 3

Here we test each aspect individually by leveraging one terminal within the container and one terminal in the host outside the container.

#### Testing CPU Use

Within the container, we can run `while true; do :; done` to try and consume all the CPU. On the host terminal we then run `ps aux` to show that the container process is taking up around our CPU limit of 50% instead of 100%, highlighting our cgroup CPU limit working.

#### Testing Memory Use

Within the container we can run `dd if=/dev/zero of=/tmp/file bs=1G` to consume all the ram, but then looking on our host system we can see it stops when it reaches the RAM allocated by our cgroup.

#### Testing Storage Limit

Here we attempt to write a file exceeding out 512MB limit and see that it fails. For example `dd if=/dev/zero of=/tmp/bigfile bs=1M count=600`

#### Testing Process Limit

Here we can run `:(){ :|:& };:` to fork bomb. But if we monitor the number of processes, we see they never exceed 100 which is our cgroup implemented limit.

### Key Aspects For Phase 3

We autodetect if cgroup v1 or cgroup v2 is active to provide working limits along both philosophies. There is a different file structure for cgroups between the 2 versions making this crucial if you want to be able to run this along both methods. It was also key to set up the cgroups after our second fork but prior to setting up our virtual filesystem.

### Code - Setting Up Cgroups

```rust
// src/cgroups.rs
pub fn setup_cgroups(container_name: &str) {
    create_cgroup_hierarchy(container_name);
    set_resource_limits(container_name);
    add_process_to_cgroup(container_name);
}

fn is_cgroup_v2() -> bool {
    Path::new("/sys/fs/cgroup/cgroup.controllers").exists()
}
```

### Code - Setting Resource Limits (v1)

```rust
// src/cgroups.rs
fn set_limits_v1(name: &str) {
    // CPU: 50% (50000/100000 microseconds)
    write_cgroup_file(&format!("cpu/{}/cpu.cfs_quota_us", name), "50000");
    write_cgroup_file(&format!("cpu/{}/cpu.cfs_period_us", name), "100000");
    // Memory: 512MB
    write_cgroup_file(&format!("memory/{}/memory.limit_in_bytes", name), "536870912");
    // PIDs: max 100 processes
    write_cgroup_file(&format!("pids/{}/pids.max", name), "100");
}
```

### Code - Setting Resource Limits (v2)

```rust
// src/cgroups.rs
fn set_limits_v2(name: &str) {
    // CPU: 50% (50000 quota per 100000 period)
    write_cgroup_file(&format!("{}/cpu.max", name), "50000 100000");
    // Memory: 512MB
    write_cgroup_file(&format!("{}/memory.max", name), "536870912");
    // PIDs: max 100 processes
    write_cgroup_file(&format!("{}/pids.max", name), "100");
}
```

### Code - Adding Process to Cgroup

```rust
// src/cgroups.rs
pub fn add_process_to_cgroup(name: &str) {
    let pid = process::id().to_string();

    if is_cgroup_v2() {
        write_cgroup_file(&format!("{}/cgroup.procs", name), &pid);
    } else {
        for controller in ["cpu", "memory", "pids"] {
            write_cgroup_file(&format!("{}/{}/cgroup.procs", controller, name), &pid);
        }
    }
}
```

---

## Phase 4 - Network Isolation

Currently the container can see all host network traffic. In addition to this not being compliant with the container paradigm this creates practical issues, such as if the container binds to port 80, the host can no longer utilize that port. This is also a security issue as the container would be able to sniff host network traffic.

What we should do here is create a network namespace just for the container, then use a virtual ethernet (veth) pair to connect the container to the host. The container will have its own IP address, can communicate with the host and the internet, and be isolated from other containers.

With the veth pair we can have one end of our virtual cable connected to the host, and one end connected to the container just like if you were to connect a physical ethernet cable between 2 machines.

To do this we are going to leverage the `CLONE_NEWNET` flag for namespaces.

### High Level Network Isolation Steps

#### Step 1 - Create The Pipe Using The veth Pair

```
[veth-7003] <-- pipe --> [veth-c-7003]
   (host)                 (container)
```

#### Step 2 - Move One End Of Pipe Into Container

```
Host side:                Container side:
[veth-7003]               [veth-c-7003]
     |                          |
  10.0.0.1                  10.0.0.2
```

#### Step 3 - Configure IP Address

```
Host side:        Container side:
10.0.0.1/24       10.0.0.2/24
```

#### Step 4 - Set Default Route In Container

Container: "To reach the internet, send packets to 10.0.0.1"

#### Step 5 - Enable NAT On Host

This allows the internet to think it is talking to the host not the container.

```
Container (10.0.0.2) --> Host translates --> Internet (sees host's IP)
Internet response --> Host translates back --> Container (10.0.0.2)
```

#### Step 6 - iptables Forward Rules

Tell host firewall to let packets flow between container and internet.

```
Allow packets: veth-7003 <--> enp0s1
```

### Journey Of A Packet

When container runs `ping 8.8.8.8`:

```
1.  Container: "I want to reach 8.8.8.8"

2.  Container checks route table: "Not 10.0.0.x, use default -> 10.0.0.1"

3.  Packet goes through veth-c-7003 --> veth-7003 (the pipe)
    Source: 10.0.0.2
    Dest: 8.8.8.8

4.  Host receives packet on veth-7003

5.  Host checks: "8.8.8.8 not local, need to forward"

6.  iptables FORWARD rule: "Allow veth-7003 -> enp0s1" [OK]

7.  iptables NAT (MASQUERADE):
    Changes source from 10.0.0.2 --> 192.168.2.31 (host IP)

8.  Packet goes out enp0s1 to internet
    Source: 192.168.2.31
    Dest: 8.8.8.8

9.  Internet sees packet from 192.168.2.31, responds

10. Response comes back to host (192.168.2.31)

11. Host's NAT remembers: "This is for 10.0.0.2"
    Changes dest from 192.168.2.31 --> 10.0.0.2

12. iptables FORWARD: "Allow enp0s1 -> veth-7003" [OK]

13. Packet goes through veth-7003 --> veth-c-7003

14. Container receives ping response!
```

### Code - Creating Network Namespace

```rust
// src/namespace.rs
pub fn create_network_namespace() {
    let flags = CloneFlags::CLONE_NEWNET;

    if let Err(e) = unshare(flags) {
        error!("Failed to create network namespace: {}", e);
        process::exit(1);
    }
}
```

### Code - Setting Up veth Pair

```rust
// src/network.rs
pub fn setup_veth_pair_with_iface(container_pid: u32, default_iface: &str) {
    let veth_host = format!("veth-{}", container_pid);
    let veth_container = format!("veth-c-{}", container_pid);

    create_veth_pair(&veth_host, &veth_container);
    move_to_netns(&veth_container, container_pid);
    configure_host_veth(&veth_host);
    configure_container_veth(&veth_container, container_pid);
    enable_nat(&veth_host, default_iface);
}

fn create_veth_pair(veth_host: &str, veth_container: &str) {
    run_ip(&["link", "add", veth_host, "type", "veth", "peer", "name", veth_container]);
}
```

### Code - Configuring Host and Container Interfaces

```rust
// src/network.rs
fn configure_host_veth(veth_host: &str) {
    run_ip(&["addr", "add", "10.0.0.1/24", "dev", veth_host]);
    run_ip(&["link", "set", veth_host, "up"]);
}

fn configure_container_veth(veth_container: &str, container_pid: u32) {
    let netns_name = format!("cnt-{}", container_pid);

    run_ip(&["netns", "exec", &netns_name, "ip", "addr", "add",
             "10.0.0.2/24", "dev", veth_container]);
    run_ip(&["netns", "exec", &netns_name, "ip", "link", "set",
             veth_container, "up"]);
    run_ip(&["netns", "exec", &netns_name, "ip", "link", "set", "lo", "up"]);
    run_ip(&["netns", "exec", &netns_name, "ip", "route", "add",
             "default", "via", "10.0.0.1"]);
}
```

### Code - Enabling NAT

```rust
// src/network.rs
fn enable_nat(veth_host: &str, default_iface: &str) {
    // MASQUERADE outgoing packets from container network
    run_iptables(&["-t", "nat", "-A", "POSTROUTING",
                   "-s", "10.0.0.0/24", "-o", default_iface, "-j", "MASQUERADE"]);
    // Allow forwarding between container and internet
    run_iptables(&["-A", "FORWARD", "-i", veth_host,
                   "-o", default_iface, "-j", "ACCEPT"]);
    run_iptables(&["-A", "FORWARD", "-i", default_iface,
                   "-o", veth_host, "-j", "ACCEPT"]);
}
```

---

## Phase 5 - Imaging

Imaging is the capstone feature that transforms a container runtime from a technical curiosity into a practical tool. While Phases 1-4 built the isolation primitives (namespaces, filesystem, cgroups, networking), imaging provides the mechanism to package, distribute, and reproducibly run applications.

An **image** is a self-contained, immutable snapshot of a filesystem that includes everything an application needs to run: code, runtime, libraries, environment variables, and configuration. When you "run" an image, the container runtime unpacks it into a rootfs and executes it within the isolated environment we built in previous phases.

### The "Works On My Machine" Problem

Before containers, deploying software was fraught with environment inconsistencies:

```
Developer's Machine          Production Server
-------------------------------------------------
Python 3.11                  Python 3.8
libssl 1.1.1                 libssl 1.0.2
pandas 2.0                   pandas 1.5
Ubuntu 22.04                 CentOS 7
```

Code that worked perfectly in development would fail in production due to missing dependencies, version mismatches, or OS differences. Teams spent countless hours debugging environment issues rather than building features.

**Containers solve this by packaging the entire environment with the application:**

```
+------------------------------------------+
|              Container Image             |
|  +------------------------------------+  |
|  |  Application Code (app.py)        |  |
|  +------------------------------------+  |
|  |  Runtime (Python 3.11)            |  |
|  +------------------------------------+  |
|  |  Libraries (pandas, numpy, etc.)  |  |
|  +------------------------------------+  |
|  |  Base OS (Alpine Linux)           |  |
|  +------------------------------------+  |
+------------------------------------------+
```

The same image that runs on a developer's laptop will run identically on any server with a container runtime. This is the fundamental power of containerization.

### Forgefile Format

A **Forgefile** (analogous to Docker's Dockerfile) is a text file containing instructions for building an image. Each instruction modifies the filesystem or sets configuration metadata.

> **Examples:** See the `example-forge-files/` directory for Forgefiles for Python, Java, Node.js, Go, Ruby, Rust, C, and C++.

#### Supported Instructions

| Instruction | Purpose | Example |
|-------------|---------|---------|
| `FROM` | Set base image | `FROM alpine:3.19` |
| `COPY` | Copy files from build context into image | `COPY app.py /app/` |
| `RUN` | Execute command during build | `RUN pip install pandas` |
| `WORKDIR` | Set working directory | `WORKDIR /app` |
| `ENV` | Set environment variable | `ENV PYTHONUNBUFFERED=1` |
| `ENTRYPOINT` | Command to run when container starts | `ENTRYPOINT ["python3", "app.py"]` |

#### Example Forgefile

```dockerfile
# Forgefile for a Python application
FROM alpine:3.19

# Install Python and pip
RUN apk add --no-cache python3 py3-pip

# Set working directory
WORKDIR /app

# Copy application files
COPY requirements.txt /app/
COPY app.py /app/

# Install Python dependencies
RUN pip install --no-cache-dir -r requirements.txt

# Set environment variables
ENV PYTHONUNBUFFERED=1

# Define the entrypoint
ENTRYPOINT ["python3", "app.py"]
```

### Code - Parsing Forgefile

The Forgefile parser reads the instruction file and converts it into structured data:

```rust
// src/forgefile.rs
#[derive(Debug, Clone)]
pub enum Instruction {
    From { image: String },
    Copy { src: String, dest: String },
    Run { command: String },
    Workdir { path: String },
    Env { key: String, value: String },
    Entrypoint { args: Vec<String> },
}

pub struct Forgefile {
    pub instructions: Vec<Instruction>,
    pub context_dir: PathBuf,  // Directory containing the Forgefile
}

impl Forgefile {
    pub fn parse(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = fs::read_to_string(path)?;
        let context_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();

        let mut instructions = Vec::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("#") {
                continue;
            }

            let parts: Vec<&str> = line.splitn(2, ' ').collect();
            if let Ok(Some(instruction)) = Self::parse_command_line(parts) {
                instructions.push(instruction);
            }
        }

        Ok(Self { instructions, context_dir })
    }
}
```

### Image Storage Architecture

Images are stored in a content-addressable filesystem structure. This design enables deduplication, integrity verification, and efficient layer sharing.

```
~/.forge-container/images/
├── layers/                          # Content-addressable layer storage
│   ├── sha256:a1b2c3d4e5f6...      # Layer tarball (Alpine base)
│   ├── sha256:b2c3d4e5f6a7...      # Layer tarball (pip install)
│   └── sha256:c3d4e5f6a7b8...      # Layer tarball (app code)
├── manifests/                       # Image metadata
│   └── myapp/
│       ├── v1.0                     # Manifest JSON
│       └── v1.0.config              # Runtime config JSON
└── cache_index.json                 # Build cache mappings
```

#### Image Manifest

The manifest tracks which layers comprise an image:

```rust
// src/image.rs
#[derive(Serialize, Deserialize, Debug)]
pub struct ImageManifest {
    pub name: String,           // "myapp"
    pub tag: String,            // "v1.0"
    pub layers: Vec<String>,    // ["sha256:a1b2...", "sha256:b2c3..."]
}
```

Example manifest JSON:
```json
{
  "name": "myapp",
  "tag": "v1.0",
  "layers": [
    "sha256:a1b2c3d4e5f6789...",
    "sha256:b2c3d4e5f6a7890...",
    "sha256:c3d4e5f6a7b8901..."
  ]
}
```

#### Image Configuration

The configuration defines how to run the container:

```rust
// src/image.rs
#[derive(Serialize, Deserialize, Debug)]
pub struct ImageConfig {
    pub entrypoint: Vec<String>,  // ["python3", "app.py"]
    pub env: Vec<String>,         // ["PATH=/usr/bin", "PYTHONUNBUFFERED=1"]
    pub working_dir: String,      // "/app"
}
```

#### Content-Addressable Storage

Layers are stored by their SHA256 hash. This provides several benefits:

1. **Integrity** - If the content changes, the hash changes
2. **Deduplication** - Identical layers are stored once, even across images
3. **Immutability** - Layers cannot be modified without detection

```rust
// src/image.rs
pub fn save_layer(&self, tarball_path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    use sha2::{Sha256, Digest};

    let data = fs::read(tarball_path)?;
    let digest = format!("sha256:{}", hex::encode(Sha256::digest(&data)));

    let dest = self.root.join("layers").join(&digest);
    fs::copy(tarball_path, dest)?;

    Ok(digest)
}
```

### Building Images

The image build process executes each Forgefile instruction sequentially, creating a layer after filesystem-modifying instructions.

#### Build Process Flow

```
+-------------+     +------------------+     +----------------+
|  Forgefile  | --> |  ImageBuilder    | --> |  Image Store   |
|  (text)     |     |  (executes)      |     |  (layers)      |
+-------------+     +------------------+     +----------------+
                            |
                            v
                    +---------------+
                    |  Build Root   |
                    |  /tmp/build/  |
                    |   rootfs/     |
                    +---------------+
```

#### Layer Creation Diagram

Each instruction that modifies the filesystem creates a new layer:

```
Instruction              Resulting Layer
-----------              ---------------

FROM alpine:3.19    -->  Layer 1: [Alpine base filesystem]
                         sha256:a1b2c3...
                                |
                                v
RUN apk add python3 -->  Layer 2: [Layer 1 + Python installed]
                         sha256:b2c3d4...
                                |
                                v
COPY app.py /app/   -->  Layer 3: [Layer 2 + app.py]
                         sha256:c3d4e5...
                                |
                                v
RUN pip install ... -->  Layer 4: [Layer 3 + dependencies]
                         sha256:d4e5f6...
```

The final image references all layers in order. When run, layers are extracted sequentially to reconstruct the complete filesystem.

### Code - Building Image Layers

```rust
// src/imagebuilder.rs
pub fn build(&self, forgefile_path: &Path, name: &str, tag: &str)
    -> Result<(), Box<dyn std::error::Error>>
{
    let forgefile = Forgefile::parse(forgefile_path)?;

    // Create temporary build directory
    let build_dir = PathBuf::from("/tmp/container-build");
    let rootfs = build_dir.join("rootfs");
    fs::create_dir_all(&rootfs)?;

    let mut config = ImageConfig {
        entrypoint: Vec::new(),
        env: vec!["PATH=/usr/local/bin:/usr/bin:/bin".to_string()],
        working_dir: "/".to_string(),
    };

    let mut layers: Vec<String> = Vec::new();

    for instruction in forgefile.instructions.iter() {
        match instruction {
            Instruction::From { image } => {
                info!("  FROM {}", image);
                self.pull_base_image(image, &rootfs)?;
                let layer_digest = self.create_layer(&rootfs)?;
                layers.push(layer_digest);
            }

            Instruction::Copy { src, dest } => {
                info!("  COPY {} -> {}", src, dest);
                let src_path = forgefile.context_dir.join(src);
                let dest_path = rootfs.join(dest.trim_start_matches("/"));
                fs::copy(&src_path, &dest_path)?;
                let layer_digest = self.create_layer(&rootfs)?;
                layers.push(layer_digest);
            }

            Instruction::Run { command } => {
                info!("  RUN {}", command);
                self.run_in_chroot(&rootfs, command)?;
                let layer_digest = self.create_layer(&rootfs)?;
                layers.push(layer_digest);
            }

            Instruction::Workdir { path } => {
                config.working_dir = path.clone();
            }

            Instruction::Env { key, value } => {
                config.env.push(format!("{}={}", key, value));
            }

            Instruction::Entrypoint { args } => {
                config.entrypoint = args.clone();
            }
        }
    }

    // Save manifest and config
    let manifest = ImageManifest { name: name.to_string(), tag: tag.to_string(), layers };
    self.store.save_manifest(&manifest)?;

    Ok(())
}
```

### Using chroot for Build vs. pivot_root for Runtime

An important architectural decision is using `chroot` during image building but `pivot_root` during container runtime.

| Phase | Mechanism | Reason |
|-------|-----------|--------|
| **Build** | `chroot` | We control the build process; simpler setup; need host access for package downloads |
| **Runtime** | `pivot_root` | Running potentially untrusted code; need complete isolation; old root fully removed |

During **build**, the code being executed (package managers like `apk` or `pip`) comes from trusted base images. We need network access to download packages and simpler filesystem access. The `chroot` provides sufficient isolation for this controlled environment.

During **runtime**, the container may run arbitrary user code. We use `pivot_root` because it completely removes access to the host filesystem, preventing escape attempts. The old root is unmounted and deleted, leaving no path back to the host.

```rust
// src/imagebuilder.rs - Build time: using chroot
fn run_in_chroot(&self, rootfs: &Path, command: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Copy DNS resolution for network access during build
    fs::copy("/etc/resolv.conf", rootfs.join("etc/resolv.conf"))?;

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
```

```rust
// src/filesystem.rs - Runtime: using pivot_root
fn pivot_to_new_root(new_root: &str) {
    chdir(new_root).expect("Failed to chdir");

    // pivot_root swaps the root mount
    pivot_root(".", "./old_root").expect("pivot_root failed");

    chdir("/").expect("Failed to chdir to /");

    // Completely remove access to old root
    umount2("/old_root", MntFlags::MNT_DETACH).ok();
    fs::remove_dir("/old_root").ok();  // No escape possible
}
```

### Layer Caching

Rebuilding an entire image for every code change would be prohibitively slow. Layer caching dramatically speeds up builds by reusing unchanged layers.

#### How Caching Works

Each instruction generates a **cache key** based on:
1. The cache key of the previous instruction (chain dependency)
2. The instruction content itself
3. For `COPY` instructions: the hash of the source file contents

```
Cache Key Computation:

  previous_key + instruction_hash = new_cache_key

Example chain:
  "base"           + hash("FROM alpine")     = key1
  key1             + hash("RUN apk add...")  = key2
  key2             + hash("COPY app.py:abc") = key3  (includes file hash)
```

#### Cache Invalidation Chain

The cache is a **chain** - if any instruction changes, all subsequent instructions must rebuild:

```
Scenario: Change to app.py (Layer 3)

Before change:           After change:
Layer 1: [CACHED]        Layer 1: [CACHED]      (no change)
Layer 2: [CACHED]        Layer 2: [CACHED]      (no change)
Layer 3: [CACHED]        Layer 3: [REBUILD]     (file changed)
Layer 4: [CACHED]        Layer 4: [REBUILD]     (depends on L3)
```

This is why Forgefiles should order instructions from least-frequently-changed to most-frequently-changed:

```dockerfile
# GOOD: Dependencies cached, only app code rebuilds
FROM alpine:3.19
RUN apk add python3 py3-pip           # Rarely changes
COPY requirements.txt /app/
RUN pip install -r requirements.txt    # Changes with dependencies
COPY app.py /app/                      # Changes frequently (at end)
ENTRYPOINT ["python3", "app.py"]
```

### Code - Layer Caching Implementation

```rust
// src/imagebuilder.rs
fn compute_cache_key(&self, prev_key: &str, instruction: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prev_key.as_bytes());
    hasher.update(instruction.as_bytes());
    format!("cache:{}", hex::encode(hasher.finalize()))
}

// For COPY instructions, include file content hash
fn hash_path(&self, path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let mut hasher = Sha256::new();

    if path.is_file() {
        hasher.update(&fs::read(path)?);
    } else if path.is_dir() {
        self.hash_dir_recursive(path, &mut hasher)?;
    }

    Ok(hex::encode(hasher.finalize()))
}
```

```rust
// src/imagebuilder.rs - Using the cache
Instruction::Run { command } => {
    let cache_key = self.compute_cache_key(&prev_cache_key, &format!("RUN:{}", command));

    // Check if we have a cached layer for this key
    if cache_valid {
        if let Some(layer_digest) = self.store.get_cached_layer(&cache_key) {
            if self.store.layer_exists(&layer_digest) {
                info!("  RUN {} (cached)", command);
                self.extract_layer(&layer_digest, &rootfs)?;
                layers.push(layer_digest);
                prev_cache_key = cache_key;
                continue;  // Skip execution, use cached layer
            }
        }
    }

    // Cache miss - execute instruction
    cache_valid = false;  // Invalidate cache for subsequent instructions
    info!("  RUN {}", command);
    self.run_in_chroot(&rootfs, command)?;

    let layer_digest = self.create_layer(&rootfs)?;
    self.store.cache_layer(&cache_key, &layer_digest)?;
    layers.push(layer_digest);
    prev_cache_key = cache_key;
}
```

```rust
// src/image.rs - Cache index storage
pub fn get_cached_layer(&self, cache_key: &str) -> Option<String> {
    let index = self.load_cache_index();  // HashMap<String, String>
    index.get(cache_key).cloned()
}

pub fn cache_layer(&self, cache_key: &str, layer_digest: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut index = self.load_cache_index();
    index.insert(cache_key.to_string(), layer_digest.to_string());
    self.save_cache_index(&index)?;
    Ok(())
}
```

### Running Images

Running an image connects the imaging system to the container runtime built in Phases 1-4.

#### Run Process Flow

```
+----------------+     +------------------+     +-------------------+
|  Image Store   | --> |  Extract Layers  | --> |  Container Setup  |
|  (manifest)    |     |  (to rootfs)     |     |  (Phase 1-4)      |
+----------------+     +------------------+     +-------------------+
                                                        |
                                                        v
                                               +-------------------+
                                               |  Execute          |
                                               |  Entrypoint       |
                                               +-------------------+
```

#### Step-by-Step Execution

1. **Load manifest** - Read image metadata to get layer list
2. **Create rootfs** - Make temporary directory for container filesystem
3. **Extract layers** - Unpack each layer tarball in order
4. **Load config** - Read entrypoint, env vars, working directory
5. **Setup container** - Apply namespaces, cgroups, network (Phases 1-4)
6. **Execute entrypoint** - Replace process with application command

### Code - Running Container From Image

```rust
// src/main.rs - Entry point for running an image
fn run_image(image_ref: &str) {
    let store = ImageStore::new(PathBuf::from("/var/lib/forge-container/images"))
        .expect("Failed to open image store");

    // Parse image reference (name:tag)
    let (name, tag) = parse_image_ref(image_ref);

    // Load the manifest
    let manifest = store.load_manifest(&name, &tag)
        .expect("Image not found");

    // Create temporary rootfs and extract layers
    let rootfs_path = format!("/tmp/container-{}", uuid::Uuid::new_v4());
    fs::create_dir_all(&rootfs_path).expect("Failed to create rootfs");

    for layer_digest in &manifest.layers {
        let layer_path = store.get_layer_path(layer_digest);
        Command::new("tar")
            .args(&["-xzf", layer_path.to_str().unwrap(), "-C", &rootfs_path])
            .status()
            .expect("Failed to extract layer");
    }

    // Load configuration
    let config = store.load_config(&name, &tag)
        .expect("Failed to load config");

    // Run the container (connects to Phase 1-4 infrastructure)
    container::run_container_from_image(&rootfs_path, &config, &name);
}
```

```rust
// src/container.rs - Running with image configuration
pub fn run_container_from_image(rootfs_path: &str, config: &ImageConfig, container_name: &str) -> ! {
    // Phase 3: Setup resource limits
    cgroups::setup_cgroups(container_name);

    // Enable IP forwarding for network
    let _ = std::fs::write("/proc/sys/net/ipv4/ip_forward", "1");
    let default_iface = network::get_default_interface_public();

    // Phase 1: Create namespaces (except network, done after fork)
    namespace::create_namespaces_without_network();

    match unsafe { fork() } {
        Ok(ForkResult::Parent { child }) => {
            // Phase 4: Setup networking from parent
            network::setup_veth_pair_with_iface(child.as_raw() as u32, &default_iface);

            let _ = waitpid(child, None);

            // Cleanup
            cgroups::cleanup_cgroup(container_name);
            let _ = fs::remove_dir_all(rootfs_path);

            process::exit(0);
        }
        Ok(ForkResult::Child) => {
            // Phase 4: Create network namespace
            namespace::create_network_namespace();

            // Phase 3: Join cgroup
            cgroups::add_process_to_cgroup(container_name);

            // Phase 2: Setup isolated filesystem (uses pivot_root)
            setup_root_filesystem(rootfs_path);

            // Apply image configuration
            for env_var in &config.env {
                if let Some(pos) = env_var.find('=') {
                    std::env::set_var(&env_var[..pos], &env_var[pos + 1..]);
                }
            }

            if let Err(e) = std::env::set_current_dir(&config.working_dir) {
                warn!("Failed to change to working directory: {}", e);
            }

            // Execute the entrypoint
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
    let program = CString::new(entrypoint[0].as_str()).unwrap();
    let args: Vec<CString> = entrypoint.iter()
        .map(|s| CString::new(s.as_str()).unwrap())
        .collect();

    match execvp(&program, &args) {
        Ok(_) => unreachable!(),
        Err(e) => panic!("Failed to exec entrypoint: {}", e),
    }
}
```

### Integration Diagram

This diagram shows how all phases work together when running an image:

```
                                    IMAGE RUN FLOW
                                    ==============

    +------------------+
    |   User Command   |
    |  forge run app:1 |
    +--------+---------+
             |
             v
    +------------------+
    |   Image Store    |  Load manifest, extract layers
    |   (Phase 5)      |  to /tmp/container-xxxx/
    +--------+---------+
             |
             v
    +------------------+
    |  Setup Cgroups   |  CPU: 50%, Memory: 512MB, PIDs: 100
    |   (Phase 3)      |
    +--------+---------+
             |
             v
    +------------------+
    |  Create NS       |  CLONE_NEWPID | CLONE_NEWNS | CLONE_NEWUTS
    |   (Phase 1)      |
    +--------+---------+
             |
             v
    +------------------+
    |     fork()       |
    +--------+---------+
             |
     +-------+-------+
     |               |
     v               v
  PARENT          CHILD
     |               |
     |               v
     |       +------------------+
     |       | Create Net NS    |  CLONE_NEWNET
     |       |   (Phase 4)      |
     |       +--------+---------+
     |                |
     v                v
+----------+  +------------------+
| Setup    |  | pivot_root       |  Isolate filesystem
| veth     |  |   (Phase 2)      |
| NAT      |  +--------+---------+
| (Ph. 4)  |           |
+----------+           v
     |       +------------------+
     |       | Apply Config     |  ENV, WORKDIR from image
     |       |   (Phase 5)      |
     |       +--------+---------+
     |                |
     |                v
     |       +------------------+
     |       | execvp()         |  ["python3", "app.py"]
     |       | (entrypoint)     |
     |       +------------------+
     |                |
     v                v
  waitpid()     APPLICATION RUNNING
     |          (isolated container)
     v
  cleanup()
```

### Testing Phase 5

#### Building an Image

```bash
# Create a test application directory
mkdir -p test-app
cat > test-app/Forgefile << 'EOF'
FROM alpine:3.19
RUN apk add --no-cache python3
WORKDIR /app
COPY hello.py /app/
ENTRYPOINT ["python3", "hello.py"]
EOF

cat > test-app/hello.py << 'EOF'
print("Hello from containerized Python!")
EOF

# Build the image
./run_container.sh build -f test-app/Forgefile -t myapp:v1.0
```

Expected output:
```
Building image: myapp:v1.0
  FROM alpine:3.19 (downloading...)
  RUN apk add --no-cache python3
  COPY hello.py -> /app/
  Build complete: myapp:v1.0
```

#### Running an Image

```bash
# Run the image
./run_container.sh run myapp:v1.0
```

Expected output:
```
Hello from containerized Python!
```

#### Testing Layer Caching

```bash
# Modify hello.py
echo 'print("Updated message!")' > test-app/hello.py

# Rebuild - should use cached layers for FROM and RUN
./run_container.sh build -f test-app/Forgefile -t myapp:v1.1
```

Expected output:
```
Building image: myapp:v1.1
  FROM alpine:3.19 (cached)
  RUN apk add --no-cache python3 (cached)
  COPY hello.py -> /app/
  Build complete: myapp:v1.1
```

### Key Aspects For Phase 5

- **Content-Addressable Storage** - Layers stored by SHA256 hash enable deduplication, integrity verification, and immutability
- **Layered Filesystem** - Each instruction creates a layer; layers stack to form complete filesystem
- **Build vs. Runtime Security** - `chroot` for controlled build environment, `pivot_root` for untrusted runtime
- **Cache Chain Invalidation** - Change propagates forward; order Forgefile instructions by change frequency
- **Manifest/Config Separation** - Manifest defines layers; config defines runtime behavior
- **Integration with Runtime** - Images provide the "what", Phases 1-4 provide the "how"

### Why Imaging Is Powerful

1. **Reproducibility** - Same image produces identical behavior everywhere
2. **Portability** - Package once, run on any compatible container runtime
3. **Versioning** - Tag images with versions; rollback by running older tag
4. **Efficiency** - Layer sharing reduces storage and network transfer
5. **Isolation** - Application dependencies don't conflict with host or other containers
6. **Speed** - Caching makes rebuilds fast; only changed layers rebuild

### Future Improvements

The current implementation provides core functionality. Production container runtimes like Docker include additional features:

| Feature | Description | Complexity |
|---------|-------------|------------|
| **User Namespaces** | Run container processes as non-root user; map UID/GID to host | Medium |
| **Multi-stage Builds** | Build in one image, copy artifacts to smaller runtime image | Medium |
| **Image Registry** | Push/pull images from remote registry (like Docker Hub) | High |
| **Overlay Filesystem** | Mount layers as overlay instead of extracting; enables copy-on-write | High |
| **Volume Mounts** | Mount host directories into container for persistent data | Medium |
| **Port Mapping** | Map container ports to host ports (`-p 8080:80`) | Medium |
| **Health Checks** | Periodic commands to verify container health | Low |
| **Resource Quotas** | Configurable CPU/memory limits per container | Low |
| **Container Networking** | Multiple containers on same virtual network | High |
| **Seccomp Profiles** | Restrict which syscalls container can make | Medium |
| **Image Signing** | Cryptographically sign images for verification | Medium |
| **Build Arguments** | Pass variables into build process (`ARG` instruction) | Low |
| **CMD Instruction** | Default arguments that can be overridden at runtime | Low |

#### Priority Recommendations

1. **User Namespaces** - Important security feature; prevents container root from being host root
2. **Volume Mounts** - Essential for stateful applications and development workflows
3. **Port Mapping** - Required for running web services
4. **Multi-stage Builds** - Reduces image size significantly for compiled languages

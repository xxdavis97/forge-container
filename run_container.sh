#!/bin/bash
# run_container.sh - Run container runtime in Multipass VM

VM_NAME="ubuntu-vm"
DEBUG_MODE=""
SKIP_BUILD=false

# Parse all flags from anywhere in args
ARGS=()
for arg in "$@"; do
    case "$arg" in
        -d|--debug)
            DEBUG_MODE="RUST_LOG=debug"
            ;;
        -n|--no-build)
            SKIP_BUILD=true
            ;;
        *)
            ARGS+=("$arg")
            ;;
    esac
done
set -- "${ARGS[@]}"

# Show help
show_help() {
    echo "Usage: ./run_container.sh [-d|--debug] [-n|--no-build] [command] [args]"
    echo ""
    echo "Options:"
    echo "  -d, --debug    - Enable verbose debug logging"
    echo "  -n, --no-build - Skip syncing code and building (use existing build)"
    echo ""
    echo "Commands:"
    echo "  run [image]    - Run container (or specific image if provided)"
    echo "  build [args]   - Build an image (e.g., build -f Containerfile -t myapp:v1.0)"
    echo "  fresh          - Delete VM, recreate, and run container"
    echo "  shell          - Open shell in VM"
    echo "  help           - Show this help"
}

# Setup environment (installs rust, build tools, etc.)
setup_environment() {
    echo "📦 Setting up environment..."

    multipass exec $VM_NAME -- bash -c "sudo apt-get update"
    multipass exec $VM_NAME -- bash -c "sudo apt-get install -y build-essential iptables iproute2"
    multipass exec $VM_NAME -- bash -c "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y"
    multipass exec $VM_NAME -- bash -c "source \$HOME/.cargo/env && rustc --version"

    echo "✅ Environment setup complete"
}

# Sync code to VM (excluding .git and build artifacts)
sync_code() {
    echo "📂 Syncing code to VM..."

    # Create tarball excluding .git and target directories
    tar -czf /tmp/container-runtime.tar.gz \
        --exclude='.git' \
        --exclude='target' \
        --exclude='*.tar.gz' \
        .

    # Transfer and extract
    multipass transfer /tmp/container-runtime.tar.gz $VM_NAME:
    multipass exec $VM_NAME -- bash -c "rm -rf container-runtime && mkdir -p container-runtime && tar -xzf container-runtime.tar.gz -C container-runtime"

    # Cleanup
    rm /tmp/container-runtime.tar.gz
}

# Ensure VM is ready (create if needed, start if stopped, sync code, build)
ensure_vm() {
    echo "🔍 Checking VM status..."

    # Check if VM exists and is not in Deleted state
    local vm_state=$(multipass list --format csv | grep "^$VM_NAME," | cut -d',' -f2)

    if [ -z "$vm_state" ]; then
        echo "❌ VM doesn't exist! Creating..."
        multipass launch --name $VM_NAME --cpus 2 --memory 4G --disk 10G
        setup_environment
        sync_code
        echo "✅ VM created and configured"
        vm_state="Running"
        # Force build on new VM
        echo "🔨 Building container runtime..."
        multipass exec $VM_NAME -- bash -c "cd container-runtime && source ~/.cargo/env && cargo build"
        return
    fi

    if [ "$vm_state" != "Running" ]; then
        echo "🚀 Starting VM (state: $vm_state)..."
        multipass start $VM_NAME
        echo "✅ VM started"
    else
        echo "✅ VM already running"
    fi

    if [ "$SKIP_BUILD" = true ]; then
        echo "⏭️  Skipping build (--no-build)"
        return
    fi

    sync_code

    echo "🔨 Building container runtime..."
    multipass exec $VM_NAME -- bash -c "cd container-runtime && source ~/.cargo/env && cargo build"
}

# Run container (optionally with a specific image)
run_container() {
    local image_ref="$1"

    ensure_vm

    if [ -n "$image_ref" ]; then
        echo "🚀 Running image: $image_ref"
        multipass exec $VM_NAME -- bash -c "cd container-runtime && source ~/.cargo/env && sudo $DEBUG_MODE ./target/debug/container-runtime run $image_ref"
    else
        echo "🚀 Running container..."
        multipass exec $VM_NAME -- bash -c "cd container-runtime && source ~/.cargo/env && sudo $DEBUG_MODE ./target/debug/container-runtime"
    fi
}

# Build an image
build_image() {
    ensure_vm

    echo "🏗️  Building image..."
    multipass exec $VM_NAME -- bash -c "cd container-runtime && source ~/.cargo/env && sudo $DEBUG_MODE ./target/debug/container-runtime build $*"
}

# Fresh VM workflow
fresh_vm() {
    echo "🗑️  Deleting existing VM..."

    # Try to delete, show errors if any
    if multipass list | grep -q "^$VM_NAME"; then
        multipass delete --force --purge $VM_NAME || {
            echo "⚠️  Force delete failed, trying stop first..."
            multipass stop $VM_NAME 2>/dev/null
            multipass delete --purge $VM_NAME 2>/dev/null
            multipass purge 2>/dev/null
        }
    fi

    # Verify it's gone
    if multipass list 2>/dev/null | grep -q "^$VM_NAME"; then
        echo "❌ Failed to delete VM. Try manually: multipass delete --force --purge $VM_NAME"
        exit 1
    fi

    echo "✅ VM cleaned"
    echo ""

    echo "🆕 Creating fresh VM..."
    multipass launch --name $VM_NAME --cpus 2 --memory 4G --disk 10G
    setup_environment
    sync_code
    echo "✅ Fresh VM ready"

    echo "🔨 Building container runtime..."
    multipass exec $VM_NAME -- bash -c "cd container-runtime && source ~/.cargo/env && cargo build"

    echo "🚀 Running container..."
    multipass exec $VM_NAME -- bash -c "cd container-runtime && source ~/.cargo/env && sudo $DEBUG_MODE ./target/debug/container-runtime"
}

# Open shell in VM
open_shell() {
    echo "🐚 Opening shell in VM..."
    multipass shell $VM_NAME
}

# Main
case "${1:-run}" in
    run) run_container "$2" ;;
    build) shift; build_image "$@" ;;
    fresh) fresh_vm ;;
    shell) open_shell ;;
    help|--help|-h) show_help ;;
    *) echo "❌ Unknown: $1"; show_help; exit 1 ;;
esac

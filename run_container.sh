#!/bin/bash
# run-container.sh - Run container runtime in Multipass VM

VM_NAME="ubuntu-vm"

# Show help
show_help() {
    echo "Usage: ./run-container.sh [command]"
    echo ""
    echo "Commands:"
    echo "  run       - Run container in VM (default)"
    echo "  fresh     - Delete VM, recreate, and run container"
    echo "  shell     - Open shell in VM"
    echo "  help      - Show this help"
}

# Setup environment
setup_environment() {
    local exec_cmd="$1"
    
    echo "📦 Setting up environment..."
    
    $exec_cmd bash -c "sudo apt-get update"
    $exec_cmd bash -c "sudo apt-get install -y build-essential iptables iproute2"
    $exec_cmd bash -c "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y"
    $exec_cmd bash -c "source \$HOME/.cargo/env && rustc --version"
    
    echo "✅ Environment setup complete"
}

# Fresh VM workflow
fresh_vm() {
    echo "🗑️  Deleting existing VM..."
    multipass delete $VM_NAME 2>/dev/null || true
    multipass purge
    echo "✅ VM cleaned"
    echo ""
    run_container
}

# Open shell in VM
open_shell() {
    echo "🐚 Opening shell in VM..."
    multipass shell $VM_NAME
}

# Run container
run_container() {
    echo "🔍 Checking VM status..."

    if ! multipass list | grep -q "$VM_NAME"; then
        echo "❌ VM doesn't exist! Creating..."
        multipass launch --name $VM_NAME --cpus 2 --memory 4G --disk 10G
        setup_environment "multipass exec $VM_NAME --"
        multipass transfer -r . $VM_NAME:container-runtime
        echo "✅ VM created and configured"
    fi

    if ! multipass list | grep "$VM_NAME" | grep -q "Running"; then
        echo "🚀 Starting VM..."
        multipass start $VM_NAME
        echo "✅ VM started"
    else
        echo "✅ VM already running"
    fi

    echo "📂 Syncing code to VM..."
    multipass transfer -r . $VM_NAME:container-runtime

    echo "🔨 Building container runtime..."
    multipass exec $VM_NAME -- bash -c "cd container-runtime && source ~/.cargo/env && cargo build"

    echo "🚀 Running container..."
    multipass exec $VM_NAME -- bash -c "cd container-runtime && source ~/.cargo/env && sudo ./target/debug/container-runtime"
}

# Main
case "${1:-run}" in
    run) run_container ;;
    fresh) fresh_vm ;;
    shell) open_shell ;;
    help|--help|-h) show_help ;;
    *) echo "❌ Unknown: $1"; show_help; exit 1 ;;
esac
#!/usr/bin/env bash

# ==============================================================================
# Hayate Performance Benchmarking Suite
# macOS Apple Silicon Host <-> Container Linux VM (Apple Virtualization Framework)
# ==============================================================================

set -euo pipefail

# Configuration
RAMDISK_SIZE_SECTORS=4194304 # 2GB (4194304 * 512 bytes)
RAMDISK_NAME="HayateRAMDisk"
RAMDISK_MOUNT="/Volumes/${RAMDISK_NAME}"
TEST_FILE_SIZE_MB=256       # 256MB test payload
PORT=50001
IMAGE_NAME="hayate-bench"

# Colors for output formatting
NC='\033*0m'
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BLUE='\033[0;34m'
BOLD='\033[1m'

# Override colors to standard text if NC or TERM is not supported
if [ ! -t 1 ]; then
    NC='' RED='' GREEN='' YELLOW='' BLUE='' BOLD=''
fi

log_info() {
    printf "${BLUE}${BOLD}[INFO]${NC} %s\n" "$1"
}

log_success() {
    printf "${GREEN}${BOLD}[SUCCESS]${NC} %s\n" "$1"
}

log_warn() {
    printf "${YELLOW}${BOLD}[WARNING]${NC} %s\n" "$1"
}

log_error() {
    printf "${RED}${BOLD}[ERROR]${NC} %s\n" "$1"
}

get_container_ip() {
    local name="$1"
    container ls | grep "$name" | awk '{print $6}' | cut -d/ -f1
}

# Cleanup hook
cleanup() {
    log_info "Cleaning up resources..."
    
    # Stop containers
    for container_id in "hayate-iperf-server" "hayate-bench-recv"; do
        if container ls | grep -q "$container_id"; then
            log_info "Stopping container: $container_id"
            container stop "$container_id" >/dev/null 2>&1 || true
        fi
    done
    
    # Detach RAM disk
    if [ -d "$RAMDISK_MOUNT" ]; then
        log_info "Detaching macOS RAM disk..."
        hdiutil detach "$RAMDISK_MOUNT" -force >/dev/null 2>&1 || true
    fi
}
trap cleanup EXIT

# 1. Dependency Checks
log_info "Starting environment validation..."

if ! command -v container &> /dev/null; then
    log_error "The 'container' command-line tool is not installed."
    exit 1
fi

if ! container system start &> /dev/null; then
    log_warn "Starting container service..."
    container system start
fi

# Ensure host release binary exists
HOST_BINARY="target/aarch64-apple-darwin/release/hayate"
if [ ! -f "$HOST_BINARY" ]; then
    HOST_BINARY="target/release/hayate"
fi
if [ ! -f "$HOST_BINARY" ]; then
    log_info "Compiling host binary in release mode..."
    cargo build --release --workspace
    HOST_BINARY="target/aarch64-apple-darwin/release/hayate"
    if [ ! -f "$HOST_BINARY" ]; then
        HOST_BINARY="target/release/hayate"
    fi
fi

# Build Linux VM benchmark image if missing
if ! container image list | grep -q "$IMAGE_NAME"; then
    log_info "Building Linux VM container benchmark image: $IMAGE_NAME"
    
    # 1. Compile Linux binary using cargo-zigbuild
    LINUX_BINARY="target/aarch64-unknown-linux-musl/release/hayate"
    if [ ! -f "$LINUX_BINARY" ]; then
        log_info "Cross-compiling Linux binary (musl) using cargo-zigbuild..."
        if ! command -v cargo-zigbuild &> /dev/null; then
            log_info "cargo-zigbuild not found, installing via Homebrew..."
            brew install cargo-zigbuild
        fi
        rustup target add aarch64-unknown-linux-musl || true
        cargo zigbuild --release --target aarch64-unknown-linux-musl --bin hayate
    fi
    
    # 2. Setup target-bench folder and copy binary
    mkdir -p target-bench
    cp "$LINUX_BINARY" target-bench/hayate
    
    # 3. Run container build with target-bench context
    container build --tag "$IMAGE_NAME" --file target-bench/Dockerfile target-bench/
fi

# 2. Baseline Network Calibration (iperf3)
log_info "=================================================================="
log_info "Phase 1: Baseline Network Calibration (iperf3)"
log_info "=================================================================="

# Start iperf3 server in container
container run --name hayate-iperf-server -d --rm --entrypoint iperf3 "$IMAGE_NAME" -s > /dev/null
sleep 2

# Resolve container IP
CONTAINER_IP=$(get_container_ip "hayate-iperf-server")
log_info "Resolved container IP: ${CONTAINER_IP}"

if command -v iperf3 &> /dev/null; then
    log_info "Running TCP baseline test (Host -> VM)..."
    iperf3 -c "$CONTAINER_IP" -t 5
    
    log_info "Running UDP baseline calibration (Host -> VM)..."
    iperf3 -c "$CONTAINER_IP" -u -b 10G -p 5201 -t 5 --get-server-output || log_warn "UDP calibration experienced packet loss."
else
    log_warn "iperf3 is not installed on macOS host. Skipping client run."
    log_warn "Install via: brew install iperf3"
fi

container stop "hayate-iperf-server" >/dev/null
sleep 1

# 3. RAM-to-RAM Isolation Test Setup
log_info "=================================================================="
log_info "Phase 2: Zero-Disk Isolation Setup (RAM Disk)"
log_info "=================================================================="

# Mount macOS RAM disk
log_info "Mounting 2GB RAM disk on macOS host..."
DEVICE_PATH=$(hdiutil attach -nomount "ram://${RAMDISK_SIZE_SECTORS}" | tr -d '[:space:]')
diskutil erasevolume HFS+ "$RAMDISK_NAME" "$DEVICE_PATH" > /dev/null
log_success "RAM disk mounted at ${RAMDISK_MOUNT}"

# Generate random file to avoid compression shortcut
log_info "Generating 1GB perfectly random file in macOS RAM disk..."
dd if=/dev/urandom of="${RAMDISK_MOUNT}/random.bin" bs=1M count="$TEST_FILE_SIZE_MB" status=none
log_success "Generated 1GB payload at ${RAMDISK_MOUNT}/random.bin"

# 4. Benchmarking Payload Transfer (AES-GCM Preferred)
log_info "=================================================================="
log_info "Phase 3: Hardware Cryptography Validation (AES-256-GCM vs ChaCha20)"
log_info "=================================================================="

# Test A: Default (Hardware AES-256-GCM if supported, otherwise ChaCha20)
log_info "Running Test A: Default Hardware Acceleration Mode..."
container run --name hayate-bench-recv --tmpfs /ramdisk -d --rm "$IMAGE_NAME" receive --bind 0.0.0.0 --port "$PORT" --output /ramdisk --auto-accept --no-progress > /dev/null
sleep 2

RECV_IP=$(get_container_ip "hayate-bench-recv")

# Capture transfer timing
start_time=$(date +%s.%N)
"$HOST_BINARY" send "${RAMDISK_MOUNT}/random.bin" "${RECV_IP}:${PORT}" --no-progress
end_time=$(date +%s.%N)

duration_aes=$(echo "$end_time - $start_time" | bc)
throughput_aes=$(echo "scale=2; $TEST_FILE_SIZE_MB * 8 / $duration_aes" | bc)
log_success "Test A Completed. Time: ${duration_aes}s, Throughput: ${throughput_aes} Mbps"

container stop "hayate-bench-recv" >/dev/null || true
sleep 2

# Test B: Forced ChaCha20 (Software-Only Mode)
log_info "Running Test B: Forced ChaCha20 Software-Only Mode..."
container run --name hayate-bench-recv --tmpfs /ramdisk -e HAYATE_FORCE_CHACHA20=1 -d --rm "$IMAGE_NAME" receive --bind 0.0.0.0 --port "$PORT" --output /ramdisk --auto-accept --no-progress > /dev/null
sleep 2

RECV_IP=$(get_container_ip "hayate-bench-recv")

start_time=$(date +%s.%N)
HAYATE_FORCE_CHACHA20=1 "$HOST_BINARY" send "${RAMDISK_MOUNT}/random.bin" "${RECV_IP}:${PORT}" --no-progress
end_time=$(date +%s.%N)

duration_chacha=$(echo "$end_time - $start_time" | bc)
throughput_chacha=$(echo "scale=2; $TEST_FILE_SIZE_MB * 8 / $duration_chacha" | bc)
log_success "Test B Completed. Time: ${duration_chacha}s, Throughput: ${throughput_chacha} Mbps"

container stop "hayate-bench-recv" >/dev/null || true
sleep 2

# Comparison Analysis
delta_pct=$(echo "scale=2; (($duration_chacha - $duration_aes) / $duration_aes) * 100" | bc)
log_info "--- Cryptography Comparison Analysis ---"
log_info "AES-256-GCM: ${throughput_aes} Mbps (${duration_aes}s)"
log_info "ChaCha20-Poly1305: ${throughput_chacha} Mbps (${duration_chacha}s)"
log_info "AES speedup over ChaCha20: ${delta_pct}%"

# 5. Kernel Thrashing Audit (strace)
log_info "=================================================================="
log_info "Phase 4: Kernel System Call Tracing (strace)"
log_info "=================================================================="

log_info "Launching container with strace enabled..."
# Running strace with CAP_SYS_PTRACE to inspect system call overheads
container run --name hayate-bench-recv --tmpfs /ramdisk --cap-add SYS_PTRACE -d --rm --entrypoint strace "$IMAGE_NAME" -c hayate receive --bind 0.0.0.0 --port "$PORT" --output /ramdisk --auto-accept --no-progress > /dev/null
sleep 2

RECV_IP=$(get_container_ip "hayate-bench-recv")

log_info "Initiating transfer for profiling..."
"$HOST_BINARY" send "${RAMDISK_MOUNT}/random.bin" "${RECV_IP}:${PORT}" --no-progress

log_info "Stopping receiver to fetch strace metrics..."
# Stop container cleanly so strace prints summary report to stderr logs
container stop "hayate-bench-recv" >/dev/null || true
sleep 2

log_info "Strace System Call Profile:"
container logs "hayate-bench-recv" || log_warn "Could not retrieve strace output logs."

# macOS Host DTrace instructions
log_info "------------------------------------------------------------------"
log_info "To trace system calls on the macOS Host (Sender), run:"
log_info "sudo dtrace -n 'syscall:::entry /execname == \"hayate\"/ { @[probefunc] = count(); }'"
log_info "------------------------------------------------------------------"

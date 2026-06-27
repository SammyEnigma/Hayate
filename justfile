set shell := ["sh", "-cu"]

default:
    @just --list

fmt:
    cargo fmt

fmt-check:
    cargo fmt -- --check

clippy:
    cargo clippy --workspace --all-targets -- -D warnings

test:
    cargo test --workspace

check: fmt-check clippy test

build target="hayate":
    cargo build --release -p hayate-cli

# Build release binary with dist profile (LTO=thin, smaller binary)
build-dist:
    cargo build --profile dist -p hayate-cli

# Cross-compile for all supported targets
build-all:
    cargo build --profile dist --target x86_64-apple-darwin -p hayate-cli
    cargo build --profile dist --target aarch64-apple-darwin -p hayate-cli
    cargo build --profile dist --target x86_64-unknown-linux-gnu -p hayate-cli
    cargo build --profile dist --target x86_64-unknown-linux-musl -p hayate-cli

# Windows cross-compile (requires mingw or MSVC toolchain)
build-windows:
    cargo build --profile dist --target x86_64-pc-windows-msvc -p hayate-cli

android-aarch64:
    rustup target add aarch64-linux-android
    CC="{{justfile_directory()}}/scripts/aarch64-linux-android-clang" \
    CXX="{{justfile_directory()}}/scripts/aarch64-linux-android-clang++" \
    AR="{{justfile_directory()}}/scripts/android-llvm-ar" \
    CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="{{justfile_directory()}}/scripts/aarch64-linux-android-clang" \
    cargo build --target aarch64-linux-android --release

android-x86_64:
    rustup target add x86_64-linux-android
    CC="{{justfile_directory()}}/scripts/x86_64-linux-android-clang" \
    CXX="{{justfile_directory()}}/scripts/x86_64-linux-android-clang++" \
    AR="{{justfile_directory()}}/scripts/android-llvm-ar" \
    CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER="{{justfile_directory()}}/scripts/x86_64-linux-android-clang" \
    cargo build --target x86_64-linux-android --release

android-all: android-aarch64 android-x86_64

run *args:
    cargo run -p hayate-cli -- {{args}}

receive port="50001" output=".":
    cargo run -p hayate-cli -- receive --port "{{port}}" --output "{{output}}"

send file peer:
    cargo run -p hayate-cli -- send "{{file}}" --peer "{{peer}}"

discover timeout="5":
    cargo run -p hayate-cli -- discover --timeout "{{timeout}}"

clean:
    cargo clean

# --- cargo-dist integration ---

# Initialise cargo-dist in the workspace (first-time setup)
dist-init:
    cargo dist init

# Build release artefacts with cargo-dist (local simulation)
dist-build:
    cargo dist build --local

# Generate CI workflow and installer scripts from dist config
dist-generate-ci:
    cargo dist generate-ci

# Build installers locally (shell + powershell scripts)
dist-installers:
    cargo dist build --installer=shell --installer=powershell --local

# --- winget submission ---

# Copy winget manifests to winget-pkgs repo for PR
winget-submit version="5.0.0" sha_x64="" sha_arm64="":
    @echo "1. Fork https://github.com/microsoft/winget-pkgs"
    @echo "2. Update winget/manifests/s/ShiinaSaku/Hayate/{{version}}/ with SHAs:"
    @echo "   x64:  {{sha_x64}}"
    @echo "   arm64: {{sha_arm64}}"
    @echo "3. Copy to winget-pkgs:"
    @echo "   cp -r winget/manifests/* ../winget-pkgs/manifests/"
    @echo "4. PR to microsoft/winget-pkgs"

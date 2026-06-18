# ⚡️ Hayate

[![CI](https://github.com/ShiinaSaku/Hayate/actions/workflows/ci.yml/badge.svg)](https://github.com/ShiinaSaku/Hayate/actions/workflows/ci.yml)
[![Builds](https://github.com/ShiinaSaku/Hayate/actions/workflows/builds.yml/badge.svg)](https://github.com/ShiinaSaku/Hayate/actions/workflows/builds.yml)
[![Rust](https://img.shields.io/badge/rust-1.95%2B-orange?logo=rust)](Cargo.toml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/ShiinaSaku/Hayate?include_prereleases&sort=semver)](https://github.com/ShiinaSaku/Hayate/releases)

Hayate is a **blazing-fast, completion-based CLI** for secure file and directory transfers across local networks. Built on QUIC and `io_uring` (via `compio`), it saturates network links while keeping your data encrypted and authenticated.

```text
  __   __     _____    __  __    _____    _______     _____
 /\_\ /_/\   /\___/\ /\  /\  /\ /\___/\ /\_______)\ /\_____\
( ( (_) ) ) / / _ \ \\ \ \/ / // / _ \ \\(___  __\/( (_____/
 \ \___/ /  \ \(_)/ / \ \__/ / \ \(_)/ /  / / /     \ \__\
 / / _ \ \  / / _ \ \  \__/ /  / / _ \ \ ( ( (      / /__/_
( (_( )_) )( (_( )_) ) / / /  ( (_( )_) ) \ \ \    ( (_____\
 \/_/ \_\/  \/_/ \_\/  \/_/    \/_/ \_\/  /_/_/     \/_____/
```

## ✨ Features

- **🚀 Extreme Performance:** Powered by `compio` (`io_uring`/IOCP), a multi-threaded AEAD/Zstd worker pool, and `compio-quic`.
- **🔒 Robust Security:** Ephemeral X25519 key agreement, HKDF key derivation, and ChaCha20-Poly1305 / AES-GCM frame encryption.
- **🛡️ Integrity:** Dynamic, ultra-fast streaming payload verification (`blake3`, `rapidhash`, `sha256`).
- **📦 Seamless Directories:** Zero-overhead tar streaming with strict path-traversal protections.
- **📡 Auto Discovery:** Pair devices via broadcast phrases—no IP typing required.
- **🗜️ Smart Compression:** `zstd` compression automatically skips already-compressed files (videos, archives, images) to save CPU.
- **💻 Polished UX:** Interactive terminal prompts, progress bars with ETAs, and interactive output path selection.

## 🚦 Quick Start

### 🤝 Pairing Mode (No IP Required)
Share a phrase to let Hayate auto-discover the peer over the LAN.

```bash
# Receiver
hayate receive --code "apple-bravo-charlie"

# Sender
hayate send ./photos.zip --code "apple-bravo-charlie"
```

### 🎯 Direct Mode
Connect directly via IP and Port. Best for restrictive networks, VPNs, or Termux (Android).

```bash
# Receiver
hayate receive --port 50001

# Sender
hayate send ./archive.tar 192.168.1.50:50001
```

## 🛠️ Commands

### `hayate receive`
Waits for an incoming transfer. Features interactive `[y/N]` confirmation and destination selection.

```text
Options:
  -b, --bind <BIND>      IP address to bind [default: 0.0.0.0]
  -p, --port <PORT>      Port to listen on [default: 50001]
  -o, --output <OUTPUT>  Default directory for received files [default: .]
      --auto-accept      Accept transfers without prompting
      --code <CODE>      Pairing code phrase
      --no-tui           Disable progress UI
```

### `hayate send`
Transfers a file or directory.

```text
Usage: hayate send [OPTIONS] <PATH> [TARGET]

Options:
      --peer <PEER>      Receiver address (equivalent to TARGET)
      --code <CODE>      Pairing code phrase
  -z, --compress         Compress chunks before encryption (default: true)
      --hash <ALGO>      Integrity algorithm: blake3, rapidhash, sha256 [default: blake3]
      --no-tui           Disable progress UI
```

### `hayate discover`
Scan the subnet for active Hayate receivers.

```text
Options:
  -t, --timeout <TIMEOUT>  Scan timeout in seconds [default: 3]
      --cidr <CIDR>        Override subnet CIDR, e.g. 192.168.1.0/24
```

## 🔐 Security Model

Hayate uses QUIC TLS 1.3 for transport encryption via ephemeral self-signed certificates. Trust is established completely at the application layer:

1. **Handshake:** Ephemeral X25519 key agreement per transfer.
2. **Derivation:** HKDF extracts a shared AEAD key. If a `--code` is provided, it acts as the HKDF salt.
3. **Metadata:** Filename, size, and hash algorithm are encrypted before the receiver prompt.
4. **Payload:** Chunks are length-capped and authenticated via AEAD before decompression or writing.
5. **Path Safety:** Directory extraction rejects absolute paths, `..`, symlinks, and hard links.

*Direct mode without a code relies solely on network locality. For untrusted environments, always use a `--code` phrase.*

## 📱 Termux & Mobile

Android restricts UDP broadcasts. Use **Direct Mode** on mobile:

```bash
# Phone
./hayate receive --port 50002

# Computer
hayate send ./document.pdf 192.168.1.13:50002
```

## 📥 Installation

**macOS, Linux, and Termux:**
```bash
curl -sSf https://raw.githubusercontent.com/ShiinaSaku/Hayate/refs/heads/master/scripts/install.sh | bash
```

**Windows PowerShell:**
```powershell
irm https://raw.githubusercontent.com/ShiinaSaku/Hayate/refs/heads/master/scripts/install.ps1 | iex
```

Pre-compiled binaries are available on the [releases page](https://github.com/ShiinaSaku/Hayate/releases).

## 🏗️ Build From Source

**Requirements:** Rust `1.95+`

```bash
git clone https://github.com/ShiinaSaku/Hayate.git
cd Hayate
cargo build --release -p hayate-cli
```

*(Optional)* `just` runner workflows:
```bash
just check
just build
just run -- --help
```

## 📦 Workspace Architecture

- `hayate-engine`: The standalone, reusable completion-based Rust library published as `hayate`.
- `hayate-cli`: The CLI application wrapper.

## 🤝 Acknowledgements

Hayate stands on the shoulders of giants: `compio`, `quinn-proto`, `rustls`, `ring`, `blake3`, `rapidhash`, `dialoguer`, `clap`, `indicatif`, and `zstd-rs`.

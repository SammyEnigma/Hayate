# Hayate

[![CI](https://github.com/ShiinaSaku/Hayate/actions/workflows/ci.yml/badge.svg)](https://github.com/ShiinaSaku/Hayate/actions/workflows/ci.yml)
[![Builds](https://github.com/ShiinaSaku/Hayate/actions/workflows/builds.yml/badge.svg)](https://github.com/ShiinaSaku/Hayate/actions/workflows/builds.yml)
[![Rust](https://img.shields.io/badge/rust-1.95%2B-orange?logo=rust)](Cargo.toml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/ShiinaSaku/Hayate?include_prereleases&sort=semver)](https://github.com/ShiinaSaku/Hayate/releases)
[![CodSpeed](https://img.shields.io/endpoint?url=https://codspeed.io/badge.json)](https://app.codspeed.io/ShiinaSaku/Hayate?utm_source=badge)

Hayate is a high-performance CLI for encrypted file and directory transfer across local networks. It uses QUIC over `compio-quic`, completion-based I/O through `compio`, an application-layer X25519/HKDF/AEAD handshake, optional zstd compression, and safe tar streaming for directories.

```text
  __   __     _____    __  __    _____    _______     _____
 /\_\ /_/\   /\___/\ /\  /\  /\ /\___/\ /\_______)\ /\_____\
( ( (_) ) ) / / _ \ \\ \ \/ / // / _ \ \\(___  __\/( (_____/
 \ \___/ /  \ \(_)/ / \ \__/ / \ \(_)/ /  / / /     \ \__\
 / / _ \ \  / / _ \ \  \__/ /  / / _ \ \ ( ( (      / /__/_
( (_( )_) )( (_( )_) ) / / /  ( (_( )_) ) \ \ \    ( (_____\
 \/_/ \_\/  \/_/ \_\/  \/_/    \/_/ \_\/  /_/_/     \/_____/
```

## Features

- Direct file and directory transfer over QUIC.
- Pairing-code discovery for LAN workflows where users do not want to exchange IP addresses.
- Application-layer encryption for metadata and payload frames using ephemeral X25519 key agreement and AEAD ciphers.
- Optional zstd level 1 compression, skipped automatically for common already-compressed formats.
- Progress output with transfer rate, elapsed time, ETA, and headless-friendly `--no-progress`.
- Safe directory extraction that rejects absolute paths, `..`, symlinks, and hard links.
- Release binaries for Linux, macOS, Windows, and Android/Termux.

## Quick Start

Pairing mode lets both sides use a shared phrase:

```bash
# Receiver
hayate receive --code "apple-bravo-charlie" --output ~/Downloads

# Sender
hayate send ./photos.zip --code "apple-bravo-charlie"
```

Direct mode skips discovery and connects to a known receiver address:

```bash
# Receiver
hayate receive --port 50001

# Sender
hayate send ./archive.tar 192.168.1.50:50001
```

You can also use `--peer` instead of positional `TARGET`:

```bash
hayate send ./archive.tar --peer 192.168.1.50:50001
```

## Commands

```text
hayate receive [OPTIONS]

Options:
  -b, --bind <BIND>      IP address to bind [env: HAYATE_BIND=] [default: 0.0.0.0]
  -p, --port <PORT>      Port to listen on [env: HAYATE_PORT=] [default: 50001]
  -o, --output <OUTPUT>  Directory for received files [default: .]
      --auto-accept      Accept transfers without prompting
      --no-progress      Disable progress UI
      --code <CODE>      Pairing code phrase
```

```text
hayate send [OPTIONS] <PATH> [TARGET]

Options:
      --peer <PEER>      Receiver address, equivalent to TARGET
      --code <CODE>      Pairing code phrase
  -z, --compress         Compress chunks before encryption when beneficial
      --no-progress      Disable progress UI
```

```text
hayate discover [OPTIONS]

Options:
  -t, --timeout <TIMEOUT>  Scan timeout in seconds [default: 3]
      --cidr <CIDR>        Override subnet CIDR, e.g. 192.168.1.0/24
```

`TARGET` and `--peer` are mutually exclusive. If neither is supplied, `hayate send` generates a pairing phrase and waits for a receiver using that phrase.

## Security Model

Hayate uses QUIC TLS 1.3 for transport encryption, but its TLS certificates are self-signed and ephemeral for zero-configuration LAN use. Peer authentication therefore comes from Hayate's application-layer handshake.

- The sender and receiver perform ephemeral X25519 key agreement per transfer.
- A shared AEAD key is derived with HKDF-SHA256.
- Pairing phrases are used as HKDF salt when present.
- Metadata is encrypted before filenames, sizes, or transfer type are exposed.
- Payload frames are length-capped and authenticated before decompression or filesystem writes.
- Receivers can reject a transfer after decrypting metadata and before accepting payload bytes.

Direct mode without a shared code phrase does not authenticate peer identity beyond the QUIC connection. Use pairing mode with a strong phrase when the local network is not trusted.

## Termux

Android and mobile networks often restrict broadcast discovery. Direct mode is usually more reliable:

```bash
# Phone
./hayate receive --port 50002 --auto-accept --no-progress

# Computer
hayate send ./document.pdf --peer 192.168.1.13:50002
```

## Installation

macOS, Linux, and Termux:

```bash
curl -sSf https://raw.githubusercontent.com/ShiinaSaku/Hayate/refs/heads/master/scripts/install.sh | bash
```

Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/ShiinaSaku/Hayate/refs/heads/master/scripts/install.ps1 | iex
```

Manual binaries are available on the [releases page](https://github.com/ShiinaSaku/Hayate/releases).

## Build From Source

Requirements:

- Rust 1.95 or newer.
- `just` is optional.

```bash
git clone https://github.com/ShiinaSaku/Hayate.git
cd Hayate
cargo build --release -p hayate-cli
```

Useful development commands:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

With `just`:

```bash
just check
just build
just run -- --help
```

## Workspace

- `hayate-engine`: reusable Rust library published as `hayate`.
- `hayate-cli`: command-line application binary named `hayate`.

The engine expects public async APIs to run inside a `compio` runtime, usually via `#[compio::main]` or `compio::runtime::Runtime::new()?.block_on(...)`.

## Acknowledgements

Hayate builds on `compio`, `compio-quic`, `quinn-proto`, `rustls`, `ring`, `x25519-dalek`, RustCrypto AEAD crates, `clap`, `indicatif`, `zstd-rs`, and `rcgen`.

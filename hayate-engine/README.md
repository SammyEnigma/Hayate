# Hayate Engine

[![Crates.io](https://img.shields.io/crates/v/hayate.svg)](https://crates.io/crates/hayate)
[![Documentation](https://docs.rs/hayate/badge.svg)](https://docs.rs/hayate)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](../LICENSE)

`hayate` is the reusable transfer engine behind the Hayate CLI. It provides high-level sender and receiver builders for encrypted file and directory transfer over QUIC, plus lower-level modules for applications that need direct protocol control.

The engine uses `compio` for completion-based async I/O, `compio-quic`/`quinn-proto` for QUIC, ephemeral X25519 key agreement, HKDF-SHA256 key derivation, AEAD-encrypted metadata and payload frames, optional zstd compression, and safe tar extraction for directories.

## Installation

```toml
[dependencies]
hayate = "2.1"
compio = { version = "0.19", features = ["macros", "runtime", "fs", "net", "time"] }
```

Public async examples are intended to run inside a `compio` runtime.

## Receive

```rust
use std::net::SocketAddr;
use hayate::HayateReceiver;

#[compio::main]
async fn main() -> Result<(), hayate::EngineError> {
    let bind_addr: SocketAddr = "0.0.0.0:50001".parse().unwrap();

    let receiver = HayateReceiver::new().bind(bind_addr);
    let (checksum, path) = receiver.receive(
        "./downloads",
        |meta| {
            println!("incoming {} ({} bytes)", meta.filename, meta.total_size);
            true
        },
        |bytes| println!("received {bytes} bytes"),
    ).await?;

    println!("saved to {}", path.display());
    println!("sha256 {checksum}");
    Ok(())
}
```

## Send

```rust
use std::net::SocketAddr;
use hayate::HayateSender;

#[compio::main]
async fn main() -> Result<(), hayate::EngineError> {
    let target: SocketAddr = "192.168.1.50:50001".parse().unwrap();

    let checksum = HayateSender::new()
        .target(target)
        .compress(true)
        .send("photos", |bytes| println!("sent {bytes} bytes"))
        .await?;

    println!("sha256 {checksum}");
    Ok(())
}
```

## Pairing Mode

Pairing mode discovers the peer through LAN broadcast and uses the same phrase in key derivation:

```rust
use hayate::{HayateReceiver, HayateSender};

# async fn sender() -> Result<(), hayate::EngineError> {
HayateSender::new()
    .code("apple-bravo-charlie".to_owned())
    .send("report.pdf", |_| {})
    .await?;
# Ok(())
# }

# async fn receiver() -> Result<(), hayate::EngineError> {
let (_checksum, _path) = HayateReceiver::new()
    .code("apple-bravo-charlie".to_owned())
    .auto_accept(true)
    .receive("./downloads", |_| true, |_| {})
    .await?;
# Ok(())
# }
```

Some networks, VPNs, mobile hotspots, and Android devices block broadcast traffic. Use direct mode when discovery is unreliable.

## Module Map

| Module | Purpose |
| --- | --- |
| `runner` | Builder-style APIs: `HayateSender` and `HayateReceiver`. |
| `transfer` | Handshake, consent, payload send, and payload receive pipeline. |
| `protocol` | Wire constants, frame flags, metadata encoding, and validation. |
| `crypto` | X25519, HKDF, AEAD sealing/opening, and cipher selection. |
| `network` | QUIC endpoint binding, transport config, and ephemeral TLS config. |
| `discovery` | Pairing-code broadcast and listener utilities. |
| `tar` | Directory packaging, safe extraction, and directory size estimation. |
| `local_addr` | Local IPv4 and subnet helpers. |
| `error` | Shared `EngineError` type. |

## Runtime Notes

`compio` is completion-based: I/O buffers are owned by the operation until completion and returned through `compio::BufResult`. `compio::fs::File` operations are positional and do not maintain an internal cursor, so Hayate tracks offsets explicitly for file reads and writes.

The runtime is thread-local. Hayate keeps blocking tar work and CPU-heavy compression/encryption off the compio executor with dedicated worker threads while preserving ordered stream writes.

## Safety

Hayate treats wire data as untrusted:

- Metadata is AEAD-authenticated before use.
- Unknown transfer types and oversized frames are rejected.
- File transfers must finish with exactly the announced byte count.
- Directory payloads are tar streams extracted under a caller-selected output root.
- Absolute paths, parent traversal, symlinks, and hard links are rejected.

Progress callbacks report plaintext payload bytes. Returned checksums are hex-encoded SHA-256 digests of the plaintext payload stream.

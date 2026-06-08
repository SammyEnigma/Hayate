# Hayate Engine (はやて)

[![Crates.io](https://img.shields.io/crates/v/hayate.svg)](https://crates.io/crates/hayate)
[![Documentation](https://docs.rs/hayate/badge.svg)](https://docs.rs/hayate)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

An encrypted, compressed, blazing-fast completion-based asynchronous file and directory transfer engine built on **QUIC** and `compio`.

This crate is the core transport library of the [Hayate CLI](https://github.com/ShiinaSaku/Hayate). It is designed to saturate high-speed local networks (Wi-Fi, Ethernet) while keeping CPU usage minimal through a completion-based (proactor) Thread-Per-Core architecture.

---

## ✦ Features

* **Proactor Thread-Per-Core Asynchronous Engine**: Built on `compio` (`io_uring` on Linux/Android, `IOCP` on Windows, and `kqueue` on macOS) for zero-copy completion-based I/O.
* **Authenticated Encryption**: Ephemeral `X25519` key exchange (DH) and `ChaCha20-Poly1305` or `AES-256-GCM` payload encryption (with hardware acceleration).
* **Smart Compression**: Concurrent `zstd` level 1 compression that automatically skips pre-compressed file extensions (e.g., `.zip`, `.mp4`, `.png`).
* **Safe Extraction**: Automatic directory streaming using tar archives with robust path-traversal protection.
* **Zero-Config Pairing**: Easy peer discovery over local subnets using random code phrase broadcasters.

---

## ✦ Quick Start

Add `hayate` and `compio` to your `Cargo.toml`:

```toml
[dependencies]
hayate = "2.0"
compio = { version = "0.19", features = ["macros", "runtime"] }
```

### 1. Sending a File or Directory

```rust
use std::net::SocketAddr;
use hayate::runner::HayateSender;

#[compio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let target_addr: SocketAddr = "192.168.1.50:50001".parse()?;

    // Initialize the sender
    let sender = HayateSender::new()
        .target(target_addr)
        .compress(true);

    // Send a file or directory
    let checksum = sender.send("path/to/my_folder", |bytes_sent| {
        println!("Progress: {bytes_sent} bytes transferred");
    }).await?;

    println!("Success! SHA-256 Checksum: {checksum}");
    Ok(())
}
```

### 2. Receiving a File or Directory

```rust
use std::net::SocketAddr;
use hayate::runner::HayateReceiver;

#[compio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bind_addr: SocketAddr = "0.0.0.0:50001".parse()?;

    // Initialize the receiver
    let receiver = HayateReceiver::new()
        .bind(bind_addr);

    // Wait and receive the transfer
    let (checksum, saved_path) = receiver.receive("./downloads", |meta| {
        println!("Accepting transfer: {} ({} bytes)?", meta.filename, meta.total_size);
        true // Return true to accept, false to reject
    }, |bytes_received| {
        println!("Progress: {bytes_received} bytes received");
    }).await?;

    println!("Saved to: {}", saved_path.display());
    println!("SHA-256 Checksum: {}", checksum);
    Ok(())
}
```

### 3. Pairing via Shared Code Phrase

You can establish connections without exchanging IP addresses manually:

**Sender:**
```rust
let sender = HayateSender::new()
    .code("apple-bravo-charlie".to_string());
sender.send("file.txt", |b| {}).await?;
```

**Receiver:**
```rust
let receiver = HayateReceiver::new()
    .code("apple-bravo-charlie".to_string());
receiver.receive(".", |meta| true, |b| {}).await?;
```

---

## ✦ System Architecture & Design

### Completion-Based I/O (Proactor)
Unlike readiness-based models (such as `epoll` or `tokio`), `compio` utilizes a completion-based model. When an I/O operation (like reading a file or socket) is requested, the buffer's ownership is passed directly to the OS kernel. The kernel writes to or reads from it, and returns the buffer back via completion queues. 

Therefore, any custom implementation using `hayate`'s low-level APIs must respect this ownership flow, using `compio::BufResult` return types.

### Thread-Per-Core Concurrency
To avoid mutex contention and thread synchronization overhead:
1. Every processor core runs its own single-threaded executor.
2. Sockets and files bound to an executor thread are touched only by that thread.
3. Heavy CPU work (such as Zstd compression and AEAD seal/open operations) is offloaded to a dedicated worker pool, leaving the reactor/proactor thread free to poll I/O completion queues.

---

## ✦ Security & Threat Model

* **MITM Defense**: Ephemeral self-signed X.509 certificates are exchanged over QUIC TLS 1.3. To prevent Man-In-The-Middle attacks in unauthenticated LAN environments, Hayate salts the Diffie-Hellman key derivation with the user's code-phrase/passphrase.
* **Payload Isolation**: Payloads and filenames are encrypted using an application-level key, ensuring that even if the TLS layer is intercepted, raw file data remains unreadable.

---

## ✦ License

This project is licensed under the MIT License. See [LICENSE](../LICENSE) for details.

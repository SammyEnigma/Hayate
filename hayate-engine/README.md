# Hayate Engine (はやて)

[![Crates.io](https://img.shields.io/crates/v/hayate.svg)](https://crates.io/crates/hayate)
[![Documentation](https://docs.rs/hayate/badge.svg)](https://docs.rs/hayate)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](../LICENSE)

Hayate Engine is the Rust library behind Hayate's encrypted LAN transfer stack. It gives applications a direct way to send and receive files or directories over QUIC while keeping the protocol, pairing flow, encryption, compression, framing, progress reporting, and archive safety inside one reusable crate.

The engine is built for local networks where speed matters but peers still cannot be blindly trusted. It uses completion-based async I/O through `compio`, QUIC through `compio-quic`, ephemeral X25519 key agreement, AEAD-encrypted metadata and payload frames, optional zstd compression, and safe tar streaming for directories.

---

## What You Get

* High-level sender and receiver builders for application code.
* Direct `SocketAddr` transfers when the peer address is already known.
* Pairing-code discovery when users should not exchange IP addresses manually.
* Encrypted metadata, filenames, and payload bytes.
* File and directory transfers through the same progress/checksum API.
* Bounded frame decoding and strict metadata validation.
* Directory extraction that rejects traversal, symlinks, and hard links.
* Lower-level protocol functions for custom transports that still use Hayate framing.

---

## Installation

```toml
[dependencies]
hayate = "2.1"
compio = { version = "0.19", features = ["macros", "runtime"] }
```

Hayate's public async examples use `#[compio::main]`, so applications should run inside a `compio` runtime.

---

## Quick Start

### Receive

Start a receiver, ask your application whether to accept the transfer, and save the payload under `./downloads`.

```rust
use std::net::SocketAddr;
use hayate::runner::HayateReceiver;

#[compio::main]
async fn main() -> Result<(), hayate::EngineError> {
    let bind_addr: SocketAddr = "0.0.0.0:50001".parse().unwrap();

    let receiver = HayateReceiver::new()
        .bind(bind_addr);

    let (checksum, saved_path) = receiver.receive(
        "./downloads",
        |meta| {
            println!("Incoming: {} ({} bytes)", meta.filename, meta.total_size);
            true
        },
        |bytes_received| {
            println!("received {bytes_received} bytes");
        },
    ).await?;

    println!("saved to {}", saved_path.display());
    println!("sha256 {checksum}");
    Ok(())
}
```

### Send

Connect directly to the receiver and send a file or directory.

```rust
use std::net::SocketAddr;
use hayate::runner::HayateSender;

#[compio::main]
async fn main() -> Result<(), hayate::EngineError> {
    let target: SocketAddr = "192.168.1.50:50001".parse().unwrap();

    let sender = HayateSender::new()
        .target(target)
        .compress(true);

    let checksum = sender.send("photos", |bytes_sent| {
        println!("sent {bytes_sent} bytes");
    }).await?;

    println!("sha256 {checksum}");
    Ok(())
}
```

---

## Pairing Mode

Pairing mode is useful when users know a shared phrase but do not want to exchange IP addresses. The sender broadcasts a channel derived from the phrase; the receiver listens for that channel, connects back, and both sides use the same phrase in key derivation.

### Sender

```rust
use hayate::runner::HayateSender;

# async fn run() -> Result<(), hayate::EngineError> {
let sender = HayateSender::new()
    .code("apple-bravo-charlie".to_owned())
    .compress(true);

let checksum = sender.send("report.pdf", |_| {}).await?;
# Ok(())
# }
```

### Receiver

```rust
use hayate::runner::HayateReceiver;

# async fn run() -> Result<(), hayate::EngineError> {
let receiver = HayateReceiver::new()
    .code("apple-bravo-charlie".to_owned())
    .auto_accept(true);

let (checksum, saved_path) = receiver.receive("./downloads", |_| true, |_| {}).await?;
# Ok(())
# }
```

Pairing discovery is LAN broadcast based. Some networks, VPNs, mobile hotspots, and Android devices may block broadcast traffic; direct mode is more predictable in those environments.

---

## API Map

Use the high-level API unless you are embedding Hayate into a custom transport or UI.

| Module | Purpose |
| --- | --- |
| `runner` | Builder-style sender and receiver APIs: `HayateSender`, `HayateReceiver`. |
| `transfer` | Handshake, consent, payload send/receive, split-stream helpers. |
| `protocol` | Wire constants, frame flags, `Metadata` encoding and validation. |
| `crypto` | X25519, HKDF, AEAD frame encryption, cipher selection helpers. |
| `network` | QUIC endpoint binding, client/server config, ephemeral TLS config. |
| `discovery` | Pairing-code broadcast and listener utilities. |
| `tar` | Directory packaging, safe extraction, directory size estimation. |
| `local_addr` | Local IPv4 and subnet helpers for discovery/UI display. |
| `error` | `EngineError`, the shared engine error type. |

The crate root re-exports `HayateSender`, `HayateReceiver`, and `EngineError`.

---

## Transfer Lifecycle

Every transfer follows the same protocol shape:

1. The sender and receiver establish a QUIC connection.
2. The sender opens a bidirectional stream.
3. Peers exchange the Hayate protocol version and cipher capability.
4. Peers perform ephemeral X25519 key agreement.
5. A shared AEAD key is derived with HKDF-SHA256.
6. The receiver selects ChaCha20-Poly1305 or AES-256-GCM.
7. The sender encrypts metadata: filename, expected size, and transfer type.
8. The receiver validates metadata and asks the application for consent.
9. The sender streams encrypted payload frames.
10. The receiver authenticates, optionally decompresses, writes, hashes, and validates the payload.

Direct mode may pass `None` for the pairing phrase. Pairing mode should pass the same code phrase on both sides, so a network attacker without the phrase cannot derive the same application-layer key.

---

## Security Model

Hayate uses QUIC TLS 1.3, but its generated certificates are self-signed and ephemeral. That is practical for zero-config LAN transfer, but it is not enough to authenticate peers by itself. Hayate therefore adds application-layer encryption over metadata and payload frames.

Important guarantees:

* X25519 key agreement is ephemeral for each transfer.
* HKDF-SHA256 derives a 32-byte transfer key.
* Pairing phrases are used as HKDF salt when supplied.
* Metadata is encrypted and authenticated before the receiver sees filenames or sizes.
* Payload frames are AEAD-authenticated before decompression or writes.
* Invalid metadata transfer types are rejected before file/directory routing.
* Receivers can reject a transfer after decrypting metadata but before payload bytes are accepted.

Notable boundaries:

* Direct mode without a shared code phrase does not authenticate peer identity beyond the QUIC connection.
* Pairing phrase strength matters. Human-friendly phrases are convenient, but applications with higher security needs should provide stronger shared secrets.
* Hayate protects transfer contents; it does not provide user identity, access control, or a persistent trust store.

---

## File and Archive Safety

The receiver treats file paths and archives as untrusted input.

For files:

* The output name is resolved from metadata using only the final path component.
* The transfer must end with exactly the announced file size.
* The SHA-256 returned by the API is computed from the plaintext payload bytes.

For directories:

* Directories are streamed as tar archives.
* Extraction creates the requested destination root before writing entries.
* Nested parent directories are created as needed.
* Absolute paths and `..` traversal are rejected.
* Symlink and hard-link entries are rejected.
* Truncated directory streams are rejected if fewer bytes arrive than announced.

---

## Compression

Compression is optional and controlled by `HayateSender::compress(true)`.

When enabled, payload chunks are compressed with zstd level 1 only when the compressed frame is smaller than the raw frame. Known already-compressed extensions, such as `zip`, `gz`, `zst`, `mp4`, `mkv`, `jpg`, `png`, `webp`, `flac`, and `opus`, skip compression to avoid wasting CPU or expanding payloads.

---

## Progress and Checksums

Both high-level APIs accept progress callbacks:

```rust
|bytes| {
    println!("{bytes} plaintext bytes processed");
}
```

The callback reports plaintext payload bytes, not encrypted frame bytes. The returned checksum is a hex-encoded SHA-256 digest of the plaintext payload stream. For directory transfers, that digest is computed over the tar stream bytes that represent the directory payload.

---

## Low-Level Use

Applications that need custom orchestration can use `transfer` directly:

* `handshake_sender_split` / `handshake_receiver_split` for split QUIC streams.
* `send_consent_write` for receiver consent.
* `send_payload_write` / `receive_payload_split` for payload transfer.
* `PayloadSource` and `PayloadSink` for file-backed or channel-backed streams.

These functions assume `compio` ownership semantics: I/O buffers are passed into async operations and returned through `compio::BufResult`. Avoid borrowing buffers across I/O calls.

---

## Error Handling

All public engine APIs return `EngineError`.

Common variants:

| Variant | Meaning |
| --- | --- |
| `Io` | Filesystem, socket, channel, or task I/O failure. |
| `ProtocolMismatch` | Peer speaks a different Hayate wire protocol version. |
| `TransferRejected` | Receiver declined after reading metadata. |
| `InvalidPassphrase` | Metadata authentication failed while a phrase was expected. |
| `Crypto` | Key derivation or AEAD operation failed. |
| `InvalidFrame` | Malformed metadata, frame length, frame flag, or transfer type. |
| `PathTraversal` | Archive entry attempted unsafe extraction. |
| `Quic` | QUIC endpoint or stream setup failed. |

---

## Validation and Testing

Run the same checks before changing protocol, crypto, network, or archive behavior:

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets
```

Focused areas worth testing when extending the engine:

* metadata length and transfer-type validation
* direct and pairing handshakes
* wrong passphrase behavior
* truncated frame and truncated file handling
* zstd and raw frame decoding
* directory extraction containment
* symlink and hard-link archive rejection

---

## License

This project is licensed under the MIT License. See [LICENSE](../LICENSE) for details.

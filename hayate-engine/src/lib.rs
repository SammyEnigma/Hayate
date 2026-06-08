//! # Hayate (はやて)
//!
//! A completion-based I/O engine for direct, encrypted, and compressed LAN transfers.
//!
//! Driven by the `compio` thread-per-core asynchronous runtime, Hayate uses QUIC
//! (via `compio-quic` and `quinn-proto`) to saturate local network links while securing
//! data with ephemeral Diffie-Hellman key exchanges (Curve25519) and symmetric AEAD
//! encryption (ChaCha20-Poly1305 or AES-256-GCM).
//!
//! This crate can be used standalone to transfer files or directories programmatically
//! using the high-level builder runners in the [`runner`] module.
//!
//! ## Standalone Usage Examples
//!
//! ### Sender Example
//! ```no_run
//! use std::net::SocketAddr;
//! use hayate::runner::HayateSender;
//!
//! # async fn run() -> Result<(), hayate::EngineError> {
//! let target: SocketAddr = "192.168.1.50:50001".parse().unwrap();
//! let sender = HayateSender::new()
//!     .target(target)
//!     .compress(true);
//!
//! sender.send("my_photo.jpg", |bytes_sent| {
//!     println!("Progress: {bytes_sent} bytes transferred");
//! }).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ### Receiver Example
//! ```no_run
//! use std::net::SocketAddr;
//! use hayate::runner::HayateReceiver;
//!
//! # async fn run() -> Result<(), hayate::EngineError> {
//! let bind_addr: SocketAddr = "0.0.0.0:50001".parse().unwrap();
//! let receiver = HayateReceiver::new()
//!     .bind(bind_addr)
//!     .auto_accept(true);
//!
//! let (checksum, path) = receiver.receive("./downloads", |meta| {
//!     println!("Receiving {}...", meta.filename);
//!     true
//! }, |bytes_received| {
//!     println!("Progress: {bytes_received} bytes received");
//! }).await?;
//! # Ok(())
//! # }
//! ```

#![warn(clippy::all, clippy::pedantic, missing_docs)]
#![allow(
    clippy::module_name_repetitions,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::too_many_lines,
    clippy::doc_markdown,
    clippy::must_use_candidate
)]

pub mod crypto;
pub mod discovery;
pub mod error;
pub mod local_addr;
pub mod network;
pub mod pool;
pub mod protocol;
pub mod runner;
pub mod tar;
pub mod transfer;

pub use error::EngineError;
pub use runner::{HayateReceiver, HayateSender};

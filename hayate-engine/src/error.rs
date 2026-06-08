//! Error types for the Hayate engine.

use thiserror::Error;

/// Top-level error type for the engine.
#[derive(Debug, Error)]
pub enum EngineError {
    /// An underlying I/O error occurred (e.g. file system or socket issues).
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// The client and server protocol versions do not match.
    #[error("protocol version mismatch: local {local}, remote {remote}")]
    ProtocolMismatch {
        /// The local protocol version supported by this node.
        local: u16,
        /// The remote protocol version sent by the peer.
        remote: u16,
    },

    /// The receiver explicitly rejected the transfer request.
    #[error("transfer rejected by receiver")]
    TransferRejected,

    /// Key exchange authentication failed due to an invalid pairing passphrase.
    #[error("invalid passphrase: key exchange authentication failed")]
    InvalidPassphrase,

    /// An error occurred during cryptographic operations (e.g. encryption/decryption/HKDF).
    #[error("crypto error: {0}")]
    Crypto(String),

    /// An invalid frame structure or size was encountered on the stream.
    #[error("invalid frame: {0}")]
    InvalidFrame(String),

    /// An error occurred during the handshake negotiation phase.
    #[error("handshake error: {0}")]
    Handshake(String),

    /// An error occurred in the underlying QUIC transport or endpoint.
    #[error("QUIC error: {0}")]
    Quic(String),

    /// An error occurred during payload compression or decompression.
    #[error("compression error: {0}")]
    Compression(String),

    /// An archive entry attempted path traversal outside the output directory.
    #[error("path traversal attack detected in archive entry")]
    PathTraversal,

    /// An unspecified error captured via `anyhow`.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

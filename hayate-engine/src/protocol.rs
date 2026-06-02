//! Binary wire protocol constants, framing, and metadata codec.
//!
//! Wire format, with all multi-byte integers encoded big-endian:
//!
//! | Direction | Bytes | Field |
//! | --- | ---: | --- |
//! | Sender to receiver | 2 | Protocol version, equal to [`PROTOCOL_VERSION`] |
//! | Sender to receiver | 1 | Sender's preferred cipher capability |
//! | Sender to receiver | 32 | Sender X25519 public key |
//! | Receiver to sender | 32 | Receiver X25519 public key |
//! | Receiver to sender | 1 | Selected cipher suite |
//! | Sender to receiver | 4 | Encrypted metadata length |
//! | Sender to receiver | N | Metadata frame: nonce, ciphertext, authentication tag |
//! | Receiver to sender | 1 | Consent byte: `0x01` accept, `0x00` reject |
//! | Sender to receiver | 4 | Encrypted payload frame length |
//! | Sender to receiver | N | Payload frame: nonce, encrypted flag/payload, authentication tag |
//!
//! Plaintext metadata layout:
//!
//! | Bytes | Field |
//! | ---: | --- |
//! | 2 | Filename byte length |
//! | M | UTF-8 filename |
//! | 8 | File size, or `0` for directory streams |
//! | 1 | Transfer type: [`TRANSFER_FILE`] or [`TRANSFER_DIR`] |
//!
//! Decrypted payload frames begin with one flag byte: [`FRAME_RAW`] for
//! uncompressed payloads, [`FRAME_ZSTD`] for zstd-compressed payloads.

/// Current binary wire protocol version.
pub const PROTOCOL_VERSION: u16 = 4;

/// Metadata transfer type for a single file.
pub const TRANSFER_FILE: u8 = 0x00;
/// Metadata transfer type for a directory encoded as a tar stream.
pub const TRANSFER_DIR: u8 = 0x01;

/// Payload frame flag for bytes that were sent without compression.
pub const FRAME_RAW: u8 = 0x00;
/// Payload frame flag for bytes compressed with zstd.
pub const FRAME_ZSTD: u8 = 0x01;

/// Maximum allowed filename length in bytes.
pub const MAX_FILENAME_BYTES: usize = 4096;

/// Maximum encrypted metadata payload size.
///
/// The cap includes the plaintext metadata fields plus nonce/tag overhead and
/// a small margin, preventing malicious peers from forcing large allocations
/// during the handshake.
pub const MAX_METADATA_ENCRYPTED: usize = 4 + MAX_FILENAME_BYTES + 8 + 1 + 12 + 16 + 16;

/// Chunk size for each data frame in bytes.
///
/// One MiB amortizes per-frame crypto/framing overhead while keeping pipeline
/// memory bounded for read-ahead, worker queues, and receiver buffering.
pub const CHUNK_SIZE: usize = 1024 * 1024;

/// Metadata that travels in the encrypted handshake.
#[derive(Debug, Clone)]
pub struct Metadata {
    /// Display name sent to the receiver and used as the output filename.
    pub filename: String,
    /// Total bytes for a file transfer; 0 for directories (streaming, unknown).
    pub total_size: u64,
    /// Transfer kind, either [`TRANSFER_FILE`] or [`TRANSFER_DIR`].
    pub transfer_type: u8,
}

impl Metadata {
    /// Serialises to the plaintext metadata blob.
    pub fn encode(&self) -> Vec<u8> {
        let name_bytes = self.filename.as_bytes();
        let mut buf = Vec::with_capacity(2 + name_bytes.len() + 8 + 1);
        buf.extend_from_slice(&(name_bytes.len() as u16).to_be_bytes());
        buf.extend_from_slice(name_bytes);
        buf.extend_from_slice(&self.total_size.to_be_bytes());
        buf.push(self.transfer_type);
        buf
    }

    /// Deserialises from the plaintext metadata blob.
    pub fn decode(raw: &[u8]) -> Result<Self, crate::EngineError> {
        if raw.len() < 11 {
            return Err(crate::EngineError::InvalidFrame(
                "metadata too short".into(),
            ));
        }
        let name_len = u16::from_be_bytes([raw[0], raw[1]]) as usize;
        if name_len == 0 || name_len > MAX_FILENAME_BYTES {
            return Err(crate::EngineError::InvalidFrame(format!(
                "invalid filename length: {name_len}"
            )));
        }
        if raw.len() < 2 + name_len + 8 + 1 {
            return Err(crate::EngineError::InvalidFrame(
                "metadata truncated".into(),
            ));
        }
        let filename = std::str::from_utf8(&raw[2..2 + name_len])
            .map_err(|_| crate::EngineError::InvalidFrame("filename not UTF-8".into()))?
            .to_owned();
        let total_size = u64::from_be_bytes(
            raw[2 + name_len..2 + name_len + 8]
                .try_into()
                .expect("slice len == 8"),
        );
        let transfer_type = raw[2 + name_len + 8];
        Ok(Self {
            filename,
            total_size,
            transfer_type,
        })
    }
}

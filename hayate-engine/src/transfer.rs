//! Transfer pipeline: handshake, send, receive.
//!
//! ## compio I/O model
//!
//! compio is completion-based (io_uring / IOCP). The kernel holds a
//! reference to the I/O buffer until the completion event fires, so every
//! buffer must be **owned** and passed by value to the I/O call. The
//! return type is `BufResult<T, B>` = `(Result<T, io::Error>, B)` where `B`
//! is the buffer returned after the kernel is done with it.

use futures_util::stream::{FuturesOrdered, StreamExt};
use std::{collections::BTreeMap, io, path::Path, sync::Arc};

use compio::io::{AsyncReadAt, AsyncReadExt, AsyncWriteAtExt, AsyncWriteExt};

use crate::{
    EngineError, crypto,
    protocol::{
        CHUNK_SIZE, FRAME_RAW, FRAME_ZSTD, MAX_METADATA_ENCRYPTED, Metadata, PROTOCOL_VERSION,
        TRANSFER_DIR, TRANSFER_FILE,
    },
};

// ---------------------------------------------------------------------------
// Non-blocking Payload Sources and Sinks
// ---------------------------------------------------------------------------

/// Source of data payload to be transferred.
pub enum PayloadSource {
    /// A local file on the filesystem.
    File {
        /// The file handle.
        file: compio::fs::File,
        /// The starting byte position.
        pos: u64,
    },
    /// An asynchronous channel yielding byte chunks.
    Channel(flume::Receiver<Result<Vec<u8>, io::Error>>),
}

/// Destination for incoming data payload.
pub enum PayloadSink {
    /// A local file on the filesystem.
    File {
        /// The file handle.
        file: compio::fs::File,
        /// The starting byte position.
        pos: u64,
    },
    /// An asynchronous channel receiving byte chunks.
    Channel(flume::Sender<Vec<u8>>),
}

// ---------------------------------------------------------------------------
// Internal I/O helpers
// ---------------------------------------------------------------------------

/// Helper for dynamic payload hashing.
enum PayloadHasher {
    Blake3(Box<blake3::Hasher>),
    RapidHash(rapidhash::v3::RapidStreamHasherV3<'static>),
    Sha256(ring::digest::Context),
}

impl PayloadHasher {
    fn new(algo: &str) -> Self {
        match algo {
            "rapidhash" => Self::RapidHash(rapidhash::v3::RapidStreamHasherV3::new(
                &rapidhash::v3::DEFAULT_RAPID_SECRETS,
            )),
            "sha256" => Self::Sha256(ring::digest::Context::new(&ring::digest::SHA256)),
            _ => Self::Blake3(Box::new(blake3::Hasher::new())),
        }
    }

    fn update(&mut self, data: &[u8]) {
        match self {
            Self::Blake3(h) => {
                h.update(data);
            }
            Self::RapidHash(h) => {
                h.write(data);
            }
            Self::Sha256(h) => {
                h.update(data);
            }
        }
    }

    fn finalize(self, algo: &str) -> String {
        let hex_hash = match self {
            Self::Blake3(h) => h.finalize().to_hex().to_string(),
            Self::RapidHash(h) => format!("{:016x}", h.finish()),
            Self::Sha256(h) => hex::encode(h.finish().as_ref()),
        };
        format!("{algo}${hex_hash}")
    }
}

/// Read exactly `N` bytes from `stream` into a fresh `Vec<u8>`.
async fn read_exact_n<S: AsyncReadExt + Unpin>(
    stream: &mut S,
    n: usize,
) -> Result<Vec<u8>, EngineError> {
    let buf = vec![0u8; n];
    let compio::BufResult(result, buf) = stream.read_exact(buf).await;
    result.map_err(EngineError::Io)?;
    Ok(buf)
}

/// Write `data` to `stream`.
async fn write_all_owned<S: AsyncWriteExt + Unpin>(
    stream: &mut S,
    data: Vec<u8>,
) -> Result<(), EngineError> {
    let compio::BufResult(result, _) = stream.write_all(data).await;
    result.map_err(EngineError::Io)
}

/// Read a `u32` from the stream.
async fn read_u32<S: AsyncReadExt + Unpin>(stream: &mut S) -> Result<u32, EngineError> {
    let bytes = read_exact_n(stream, 4).await?;
    Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Write a `u32` to the stream.
async fn write_u32<S: AsyncWriteExt + Unpin>(stream: &mut S, v: u32) -> Result<(), EngineError> {
    write_all_owned(stream, v.to_be_bytes().to_vec()).await
}

// ---------------------------------------------------------------------------
// Handshake — sender side
// ---------------------------------------------------------------------------

/// Performs the cryptographic version check, key exchange, cipher negotiation,
/// and metadata handshake on the sender side.
///
/// This function works with a single combined stream implementing both `AsyncRead`
/// and `AsyncWrite`.
///
/// # Errors
///
/// Returns [`EngineError`] if version mismatch, key exchange derivation, cipher negotiation,
/// or encryption/decryption fails, or if the receiver rejects the transfer.
pub async fn handshake_sender<S>(
    stream: &mut S,
    meta: &Metadata,
    passphrase: Option<&str>,
) -> Result<([u8; 32], u8), EngineError>
where
    S: compio::io::AsyncRead + compio::io::AsyncWrite + Unpin,
{
    meta.validate()?;

    // 1. Protocol version + Sender Capability
    let mut ver_and_cap = Vec::with_capacity(3);
    ver_and_cap.extend_from_slice(&PROTOCOL_VERSION.to_be_bytes());
    let sender_cap = if crypto::features::is_aes_hw_accelerated() {
        crypto::CIPHER_AES256_GCM
    } else {
        crypto::CIPHER_CHACHA20
    };
    ver_and_cap.push(sender_cap);
    write_all_owned(stream, ver_and_cap).await?;

    // 2. Key exchange
    let (secret, our_pub) = crypto::generate_keypair();
    write_all_owned(stream, our_pub.to_vec()).await?;

    let peer_pub_bytes = read_exact_n(stream, 32).await?;
    let mut peer_pub = [0u8; 32];
    peer_pub.copy_from_slice(&peer_pub_bytes);

    let key = crypto::derive_key(secret, &peer_pub, passphrase)?;

    // 3. Receive the selected cipher from receiver
    let cipher_bytes = read_exact_n(stream, 1).await?;
    let selected_cipher = cipher_bytes[0];
    if selected_cipher != crypto::CIPHER_CHACHA20 && selected_cipher != crypto::CIPHER_AES256_GCM {
        return Err(EngineError::Handshake(
            "Unknown cipher suite selected by receiver".into(),
        ));
    }

    // 4. Encrypted metadata
    let encrypted = crypto::encrypt_metadata(&key, selected_cipher, &meta.encode())?;
    write_u32(stream, encrypted.len() as u32).await?;
    write_all_owned(stream, encrypted).await?;

    // 5. Consent
    let consent = read_exact_n(stream, 1).await?;
    match consent[0] {
        0x01 => Ok((key, selected_cipher)),
        0x00 => Err(EngineError::TransferRejected),
        other => Err(EngineError::InvalidFrame(format!(
            "unexpected consent byte 0x{other:02x}"
        ))),
    }
}

/// Performs the cryptographic version check, key exchange, cipher negotiation,
/// and metadata handshake on the sender side using separate write and read streams.
///
/// This is particularly useful for protocols like QUIC that split connections into
/// separate unidirectional streams.
///
/// # Errors
///
/// Returns [`EngineError`] if version mismatch, key exchange derivation, cipher negotiation,
/// or encryption/decryption fails, or if the receiver rejects the transfer.
pub async fn handshake_sender_split<W, R>(
    send: &mut W,
    recv: &mut R,
    meta: &Metadata,
    passphrase: Option<&str>,
) -> Result<([u8; 32], u8), EngineError>
where
    W: compio::io::AsyncWrite + Unpin,
    R: compio::io::AsyncRead + Unpin,
{
    meta.validate()?;

    // 1. Protocol version + Sender Capability
    let mut ver_and_cap = Vec::with_capacity(3);
    ver_and_cap.extend_from_slice(&PROTOCOL_VERSION.to_be_bytes());
    let sender_cap = if crypto::features::is_aes_hw_accelerated() {
        crypto::CIPHER_AES256_GCM
    } else {
        crypto::CIPHER_CHACHA20
    };
    ver_and_cap.push(sender_cap);
    write_all_owned(send, ver_and_cap).await?;

    // 2. Key exchange
    let (secret, our_pub) = crypto::generate_keypair();
    write_all_owned(send, our_pub.to_vec()).await?;

    let peer_pub_bytes = read_exact_n(recv, 32).await?;
    let mut peer_pub = [0u8; 32];
    peer_pub.copy_from_slice(&peer_pub_bytes);

    let key = crypto::derive_key(secret, &peer_pub, passphrase)?;

    // 3. Receive the selected cipher from receiver
    let cipher_bytes = read_exact_n(recv, 1).await?;
    let selected_cipher = cipher_bytes[0];
    if selected_cipher != crypto::CIPHER_CHACHA20 && selected_cipher != crypto::CIPHER_AES256_GCM {
        return Err(EngineError::Handshake(
            "Unknown cipher suite selected by receiver".into(),
        ));
    }

    // 4. Encrypted metadata
    let encrypted = crypto::encrypt_metadata(&key, selected_cipher, &meta.encode())?;
    write_u32(send, encrypted.len() as u32).await?;
    write_all_owned(send, encrypted).await?;

    // 5. Consent
    let consent = read_exact_n(recv, 1).await?;
    match consent[0] {
        0x01 => Ok((key, selected_cipher)),
        0x00 => Err(EngineError::TransferRejected),
        other => Err(EngineError::InvalidFrame(format!(
            "unexpected consent byte 0x{other:02x}"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Handshake — receiver side
// ---------------------------------------------------------------------------

/// Performs the cryptographic version check, key exchange, cipher negotiation,
/// and metadata handshake on the receiver side.
///
/// This function works with a single combined stream implementing both `AsyncRead`
/// and `AsyncWrite`.
///
/// # Errors
///
/// Returns [`EngineError`] if version mismatch, key exchange derivation, cipher negotiation,
/// or encryption/decryption fails.
pub async fn handshake_receiver<S>(
    stream: &mut S,
    passphrase: Option<&str>,
) -> Result<(([u8; 32], u8), Metadata), EngineError>
where
    S: compio::io::AsyncRead + compio::io::AsyncWrite + Unpin,
{
    // 1. Version check + Sender Capability
    let ver_cap = read_exact_n(stream, 3).await?;
    let remote_ver = u16::from_be_bytes([ver_cap[0], ver_cap[1]]);
    if remote_ver != PROTOCOL_VERSION {
        return Err(EngineError::ProtocolMismatch {
            local: PROTOCOL_VERSION,
            remote: remote_ver,
        });
    }
    let sender_cap = ver_cap[2];
    if sender_cap != crypto::CIPHER_CHACHA20 && sender_cap != crypto::CIPHER_AES256_GCM {
        return Err(EngineError::Handshake(
            "Unknown cipher capability sent by sender".into(),
        ));
    }

    let selected_cipher =
        if sender_cap == crypto::CIPHER_AES256_GCM && crypto::features::is_aes_hw_accelerated() {
            crypto::CIPHER_AES256_GCM
        } else {
            crypto::CIPHER_CHACHA20
        };

    // 2. Key exchange
    let peer_pub_bytes = read_exact_n(stream, 32).await?;
    let mut peer_pub = [0u8; 32];
    peer_pub.copy_from_slice(&peer_pub_bytes);

    let (secret, our_pub) = crypto::generate_keypair();
    write_all_owned(stream, our_pub.to_vec()).await?;

    let key = crypto::derive_key(secret, &peer_pub, passphrase)?;

    // 3. Write selected cipher back to sender
    write_all_owned(stream, vec![selected_cipher]).await?;

    // 4. Metadata
    let enc_len = read_u32(stream).await? as usize;
    if enc_len == 0 || enc_len > MAX_METADATA_ENCRYPTED {
        return Err(EngineError::InvalidFrame(format!(
            "invalid metadata length: {enc_len}"
        )));
    }
    let enc = read_exact_n(stream, enc_len).await?;
    let plain = match crypto::decrypt_metadata(&key, selected_cipher, &enc) {
        Ok(p) => p,
        Err(e) => {
            if passphrase.is_some() {
                return Err(EngineError::InvalidPassphrase);
            }
            return Err(e);
        }
    };
    let meta = Metadata::decode(&plain)?;
    Ok(((key, selected_cipher), meta))
}

/// Performs the cryptographic version check, key exchange, cipher negotiation,
/// and metadata handshake on the receiver side using separate write and read streams.
///
/// This is particularly useful for protocols like QUIC that split connections into
/// separate unidirectional streams.
///
/// # Errors
///
/// Returns [`EngineError`] if version mismatch, key exchange derivation, cipher negotiation,
/// or encryption/decryption fails.
pub async fn handshake_receiver_split<W, R>(
    send: &mut W,
    recv: &mut R,
    passphrase: Option<&str>,
) -> Result<(([u8; 32], u8), Metadata), EngineError>
where
    W: compio::io::AsyncWrite + Unpin,
    R: compio::io::AsyncRead + Unpin,
{
    // 1. Version check + Sender Capability
    let ver_cap = read_exact_n(recv, 3).await?;
    let remote_ver = u16::from_be_bytes([ver_cap[0], ver_cap[1]]);
    if remote_ver != PROTOCOL_VERSION {
        return Err(EngineError::ProtocolMismatch {
            local: PROTOCOL_VERSION,
            remote: remote_ver,
        });
    }
    let sender_cap = ver_cap[2];
    if sender_cap != crypto::CIPHER_CHACHA20 && sender_cap != crypto::CIPHER_AES256_GCM {
        return Err(EngineError::Handshake(
            "Unknown cipher capability sent by sender".into(),
        ));
    }

    let selected_cipher =
        if sender_cap == crypto::CIPHER_AES256_GCM && crypto::features::is_aes_hw_accelerated() {
            crypto::CIPHER_AES256_GCM
        } else {
            crypto::CIPHER_CHACHA20
        };

    // 2. Key exchange
    let peer_pub_bytes = read_exact_n(recv, 32).await?;
    let mut peer_pub = [0u8; 32];
    peer_pub.copy_from_slice(&peer_pub_bytes);

    let (secret, our_pub) = crypto::generate_keypair();
    write_all_owned(send, our_pub.to_vec()).await?;

    let key = crypto::derive_key(secret, &peer_pub, passphrase)?;

    // 3. Write selected cipher back to sender
    write_all_owned(send, vec![selected_cipher]).await?;

    // 4. Metadata
    let enc_len = read_u32(recv).await? as usize;
    if enc_len == 0 || enc_len > MAX_METADATA_ENCRYPTED {
        return Err(EngineError::InvalidFrame(format!(
            "invalid metadata length: {enc_len}"
        )));
    }
    let enc = read_exact_n(recv, enc_len).await?;
    let plain = match crypto::decrypt_metadata(&key, selected_cipher, &enc) {
        Ok(p) => p,
        Err(e) => {
            if passphrase.is_some() {
                return Err(EngineError::InvalidPassphrase);
            }
            return Err(e);
        }
    };
    let meta = Metadata::decode(&plain)?;
    Ok(((key, selected_cipher), meta))
}

/// Writes the consent byte (0x01 = accept, 0x00 = reject).
pub async fn send_consent<S>(stream: &mut S, accept: bool) -> Result<(), EngineError>
where
    S: compio::io::AsyncWrite + Unpin,
{
    write_all_owned(stream, vec![u8::from(accept)]).await
}

// ---------------------------------------------------------------------------
// Send payload
// ---------------------------------------------------------------------------

/// Encrypts and transmits a payload source (file or channel) to the receiver.
///
/// Chunks of [`crate::protocol::CHUNK_SIZE`] are read from the source, compressed
/// (if requested and the file extension suggests it is beneficial), encrypted with the
/// negotiated AEAD cipher, and written to the stream.
///
/// # Errors
///
/// Returns [`EngineError`] if reading from source, compressing, encrypting, or writing
/// to the network fails.
#[allow(clippy::arc_with_non_send_sync, clippy::too_many_arguments)]
pub async fn send_payload<S>(
    key: &[u8; 32],
    cipher_id: u8,
    source: PayloadSource,
    stream: &mut S,
    compress: bool,
    filename: Option<&str>,
    hash_algo: &str,
    mut progress_cb: impl FnMut(u64),
) -> Result<String, EngineError>
where
    S: compio::io::AsyncWrite + Unpin,
{
    // Compression is counterproductive for formats that are already entropy
    // coded; avoid burning CPU and expanding payloads for those extensions.
    let mut do_compress = compress;
    let ext_opt = if compress {
        filename
            .and_then(|name| std::path::Path::new(name).extension())
            .and_then(|s| s.to_str())
    } else {
        None
    };

    if let Some(ext) = ext_opt {
        let ext_lower = ext.to_lowercase();
        let skipped = [
            "zip", "gz", "zst", "mp4", "mkv", "jpg", "png", "rar", "7z", "bz2", "xz", "br", "webm",
            "webp", "m4v", "mov", "flac", "opus",
        ];
        if skipped.contains(&ext_lower.as_str()) {
            do_compress = false;
        }
    }

    let pool = crate::pool::BufferPool::new(32, CHUNK_SIZE);

    let (chunk_tx, chunk_rx) =
        flume::bounded::<Result<(usize, Vec<u8>, usize, bool), io::Error>>(16);
    let (hash_tx, hash_rx) = flume::bounded::<String>(1);

    // Reader task: keep a small read-ahead queue so disk latency overlaps with
    // compression/encryption without allowing unbounded memory growth.
    let pool_clone = pool.clone();
    let algo = hash_algo.to_owned();
    compio::runtime::spawn(async move {
        let mut index = 0;
        let mut hasher = PayloadHasher::new(&algo);

        match source {
            PayloadSource::File { file, pos } => {
                let file = Arc::new(file);
                let read_future = |f: Arc<compio::fs::File>, buf: Vec<u8>, p: u64| async move {
                    let compio::BufResult(result, buf) = f.read_at(buf, p).await;
                    (result, buf, p)
                };
                let mut reads = FuturesOrdered::new();
                let queue_depth = 4;
                let mut current_pos = pos;
                let mut file_ended = false;

                for _ in 0..queue_depth {
                    if file_ended {
                        break;
                    }
                    let buf = pool_clone.lease().await;
                    let f = file.clone();
                    let p = current_pos;
                    current_pos += CHUNK_SIZE as u64;

                    reads.push_back(read_future(f, buf, p));
                }

                while let Some((result, buf, _)) = reads.next().await {
                    match result {
                        Ok(n) => {
                            if n == 0 {
                                pool_clone.release(buf);
                                break;
                            }
                            if n < CHUNK_SIZE {
                                file_ended = true;
                            }
                            hasher.update(&buf[..n]);

                            if chunk_tx
                                .send_async(Ok((index, buf, n, true)))
                                .await
                                .is_err()
                            {
                                break;
                            }
                            index += 1;
                        }
                        Err(e) => {
                            pool_clone.release(buf);
                            let _ = chunk_tx.send_async(Err(e)).await;
                            break;
                        }
                    }

                    if !file_ended {
                        let buf = pool_clone.lease().await;
                        let f = file.clone();
                        let p = current_pos;
                        current_pos += CHUNK_SIZE as u64;

                        reads.push_back(read_future(f, buf, p));
                    }
                }

                // Reclaim leased buffers for any reads that completed after
                // the consumer side stopped accepting chunks.
                while let Some((_, buf, _)) = reads.next().await {
                    pool_clone.release(buf);
                }
            }
            PayloadSource::Channel(rx) => {
                while let Ok(res) = rx.recv_async().await {
                    match res {
                        Ok(data) => {
                            if data.is_empty() {
                                break;
                            }
                            hasher.update(&data);
                            let len = data.len();
                            if chunk_tx
                                .send_async(Ok((index, data, len, false)))
                                .await
                                .is_err()
                            {
                                break;
                            }
                            index += 1;
                        }
                        Err(e) => {
                            let _ = chunk_tx.send_async(Err(e)).await;
                            break;
                        }
                    }
                }
            }
        }

        let hash = hasher.finalize(&algo);
        let _ = hash_tx.send(hash);
    })
    .detach();

    // Worker pool: CPU-bound compression and AEAD sealing are isolated from
    // the compio executor. Each worker prepares the AEAD key once, then reuses
    // it for every frame it handles.
    let num_workers = std::thread::available_parallelism()
        .map_or(4, std::num::NonZeroUsize::get)
        .saturating_sub(1)
        .max(2);

    let (result_tx, result_rx) = flume::bounded::<Result<(usize, Vec<u8>, usize), EngineError>>(16);

    for _ in 0..num_workers {
        let chunk_rx = chunk_rx.clone();
        let result_tx = result_tx.clone();
        let key = *key;
        let pool = pool.clone();
        std::thread::spawn(move || {
            let aead_key = match crypto::AeadKey::new(&key, cipher_id) {
                Ok(key) => key,
                Err(e) => {
                    let _ = result_tx.send(Err(e));
                    return;
                }
            };
            while let Ok(res) = chunk_rx.recv() {
                let (index, chunk, chunk_len, pooled) = match res {
                    Ok(val) => val,
                    Err(e) => {
                        let _ = result_tx.send(Err(EngineError::Io(e)));
                        break;
                    }
                };

                let chunk_data = &chunk[..chunk_len];
                let plain_frame: Vec<u8> = if do_compress {
                    match zstd::encode_all(chunk_data, 1) {
                        Ok(compressed) if compressed.len() < chunk_len => {
                            let mut pf = Vec::with_capacity(1 + compressed.len());
                            pf.push(FRAME_ZSTD);
                            pf.extend_from_slice(&compressed);
                            pf
                        }
                        _ => {
                            let mut pf = Vec::with_capacity(1 + chunk_len);
                            pf.push(FRAME_RAW);
                            pf.extend_from_slice(chunk_data);
                            pf
                        }
                    }
                } else {
                    let mut pf = Vec::with_capacity(1 + chunk_len);
                    pf.push(FRAME_RAW);
                    pf.extend_from_slice(chunk_data);
                    pf
                };

                if pooled {
                    pool.release(chunk);
                }

                let mut enc_buf = Vec::with_capacity(4 + 12 + plain_frame.len() + 16);
                enc_buf.extend_from_slice(&[0u8; 4]); // placeholder for length
                match crypto::encrypt_frame_with_key(&aead_key, &plain_frame, &mut enc_buf) {
                    Ok(_) => {
                        let len = (enc_buf.len() - 4) as u32;
                        enc_buf[0..4].copy_from_slice(&len.to_be_bytes());
                        if result_tx.send(Ok((index, enc_buf, chunk_len))).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = result_tx.send(Err(e));
                        break;
                    }
                }
            }
        });
    }
    // Drop our handle to result_tx so result_rx completes when all workers exit
    drop(result_tx);

    // Writer loop: worker output may complete out of order, but the stream
    // remains deterministic by buffering gaps until the next frame arrives.
    let mut pending = std::collections::BTreeMap::new();
    let mut next_index = 0;
    let mut total: u64 = 0;

    while let Ok(res) = result_rx.recv_async().await {
        let (index, frame, plaintext_len) = res?;
        pending.insert(index, (frame, plaintext_len));
        while let Some((frame, p_len)) = pending.remove(&next_index) {
            write_all_owned(stream, frame).await?;
            total += p_len as u64;
            progress_cb(total);
            next_index += 1;
        }
    }

    let hash = hash_rx.recv_async().await.map_err(|_| {
        EngineError::Io(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "hasher task exited prematurely",
        ))
    })?;

    Ok(hash)
}

// ---------------------------------------------------------------------------
// Receive payload
// ---------------------------------------------------------------------------

/// Receives, decrypts, and extracts a payload from a stream, saving it to `output_path`.
///
/// Handles decryption, decompression (zstd), size validation, and safe path extraction
/// for directory transfers.
///
/// # Errors
///
/// Returns [`EngineError`] if reading from stream, decrypting, decompressing, writing to
/// target file/directory, or size validation fails.
#[allow(clippy::too_many_arguments)]
pub async fn receive_payload<S>(
    key: &[u8; 32],
    cipher_id: u8,
    stream: &mut S,
    output_path: &Path,
    transfer_type: u8,
    expected_size: u64,
    hash_algo: &str,
    mut progress_cb: impl FnMut(u64) + 'static,
) -> Result<String, EngineError>
where
    S: compio::io::AsyncRead + Unpin,
{
    if transfer_type != TRANSFER_FILE && transfer_type != TRANSFER_DIR {
        return Err(EngineError::InvalidFrame(format!(
            "invalid transfer type: 0x{transfer_type:02x}"
        )));
    }

    let (tx, rx) = flume::bounded::<Vec<u8>>(8);

    let extract_handle = if transfer_type == TRANSFER_DIR {
        let out = output_path.to_path_buf();
        Some(std::thread::spawn(move || -> Result<(), EngineError> {
            struct ChanReader {
                rx: flume::Receiver<Vec<u8>>,
                buf: Vec<u8>,
                pos: usize,
            }
            impl io::Read for ChanReader {
                fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                    while self.pos >= self.buf.len() {
                        match self.rx.recv() {
                            Ok(chunk) => {
                                self.buf = chunk;
                                self.pos = 0;
                            }
                            Err(_) => return Ok(0),
                        }
                    }
                    let n = std::cmp::min(buf.len(), self.buf.len() - self.pos);
                    buf[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
                    self.pos += n;
                    Ok(n)
                }
            }
            crate::tar::extract_tar_sync(
                ChanReader {
                    rx,
                    buf: Vec::new(),
                    pos: 0,
                },
                &out,
            )
        }))
    } else {
        None
    };

    let mut sink = if transfer_type == TRANSFER_FILE {
        let f = compio::fs::File::create(output_path)
            .await
            .map_err(EngineError::Io)?;
        PayloadSink::File { file: f, pos: 0 }
    } else {
        PayloadSink::Channel(tx)
    };

    let (enc_tx, enc_rx) = flume::bounded::<Result<(usize, Vec<u8>), io::Error>>(16);
    let (plain_tx, plain_rx) = flume::bounded::<Result<(usize, Vec<u8>), EngineError>>(16);

    // Decrypt/decompress workers: isolate CPU-bound tasks in a worker pool.
    let num_workers = std::thread::available_parallelism()
        .map_or(4, std::num::NonZeroUsize::get)
        .saturating_sub(1)
        .max(2);

    for _ in 0..num_workers {
        let enc_rx = enc_rx.clone();
        let plain_tx = plain_tx.clone();
        let key = *key;
        std::thread::spawn(move || {
            let aead_key = match crypto::AeadKey::new(&key, cipher_id) {
                Ok(key) => key,
                Err(e) => {
                    let _ = plain_tx.send(Err(e));
                    return;
                }
            };
            let mut decrypted_buf = Vec::with_capacity(CHUNK_SIZE + 256);
            while let Ok(res) = enc_rx.recv() {
                let (index, enc) = match res {
                    Ok(e) => e,
                    Err(e) => {
                        let _ = plain_tx.send(Err(EngineError::Io(e)));
                        break;
                    }
                };
                match crypto::decrypt_frame_into_with_key(&aead_key, &enc, &mut decrypted_buf) {
                    Ok(()) => {
                        if decrypted_buf.is_empty() {
                            let _ = plain_tx.send(Err(EngineError::InvalidFrame(
                                "empty decrypted frame".into(),
                            )));
                            break;
                        }
                        let flag = decrypted_buf[0];
                        let data = &decrypted_buf[1..];
                        let plaintext_res: Result<Vec<u8>, EngineError> = match flag {
                            FRAME_RAW => Ok(data.to_vec()),
                            FRAME_ZSTD => zstd::decode_all(data)
                                .map_err(|e| EngineError::Compression(e.to_string())),
                            other => Err(EngineError::InvalidFrame(format!(
                                "unknown frame flag 0x{other:02x}"
                            ))),
                        };
                        match plaintext_res {
                            Ok(plain) => {
                                if plain_tx.send(Ok((index, plain))).is_err() {
                                    break;
                                }
                            }
                            Err(e) => {
                                let _ = plain_tx.send(Err(e));
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        let _ = plain_tx.send(Err(e));
                        break;
                    }
                }
            }
        });
    }
    // Drop our handle so plain_rx completes when all workers exit
    drop(plain_tx);

    // Writer task: reorder, hash, and persist plaintext after validation.
    let algo = hash_algo.to_owned();
    let write_handle = compio::runtime::spawn(async move {
        let mut hasher = PayloadHasher::new(&algo);
        let mut total: u64 = 0;
        let mut pos: u64 = 0;
        let mut next_index = 0;
        let mut pending = BTreeMap::new();

        while let Ok(res) = plain_rx.recv_async().await {
            let (index, plaintext) = res?;
            pending.insert(index, plaintext);

            while let Some(plaintext) = pending.remove(&next_index) {
                hasher.update(&plaintext);
                total += plaintext.len() as u64;
                let plaintext_len = plaintext.len() as u64;

                match &mut sink {
                    PayloadSink::File { file, pos: _ } => {
                        let compio::BufResult(result, _) = file.write_all_at(plaintext, pos).await;
                        result.map_err(EngineError::Io)?;
                        pos += plaintext_len;
                    }
                    PayloadSink::Channel(tx) => {
                        tx.send_async(plaintext).await.map_err(|_| {
                            EngineError::Io(io::Error::new(
                                io::ErrorKind::BrokenPipe,
                                "extractor thread exited",
                            ))
                        })?;
                    }
                }
                progress_cb(total);
                next_index += 1;
            }
        }

        // Wait for extraction to finish if this is a directory transfer
        if let Some(handle) = extract_handle {
            if let PayloadSink::Channel(tx) = sink {
                drop(tx);
            }
            handle.join().map_err(|e| {
                let msg = if let Some(s) = e.downcast_ref::<String>() {
                    s.clone()
                } else if let Some(s) = e.downcast_ref::<&str>() {
                    (*s).to_owned()
                } else {
                    "unknown panic".to_owned()
                };
                EngineError::Io(io::Error::other(format!("extractor panicked: {msg}")))
            })??;
        }

        // Size validation
        if transfer_type == TRANSFER_FILE {
            if total != expected_size {
                return Err(EngineError::Io(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    format!(
                        "Transfer truncated: received {total} bytes, expected {expected_size} bytes"
                    ),
                )));
            }
        } else {
            // TRANSFER_DIR
            if total < expected_size {
                return Err(EngineError::Io(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    format!(
                        "Transfer truncated: received {total} bytes, expected at least {expected_size} bytes"
                    ),
                )));
            }
            if total < 1024 {
                return Err(EngineError::Io(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "Empty or truncated tar archive received".to_string(),
                )));
            }
        }

        Ok::<_, EngineError>(hasher.finalize(&algo))
    });

    // Network reader loop: each frame is length-prefixed.
    let mut read_error = None;
    let mut current_index = 0;
    loop {
        let len_buf_owned = vec![0u8; 4];
        let compio::BufResult(result, len_buf) = stream.read_exact(len_buf_owned).await;
        match result {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => {
                read_error = Some(e);
                break;
            }
        }
        let frame_len =
            u32::from_be_bytes([len_buf[0], len_buf[1], len_buf[2], len_buf[3]]) as usize;

        if frame_len == 0 || frame_len > (16 * 1024 * 1024) {
            read_error = Some(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("frame length out of range: {frame_len}"),
            ));
            break;
        }

        let enc_owned = vec![0u8; frame_len];
        let compio::BufResult(result, enc) = stream.read_exact(enc_owned).await;
        match result {
            Ok(()) => {
                if enc_tx.send_async(Ok((current_index, enc))).await.is_err() {
                    break;
                }
                current_index += 1;
            }
            Err(e) => {
                read_error = Some(e);
                break;
            }
        }
    }

    if let Some(e) = read_error {
        let _ = enc_tx.send_async(Err(e)).await;
    }

    drop(enc_tx);

    let hash = write_handle.await.map_err(|e| {
        EngineError::Io(io::Error::other(format!(
            "receive writer task failed: {e:?}"
        )))
    })??;
    Ok(hash)
}

// ---------------------------------------------------------------------------
// Split-stream wrappers (compio-quic SendStream / RecvStream)
// ---------------------------------------------------------------------------

/// Sends the payload using a split `compio_quic::SendStream`.
///
/// # Errors
///
/// Returns [`EngineError`] if sending fails.
#[allow(clippy::too_many_arguments)]
pub async fn send_payload_write(
    key: &[u8; 32],
    cipher_id: u8,
    source: PayloadSource,
    stream: &mut compio_quic::SendStream,
    compress: bool,
    filename: Option<&str>,
    hash_algo: &str,
    progress_cb: impl FnMut(u64),
) -> Result<String, EngineError> {
    send_payload(
        key,
        cipher_id,
        source,
        stream,
        compress,
        filename,
        hash_algo,
        progress_cb,
    )
    .await
}

/// Receives the payload using a split `compio_quic::RecvStream`.
///
/// # Errors
///
/// Returns [`EngineError`] if receiving fails.
#[allow(clippy::too_many_arguments)]
pub async fn receive_payload_split(
    key: &[u8; 32],
    cipher_id: u8,
    stream: &mut compio_quic::RecvStream,
    output_path: &Path,
    transfer_type: u8,
    expected_size: u64,
    hash_algo: &str,
    progress_cb: impl FnMut(u64) + 'static,
) -> Result<String, EngineError> {
    receive_payload(
        key,
        cipher_id,
        stream,
        output_path,
        transfer_type,
        expected_size,
        hash_algo,
        progress_cb,
    )
    .await
}

/// Writes the consent byte (accept/reject) to a split `compio_quic::SendStream`.
///
/// # Errors
///
/// Returns [`EngineError`] if writing fails.
pub async fn send_consent_write(
    stream: &mut compio_quic::SendStream,
    accept: bool,
) -> Result<(), EngineError> {
    send_consent(stream, accept).await
}

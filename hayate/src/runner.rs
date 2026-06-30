//! High-level developer-friendly API runners for sending and receiving files.
//!
//! This module provides the [`HayateSender`] and [`HayateReceiver`] builders, which
//! abstract away low-level QUIC socket bindings, stream negotiations, cryptographic handshakes,
//! consent flows, and file/directory transfers (including automatic tar packaging/extraction).

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use compio::io::AsyncRead;

use crate::{
    EngineError, network,
    protocol::{Metadata, TRANSFER_DIR, TRANSFER_FILE},
    transfer,
};

/// High-level builder for sending a file or directory over the network.
///
/// `HayateSender` handles both direct IP transfers and pairing-code-based discovery.
///
/// # Examples
///
/// ```no_run
/// use std::net::SocketAddr;
/// use hayate::runner::HayateSender;
///
/// # async fn run() -> Result<(), hayate::EngineError> {
/// let target: SocketAddr = "192.168.1.50:50001".parse().unwrap();
/// let sender = HayateSender::new()
///     .target(target)
///     .compress(true);
///
/// let checksum = sender.send("path/to/file.txt", |progress| {
///     println!("Sent {progress} bytes");
/// }).await?;
/// println!("Transfer complete. Checksum: {checksum}");
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct HayateSender {
    target: Option<SocketAddr>,
    code: Option<String>,
    compress: bool,
    hash_algo: String,
}

impl Default for HayateSender {
    fn default() -> Self {
        Self {
            target: None,
            code: None,
            compress: true,
            hash_algo: "blake3".to_owned(),
        }
    }
}

impl HayateSender {
    /// Creates a new `HayateSender` with default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the target receiver address.
    ///
    /// Use this for direct IP transfers. Mutually exclusive with [`Self::code`].
    #[must_use]
    pub fn target(mut self, target: SocketAddr) -> Self {
        self.target = Some(target);
        self.code = None;
        self
    }

    /// Sets a cryptographic code-phrase for pairing.
    ///
    /// The sender will broadcast its availability over the local subnet so the receiver
    /// can find and connect to it automatically. Mutually exclusive with [`Self::target`].
    #[must_use]
    pub fn code(mut self, code: String) -> Self {
        self.code = Some(code);
        self.target = None;
        self
    }

    /// Enables or disables zstd compression for the transfer (enabled by default).
    #[must_use]
    pub fn compress(mut self, compress: bool) -> Self {
        self.compress = compress;
        self
    }

    /// Sets the hash algorithm for payload integrity (default is "blake3").
    #[must_use]
    pub fn hash_algo(mut self, algo: String) -> Self {
        self.hash_algo = algo;
        self
    }

    /// Initiates the transfer of the file or directory at `path`.
    ///
    /// The `progress_cb` closure is periodically called with the total number of bytes
    /// written to the network.
    ///
    /// Returns the checksum of the transferred payload.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] if the path is invalid, network connection/handshake fails,
    /// or the transfer is rejected by the receiver.
    pub async fn send(
        self,
        path: impl AsRef<Path>,
        progress_cb: impl FnMut(u64) + Send + 'static,
    ) -> Result<String, EngineError> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(EngineError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Path does not exist: {}", path.display()),
            )));
        }

        let (meta, _) = self.build_metadata(path)?;

        // Establish the QUIC connection
        let (_endpoint, conn) = if let Some(target_addr) = self.target {
            let endpoint = network::bind_client().await?;
            let client_cfg = network::client_config()?;
            let connecting = endpoint.connect(target_addr, "hayate.local", Some(client_cfg))?;
            let conn = connecting.await?;
            (endpoint, conn)
        } else {
            let phrase = self.code.as_ref().ok_or_else(|| {
                EngineError::Handshake("Neither target nor code specified".into())
            })?;

            let bind_addr =
                SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED), 0);
            let endpoint = network::bind_server(bind_addr).await?;
            let local_port = endpoint.local_addr()?.port();

            let phrase_clone = phrase.clone();
            let channel_id = crate::discovery::derive_channel_id(&phrase_clone);
            let os_name = std::env::consts::OS.to_owned();
            let _broadcaster_guard =
                crate::discovery::start_broadcaster_hybrid(&channel_id, local_port, &os_name)
                    .map_err(|e| {
                        EngineError::Handshake(format!("broadcaster start failed: {e}"))
                    })?;

            let incoming = endpoint
                .wait_incoming()
                .await
                .ok_or_else(|| EngineError::Handshake("Endpoint closed during pairing".into()))?;
            let conn = incoming.await?;
            (endpoint, conn)
        };

        let (mut send_stream, mut recv_stream) = conn.open_bi()?;

        // Perform split handshake protocol
        let (key, cipher_id) = transfer::handshake_sender_split(
            &mut send_stream,
            &mut recv_stream,
            &meta,
            self.code.as_deref(),
        )
        .await?;

        // Send payload based on file or directory type
        let checksum = if path.is_dir() {
            self.send_directory(
                path,
                &key,
                cipher_id,
                &self.hash_algo,
                &mut send_stream,
                progress_cb,
            )
            .await?
        } else {
            self.send_file(
                path,
                &key,
                cipher_id,
                &self.hash_algo,
                &mut send_stream,
                progress_cb,
            )
            .await?
        };

        send_stream.finish()?;

        // Wait for the receiver to acknowledge completion with a time-bounded
        // read. If the receiver has closed the connection, reading returns EOF
        // or an error. The timeout prevents hanging if the receiver disappears.
        let drain_buf = vec![0u8; 1];
        let _ = compio::time::timeout(
            std::time::Duration::from_secs(10),
            recv_stream.read(drain_buf),
        )
        .await;

        conn.close(0u32.into(), b"complete");
        Ok(checksum)
    }

    /// Builds [`Metadata`] and estimates total byte size for `path`.
    ///
    /// Callers who perform their own QUIC connection and handshake can use this
    /// instead of [`Self::send`] to interleave terminal UI between stages.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] if the path has no filename or metadata cannot
    /// be read.
    pub fn build_metadata(&self, path: &Path) -> Result<(Metadata, u64), EngineError> {
        let filename = path
            .file_name()
            .ok_or_else(|| {
                EngineError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Path has no filename",
                ))
            })?
            .to_string_lossy()
            .into_owned();

        if path.is_dir() {
            let total = crate::tar::estimate_dir_size(path);
            Ok((
                Metadata::new(filename, total, TRANSFER_DIR, self.hash_algo.clone()),
                total,
            ))
        } else {
            let total = std::fs::metadata(path).map_err(EngineError::Io)?.len();
            Ok((
                Metadata::new(filename, total, TRANSFER_FILE, self.hash_algo.clone()),
                total,
            ))
        }
    }

    /// Sends a single file over an already-established QUIC send stream.
    ///
    /// Callers who perform their own QUIC connection and handshake can use this
    /// instead of [`Self::send`] to interleave terminal UI between stages.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] if file I/O, compression, encryption, or network
    /// write fails.
    pub async fn send_file(
        &self,
        path: &Path,
        key: &[u8; 32],
        cipher_id: u8,
        hash_algo: &str,
        stream: &mut compio_quic::SendStream,
        progress_cb: impl FnMut(u64) + Send + 'static,
    ) -> Result<String, EngineError> {
        let file = compio::fs::File::open(path)
            .await
            .map_err(EngineError::Io)?;
        let source = transfer::PayloadSource::File { file, pos: 0 };
        let filename = path.file_name().and_then(|s| s.to_str());
        transfer::send_payload_write(
            key,
            cipher_id,
            source,
            stream,
            self.compress,
            filename,
            hash_algo,
            progress_cb,
        )
        .await
    }

    /// Sends a directory as a tar stream over an already-established QUIC send stream.
    ///
    /// Callers who perform their own QUIC connection and handshake can use this
    /// instead of [`Self::send`] to interleave terminal UI between stages.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] if tar packaging, compression, encryption, or
    /// network write fails.
    pub async fn send_directory(
        &self,
        dir: &Path,
        key: &[u8; 32],
        cipher_id: u8,
        hash_algo: &str,
        stream: &mut compio_quic::SendStream,
        progress_cb: impl FnMut(u64) + Send + 'static,
    ) -> Result<String, EngineError> {
        let (tx, rx) = flume::bounded::<Result<Vec<u8>, std::io::Error>>(8);
        let dir_clone = dir.to_path_buf();

        std::thread::spawn(move || {
            use std::io::Write;
            struct ChanWriter {
                tx: flume::Sender<Result<Vec<u8>, std::io::Error>>,
            }
            impl std::io::Write for ChanWriter {
                fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                    self.tx.send(Ok(buf.to_vec())).map_err(|_| {
                        std::io::Error::new(std::io::ErrorKind::BrokenPipe, "receiver gone")
                    })?;
                    Ok(buf.len())
                }
                fn flush(&mut self) -> std::io::Result<()> {
                    Ok(())
                }
            }
            let writer = ChanWriter { tx: tx.clone() };
            let mut buffered_writer = std::io::BufWriter::with_capacity(128 * 1024, writer);
            let mut run = move || -> Result<(), std::io::Error> {
                crate::tar::write_tar_sync(&dir_clone, &mut buffered_writer)?;
                buffered_writer.flush()?;
                Ok(())
            };
            if let Err(e) = run() {
                let _ = tx.send(Err(e));
            }
        });

        let source = transfer::PayloadSource::Channel(rx);
        transfer::send_payload_write(
            key,
            cipher_id,
            source,
            stream,
            self.compress,
            None,
            hash_algo,
            progress_cb,
        )
        .await
    }
}

/// High-level builder for receiving a file or directory over the network.
///
/// `HayateReceiver` handles both direct IP listening and pairing-code-based discovery.
///
/// # Examples
///
/// ```no_run
/// use std::net::SocketAddr;
/// use hayate::runner::HayateReceiver;
///
/// # async fn run() -> Result<(), hayate::EngineError> {
/// let bind_addr: SocketAddr = "0.0.0.0:50001".parse().unwrap();
/// let receiver = HayateReceiver::new()
///     .bind(bind_addr);
///
/// let (checksum, path) = receiver.receive("downloads", |meta| {
///     println!("Accepting {} ({} bytes)?", meta.filename, meta.total_size);
///     true // Accept the transfer
/// }, |progress| {
///     println!("Received {progress} bytes");
/// }).await?;
///
/// println!("Successfully saved to {} (Checksum: {})", path.display(), checksum);
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct HayateReceiver {
    bind_addr: SocketAddr,
    code: Option<String>,
    auto_accept: bool,
}

impl Default for HayateReceiver {
    fn default() -> Self {
        Self {
            bind_addr: "0.0.0.0:50001".parse().unwrap(),
            code: None,
            auto_accept: false,
        }
    }
}

impl HayateReceiver {
    /// Creates a new `HayateReceiver` with default configuration (binding to `0.0.0.0:50001`).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the local address and port to bind to.
    #[must_use]
    pub fn bind(mut self, addr: SocketAddr) -> Self {
        self.bind_addr = addr;
        self
    }

    /// Sets a cryptographic code-phrase for pairing.
    ///
    /// The receiver will listen for UDP broadcast announcements matching this code-phrase,
    /// locate the sender, and connect automatically.
    #[must_use]
    pub fn code(mut self, code: String) -> Self {
        self.code = Some(code);
        self
    }

    /// Automatically accepts all incoming transfers without calling the consent callback.
    #[must_use]
    pub fn auto_accept(mut self, auto_accept: bool) -> Self {
        self.auto_accept = auto_accept;
        self
    }

    /// Starts the receiver and waits for a single incoming connection.
    ///
    /// Once connected, it performs the handshake, invokes `consent_cb` with the metadata
    /// of the incoming transfer, and if accepted, downloads the files to `output_dir`.
    ///
    /// The `progress_cb` closure is periodically called with the total number of bytes
    /// received from the network.
    ///
    /// Returns a tuple containing the checksum of the payload and the actual path
    /// where it was written.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] if pairing times out, connection or handshake fails,
    /// or the transfer is rejected.
    pub async fn receive(
        self,
        output_dir: impl AsRef<Path>,
        consent_cb: impl FnOnce(&Metadata) -> bool,
        progress_cb: impl FnMut(u64) + Send + 'static,
    ) -> Result<(String, PathBuf), EngineError> {
        let output_dir = output_dir.as_ref();

        let (_endpoint, conn) = if let Some(phrase) = &self.code {
            let Some((_name, peer_addr, _os)) = crate::discovery::listen_for_broadcast(
                Some(phrase.as_str()),
                Duration::from_mins(1),
            )?
            else {
                return Err(EngineError::Handshake(
                    "Timed out waiting for sender broadcast".into(),
                ));
            };

            let endpoint = network::bind_client().await?;
            let client_cfg = network::client_config()?;
            let connecting = endpoint.connect(peer_addr, "hayate.local", Some(client_cfg))?;
            let conn = connecting.await?;
            (endpoint, conn)
        } else {
            let endpoint = network::bind_server(self.bind_addr).await?;
            let incoming = endpoint
                .wait_incoming()
                .await
                .ok_or_else(|| EngineError::Handshake("Endpoint closed".into()))?;
            let conn = incoming.await?;
            (endpoint, conn)
        };

        let (mut send_stream, mut recv_stream) = conn.accept_bi().await?;

        // Perform handshake
        let ((key, cipher_id), meta) = transfer::handshake_receiver_split(
            &mut send_stream,
            &mut recv_stream,
            self.code.as_deref(),
        )
        .await?;

        // Ask for consent
        let accept = self.auto_accept || consent_cb(&meta);
        transfer::send_consent_write(&mut send_stream, accept).await?;

        if !accept {
            conn.close(0u32.into(), b"rejected");
            return Err(EngineError::TransferRejected);
        }

        let dest = resolve_output(output_dir, &meta);

        // Receive payload
        let checksum = transfer::receive_payload_split(
            &key,
            cipher_id,
            &mut recv_stream,
            &dest,
            meta.transfer_type,
            meta.total_size,
            &meta.hash_algo,
            progress_cb,
        )
        .await?;

        conn.close(0u32.into(), b"complete");
        Ok((checksum, dest))
    }
}

/// Helper to resolve the output path for received files/directories.
fn resolve_output(output_dir: &Path, meta: &Metadata) -> PathBuf {
    let name = Path::new(&meta.filename)
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("received_file"));
    output_dir.join(name)
}

//! `hayate send` subcommand.

use std::{io, net::ToSocketAddrs, path::Path, time::Instant};

use anyhow::{Context, Result, bail};
use compio::io::AsyncRead;
use hayate::{
    network,
    protocol::{Metadata, TRANSFER_DIR, TRANSFER_FILE},
    transfer,
};

use crate::{cli::SendArgs, output};

pub async fn run(args: SendArgs) -> Result<()> {
    let path = &args.path;
    if !path.exists() {
        bail!("Path does not exist: {}", path.display());
    }

    if args.peer.is_some() && args.target.is_some() {
        bail!("pass either TARGET or --peer, not both");
    }

    let target = args.peer.as_ref().or(args.target.as_ref());

    let (phrase, print_instruction) = if let Some(code) = &args.code {
        (code.clone(), false)
    } else if target.is_none() {
        let p = crate::words::generate_phrase();
        (p, true)
    } else {
        (String::new(), false)
    };

    // ── Stage 1: Connect ─────────────────────────────────────────────
    let (conn, passphrase) = if let Some(target_str) = target {
        let target_addr = target_str
            .to_socket_addrs()
            .context("invalid target address")?
            .next()
            .context("could not resolve target")?;

        output::stage("connect", format!("dialing {target_addr}"));

        let endpoint = network::bind_client().await?;
        let client_config = network::client_config()?;
        let spinner = if args.no_progress {
            None
        } else {
            let spinner = output::spinner("Connecting");
            spinner.set_message(target_addr.to_string());
            Some(spinner)
        };
        let conn_result: Result<_> =
            match endpoint.connect(target_addr, "hayate.local", Some(client_config)) {
                Ok(connecting) => connecting.await.map_err(Into::into),
                Err(e) => Err(e.into()),
            };
        if let Some(spinner) = &spinner {
            spinner.finish_and_clear();
        }
        let conn = conn_result?;
        (conn, args.code.clone())
    } else {
        if print_instruction {
            output::pairing_code(&phrase, &format!("hayate receive --code \"{phrase}\""));
        } else {
            output::stage("pairing", format!("waiting with code \"{phrase}\""));
        }

        let bind_addr: std::net::SocketAddr = "0.0.0.0:0".parse().unwrap();
        let endpoint = network::bind_server(bind_addr).await?;
        let local_port = endpoint.local_addr()?.port();

        let phrase_clone = phrase.clone();
        compio::runtime::spawn(async move {
            let channel_id = hayate::discovery::derive_channel_id(&phrase_clone);
            let _ = hayate::discovery::start_broadcaster(channel_id, local_port).await;
        })
        .detach();

        let spinner = if args.no_progress {
            None
        } else {
            let spinner = output::spinner("Pairing");
            spinner.set_message("waiting for receiver…");
            Some(spinner)
        };
        let incoming = endpoint
            .wait_incoming()
            .await
            .context("endpoint closed while waiting for pairing");
        let incoming = match incoming {
            Ok(incoming) => incoming,
            Err(e) => {
                if let Some(spinner) = &spinner {
                    spinner.finish_and_clear();
                }
                return Err(e);
            }
        };
        if let Some(spinner) = &spinner {
            spinner.set_message("receiver connected");
        }
        let conn_result = incoming.await;
        if let Some(spinner) = &spinner {
            spinner.finish_and_clear();
        }
        let conn = conn_result?;
        (conn, Some(phrase))
    };

    output::ok(&format!("Connected to {}", conn.remote_address()));

    let (mut send_stream, mut recv_stream) = conn.open_bi()?;

    // ── Stage 2: Prepare ─────────────────────────────────────────────
    let (meta, total_size) = build_metadata(path, args.hash.clone())?;

    // ── Stage 3: Handshake ───────────────────────────────────────────
    output::stage("handshake", "negotiating cipher…");
    let (key, cipher_id) = transfer::handshake_sender_split(
        &mut send_stream,
        &mut recv_stream,
        &meta,
        passphrase.as_deref(),
    )
    .await?;

    // ── Show transfer info card ──────────────────────────────────────
    let kind = if meta.transfer_type == TRANSFER_DIR {
        "directory"
    } else {
        "file"
    };
    output::print_info_card(
        "Sending",
        &[
            ("file", meta.filename.clone()),
            ("type", kind.to_owned()),
            ("size", output::format_bytes(total_size)),
            (
                "compress",
                if args.compress {
                    "zstd level 1".to_owned()
                } else {
                    "off".to_owned()
                },
            ),
            ("hash", args.hash.clone()),
            ("cipher", output::cipher_name(cipher_id).to_owned()),
            ("peer", conn.remote_address().to_string()),
        ],
    );

    // ── Stage 4: Transfer ────────────────────────────────────────────
    let pb = if args.no_progress || total_size == 0 {
        None
    } else {
        let pb = output::transfer_progress_bar("send", total_size);
        Some(pb)
    };

    let start = Instant::now();

    let checksum = if path.is_dir() {
        send_directory(
            path,
            &key,
            cipher_id,
            &args.hash,
            &mut send_stream,
            args.compress,
            |b| {
                if let Some(pb) = &pb {
                    output::set_transfer_position(pb, b);
                }
            },
        )
        .await?
    } else {
        send_file(
            path,
            &key,
            cipher_id,
            &args.hash,
            &mut send_stream,
            args.compress,
            |b| {
                if let Some(pb) = &pb {
                    output::set_transfer_position(pb, b);
                }
            },
        )
        .await?
    };

    send_stream.finish()?;

    // Wait for receiver to finish processing and close the connection
    let drain_buf = vec![0u8; 1];
    let _ = recv_stream.read(drain_buf).await;

    if let Some(pb) = &pb {
        output::finish_transfer_progress(pb, total_size);
    }

    // ── Stage 5: Summary ─────────────────────────────────────────────
    let elapsed = start.elapsed().as_secs_f64();
    output::print_transfer_summary(
        &meta.filename,
        total_size,
        elapsed,
        &checksum,
        args.compress,
        output::cipher_name(cipher_id),
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn build_metadata(path: &Path, hash: String) -> Result<(Metadata, u64)> {
    let filename = path
        .file_name()
        .context("path has no filename")?
        .to_string_lossy()
        .into_owned();

    if path.is_dir() {
        let total = hayate::tar::estimate_dir_size(path);
        Ok((
            Metadata {
                filename,
                total_size: total,
                transfer_type: TRANSFER_DIR,
                hash_algo: hash,
            },
            total,
        ))
    } else {
        let total = std::fs::metadata(path)?.len();
        Ok((
            Metadata {
                filename,
                total_size: total,
                transfer_type: TRANSFER_FILE,
                hash_algo: hash,
            },
            total,
        ))
    }
}

async fn send_file(
    path: &Path,
    key: &[u8; 32],
    cipher_id: u8,
    hash_algo: &str,
    stream: &mut compio_quic::SendStream,
    compress: bool,
    progress_cb: impl FnMut(u64),
) -> Result<String> {
    let file = compio::fs::File::open(path).await?;
    let source = hayate::transfer::PayloadSource::File { file, pos: 0 };
    let filename = path.file_name().and_then(|s| s.to_str());
    Ok(transfer::send_payload_write(
        key,
        cipher_id,
        source,
        stream,
        compress,
        filename,
        hash_algo,
        progress_cb,
    )
    .await?)
}

async fn send_directory(
    dir: &Path,
    key: &[u8; 32],
    cipher_id: u8,
    hash_algo: &str,
    stream: &mut compio_quic::SendStream,
    compress: bool,
    progress_cb: impl FnMut(u64),
) -> Result<String> {
    let (tx, rx) = flume::bounded::<Result<Vec<u8>, io::Error>>(8);
    let dir_clone = dir.to_path_buf();

    std::thread::spawn(move || {
        struct ChanWriter {
            tx: flume::Sender<Result<Vec<u8>, io::Error>>,
        }
        impl io::Write for ChanWriter {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                self.tx
                    .send(Ok(buf.to_vec()))
                    .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "receiver gone"))?;
                Ok(buf.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut writer = ChanWriter { tx: tx.clone() };
        if let Err(e) = hayate::tar::write_tar_sync(&dir_clone, &mut writer) {
            let _ = tx.send(Err(e));
        }
    });

    let source = hayate::transfer::PayloadSource::Channel(rx);
    Ok(transfer::send_payload_write(
        key,
        cipher_id,
        source,
        stream,
        compress,
        None,
        hash_algo,
        progress_cb,
    )
    .await?)
}

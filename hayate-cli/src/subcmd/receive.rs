//! `hayate receive` subcommand.

use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use hayate::{
    local_addr, network,
    protocol::{Metadata, TRANSFER_DIR},
    transfer,
};

use crate::{cli::ReceiveArgs, output};

pub async fn run(args: ReceiveArgs, cancelled: Arc<AtomicBool>) -> Result<()> {
    if let Some(code) = &args.code {
        // ── Pairing-code mode ────────────────────────────────────────
        output::stage("pairing", format!("scanning for code \"{code}\""));
        let spinner = if args.no_progress {
            None
        } else {
            let s = output::spinner("Discovering");
            s.set_message("listening for sender broadcast…");
            Some(s)
        };

        if cancelled.load(Ordering::SeqCst) {
            bail!("cancelled");
        }

        let peer_addr = match hayate::discovery::listen_for_broadcast(
            Some(code.as_str()),
            Duration::from_secs(60),
        )? {
            Some((_name, addr, _os)) => {
                if let Some(s) = &spinner {
                    s.finish_and_clear();
                }
                addr
            }
            None => {
                if let Some(s) = &spinner {
                    s.finish_and_clear();
                }
                bail!("Timed out waiting for sender broadcast.");
            }
        };

        output::stage("connect", format!("dialing sender at {peer_addr}"));
        let endpoint = network::bind_client().await?;
        let client_config = network::client_config()?;
        let spinner = if args.no_progress {
            None
        } else {
            let spinner = output::spinner("Connecting");
            spinner.set_message(peer_addr.to_string());
            Some(spinner)
        };
        let conn_result: Result<_> =
            match endpoint.connect(peer_addr, "hayate.local", Some(client_config)) {
                Ok(connecting) => connecting
                    .await
                    .context("Failed to establish QUIC connection to the sender"),
                Err(e) => Err(e.into()),
            };
        if let Some(spinner) = &spinner {
            spinner.finish_and_clear();
        }
        let conn = conn_result?;

        let peer = conn.remote_address();
        output::ok(&format!("Connected to {peer}"));

        let (mut send_stream, mut recv_stream) = conn
            .accept_bi()
            .await
            .context("Failed to accept bidirectional streams from sender")?;

        // ── Handshake ────────────────────────────────────────────────
        output::stage("handshake", "negotiating cipher…");
        let ((key, cipher_id), meta) = transfer::handshake_receiver_split(
            &mut send_stream,
            &mut recv_stream,
            Some(code.as_str()),
        )
        .await
        .context("Handshake cipher negotiation failed")?;

        // ── Transfer offer card ──────────────────────────────────────
        let kind = if meta.transfer_type == TRANSFER_DIR {
            "directory"
        } else {
            "file"
        };
        output::print_transfer_offer(
            &meta.filename,
            meta.total_size,
            kind,
            peer,
            output::cipher_name(cipher_id),
            &meta.hash_algo,
        );

        let dest = if args.auto_accept {
            Some(resolve_output(&args.output, &meta)?)
        } else {
            prompt_accept(&meta, peer, &args.output)?
        };

        let accept = dest.is_some();
        if cancelled.load(Ordering::SeqCst) {
            bail!("cancelled");
        }
        transfer::send_consent_write(&mut send_stream, accept)
            .await
            .context("Failed to send transfer acceptance to peer")?;
        if !accept {
            output::warn("Transfer rejected.");
            conn.close(0u32.into(), b"rejected");
            return Ok(());
        }
        let dest = dest.unwrap();

        // ── Receive ──────────────────────────────────────────────────
        output::stage("receive", &meta.filename);
        output::key_value("output", dest.display());
        let start = Instant::now();

        let pb = if args.no_progress || meta.total_size == 0 {
            None
        } else {
            let pb = output::transfer_progress_bar("receive", meta.total_size);
            Some(pb)
        };

        let pb_clone = pb.clone();
        let cancelled_clone = Arc::clone(&cancelled);
        let checksum_result = transfer::receive_payload_split(
            &key,
            cipher_id,
            &mut recv_stream,
            &dest,
            meta.transfer_type,
            meta.total_size,
            &meta.hash_algo,
            move |bytes| {
                if cancelled_clone.load(Ordering::SeqCst) {
                    return;
                }
                if let Some(pb) = &pb_clone {
                    output::set_transfer_position(pb, bytes);
                }
            },
        )
        .await
        .context("File transfer failed during payload delivery");

        if let Some(pb) = &pb {
            output::finish_transfer_progress(pb, meta.total_size);
        }

        if cancelled.load(Ordering::SeqCst) {
            bail!("transfer cancelled");
        }

        let checksum = checksum_result?;

        let elapsed = start.elapsed().as_secs_f64();
        output::print_transfer_summary(
            &meta.filename,
            meta.total_size,
            elapsed,
            &checksum,
            false,
            output::cipher_name(cipher_id),
        );

        // Finish our send stream to signal the sender we're done, then
        // close the connection gracefully.
        let _ = send_stream.finish();
        compio::time::sleep(std::time::Duration::from_millis(200)).await;
        conn.close(0u32.into(), b"complete");
        return Ok(());
    }

    // ── Direct listener mode ─────────────────────────────────────────
    let bind_addr = SocketAddr::new(args.bind, args.port);
    let endpoint = network::bind_server(bind_addr).await?;
    let local_port = endpoint.local_addr()?.port();
    if bind_addr.ip().is_unspecified() {
        let ips = local_addr::local_ipv4s();
        if ips.is_empty() {
            output::print_listener_active(format!("127.0.0.1:{local_port}"));
        } else {
            for ip in ips {
                output::print_listener_active(format!("{ip}:{local_port}"));
            }
        }
    } else {
        output::print_listener_active(endpoint.local_addr()?);
    }

    let spinner = if args.no_progress {
        None
    } else {
        let s = output::spinner("Waiting");
        s.set_message("for incoming connection…");
        Some(s)
    };

    loop {
        if cancelled.load(Ordering::SeqCst) {
            break;
        }

        let incoming = match endpoint.wait_incoming().await {
            Some(i) => {
                if let Some(s) = &spinner {
                    s.finish_and_clear();
                }
                i
            }
            None => break,
        };
        let conn = match incoming.await {
            Ok(c) => c,
            Err(e) => {
                output::err(&format!("Connection failed: {e}"));
                continue;
            }
        };
        let peer = conn.remote_address();
        output::ok(&format!("Connection from {peer}"));

        let (mut send_stream, mut recv_stream) = match conn.accept_bi().await {
            Ok(streams) => streams,
            Err(e) => {
                output::err(&format!("Failed to accept streams: {e}"));
                continue;
            }
        };

        output::stage("handshake", "negotiating cipher…");
        let ((key, cipher_id), meta) = match transfer::handshake_receiver_split(
            &mut send_stream,
            &mut recv_stream,
            None,
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                output::err(&format!("Handshake failed: {e}"));
                continue;
            }
        };

        // ── Transfer offer card ──────────────────────────────────────
        let kind = if meta.transfer_type == TRANSFER_DIR {
            "directory"
        } else {
            "file"
        };
        output::print_transfer_offer(
            &meta.filename,
            meta.total_size,
            kind,
            peer,
            output::cipher_name(cipher_id),
            &meta.hash_algo,
        );

        let dest = if args.auto_accept {
            Some(resolve_output(&args.output, &meta)?)
        } else {
            prompt_accept(&meta, peer, &args.output)?
        };

        let accept = dest.is_some();
        if cancelled.load(Ordering::SeqCst) {
            break;
        }
        if let Err(e) = transfer::send_consent_write(&mut send_stream, accept).await {
            output::err(&format!("Failed to send transfer consent: {e}"));
            continue;
        }
        if !accept {
            output::warn("Transfer rejected.");
            conn.close(0u32.into(), b"rejected");
            continue;
        }
        let dest = dest.unwrap();

        output::stage("receive", &meta.filename);
        output::key_value("output", dest.display());
        let start = Instant::now();

        let pb = if args.no_progress || meta.total_size == 0 {
            None
        } else {
            let pb = output::transfer_progress_bar("receive", meta.total_size);
            Some(pb)
        };

        let pb_clone = pb.clone();
        let cancelled_clone = Arc::clone(&cancelled);
        let receive_result = transfer::receive_payload_split(
            &key,
            cipher_id,
            &mut recv_stream,
            &dest,
            meta.transfer_type,
            meta.total_size,
            &meta.hash_algo,
            move |bytes| {
                if cancelled_clone.load(Ordering::SeqCst) {
                    return;
                }
                if let Some(pb) = &pb_clone {
                    output::set_transfer_position(pb, bytes);
                }
            },
        )
        .await;

        if let Some(pb) = &pb {
            output::finish_transfer_progress(pb, meta.total_size);
        }

        let checksum = match receive_result {
            Ok(checksum) => checksum,
            Err(e) => {
                output::err(&format!("Transfer failed: {e}"));
                conn.close(1u32.into(), b"failed");
                continue;
            }
        };

        if cancelled.load(Ordering::SeqCst) {
            break;
        }

        let elapsed = start.elapsed().as_secs_f64();
        output::print_transfer_summary(
            &meta.filename,
            meta.total_size,
            elapsed,
            &checksum,
            false,
            output::cipher_name(cipher_id),
        );

        // Finish our send stream to signal the sender, then close.
        let _ = send_stream.finish();
        compio::time::sleep(std::time::Duration::from_millis(200)).await;
        conn.close(0u32.into(), b"complete");
        break;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct DirCompleter;

impl inquire::Autocomplete for DirCompleter {
    fn get_suggestions(&mut self, input: &str) -> Result<Vec<String>, inquire::CustomUserError> {
        let path = std::path::Path::new(input);
        let (dir_path, prefix) = if input.ends_with('/') || input.is_empty() {
            (path, "")
        } else {
            (
                path.parent().unwrap_or_else(|| std::path::Path::new("")),
                path.file_name().and_then(|s| s.to_str()).unwrap_or(""),
            )
        };

        let dir_to_read = if dir_path.as_os_str().is_empty() {
            std::path::Path::new(".")
        } else {
            dir_path
        };

        let mut suggestions = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir_to_read) {
            for entry in entries.flatten() {
                let Ok(file_type) = entry.file_type() else {
                    continue;
                };
                if !file_type.is_dir() {
                    continue;
                }
                let name = entry.file_name();
                let Some(name_str) = name.to_str() else {
                    continue;
                };
                if !name_str.starts_with(prefix) {
                    continue;
                }

                let full_path = dir_path.join(name_str);
                let mut path_str = full_path.to_string_lossy().into_owned();
                if !path_str.ends_with('/') {
                    path_str.push('/');
                }
                suggestions.push(path_str);
            }
        }

        Ok(suggestions)
    }

    fn get_completion(
        &mut self,
        _input: &str,
        highlighted_suggestion: Option<String>,
    ) -> Result<inquire::autocompletion::Replacement, inquire::CustomUserError> {
        Ok(highlighted_suggestion)
    }
}

fn prompt_accept(
    meta: &Metadata,
    peer: SocketAddr,
    default_dir: &std::path::Path,
) -> Result<Option<PathBuf>> {
    let kind = if meta.transfer_type == TRANSFER_DIR {
        "directory"
    } else {
        "file"
    };

    let prompt = format!(
        "   Accept {kind} \"{}\" ({}) from {peer}?",
        meta.filename,
        output::format_bytes(meta.total_size)
    );

    let accept = match inquire::Confirm::new(&prompt).with_default(false).prompt() {
        Ok(val) => val,
        Err(inquire::InquireError::OperationInterrupted) => {
            std::process::exit(130);
        }
        Err(e) => return Err(anyhow::anyhow!(e)),
    };

    if accept {
        let dest_dir = match inquire::Text::new("   Save to directory")
            .with_default(&default_dir.to_string_lossy())
            .with_autocomplete(DirCompleter)
            .prompt()
        {
            Ok(val) => val,
            Err(inquire::InquireError::OperationInterrupted) => {
                std::process::exit(130);
            }
            Err(e) => return Err(anyhow::anyhow!(e)),
        };

        let dest = PathBuf::from(dest_dir);
        let name = std::path::Path::new(&meta.filename)
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("received_file"));
        Ok(Some(dest.join(name)))
    } else {
        Ok(None)
    }
}

fn resolve_output(output_dir: &std::path::Path, meta: &Metadata) -> Result<PathBuf> {
    let name = std::path::Path::new(&meta.filename)
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("received_file"));
    Ok(output_dir.join(name))
}

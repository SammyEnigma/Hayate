//! `hayate receive` subcommand.

use std::{
    io::{self, Write},
    net::SocketAddr,
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::{Result, bail};
use hayate::{
    local_addr, network,
    protocol::{Metadata, TRANSFER_DIR},
    transfer,
};

use crate::{cli::ReceiveArgs, output};

pub async fn run(args: ReceiveArgs) -> Result<()> {
    if let Some(code) = &args.code {
        output::stage("pairing", format!("scanning for code \"{code}\""));
        let peer_addr = match hayate::discovery::listen_for_broadcast(
            Some(code.clone()),
            Duration::from_secs(30),
        )
        .await?
        {
            Some((_name, addr, _os)) => addr,
            None => bail!("Timed out waiting for sender broadcast."),
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
                Ok(connecting) => connecting.await.map_err(Into::into),
                Err(e) => Err(e.into()),
            };
        if let Some(spinner) = &spinner {
            spinner.finish_and_clear();
        }
        let conn = conn_result?;

        let peer = conn.remote_address();
        output::ok(&format!("Connected to {peer}"));

        let (mut send_stream, mut recv_stream) = conn.accept_bi().await?;
        let ((key, cipher_id), meta) = transfer::handshake_receiver_split(
            &mut send_stream,
            &mut recv_stream,
            Some(code.as_str()),
        )
        .await?;
        output::key_value("cipher", output::cipher_name(cipher_id));

        let accept = if args.auto_accept {
            true
        } else {
            prompt_accept(&meta, peer)?
        };

        transfer::send_consent_write(&mut send_stream, accept).await?;
        if !accept {
            output::warn("Transfer rejected.");
            conn.close(0u32.into(), b"rejected");
            return Ok(());
        }

        output::stage("receive", &meta.filename);
        let dest = resolve_output(&args.output, &meta)?;
        output::key_value("output", dest.display());
        output::key_value("size", output::format_bytes(meta.total_size));
        let start = Instant::now();

        let pb = if args.no_progress || meta.total_size == 0 {
            None
        } else {
            let pb = output::transfer_progress_bar("receive", meta.total_size);
            Some(pb)
        };

        let pb_clone = pb.clone();
        let checksum_result = transfer::receive_payload_split(
            &key,
            cipher_id,
            &mut recv_stream,
            &dest,
            meta.transfer_type,
            meta.total_size,
            move |bytes| {
                if let Some(pb) = &pb_clone {
                    output::set_transfer_position(pb, bytes);
                }
            },
        )
        .await;

        if let Some(pb) = &pb {
            output::finish_transfer_progress(pb, meta.total_size);
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
        conn.close(0u32.into(), b"complete");
        return Ok(());
    }

    let bind_addr = SocketAddr::new(args.bind, args.port);
    let endpoint = network::bind_server(bind_addr).await?;
    let local_port = endpoint.local_addr()?.port();
    if bind_addr.ip().is_unspecified() {
        let ips = local_addr::local_ipv4s();
        if ips.is_empty() {
            output::info(&format!(
                "Listening on 127.0.0.1:{local_port} (QUIC / io_uring)"
            ));
        } else {
            for ip in ips {
                output::info(&format!("Listening on {ip}:{local_port} (QUIC / io_uring)"));
            }
        }
    } else {
        output::info(&format!(
            "Listening on {} (QUIC / io_uring)",
            endpoint.local_addr()?
        ));
    }
    output::info("Waiting for incoming connection...");

    loop {
        let incoming = match endpoint.wait_incoming().await {
            Some(i) => i,
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

        let accept = if args.auto_accept {
            true
        } else {
            prompt_accept(&meta, peer)?
        };

        if let Err(e) = transfer::send_consent_write(&mut send_stream, accept).await {
            output::err(&format!("Failed to send transfer consent: {e}"));
            continue;
        }
        if !accept {
            output::warn("Transfer rejected.");
            conn.close(0u32.into(), b"rejected");
            continue;
        }

        output::stage("receive", &meta.filename);
        let dest = resolve_output(&args.output, &meta)?;
        output::key_value("output", dest.display());
        output::key_value("size", output::format_bytes(meta.total_size));
        let start = Instant::now();

        let pb = if args.no_progress || meta.total_size == 0 {
            None
        } else {
            let pb = output::transfer_progress_bar("receive", meta.total_size);
            Some(pb)
        };

        let pb_clone = pb.clone();
        let receive_result = transfer::receive_payload_split(
            &key,
            cipher_id,
            &mut recv_stream,
            &dest,
            meta.transfer_type,
            meta.total_size,
            move |bytes| {
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

        let elapsed = start.elapsed().as_secs_f64();
        output::print_transfer_summary(
            &meta.filename,
            meta.total_size,
            elapsed,
            &checksum,
            false,
            output::cipher_name(cipher_id),
        );
        conn.close(0u32.into(), b"complete");
        break;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn prompt_accept(meta: &Metadata, peer: SocketAddr) -> Result<bool> {
    let kind = if meta.transfer_type == TRANSFER_DIR {
        "directory"
    } else {
        "file"
    };
    output::info(&format!(
        "Incoming {kind}: \"{}\" from {peer}",
        meta.filename
    ));
    print!("   Accept? [y/N]: ");
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(matches!(line.trim().to_lowercase().as_str(), "y" | "yes"))
}

fn resolve_output(output_dir: &std::path::Path, meta: &Metadata) -> Result<PathBuf> {
    let name = std::path::Path::new(&meta.filename)
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("received_file"));
    Ok(output_dir.join(name))
}

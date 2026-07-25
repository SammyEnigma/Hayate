//! `hayate send` subcommand.

use std::net::ToSocketAddrs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use hayate::protocol::TransferKind;
use hayate::{EngineError, HayateSender, TransferStage};
use indicatif::ProgressBar;

use crate::cli::SendArgs;
use crate::{history, output, peers, policy};

pub async fn run(args: SendArgs, cancelled: Arc<AtomicBool>) -> Result<()> {
    if cancelled.load(Ordering::SeqCst) {
        bail!("cancelled");
    }

    let path = match &args.path {
        Some(p) => p.clone(),
        None => prompt_path()?,
    };
    if !path.exists() {
        bail!("Path does not exist: {}", path.display());
    }

    // Resolve the effective target: positional addr > --to NAME > --pick.
    let target_str: Option<String> = if let Some(name) = &args.to {
        let addr = peers::resolve(name)?
            .with_context(|| format!("unknown peer \"{name}\" (see `hayate peers list`)"))?;
        output::info(&format!("Sending to saved peer \"{name}\" ({addr})"));
        Some(addr)
    } else if args.pick {
        Some(pick_peer(&cancelled).await?)
    } else {
        args.target.clone()
    };

    let compress = args.compress && !args.no_compress;
    let hash_algo = args.hash.as_str().to_owned();

    let rate_limit = match &args.bandwidth_limit {
        Some(raw) => Some(parse_rate(raw)?),
        None => None,
    };

    let mut builder = HayateSender::new().compress(compress).hash_algo(hash_algo.clone());
    if let Some(rate) = rate_limit {
        builder = builder.bandwidth_limit(rate);
    }

    // Pairing code: explicit `--code`, or auto-generated when no target is given.
    let (phrase, print_instruction) = if let Some(code) = &args.code {
        (Some(code.clone()), false)
    } else if target_str.is_none() {
        let p = crate::words::generate_phrase();
        (Some(p), policy::get().normal())
    } else {
        (None, false)
    };

    if let Some(target_str) = &target_str {
        let target_addr = target_str
            .to_socket_addrs()
            .context("invalid target address")?
            .next()
            .context("could not resolve target")?;
        builder = builder.target(target_addr);
        // Optional out-of-band secret on a direct transfer.
        if let Some(code) = phrase {
            builder = builder.passphrase(code);
        }
    } else {
        let phrase = phrase.expect("pairing mode always has a phrase");
        if print_instruction {
            output::pairing_code(&phrase, &format!("hayate receive --code \"{phrase}\""));
        }
        builder = builder.code(phrase);
    }

    let spinner: Arc<Mutex<Option<ProgressBar>>> = Arc::new(Mutex::new(None));
    let progress: Arc<Mutex<Option<ProgressBar>>> = Arc::new(Mutex::new(None));
    let no_progress = args.no_progress || policy::get().no_progress();
    let transfer_start = Arc::new(Mutex::new(None));

    let spinner_for_stages = Arc::clone(&spinner);
    let progress_for_stages = Arc::clone(&progress);
    let cancelled_transfer = Arc::clone(&cancelled);
    let transfer_start_stage = Arc::clone(&transfer_start);

    let result = builder
        .send_with(
            &path,
            move |stage| {
                if cancelled_transfer.load(Ordering::SeqCst) {
                    return Err(EngineError::Cancelled("transfer cancelled by user".into()));
                }
                match stage {
                    TransferStage::Connecting { peer } => {
                        output::stage("connect", format!("dialing {peer}"));
                        if !no_progress {
                            *spinner_for_stages.lock().unwrap() =
                                Some(output::spinner("Connecting", &peer.to_string()));
                        }
                    },
                    TransferStage::Pairing { code } => {
                        if !print_instruction {
                            output::stage("pairing", format!("waiting with code \"{code}\""));
                        }
                        if !no_progress {
                            *spinner_for_stages.lock().unwrap() =
                                Some(output::spinner("Pairing", "waiting for receiver…"));
                        }
                    },
                    TransferStage::Connected { peer } => {
                        clear_spinner(&spinner_for_stages);
                        output::ok(&format!("Connected to {peer}"));
                    },
                    TransferStage::Handshaking => {
                        output::stage("handshake", "negotiating cipher…");
                    },
                    TransferStage::Ready { meta, cipher_id, peer, total_size } => {
                        let kind = if meta.transfer_type == TransferKind::Directory {
                            "directory"
                        } else {
                            "file"
                        };
                        let mut rows = vec![
                            ("file", meta.filename.clone()),
                            ("type", kind.to_owned()),
                            ("size", output::format_bytes(total_size)),
                            (
                                "compress",
                                if compress { "zstd level 1".to_owned() } else { "off".to_owned() },
                            ),
                            ("hash", hash_algo.clone()),
                            ("cipher", output::cipher_name(cipher_id).to_owned()),
                            ("peer", peer.to_string()),
                        ];
                        if let Some(rate) = rate_limit {
                            rows.push(("limit", format!("{}/s", output::format_bytes(rate))));
                        }
                        output::print_info_card("Sending", &rows);
                        if !no_progress && total_size > 0 {
                            *progress_for_stages.lock().unwrap() =
                                Some(output::transfer_progress_bar("send", total_size));
                        }
                    },
                    TransferStage::Resuming { offset } => {
                        output::stage(
                            "resume",
                            format!("continuing from {}", output::format_bytes(offset)),
                        );
                    },
                    TransferStage::Transferring { .. } => {
                        *transfer_start_stage.lock().unwrap_or_else(|e| e.into_inner()) =
                            Some(Instant::now());
                    },
                    TransferStage::Finishing
                    | TransferStage::WaitingForPeer
                    | TransferStage::Discovering { .. }
                    | TransferStage::Offer { .. } => {},
                    // `TransferStage` is non_exhaustive for future engine stages.
                    _ => {},
                }
                Ok(())
            },
            {
                let cancelled = Arc::clone(&cancelled);
                let progress = Arc::clone(&progress);
                move |bytes| {
                    if cancelled.load(Ordering::SeqCst) {
                        return Err(EngineError::Cancelled("transfer cancelled by user".into()));
                    }
                    if let Some(pb) = progress.lock().unwrap().as_ref() {
                        output::set_transfer_position(pb, bytes);
                    }
                    Ok(())
                }
            },
        )
        .await;

    clear_spinner(&spinner);
    let outcome = match result {
        Ok(outcome) => outcome,
        Err(error) => {
            clear_progress(&progress);
            return Err(error).context("send failed");
        },
    };
    if let Some(pb) = progress.lock().unwrap().take() {
        output::finish_transfer_progress(&pb, outcome.total_size);
    }

    let elapsed = transfer_start
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .map_or(0.0, |start| start.elapsed().as_secs_f64());
    output::print_transfer_summary(
        &outcome.meta.filename,
        outcome.total_size,
        elapsed,
        &outcome.checksum,
        compress,
        output::cipher_name(outcome.cipher_id),
    );

    // Remember direct-transfer peers for `hayate send --to`.
    if target_str.is_some()
        && args.code.is_none()
        && let Err(e) = peers::record(&outcome.peer.ip().to_string(), &outcome.peer.to_string())
    {
        output::warn(&format!("could not save peer: {e}"));
    }
    if let Err(e) = history::record(history::HistoryEntry {
        ts: 0,
        direction: "send".to_owned(),
        filename: outcome.meta.filename.clone(),
        size: outcome.total_size,
        elapsed_secs: elapsed,
        speed_bps: if elapsed > f64::EPSILON {
            (outcome.total_size as f64 / elapsed) as u64
        } else {
            outcome.total_size
        },
        checksum: outcome.checksum.clone(),
        cipher: output::cipher_name(outcome.cipher_id).to_owned(),
        peer: outcome.peer.to_string(),
        path: None,
    }) {
        output::warn(&format!("could not record history: {e}"));
    }

    Ok(())
}

/// Interactive LAN scan + receiver selection for `--pick`.
async fn pick_peer(cancelled: &Arc<AtomicBool>) -> Result<String> {
    if !output::is_tty() {
        bail!("--pick requires an interactive terminal (no TTY detected)");
    }
    let found =
        crate::subcmd::discover::scan_for_peers(5, None, Arc::clone(cancelled), true, |_| {})
            .await?;
    if found.is_empty() {
        bail!("no receivers found on the network");
    }
    let options: Vec<String> = found.iter().map(|p| format!("{}  ({})", p.addr, p.name)).collect();
    let choice =
        output::suspend_for_prompt(|| inquire::Select::new("Select a receiver", options).prompt());
    match choice {
        Ok(selected) => {
            let idx = found
                .iter()
                .position(|p| selected.starts_with(&p.addr.to_string()))
                .context("selection did not match a discovered peer")?;
            Ok(found[idx].addr.to_string())
        },
        Err(inquire::InquireError::OperationInterrupted) => bail!("cancelled"),
        Err(e) => Err(anyhow::anyhow!(e)),
    }
}

/// Parses a throughput cap like `10MiB`, `500KiB`, `2M`, or a plain number
/// (bytes per second).
fn parse_rate(raw: &str) -> Result<u64> {
    let s = raw.trim();
    let split = s.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(s.len());
    let (num_part, unit_part) = s.split_at(split);
    let value: f64 =
        num_part.trim().parse().with_context(|| format!("invalid bandwidth limit \"{raw}\""))?;
    let mult: f64 = match unit_part.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1.0,
        "k" | "kb" => 1_000.0,
        "m" | "mb" => 1_000_000.0,
        "g" | "gb" => 1_000_000_000.0,
        "kib" => 1024.0,
        "mib" => 1024.0 * 1024.0,
        "gib" => 1024.0 * 1024.0 * 1024.0,
        other => bail!("unknown unit \"{other}\" in bandwidth limit (try 10MiB, 500KiB, 2M)"),
    };
    let rate = value * mult;
    if !(rate.is_finite() && rate > 0.0) {
        bail!("bandwidth limit must be positive");
    }
    Ok(rate as u64)
}

/// Interactive path prompt for `hayate send` with no positional path.
fn prompt_path() -> Result<PathBuf> {
    if !output::is_tty() {
        bail!("no path given and no TTY for the interactive picker");
    }
    let answer = output::suspend_for_prompt(|| {
        inquire::Text::new("File or directory to send").with_autocomplete(FileCompleter).prompt()
    });
    match answer {
        Ok(text) => {
            let p = PathBuf::from(text.trim());
            if p.as_os_str().is_empty() {
                bail!("no path given");
            }
            Ok(p)
        },
        Err(inquire::InquireError::OperationInterrupted) => bail!("cancelled"),
        Err(e) => Err(anyhow::anyhow!(e)),
    }
}

/// Path autocompletion for the send picker — suggests files and directories.
#[derive(Clone)]
struct FileCompleter;

impl inquire::Autocomplete for FileCompleter {
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

        let dir_to_read =
            if dir_path.as_os_str().is_empty() { std::path::Path::new(".") } else { dir_path };

        let mut suggestions = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir_to_read) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let Some(name_str) = name.to_str() else {
                    continue;
                };
                if !name_str.starts_with(prefix) {
                    continue;
                }
                let full_path = dir_path.join(name_str);
                let mut path_str = full_path.to_string_lossy().into_owned();
                if entry.file_type().is_ok_and(|t| t.is_dir()) && !path_str.ends_with('/') {
                    path_str.push('/');
                }
                suggestions.push(path_str);
            }
        }
        suggestions.sort();
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

fn clear_spinner(spinner: &Arc<Mutex<Option<ProgressBar>>>) {
    if let Some(s) = spinner.lock().unwrap_or_else(|e| e.into_inner()).take() {
        s.finish_and_clear();
    }
}

fn clear_progress(progress: &Arc<Mutex<Option<ProgressBar>>>) {
    if let Some(pb) = progress.lock().unwrap_or_else(|e| e.into_inner()).take() {
        pb.finish_and_clear();
    }
}

#[cfg(test)]
mod tests {
    use super::parse_rate;

    #[test]
    fn parse_rate_plain_bytes() {
        assert_eq!(parse_rate("1024").unwrap(), 1024);
    }

    #[test]
    fn parse_rate_binary_units() {
        assert_eq!(parse_rate("10MiB").unwrap(), 10 * 1024 * 1024);
        assert_eq!(parse_rate("500KiB").unwrap(), 500 * 1024);
        assert_eq!(parse_rate("1GiB").unwrap(), 1024 * 1024 * 1024);
    }

    #[test]
    fn parse_rate_decimal_units() {
        assert_eq!(parse_rate("2M").unwrap(), 2_000_000);
        assert_eq!(parse_rate("3kb").unwrap(), 3_000);
    }

    #[test]
    fn parse_rate_fractional() {
        assert_eq!(parse_rate("1.5MiB").unwrap(), 1024 * 1024 * 3 / 2);
    }

    #[test]
    fn parse_rate_rejects_garbage() {
        assert!(parse_rate("").is_err());
        assert!(parse_rate("fast").is_err());
        assert!(parse_rate("10TiB").is_err());
        assert!(parse_rate("0").is_err());
        assert!(parse_rate("-5M").is_err());
    }
}

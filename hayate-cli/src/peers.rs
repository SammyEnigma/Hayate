//! Named peer store backing `hayate peers` and `hayate send --to NAME`.
//!
//! Peers live in a small JSON map (`name -> "ip:port"`) inside the user
//! config directory, e.g. `~/.config/hayate/peers.json` on Linux.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use crate::cli::{PeersAction, PeersArgs};
use crate::output;

/// Resolves the on-disk location of the peer store.
pub fn store_path() -> Result<PathBuf> {
    let dir = dirs::config_dir().context("could not locate the user config directory")?;
    Ok(dir.join("hayate").join("peers.json"))
}

/// Loads all saved peers (empty map when the store does not exist yet).
pub fn load() -> Result<BTreeMap<String, String>> {
    let path = store_path()?;
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("could not read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("corrupt peer store at {}", path.display()))
}

/// Persists the full peer map, creating the config directory on first use.
fn save(peers: &BTreeMap<String, String>) -> Result<()> {
    let path = store_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(peers)?;
    std::fs::write(&path, text).with_context(|| format!("could not write {}", path.display()))
}

/// Validates a peer name: short, shell-friendly, no whitespace.
fn validate_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
    if !ok {
        bail!("invalid peer name \"{name}\" (use letters, digits, '-', '_', '.')");
    }
    Ok(())
}

/// Resolves a peer name to its saved `ip:port` string.
pub fn resolve(name: &str) -> Result<Option<String>> {
    Ok(load()?.get(name).cloned())
}

/// Saves (or overwrites) a peer entry.
pub fn record(name: &str, addr: &str) -> Result<()> {
    validate_name(name)?;
    let mut peers = load()?;
    peers.insert(name.to_owned(), addr.to_owned());
    save(&peers)
}

/// `hayate peers` subcommand entry point.
pub fn run(args: PeersArgs) -> Result<()> {
    match args.action {
        PeersAction::List => {
            let peers = load()?;
            if peers.is_empty() {
                output::info("No saved peers. Add one with `hayate peers add NAME ip:port`.");
                return Ok(());
            }
            let rows: Vec<(&str, String)> =
                peers.iter().map(|(name, addr)| (name.as_str(), addr.clone())).collect();
            output::print_info_card("Saved Peers", &rows);
            Ok(())
        },
        PeersAction::Add { name, addr } => {
            validate_name(&name)?;
            if addr.rsplit_once(':').and_then(|(_, p)| p.parse::<u16>().ok()).is_none() {
                bail!("invalid address \"{addr}\" (expected ip:port or hostname:port)");
            }
            record(&name, &addr)?;
            output::ok(&format!("Saved peer \"{name}\" -> {addr}"));
            Ok(())
        },
        PeersAction::Remove { name } => {
            let mut peers = load()?;
            if peers.remove(&name).is_some() {
                save(&peers)?;
                output::ok(&format!("Removed peer \"{name}\""));
            } else {
                output::warn(&format!("No peer named \"{name}\""));
            }
            Ok(())
        },
    }
}

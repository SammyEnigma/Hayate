//! Terminal output helpers: banner, status lines, progress bars, and summaries.

use console::style;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

const VERSION: &str = env!("CARGO_PKG_VERSION");

// ---------------------------------------------------------------------------
// Banner
// ---------------------------------------------------------------------------

pub fn print_banner() {
    let term = console::Term::stdout();
    let width = term.size_checked().map(|(_, w)| w).unwrap_or(80);

    if width >= 65 {
        let logo = r#"
  __   __     _____    __  __    _____    _______     _____  
 /\_\ /_/\   /\___/\ /\  /\  /\ /\___/\ /\_______)\ /\_____\ 
( ( (_) ) ) / / _ \ \\ \ \/ / // / _ \ \\(___  __\/( (_____/ 
 \ \___/ /  \ \(_)/ / \ \__/ / \ \(_)/ /  / / /     \ \__\   
 / / _ \ \  / / _ \ \  \__/ /  / / _ \ \ ( ( (      / /__/_  
( (_( )_) )( (_( )_) ) / / /  ( (_( )_) ) \ \ \    ( (_____\ 
 \/_/ \_\/  \/_/ \_\/  \/_/    \/_/ \_\/  /_/_/     \/_____/ 
"#;
        println!("{}", style(logo).bold().cyan());
    } else {
        let logo = r#"
  _  _  _  _  _  _ ___ ___ 
 | || |/ _ \| || / _ \ | | 
 | __ | (_) \  / | (_) | | 
 |_||_|\___/ \/   \___/|_| 
"#;
        println!("{}", style(logo).bold().cyan());
    }

    println!(
        "   {} {} {} {} {}",
        style("Hayate").bold().green(),
        style("|").dim(),
        style("encrypted LAN transfer").white(),
        style("|").dim(),
        style(format!("v{VERSION}")).cyan().bold()
    );
    println!("   {}\n", style("direct, fast, private").dim());
}

// ---------------------------------------------------------------------------
// Status lines
// ---------------------------------------------------------------------------

pub fn info(msg: &str) {
    println!("   {}  {}", style("*").bold().blue(), msg);
}

pub fn ok(msg: &str) {
    println!("   {}  {}", style("+").bold().green(), msg);
}

pub fn warn(msg: &str) {
    println!("   {}  {}", style("!").bold().yellow(), msg);
}

pub fn err(msg: &str) {
    eprintln!("   {}  {}", style("x").bold().red(), msg);
}

pub fn stage(name: &str, detail: impl std::fmt::Display) {
    println!(
        "   {}  {:<11} {}",
        style(">").bold().cyan(),
        style(name).bold(),
        detail
    );
}

pub fn key_value(key: &str, value: impl std::fmt::Display) {
    println!(
        "      {} {}",
        style(format!("{key:<10}")).dim(),
        style(value).white()
    );
}

pub fn pairing_code(code: &str, command: &str) {
    println!();
    println!("   {}", style("Pairing code").bold().cyan());
    println!("      {}", style(code).bold().yellow());
    println!("   {}", style("Receiver command").dim());
    println!("      {}", style(command).bold().green());
    println!();
}

#[must_use]
pub fn cipher_name(cipher_id: u8) -> &'static str {
    match cipher_id {
        hayate::crypto::CIPHER_AES256_GCM => "AES-256-GCM",
        hayate::crypto::CIPHER_CHACHA20 => "ChaCha20-Poly1305",
        _ => "unknown",
    }
}

// ---------------------------------------------------------------------------
// Progress bar
// ---------------------------------------------------------------------------

/// Creates a labelled transfer progress bar.
pub fn transfer_progress_bar(label: &str, total_bytes: u64) -> ProgressBar {
    let style = ProgressStyle::with_template(
        "   {prefix:.bold} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({percent:>3}%) {bytes_per_sec} ETA {eta_precise}",
    )
    .expect("valid template")
    .progress_chars("=> ");
    let pb = ProgressBar::new(total_bytes);
    pb.set_style(style);
    pb.set_prefix(label.to_owned());
    pb.set_draw_target(ProgressDrawTarget::stdout_with_hz(8));
    pb
}

pub fn set_transfer_position(pb: &ProgressBar, bytes: u64) {
    if let Some(len) = pb.length()
        && bytes > len
    {
        pb.set_length(bytes);
    }
    pb.set_position(bytes);
}

pub fn finish_transfer_progress(pb: &ProgressBar, total_bytes: u64) {
    set_transfer_position(pb, total_bytes.max(pb.position()));
    pb.finish_and_clear();
}

/// Creates a spinner for indeterminate progress.
#[allow(dead_code)]
pub fn spinner(prefix: &str) -> ProgressBar {
    let style = ProgressStyle::with_template(&format!(
        "   {{spinner:.cyan}} {} {{msg}}",
        style(prefix).bold()
    ))
    .expect("valid template")
    .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏");
    let pb = ProgressBar::new_spinner();
    pb.set_style(style);
    pb.set_draw_target(ProgressDrawTarget::stdout_with_hz(12));
    pb.enable_steady_tick(std::time::Duration::from_millis(80));
    pb
}

// ---------------------------------------------------------------------------
// Peer discovery table
// ---------------------------------------------------------------------------

pub fn print_peer_table(peers: &[(String, std::net::SocketAddr, String)]) {
    if peers.is_empty() {
        warn("No peers found.");
        return;
    }
    println!();
    ok(&format!("Found {} peer(s)", peers.len()));
    println!(
        "      {:<4} {:<22} {:<24} OS",
        style("#").dim(),
        style("NAME").dim(),
        style("ADDRESS").dim()
    );
    println!("      {}", style("-".repeat(60)).dim());
    for (idx, (name, addr, os)) in peers.iter().enumerate() {
        println!("      {:<4} {:<22} {:<24} {}", idx + 1, name, addr, os);
    }
    println!();
}

// ---------------------------------------------------------------------------
// Transfer summary
// ---------------------------------------------------------------------------

pub fn print_transfer_summary(
    filename: &str,
    bytes: u64,
    elapsed_secs: f64,
    checksum: &str,
    compressed: bool,
    cipher: &str,
) {
    println!();
    ok(&format!("Transfer complete: {filename}"));
    key_value("size", format_bytes(bytes));
    key_value("time", format!("{elapsed_secs:.2}s"));
    key_value(
        "speed",
        format!("{}/s", format_bytes(speed(bytes, elapsed_secs))),
    );
    key_value("cipher", cipher);
    key_value("compress", if compressed { "zstd" } else { "off" });
    key_value("checksum", checksum);
    println!();
}

#[must_use]
pub fn format_bytes(b: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut v = b as f64;
    let mut unit = UNITS[0];
    for &u in &UNITS[1..] {
        if v < 1024.0 {
            break;
        }
        v /= 1024.0;
        unit = u;
    }
    if v < 10.0 {
        format!("{v:.2} {unit}")
    } else if v < 100.0 {
        format!("{v:.1} {unit}")
    } else {
        format!("{v:.0} {unit}")
    }
}

fn speed(bytes: u64, elapsed_secs: f64) -> u64 {
    if elapsed_secs <= f64::EPSILON {
        return bytes;
    }
    (bytes as f64 / elapsed_secs) as u64
}

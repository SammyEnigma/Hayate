//! Terminal output helpers: banner, status lines, progress bars, cards, and summaries.
//!
//! All visual output flows through this module so the rest of the CLI never
//! constructs raw ANSI escapes or guesses column widths.

use console::style;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

const VERSION: &str = env!("GIT_VERSION");

// ─────────────────────────────────────────────────────────────────────────────
// Unicode status icons
// ─────────────────────────────────────────────────────────────────────────────

const ICON_INFO: &str = "ℹ";
const ICON_OK: &str = "✓";
const ICON_WARN: &str = "⚠";
const ICON_ERR: &str = "✗";
const ICON_ARROW: &str = "▶";
const ICON_DOT: &str = "●";
const ICON_LOCK: &str = "🔒";

// ─────────────────────────────────────────────────────────────────────────────
// Box-drawing primitives
// ─────────────────────────────────────────────────────────────────────────────

const BOX_TL: &str = "╭";
const BOX_TR: &str = "╮";
const BOX_BL: &str = "╰";
const BOX_BR: &str = "╯";
const BOX_H: &str = "─";
const BOX_V: &str = "│";

fn box_line(width: usize) -> String {
    BOX_H.repeat(width)
}

// ─────────────────────────────────────────────────────────────────────────────
// Banner
// ─────────────────────────────────────────────────────────────────────────────

pub fn print_banner() {
    let term = console::Term::stdout();
    let width = term.size_checked().map(|(_, w)| w).unwrap_or(80);

    if width >= 65 {
        let logo = r#"
  __   __     _____    __  __    _____    _______     _____  
 /\_\ /_/\   /\___/\ /\  /\  /\ /\___/\ /\_______)\  /\_____\ 
( ( (_) ) ) / / _ \ \\ \ \/ / // / _ \ \\(___  __\/( (_____/ 
 \ \___/ /  \ \(_)/ / \ \__/ / \ \(_)/ /  / / /     \ \__\   
 / / _ \ \  / / _ \ \  \__/ /  / / _ \ \ ( ( (      / /__/_  
( (_( )_) )( (_( )_) ) / / /  ( (_( )_) ) \ \ \    ( (_____ \ 
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
        style("│").dim(),
        style("encrypted LAN transfer").white(),
        style("│").dim(),
        style(format!("v{VERSION}")).cyan().bold()
    );
    println!("   {}", style("━".repeat(50)).dim());
    println!();
}

// ─────────────────────────────────────────────────────────────────────────────
// Status lines
// ─────────────────────────────────────────────────────────────────────────────

pub fn info(msg: &str) {
    println!(
        "   {}  {}",
        style(ICON_INFO).bold().blue(),
        style(msg).white()
    );
}

pub fn ok(msg: &str) {
    println!("   {}  {}", style(ICON_OK).bold().green(), msg);
}

pub fn warn(msg: &str) {
    println!(
        "   {}  {}",
        style(ICON_WARN).bold().yellow(),
        style(msg).yellow()
    );
}

pub fn err(msg: &str) {
    eprintln!("   {}  {}", style(ICON_ERR).bold().red(), style(msg).red());
}

pub fn stage(name: &str, detail: impl std::fmt::Display) {
    println!(
        "   {}  {:<11} {}",
        style(ICON_ARROW).bold().cyan(),
        style(name).bold(),
        style(detail).white()
    );
}

pub fn key_value(key: &str, value: impl std::fmt::Display) {
    println!(
        "      {} {}",
        style(format!("{key:<10}")).dim(),
        style(value).white().bold()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Pairing code display
// ─────────────────────────────────────────────────────────────────────────────

pub fn pairing_code(code: &str, command: &str) {
    let inner_width = 50;
    println!();
    println!(
        "   {}{}{}",
        style(BOX_TL).dim(),
        style(box_line(inner_width)).dim(),
        style(BOX_TR).dim()
    );
    println!(
        "   {}  {} {}{}",
        style(BOX_V).dim(),
        style(ICON_LOCK).bold(),
        style(" Pairing Code").bold().cyan(),
        pad_right("", inner_width - 18, BOX_V),
    );
    println!(
        "   {}{}{}",
        style(BOX_V).dim(),
        style(format!("  {}", box_line(inner_width - 2))).dim(),
        style(BOX_V).dim()
    );
    println!(
        "   {}  {}{}",
        style(BOX_V).dim(),
        style(code).bold().yellow(),
        pad_right(code, inner_width - 2, BOX_V),
    );
    println!(
        "   {}{}{}",
        style(BOX_V).dim(),
        style(format!("  {}", box_line(inner_width - 2))).dim(),
        style(BOX_V).dim()
    );
    println!(
        "   {}  {} {}{}",
        style(BOX_V).dim(),
        style(ICON_DOT).dim(),
        style("Run on receiver:").dim(),
        pad_right("● Run on receiver:", inner_width - 2, BOX_V),
    );
    println!(
        "   {}  {}{}",
        style(BOX_V).dim(),
        style(command).green().bold(),
        pad_right(command, inner_width - 2, BOX_V),
    );
    println!(
        "   {}{}{}",
        style(BOX_TL).dim().for_stderr(),
        style(box_line(inner_width)).dim(),
        style(BOX_BR).dim()
    );
    println!();
}

/// Returns padding + closing border character.
fn pad_right(content: &str, total_width: usize, border: &str) -> String {
    let content_len = console::measure_text_width(content);
    let pad = if total_width > content_len {
        total_width - content_len
    } else {
        1
    };
    format!("{}{}", " ".repeat(pad), style(border).dim())
}

#[must_use]
pub fn cipher_name(cipher_id: u8) -> &'static str {
    match cipher_id {
        hayate::crypto::CIPHER_AES256_GCM => "AES-256-GCM",
        hayate::crypto::CIPHER_CHACHA20 => "ChaCha20-Poly1305",
        _ => "unknown",
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Transfer info card
// ─────────────────────────────────────────────────────────────────────────────

/// Displays a compact info card with key-value pairs inside a box.
pub fn print_info_card(title: &str, rows: &[(&str, String)]) {
    let inner_width = 54;
    println!();
    // Top border
    println!(
        "   {}{}{}",
        style(BOX_TL).cyan(),
        style(box_line(inner_width)).cyan(),
        style(BOX_TR).cyan()
    );
    // Title row
    let title_display = format!("  {} {}", ICON_ARROW, title);
    println!(
        "   {} {}{}",
        style(BOX_V).cyan(),
        style(&title_display).bold().cyan(),
        pad_right_colored(&title_display, inner_width - 1, BOX_V),
    );
    // Separator
    println!(
        "   {}{}{}",
        style(BOX_V).cyan(),
        style(format!("  {}", box_line(inner_width - 2))).dim(),
        style(BOX_V).cyan()
    );
    // Key-value rows
    for (key, value) in rows {
        let row_text = format!("  {:<12} {}", key, value);
        println!(
            "   {}    {} {}{}",
            style(BOX_V).cyan(),
            style(format!("{key:<12}")).dim(),
            style(value).white().bold(),
            pad_right_colored(&row_text, inner_width - 1, BOX_V),
        );
    }
    // Bottom border
    println!(
        "   {}{}{}",
        style(BOX_BL).cyan(),
        style(box_line(inner_width)).cyan(),
        style(BOX_BR).cyan()
    );
    println!();
}

/// Returns padding + closing border (for colored content where we measure the raw text).
fn pad_right_colored(raw_text: &str, total_width: usize, border: &str) -> String {
    let content_len = console::measure_text_width(raw_text);
    let pad = if total_width > content_len {
        total_width - content_len
    } else {
        1
    };
    format!("{}{}", " ".repeat(pad), style(border).cyan())
}

// ─────────────────────────────────────────────────────────────────────────────
// Transfer offer card (for receiver prompts)
// ─────────────────────────────────────────────────────────────────────────────

/// Displays a transfer offer card before the accept/reject prompt.
pub fn print_transfer_offer(
    filename: &str,
    size: u64,
    kind: &str,
    peer: std::net::SocketAddr,
    cipher: &str,
    hash_algo: &str,
) {
    let rows = [
        ("filename", filename.to_owned()),
        ("type", kind.to_owned()),
        ("size", format_bytes(size)),
        ("from", peer.to_string()),
        ("cipher", cipher.to_owned()),
        ("hash", hash_algo.to_owned()),
    ];
    print_info_card("Incoming Transfer", &rows);
}

// ─────────────────────────────────────────────────────────────────────────────
// Progress bar
// ─────────────────────────────────────────────────────────────────────────────

/// Creates a labelled transfer progress bar with premium styling.
pub fn transfer_progress_bar(label: &str, total_bytes: u64) -> ProgressBar {
    let style = ProgressStyle::with_template(
        "   {prefix:.bold.cyan} [{elapsed_precise}] {wide_bar:.cyan/dark.cyan} {bytes}/{total_bytes} {bytes_per_sec:.green} ETA {eta_precise:.dim}",
    )
    .expect("valid template")
    .progress_chars("━━╸ ");
    let pb = ProgressBar::new(total_bytes);
    pb.set_style(style);
    pb.set_prefix(label.to_owned());
    pb.set_draw_target(ProgressDrawTarget::stdout_with_hz(12));
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
pub fn spinner(prefix: &str) -> ProgressBar {
    let style = ProgressStyle::with_template(&format!(
        "   {{spinner:.cyan.bold}} {} {{msg:.dim}}",
        style(prefix).bold()
    ))
    .expect("valid template")
    .tick_chars("⣾⣽⣻⢿⡿⣟⣯⣷⠿");
    let pb = ProgressBar::new_spinner();
    pb.set_style(style);
    pb.set_draw_target(ProgressDrawTarget::stdout_with_hz(12));
    pb.enable_steady_tick(std::time::Duration::from_millis(80));
    pb
}

/// Creates a progress bar for network scanning with host count.
pub fn scan_progress_bar(total_hosts: u64) -> ProgressBar {
    let style = ProgressStyle::with_template(
        "   {spinner:.cyan.bold} Scanning [{wide_bar:.blue/dark.blue}] {pos}/{len} hosts {msg:.dim}",
    )
    .expect("valid template")
    .progress_chars("━━╸ ")
    .tick_chars("⣾⣽⣻⢿⡿⣟⣯⣷⠿");
    let pb = ProgressBar::new(total_hosts);
    pb.set_style(style);
    pb.set_draw_target(ProgressDrawTarget::stdout_with_hz(12));
    pb.enable_steady_tick(std::time::Duration::from_millis(80));
    pb
}

// ─────────────────────────────────────────────────────────────────────────────
// Peer discovery table
// ─────────────────────────────────────────────────────────────────────────────

pub fn print_peer_table(peers: &[(String, std::net::SocketAddr, String)]) {
    if peers.is_empty() {
        warn("No peers found on the network.");
        return;
    }

    let inner_width = 62;
    println!();
    ok(&format!("Discovered {} peer(s)", peers.len()));
    println!();

    // Header
    println!(
        "   {}{}{}",
        style(BOX_TL).dim(),
        style(box_line(inner_width)).dim(),
        style(BOX_TR).dim()
    );
    println!(
        "   {}  {:<4} {:<22} {:<24} {}  {}",
        style(BOX_V).dim(),
        style("#").bold().dim(),
        style("NAME").bold().dim(),
        style("ADDRESS").bold().dim(),
        style("OS").bold().dim(),
        style(BOX_V).dim()
    );
    println!(
        "   {}{}{}",
        style(BOX_V).dim(),
        style(format!("  {}", box_line(inner_width - 2))).dim(),
        style(BOX_V).dim()
    );

    // Rows
    for (idx, (name, addr, os)) in peers.iter().enumerate() {
        let num = format!("{}", idx + 1);
        let name_display = if name.len() > 20 {
            format!("{}…", &name[..19])
        } else {
            name.clone()
        };
        let addr_str = addr.to_string();
        println!(
            "   {}  {:<4} {:<22} {:<24} {}{}",
            style(BOX_V).dim(),
            style(&num).cyan().bold(),
            style(&name_display).white(),
            style(&addr_str).green(),
            style(os).dim(),
            pad_right(
                &format!("  {num:<4} {name_display:<22} {addr_str:<24} {os}"),
                inner_width,
                BOX_V,
            )
        );
    }

    // Bottom border
    println!(
        "   {}{}{}",
        style(BOX_BL).dim(),
        style(box_line(inner_width)).dim(),
        style(BOX_BR).dim()
    );
    println!();
}

// ─────────────────────────────────────────────────────────────────────────────
// Transfer summary card
// ─────────────────────────────────────────────────────────────────────────────

pub fn print_transfer_summary(
    filename: &str,
    bytes: u64,
    elapsed_secs: f64,
    checksum: &str,
    compressed: bool,
    cipher: &str,
) {
    let speed_val = speed(bytes, elapsed_secs);
    let speed_str = format!("{}/s", format_bytes(speed_val));
    let speed_styled = color_speed(speed_val, &speed_str);

    let rows = [
        ("file", filename.to_owned()),
        ("size", format_bytes(bytes)),
        ("time", format!("{elapsed_secs:.2}s")),
        ("speed", speed_str.clone()),
        ("cipher", cipher.to_owned()),
        (
            "compress",
            if compressed {
                "zstd".to_owned()
            } else {
                "off".to_owned()
            },
        ),
        ("checksum", truncate_checksum(checksum)),
    ];

    let inner_width = 54;
    println!();
    // Top border
    println!(
        "   {}{}{}",
        style(BOX_TL).green(),
        style(box_line(inner_width)).green(),
        style(BOX_TR).green()
    );
    // Title
    let title_text = format!("  {} Transfer Complete", ICON_OK);
    println!(
        "   {} {}{}",
        style(BOX_V).green(),
        style(&title_text).bold().green(),
        pad_right_green(&title_text, inner_width - 1),
    );
    // Separator
    println!(
        "   {}{}{}",
        style(BOX_V).green(),
        style(format!("  {}", box_line(inner_width - 2))).dim(),
        style(BOX_V).green()
    );
    // Rows
    for (key, value) in &rows {
        let row_text = format!("  {:<12} {}", key, value);
        if *key == "speed" {
            println!(
                "   {}    {} {}{}",
                style(BOX_V).green(),
                style(format!("{key:<12}")).dim(),
                speed_styled,
                pad_right_green(&row_text, inner_width - 1),
            );
        } else {
            println!(
                "   {}    {} {}{}",
                style(BOX_V).green(),
                style(format!("{key:<12}")).dim(),
                style(value).white().bold(),
                pad_right_green(&row_text, inner_width - 1),
            );
        }
    }
    // Bottom border
    println!(
        "   {}{}{}",
        style(BOX_BL).green(),
        style(box_line(inner_width)).green(),
        style(BOX_BR).green()
    );
    println!();
}

fn pad_right_green(raw_text: &str, total_width: usize) -> String {
    let content_len = console::measure_text_width(raw_text);
    let pad = if total_width > content_len {
        total_width - content_len
    } else {
        1
    };
    format!("{}{}", " ".repeat(pad), style(BOX_V).green())
}

/// Color-code speed based on performance tiers.
fn color_speed(bytes_per_sec: u64, display: &str) -> String {
    const MB: u64 = 1024 * 1024;
    if bytes_per_sec >= 100 * MB {
        format!("{}", style(display).green().bold())
    } else if bytes_per_sec >= 10 * MB {
        format!("{}", style(display).yellow().bold())
    } else {
        format!("{}", style(display).red().bold())
    }
}

fn truncate_checksum(checksum: &str) -> String {
    if checksum.len() > 16 {
        format!("{}…{}", &checksum[..8], &checksum[checksum.len() - 8..])
    } else {
        checksum.to_owned()
    }
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

#[must_use]
pub fn get_backend_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "IOCP"
    }
    #[cfg(target_os = "macos")]
    {
        "kqueue"
    }
    #[cfg(target_os = "android")]
    {
        "epoll"
    }
    #[cfg(target_os = "linux")]
    {
        "io_uring"
    }
    #[cfg(not(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "android",
        target_os = "linux"
    )))]
    {
        "polling"
    }
}

pub fn print_listener_active(addr: impl std::fmt::Display) {
    let backend = get_backend_name();
    println!(
        "   {}  {} {} {} {} {}",
        style("●").bold().green(),
        style("Listening on").bold().white(),
        style(addr.to_string()).bold().yellow(),
        style("via").white(),
        style("QUIC").bold().magenta(),
        style(format!("[{backend}]")).bold().cyan()
    );
}

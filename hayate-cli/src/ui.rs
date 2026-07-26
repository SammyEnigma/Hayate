//! Transfer UI: the single owner of progress-bar and spinner lifecycle for
//! send/receive.
//!
//! Before this module, every subcommand juggled its own
//! `Arc<Mutex<Option<ProgressBar>>>` soup plus ad-hoc `clear_*` helpers, which
//! leaked bars on error paths and produced wrong speed/ETA math on resumed
//! transfers (bars started at position 0 while the payload continued from an
//! offset). `TransferUi` collapses all of that into one object:
//!
//! * bars are created, updated, and cleared in exactly one place;
//! * [`TransferUi::start_transfer`] seeds the bar at the resume offset, so
//!   indicatif's rate estimator measures real throughput from an honest
//!   baseline;
//! * `Drop` clears any live bar, making error paths bar-leak-free;
//! * bar creation is gated by the global output policy (JSON/plain/quiet modes
//!   never draw live bars).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use hayate::EngineError;
use indicatif::ProgressBar;

use crate::{output, policy};

/// Shared handle to a [`TransferUi`] — cheap to clone into stage/progress
/// closures.
#[derive(Clone)]
pub struct TransferUi {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    spinner: Option<ProgressBar>,
    progress: Option<ProgressBar>,
    /// Resume offset seen via `TransferStage::Resuming`; seeds the next bar.
    resume_offset: u64,
    start: Option<Instant>,
    cancelled: Arc<AtomicBool>,
    /// Subcommand-level suppression (`--no-progress`), OR-ed with policy.
    force_no_bars: bool,
}

impl TransferUi {
    /// Creates a UI bound to the shared cancellation flag.
    ///
    /// `no_progress` is the subcommand flag; bars are suppressed when either
    /// it or the global output policy says so.
    #[must_use]
    pub fn new(cancelled: Arc<AtomicBool>, no_progress: bool) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                spinner: None,
                progress: None,
                resume_offset: 0,
                start: None,
                cancelled,
                force_no_bars: no_progress,
            })),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Engine-cancellation check for stage/progress closures.
    pub fn check_cancelled(&self) -> Result<(), EngineError> {
        if self.lock().cancelled.load(Ordering::SeqCst) {
            return Err(EngineError::Cancelled("transfer cancelled by user".into()));
        }
        Ok(())
    }

    /// Shows an indeterminate spinner (no-op under no-progress policy).
    pub fn spinner(&self, label: &str, detail: &str) {
        let mut inner = self.lock();
        if inner.no_bars() {
            return;
        }
        inner.clear_spinner();
        inner.spinner = Some(output::spinner(label, detail));
    }

    /// Clears the spinner if one is active.
    pub fn clear_spinner(&self) {
        self.lock().clear_spinner();
    }

    /// Records a resume offset; the next [`Self::start_transfer`] seeds its
    /// bar from here.
    pub fn set_resume_offset(&self, offset: u64) {
        self.lock().resume_offset = offset;
    }

    /// Marks payload start and opens the transfer bar.
    ///
    /// `label` is `"send"` or `"receive"`. When `total` is 0 (unknown, e.g.
    /// directory streams) no bar is drawn.
    pub fn start_transfer(&self, label: &str, total: u64) {
        let mut inner = self.lock();
        inner.start = Some(Instant::now());
        if inner.no_bars() || total == 0 {
            return;
        }
        let pb = output::transfer_progress_bar(label, total);
        if inner.resume_offset > 0 {
            // Seed at the resume offset: the first engine progress report is
            // absolute (offset + bytes), and indicatif's rate estimator needs
            // this baseline to compute honest speed/ETA.
            pb.set_position(inner.resume_offset.min(total));
        }
        inner.progress = Some(pb);
    }

    /// Updates the transfer bar to an absolute payload position.
    pub fn set_position(&self, bytes: u64) {
        let inner = self.lock();
        if let Some(pb) = &inner.progress {
            output::set_transfer_position(pb, bytes);
        }
    }

    /// Finishes the transfer bar at `total` and clears it.
    pub fn finish_progress(&self, total: u64) {
        let mut inner = self.lock();
        if let Some(pb) = inner.progress.take() {
            output::finish_transfer_progress(&pb, total);
        }
    }

    /// Clears any live spinner/bar without finishing (error path). `Drop`
    /// also calls this, so it is safe to rely on scope exit alone.
    pub fn clear_all(&self) {
        let mut inner = self.lock();
        inner.clear_spinner();
        if let Some(pb) = inner.progress.take() {
            pb.finish_and_clear();
        }
    }

    /// Seconds since payload start (0.0 when never started).
    pub fn elapsed(&self) -> f64 {
        self.lock().start.map_or(0.0, |s| s.elapsed().as_secs_f64())
    }
}

impl Inner {
    fn no_bars(&self) -> bool {
        self.force_no_bars || policy::get().no_progress()
    }

    fn clear_spinner(&mut self) {
        if let Some(s) = self.spinner.take() {
            s.finish_and_clear();
        }
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.clear_spinner();
        if let Some(pb) = self.progress.take() {
            pb.finish_and_clear();
        }
    }
}

/// Filesystem path autocompletion for interactive prompts.
///
/// One completer serves both the send file-picker (`dirs_only: false`) and
/// the receive save-directory prompt (`dirs_only: true`); the only difference
/// is whether plain files appear as suggestions.
#[derive(Clone)]
pub struct PathCompleter {
    dirs_only: bool,
}

impl PathCompleter {
    /// Completes files and directories.
    #[must_use]
    pub fn files_and_dirs() -> Self {
        Self { dirs_only: false }
    }

    /// Completes directories only.
    #[must_use]
    pub fn dirs_only() -> Self {
        Self { dirs_only: true }
    }
}

impl inquire::Autocomplete for PathCompleter {
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
                let Ok(file_type) = entry.file_type() else {
                    continue;
                };
                if self.dirs_only && !file_type.is_dir() {
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
                if file_type.is_dir() && !path_str.ends_with('/') {
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

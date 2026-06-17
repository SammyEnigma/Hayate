//! Streaming tar archive reader for directory transfers.
//!
//! Directory packaging uses the synchronous `tar` crate on a dedicated thread
//! and forwards archive chunks into the encrypted async send pipeline.
//!
//! Extraction also runs on a dedicated thread because archive parsing and
//! filesystem traversal are synchronous. The extractor validates paths before
//! unpacking entries.

use std::{
    io,
    path::{Component, Path},
};

use crate::EngineError;

// ---------------------------------------------------------------------------
// Synchronous tar generator (run inside compio::spawn_blocking)
// ---------------------------------------------------------------------------

/// Generates a POSIX ustar tar stream for `root_dir` and writes it into `out`.
/// This function is entirely synchronous and must be called on a dedicated
/// thread when used from async code.
pub fn write_tar_sync(root_dir: &Path, out: &mut impl io::Write) -> Result<u64, io::Error> {
    let mut builder = tar::Builder::new(out);
    builder.follow_symlinks(false);
    builder.append_dir_all(".", root_dir)?;
    builder.finish()?;
    // Return 0 — actual byte count not needed; caller tracks progress via
    // the write callbacks.
    Ok(0)
}

// ---------------------------------------------------------------------------
// Safe extraction
// ---------------------------------------------------------------------------

/// Extracts a tar stream read from `input` into `output_dir`.
///
/// Path-traversal protection: rejects any entry whose cleaned path starts
/// outside `output_dir` (i.e. `..` components or absolute paths).
pub fn extract_tar_sync(input: impl io::Read, output_dir: &Path) -> Result<(), EngineError> {
    std::fs::create_dir_all(output_dir).map_err(EngineError::Io)?;

    let mut archive = tar::Archive::new(input);
    for entry in archive.entries().map_err(EngineError::Io)? {
        let mut entry = entry.map_err(EngineError::Io)?;
        let entry_path = entry.path().map_err(EngineError::Io)?;
        let entry_type = entry.header().entry_type();

        if entry_type.is_symlink() || entry_type.is_hard_link() {
            return Err(EngineError::PathTraversal);
        }

        // Validate: no absolute paths, no `..` traversal.
        if entry_path.is_absolute() {
            return Err(EngineError::PathTraversal);
        }
        for component in entry_path.components() {
            if matches!(component, Component::ParentDir) {
                return Err(EngineError::PathTraversal);
            }
        }

        let dest = output_dir.join(&entry_path);
        // Final check: dest must be under output_dir.
        if !dest.starts_with(output_dir) {
            return Err(EngineError::PathTraversal);
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(EngineError::Io)?;
        }
        entry.unpack(&dest).map_err(EngineError::Io)?;
    }
    Ok(())
}

/// Estimates the total size of all files under `root_dir` by walking the tree.
/// Used to report an approximate total for the progress bar.
pub fn estimate_dir_size(root_dir: &Path) -> u64 {
    walkdir::WalkDir::new(root_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::Cursor,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    fn temp_output(name: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is before UNIX_EPOCH")
            .as_nanos();
        std::env::temp_dir().join(format!("hayate-{name}-{}-{unique}", std::process::id()))
    }

    #[test]
    fn extract_tar_creates_output_root() {
        let mut archive = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut archive);
            let bytes = b"hello";
            let mut header = tar::Header::new_gnu();
            header.set_path("nested/file.txt").unwrap();
            header.set_size(bytes.len() as u64);
            header.set_cksum();
            builder.append(&header, bytes.as_slice()).unwrap();
            builder.finish().unwrap();
        }

        let out = temp_output("extract-root");
        let result = extract_tar_sync(Cursor::new(archive), &out);
        assert!(result.is_ok());
        assert_eq!(
            fs::read_to_string(out.join("nested/file.txt")).unwrap(),
            "hello"
        );

        fs::remove_dir_all(out).unwrap();
    }

    #[test]
    fn extract_tar_rejects_symlink_entries() {
        let mut archive = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut archive);
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_path("link").unwrap();
            header.set_link_name("../outside").unwrap();
            header.set_size(0);
            header.set_cksum();
            builder.append(&header, Cursor::new(Vec::new())).unwrap();
            builder.finish().unwrap();
        }

        let out = temp_output("reject-link");
        let result = extract_tar_sync(Cursor::new(archive), &out);
        assert!(matches!(result, Err(EngineError::PathTraversal)));

        let _ = fs::remove_dir_all(out);
    }
}

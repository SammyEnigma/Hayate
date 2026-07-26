//! End-to-end CLI tests driving the real `hayate` binary (assert_cmd style,
//! kept inline per repo convention — there are no `tests/` directories).
//!
//! Every test runs with `HOME` pointed at a fresh tempdir, so the peers store
//! and history log land in throwaway locations on every platform (dirs-rs
//! resolves config/data dirs relative to `HOME`).

#[cfg(test)]
mod tests {
    use assert_cmd::Command;
    use predicates::prelude::*;
    use tempfile::TempDir;

    /// `hayate` command with an isolated HOME.
    fn hayate(home: &TempDir) -> Command {
        let mut cmd = Command::new(hayate_bin());
        cmd.env("HOME", home.path())
            .env("NO_COLOR", "1")
            .env("XDG_CONFIG_HOME", home.path().join(".config"))
            .env("XDG_DATA_HOME", home.path().join(".local/share"));
        cmd
    }

    /// Locates the built `hayate` binary: `CARGO_BIN_EXE` in integration-test
    /// contexts, otherwise the sibling of the test binary in `target/<prof>`.
    fn hayate_bin() -> std::path::PathBuf {
        if let Some(path) = option_env!("CARGO_BIN_EXE_hayate") {
            return path.into();
        }
        let mut dir = std::env::current_exe().expect("test binary path");
        dir.pop(); // …/deps
        dir.pop(); // …/<profile>
        dir.push(if cfg!(windows) { "hayate.exe" } else { "hayate" });
        assert!(dir.exists(), "hayate binary not found at {}", dir.display());
        dir
    }

    #[test]
    fn version_prints() {
        let home = TempDir::new().unwrap();
        hayate(&home)
            .arg("--version")
            .assert()
            .success()
            .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn man_renders_roff() {
        let home = TempDir::new().unwrap();
        hayate(&home)
            .arg("man")
            .assert()
            .success()
            .stdout(predicate::str::contains(".TH hayate 1"))
            .stdout(predicate::str::contains("hayate\\-"));
    }

    #[test]
    fn peers_add_list_remove_roundtrip() {
        let home = TempDir::new().unwrap();

        hayate(&home)
            .args(["peers", "add", "laptop", "192.168.1.20:50001"])
            .assert()
            .success()
            .stdout(predicate::str::contains("Saved peer \"laptop\""));

        hayate(&home)
            .args(["peers", "list"])
            .assert()
            .success()
            .stdout(predicate::str::contains("laptop"))
            .stdout(predicate::str::contains("192.168.1.20:50001"));

        hayate(&home)
            .args(["peers", "remove", "laptop"])
            .assert()
            .success()
            .stdout(predicate::str::contains("Removed peer \"laptop\""));

        hayate(&home)
            .args(["peers", "list"])
            .assert()
            .success()
            .stdout(predicate::str::contains("No saved peers"));
    }

    #[test]
    fn peers_add_rejects_bad_address() {
        let home = TempDir::new().unwrap();
        hayate(&home)
            .args(["peers", "add", "laptop", "not-an-address"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("invalid address"));
    }

    #[test]
    fn peers_add_rejects_bad_name() {
        let home = TempDir::new().unwrap();
        hayate(&home)
            .args(["peers", "add", "bad name!", "1.2.3.4:50001"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("invalid peer name"));
    }

    #[test]
    fn send_to_unknown_peer_fails_cleanly() {
        let home = TempDir::new().unwrap();
        let file = home.path().join("f.bin");
        std::fs::write(&file, b"x").unwrap();
        hayate(&home)
            .args(["send", file.to_str().unwrap(), "--to", "ghost"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("unknown peer \"ghost\""));
    }

    #[test]
    fn send_rejects_garbage_bandwidth_limit() {
        let home = TempDir::new().unwrap();
        let file = home.path().join("f.bin");
        std::fs::write(&file, b"x").unwrap();
        hayate(&home)
            .args([
                "send",
                file.to_str().unwrap(),
                "127.0.0.1:50001",
                "--bandwidth-limit",
                "ludicrous",
            ])
            .assert()
            .failure()
            .stderr(predicate::str::contains("invalid bandwidth limit"));
    }

    #[test]
    fn send_missing_path_reports() {
        let home = TempDir::new().unwrap();
        hayate(&home)
            .args(["send", "/definitely/not/here.bin", "127.0.0.1:50001"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("Path does not exist"));
    }

    #[test]
    fn history_empty_and_clear() {
        let home = TempDir::new().unwrap();
        hayate(&home)
            .args(["history"])
            .assert()
            .success()
            .stdout(predicate::str::contains("No transfers recorded yet"));

        hayate(&home)
            .args(["history", "--clear"])
            .assert()
            .success()
            .stdout(predicate::str::contains("History cleared"));
    }

    #[test]
    fn docs_topics_render() {
        let home = TempDir::new().unwrap();
        for topic in ["send", "receive", "peers", "history", "security"] {
            hayate(&home).args(["docs", topic]).assert().success();
        }
    }

    #[test]
    fn docs_unknown_topic_fails_with_suggestions() {
        let home = TempDir::new().unwrap();
        hayate(&home)
            .args(["docs", "nonsense"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("unknown docs topic"));
    }

    #[test]
    fn json_events_carry_schema_tag() {
        let home = TempDir::new().unwrap();
        hayate(&home)
            .args(["--format", "json", "history"])
            .assert()
            .success()
            .stdout(predicate::str::contains("\"schema\":\"hayate/1\""));
    }
}

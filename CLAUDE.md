# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project overview

Hayate is an encrypted, compressed LAN file transfer tool written in Rust. It uses QUIC transport with application-layer crypto (X25519 + AEAD) and automatic peer discovery via mDNS + UDP broadcast. No cloud, no accounts — peers pair with a 4-word code phrase.

- **Crates**: `hayate` (library — the transfer engine) and `hayate-cli` (binary — the CLI)
- **Runtime**: `compio` (io_uring/IOCP/kqueue) — _not_ tokio
- **MSRV**: 1.96 (stable)
- **Edition**: Rust 2024

## Common commands

Uses `just` for task orchestration. All commands run from the workspace root.

```bash
# Development loop
just fmt              # Format code
just clippy           # Clippy with -D warnings
just test             # Run all tests (`cargo test --workspace`)
just check            # fmt-check + clippy + test (full pre-commit gate)

# Build
just build            # Release build of hayate-cli
just build-dist       # Release build with LTO=thin (smaller binary)
just run <args>       # Run the CLI (`cargo run -p hayate-cli -- <args>`)

# Cross-compile
just build-all        # macOS (x86_64+aarch64) + Linux (x86_64+musl)
just build-windows    # Windows MSVC (x86_64)

# Docs site (cd docs/ first)
pnpm dev              # Live preview at localhost:5173
pnpm build            # Production build into doc_build/
pnpm format           # Prettier
```

Additional Cargo aliases are defined in `.cargo/config.toml`:
- `cargo check-all` — check workspace with all targets
- `cargo clippy-all` — clippy workspace with all targets
- `cargo test-all` — test workspace
- `cargo bench-all` — bench workspace (needs `--features benchmarks`)

## Running a single test

```bash
cargo test -p hayate <test_name>           # specific test in the library
cargo test -p hayate-cli <test_name>       # specific test in the CLI crate
cargo test -- test_name --nocapture        # with stdout
```

## Architecture

### Crate separation

The library (`hayate/`) exposes two builder-pattern entry points:

- **`HayateSender`** — configures cipher, compression, hash algorithm; connects to a receiver by peer address + code phrase; sends files or directories.
- **`HayateReceiver`** — binds a QUIC endpoint, listens for incoming transfers, accepts/rejects based on metadata, writes received data to disk.

The CLI (`hayate-cli/`) is a thin layer over the library: clap argument parsing → `HayateSender`/`HayateReceiver` builder → progress bars and terminal UI via `indicatif` + `console`.

### Key modules (hayate/src/)

| Module | Role |
|---|---|
| `runner.rs` | High-level `HayateSender` / `HayateReceiver` builder API |
| `transfer.rs` | Handshake state machine + payload send/receive pipeline |
| `protocol.rs` | Wire format: version negotiation, `Metadata` struct, frame encoding |
| `crypto.rs` | X25519 ECDH, HKDF-SHA256 key derivation, AEAD seal/open, cipher capability negotiation |
| `network.rs` | QUIC endpoint setup, ephemeral TLS certs (`rcgen`), transport config |
| `discovery.rs` | Hybrid mDNS + UDP broadcast peer discovery |
| `pool.rs` | Thread-safe pre-allocated `BufferPool` (flume channels) for hot-path buffer reuse |
| `tar.rs` | Directory → tar stream (sender) and safe extraction (receiver, rejects absolute/`..`/symlink paths) |
| `local_addr.rs` | Network interface detection (via `if-addrs`) and subnet resolution |
| `error.rs` | `EngineError` enum |

### Protocol overview

1. QUIC connection with ephemeral self-signed TLS cert (trust-on-first-use)
2. Application handshake over bidirectional stream:
   - Sender sends: protocol version + cipher capability (AES-256-GCM or ChaCha20-Poly1305)
   - X25519 key exchange (32-byte public keys each way)
   - Receiver selects cipher
   - Sender sends encrypted metadata (filename, size, type, hash algorithm)
   - Receiver sends accept/reject byte
3. Payload transfer: length-prefixed AEAD-encrypted frames, optionally zstd-compressed (auto-skipped for already-compressed formats: `.zip`, `.mp4`, `.jpg`, etc.)
4. Stream hash (blake3/rapidhash/sha256) computed on plaintext for integrity verification

### Threading model

- `compio` runs a thread-per-core async event loop (one thread per CPU on io_uring)
- CPU-intensive work (AEAD encrypt/decrypt, zstd compress/decompress) is offloaded to dedicated `std::thread` workers communicating via `flume` channels
- `BufferPool` pre-allocates fixed-size buffers to avoid hot-path allocation
- Receiver reorders chunks via `BTreeMap` to guarantee sequential disk writes
- Directory tar extraction runs on its own synchronous thread

### Discovery

- Sender broadcasts on both mDNS (`_hayate._udp.local.` with TXT records) and raw UDP (`255.255.255.255:50002`)
- Receiver listens on both channels simultaneously
- Channel ID = `SHA-256(phrase)[0..4]` as hex — only matching peers connect
- `discover` subcommand probes all hosts in each detected subnet with 128 concurrent QUIC connections + RTT measurement

## Benchmarks

In `hayate/benches/`:
```bash
cargo bench --features benchmarks
```

Uses `codspeed-divan-compat` (Valgrind-based CPU simulation in CI via `.github/workflows/codspeed.yml`).

## Release process

See `RELEASE.md` for full details. Summary:

1. Push conventional commits to master (`feat:`, `fix:`, `refactor:`, `docs:`)
2. release-plz CI opens a PR with version bump + changelog
3. Edit the PR's `CHANGELOG.md` to add detail, then merge
4. release-plz publishes `hayate` to crates.io + pushes a `vX.Y.Z` tag
5. cargo-dist CI (triggered by tag) builds 8 platform binaries + installers → GitHub Release

Both crates share `[workspace.package] version` — they stay in lockstep. `hayate-cli` has `publish = false`.

**Never**: force-push tags, manually bump Cargo.toml version, or merge two release PRs at once.

## CI

Five workflows (`.github/workflows/`):

| Workflow | Trigger | What |
|---|---|---|
| `ci.yml` | push to master, PRs | fmt, clippy, MSRV check, security audit, test matrix (ubuntu/macos/windows/windows-arm) |
| `codspeed.yml` | push to master, PRs | CodSpeed benchmarks |
| `release-plz.yml` | push to master | Creates release PR; publishes to crates.io on merge |
| `release.yml` | tag `v*` | cargo-dist binary builds + GitHub Release |
| `deploy-docs.yml` | push to master (docs/ changes) | Rspress docs → GitHub Pages |

Dependabot updates GitHub Actions and Cargo deps weekly (`.github/dependabot.yml`).

## Lint configuration

Workspace lints in `Cargo.toml`:
- `missing_docs = "warn"` (all public items must be documented)
- Clippy: correctness = deny, suspicious/style/complexity/perf = warn

## Code style

- Public API uses the builder pattern (`HayateSender::new().cipher(...).code_phrase(...).send(path)`)
- Error type is `EngineError` (thiserror enum) in the library; `anyhow::Result` in the CLI
- Module docs (`//!`) on every source file describing the module's role
- `#[cfg(test)] mod tests` blocks inline in source files (no separate test files)
- Runtime-agnostic library: the library crate does not depend on `compio` directly at the public API level; the runtime is internal

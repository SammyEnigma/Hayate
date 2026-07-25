---
packages:
  hayate: minor
  hayate-cli: minor
---

### Resumable transfers, rate limiting, and a richer CLI

- **Resume interrupted transfers**: `hayate receive --resume` continues a partial
  single-file transfer from the last complete 4 MiB frame instead of restarting.
  An already-complete file is hash-verified without re-sending payload. (Wire
  protocol v7: the receiver now sends an 8-byte resume offset after accepting;
  v6 and v7 peers refuse to pair rather than mis-transfer.)
- **Bandwidth cap**: `hayate send --bandwidth-limit 10MiB` throttles sustained
  send throughput (also available as `HayateSender::bandwidth_limit`).
- **Named peers**: `hayate peers add/list/remove` saves receiver addresses;
  `hayate send <path> --to NAME` dials by name. Direct sends auto-remember the
  peer by IP.
- **Interactive send**: omit the path for a file prompt, or use `--pick` to scan
  the LAN and choose a receiver from a list.
- **Transfer history**: every completed transfer is logged locally;
  `hayate history` prints it (`--clear` to wipe, `--format json` for JSONL).
- **Integrity report**: receivers print an explicit "integrity verified" line
  and emit a matching JSON event after each transfer.
- **`receive --once`**: exit after the first completed transfer. Without it,
  direct-mode receive now keeps listening for more transfers.
- **JSON schema tag**: every `--format json` event now carries
  `"schema": "hayate/1"` for version-locked parsing.
- **Man pages**: release archives and `.deb` packages now ship a `hayate(1)`
  man page generated from the CLI definitions (`man hayate`).

### Fixes

- Windows binary builds no longer fail with `tar: Cannot connect to D:` —
  GNU tar on Git for Windows treated the drive-letter colon as a remote host;
  the build script now passes `--force-local`.
- The publish workflow triggers release binaries via the GitHub REST API
  instead of the `gh` CLI.

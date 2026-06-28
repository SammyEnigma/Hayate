# How to Release

Hayate uses **release-plz** (crates.io publishing) + **cargo-dist** (binary builds). Everything is automated — you just merge a PR.

---

## The pipeline

```
You push commits to master
         │
         ▼
release-plz CI runs `release-pr`
  → Creates a PR with version bump + changelog
  → PR title: "chore: release vX.Y.Z"
         │
You review the PR, edit the changelog, merge it
         │
         ▼
release-plz CI runs `release`
  → Publishes `hayate` to crates.io
  → Pushes git tag `vX.Y.Z`
         │
         ▼
Dist `release.yml` triggers on the tag
  → Builds 8 platform binaries
  → Creates installers (shell, powershell)
  → Uploads everything to GitHub Release
  → Generates SHA256 checksums + self-updater
```

**Total time from merge → GitHub Release with all assets: ~15 minutes.**

---

## Step by step

### 1. Write code, commit conventionally

Use [conventional commits](https://www.conventionalcommits.org/) so release-plz can categorise the changelog:

```bash
git commit -m "feat: add something new"       # → Added section
git commit -m "fix: resolve a bug"            # → Fixed section
git commit -m "refactor: clean up internals"  # → Changed section
git commit -m "docs: update README"           # → Other section
```

Push to master.

### 2. Wait for the release-plz PR

Within 30 seconds of pushing, the `Release` workflow creates a PR on a branch like `release-plz-2026-06-28-012345`.

The PR title looks like:

> chore: release v5.2.0

The PR body contains an auto-generated changelog from your commit messages.

### 3. Review and edit the PR

**The auto-generated changelog is bare.** Before merging, edit `CHANGELOG.md` in the PR to add detail:

```markdown
## [5.2.0] - 2026-06-28

### Added

- Streaming support for stdin/stdout transfers
- `--quiet` flag to suppress all non-error output

### Fixed

- Memory leak in BufferPool when channel sender disconnects mid-transfer
- Windows terminal detection failing on ConPTY hosts
```

The version bump in `Cargo.toml` and `Cargo.lock` is handled automatically.

### 4. Merge the PR

Click **Merge pull request** on the release-plz PR. That's it.

After merge:

- **~2 minutes**: `hayate` appears on crates.io at the new version
- **~12 minutes**: GitHub Release appears with binaries for all 8 platforms
- **~15 minutes**: install scripts (`install.sh`, `install.ps1`) are live on GitHub Pages

### 5. Verify the release

```bash
# Check crates.io
cargo search hayate

# Check GitHub Release has assets
open https://github.com/ShiinaSaku/Hayate/releases/latest

# Test the install script
curl -sSf https://shiinasaku.github.io/Hayate/install.sh | bash
```

---

## What NOT to do

| Don't                                   | Why                                                                                         |
| --------------------------------------- | ------------------------------------------------------------------------------------------- |
| Force-push tags                         | Breaks running dist workflows with "ref does not point to expected commit"                  |
| Delete and re-create tags               | Same reason — let the pipeline finish                                                       |
| Manually bump `Cargo.toml` version      | release-plz handles this. Manual bumps cause merge conflicts with the release PR            |
| Edit the release-plz PR branch directly | Always edit via the PR on GitHub                                                            |
| Merge two release PRs at once           | Only one release at a time. Merge, wait for completion, then push more commits for the next |

---

## If something goes wrong

### The dist workflow failed

1. Check the failed run at `https://github.com/ShiinaSaku/Hayate/actions/workflows/release.yml`
2. Fix the issue, commit, push
3. Delete the tag: `git push origin --delete vX.Y.Z`
4. **Wait for the failed workflow to finish** (or cancel it)
5. Re-tag: `git tag -a vX.Y.Z -m "vX.Y.Z" && git push origin vX.Y.Z`

### The changelog came out wrong

Edit `CHANGELOG.md`, commit, and push. The fix will appear in the next release. Don't edit the current release's changelog after merging.

### Need to skip a version

If release-plz creates a PR for a version you don't want to release yet, just close the PR. It won't re-create it until you push new commits.

---

## Version numbers

Both crates share a single workspace version:

```
[workspace.package]
version = "5.1.0"
```

`hayate` and `hayate-cli` use `version.workspace = true` — they always stay in lockstep. This is the standard pattern for monorepos with one published crate and one companion binary.

| Crate              | Published?             | Version source              |
| ------------------ | ---------------------- | --------------------------- |
| `hayate` (lib)     | crates.io              | `workspace.package.version` |
| `hayate-cli` (bin) | No (`publish = false`) | `workspace.package.version` |

---

## Artifacts produced

Every release creates these files on the GitHub Release page:

| File                         | Description                          |
| ---------------------------- | ------------------------------------ |
| `hayate-cli-{target}.tar.xz` | Binary for each platform (8 targets) |
| `hayate-cli-{target}.zip`    | Same, for Windows MSVC               |
| `hayate-cli-{target}-update` | Self-updater binary                  |
| `hayate-cli-installer.sh`    | Shell installer (macOS/Linux)        |
| `hayate-cli-installer.ps1`   | PowerShell installer (Windows)       |
| `source.tar.gz`              | Source tarball                       |
| `sha256.sum`                 | All checksums                        |

# winget submission for Hayate

This directory contains the [winget](https://github.com/microsoft/winget-cli) (Windows Package Manager) manifests for Hayate.

## How to publish to winget

### 1. Create a release on GitHub

Tag and release a new version (e.g., `v5.0.0`). The dist CI workflow will produce:

```
hayate-cli-x86_64-pc-windows-msvc.zip     (x64 Windows)
hayate-cli-aarch64-pc-windows-msvc.zip    (ARM64 Windows)
```

And the `sha256.sum` file listing all checksums.

### 2. Get the SHA256 hashes

Download `sha256.sum` from the GitHub Release and find:

```bash
curl -sSfL https://github.com/ShiinaSaku/Hayate/releases/download/v5.0.0/sha256.sum | grep windows-msvc
```

### 3. Update the manifest

Edit `manifests/s/ShiinaSaku/Hayate/<VERSION>/ShiinaSaku.Hayate.installer.yaml`:
- Replace `<INSERT_SHA256_FROM_RELEASE>` with the actual SHA256 from step 2
- Update `PackageVersion` and paths if needed

### 4. Fork and PR to winget-pkgs

```bash
# Clone winget-pkgs
git clone https://github.com/microsoft/winget-pkgs.git
cd winget-pkgs

# Copy the manifest
cp -r ../Hayate/winget/manifests/* manifests/

# Commit and push
git checkout -b add-hayate-5.0.0
git add manifests/s/ShiinaSaku/Hayate/
git commit -m "Add ShiinaSaku.Hayate version 5.0.0"
git push origin add-hayate-5.0.0
```

Create a Pull Request at https://github.com/microsoft/winget-pkgs.

### 5. Verify

Once merged, users can install via:

```powershell
winget install ShiinaSaku.Hayate
```

## Manifest structure

```
winget/
└── manifests/
    └── s/
        └── ShiinaSaku/
            └── Hayate/
                └── 5.0.0/
                    ├── ShiinaSaku.Hayate.yaml
                    ├── ShiinaSaku.Hayate.locale.en-US.yaml
                    └── ShiinaSaku.Hayate.installer.yaml
```

## Updating for new releases

Copy the `5.0.0/` directory to the new version, update:
1. `PackageVersion` in all three files
2. `ReleaseDate` in installer.yaml
3. `InstallerUrl` and `InstallerSha256` in installer.yaml
4. `ReleaseNotes` URL in locale file

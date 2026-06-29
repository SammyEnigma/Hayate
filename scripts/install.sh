#!/usr/bin/env bash
set -euo pipefail

REPO="ShiinaSaku/Hayate"
BINARY_NAME="hayate"

# ── Header ────────────────────────────────────────────────────────────────
CYAN='\033[38;5;45m'
PURPLE='\033[38;5;141m'
NC='\033[0m'
echo -e "${CYAN}"
cat << "EOF"
    __  _______  _____  ____________
   / / / /   \ \/ /   |/_  __/ ____/
  / /_/ / /| |\  / /| | / / / __/
 / __  / ___ |/ / ___ |/ / / /___
/_/ /_/_/  |_/_/_/  |_/_/ /_____/

EOF
echo -e "${PURPLE}  Swift, Secure, Encrypted & Compressed Local File Transfers${NC}\n"

# ── Termux detection ──────────────────────────────────────────────────────
# Termux uses Android's Bionic libc, so standard Linux binaries (glibc/musl)
# won't work. Fall back to our custom Termux build.
if [ -n "${PREFIX:-}" ] && [[ "$PREFIX" == *com.termux* ]]; then
    echo "[*] Termux detected — using Android NDK build."

    ARCH="$(uname -m)"
    if [ "$ARCH" != "aarch64" ] && [ "$ARCH" != "arm64" ]; then
        echo "[-] Termux on $ARCH is not supported." >&2
        exit 1
    fi

    # Resolve latest version
    if command -v curl >/dev/null 2>&1; then
        LATEST_TAG=$(curl -sLI -o /dev/null -w "%{url_effective}" "https://github.com/${REPO}/releases/latest" | sed 's|.*/||')
    else
        LATEST_TAG=$(wget --max-redirect=0 "https://github.com/${REPO}/releases/latest" 2>&1 | grep "Location:" | sed 's|.*/||' || true)
    fi

    if [ -z "$LATEST_TAG" ] || [ "$LATEST_TAG" = "latest" ]; then
        echo "[-] Failed to fetch latest release tag." >&2
        exit 1
    fi

    ASSET="${BINARY_NAME}-termux-arm64"
    DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${LATEST_TAG}/${ASSET}"
    TMP_DIR=$(mktemp -d)
    TMP_BIN="${TMP_DIR}/${BINARY_NAME}"

    echo "[*] Downloading ${ASSET} (${LATEST_TAG})..."
    if command -v curl >/dev/null 2>&1; then
        curl -# -sLf -o "$TMP_BIN" "$DOWNLOAD_URL"
    else
        wget -q --show-progress -O "$TMP_BIN" "$DOWNLOAD_URL"
    fi

    chmod +x "$TMP_BIN"
    mv "$TMP_BIN" "${PREFIX}/bin/${BINARY_NAME}"
    rm -rf "$TMP_DIR"

    echo "[+] Hayate ${LATEST_TAG} installed to ${PREFIX}/bin/"
    echo "    Run 'hayate help' to get started."
    exit 0
fi

# ── Standard desktop: delegate to cargo-dist installer ────────────────────
echo "[*] Delegating to the cargo-dist installer..."

INSTALLER_URL="https://github.com/${REPO}/releases/latest/download/hayate-cli-installer.sh"

if command -v curl >/dev/null 2>&1; then
    curl --proto '=https' --tlsv1.2 -LsSf "$INSTALLER_URL" | sh
else
    wget -qO- "$INSTALLER_URL" | sh
fi

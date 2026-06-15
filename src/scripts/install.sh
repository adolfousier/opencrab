#!/usr/bin/env bash
set -euo pipefail

# OpenCrabs — one-line install
# curl -fsSL https://raw.githubusercontent.com/adolfousier/opencrabs/main/src/scripts/install.sh | bash

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info()  { echo -e "${GREEN}🦀${NC} $*"; }
warn()  { echo -e "${YELLOW}⚠️${NC}  $*"; }
error() { echo -e "${RED}❌${NC} $*" >&2; exit 1; }

# Detect OS and arch
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$ARCH" in
  x86_64)  ARCH="amd64" ;;
  aarch64) ARCH="arm64" ;;
  arm64)   ARCH="arm64" ;;
  *)       error "Unsupported architecture: $ARCH" ;;
esac

case "$OS" in
  linux)  EXT="tar.gz" ;;
  darwin) EXT="tar.gz" ;;
  *)      error "Unsupported OS: $OS (linux and darwin only)" ;;
esac

INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"

# Check if install dir is writable
if [ ! -w "$INSTALL_DIR" ] 2>/dev/null; then
  SUDO="sudo"
  warn "Need sudo to install to $INSTALL_DIR"
else
  SUDO=""
fi

info "Detecting latest release..."
TAG=$(curl -fsSL https://api.github.com/repos/adolfousier/opencrabs/releases/latest \
  | grep -o '"tag_name": *"[^"]*"' \
  | head -1 \
  | cut -d'"' -f4)

if [ -z "$TAG" ]; then
  error "Could not determine latest release tag"
fi

FILENAME="opencrabs-${TAG}-${OS}-${ARCH}.tar.gz"
DOWNLOAD_URL="https://github.com/adolfousier/opencrabs/releases/download/${TAG}/${FILENAME}"

info "Downloading ${TAG} for ${OS}-${ARCH}..."
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

if ! curl -fsSL "$DOWNLOAD_URL" -o "${TMPDIR}/${FILENAME}"; then
  error "Failed to download ${FILENAME}\n   URL: ${DOWNLOAD_URL}\n   Check https://github.com/adolfousier/opencrabs/releases for available releases"
fi

info "Extracting..."
tar xzf "${TMPDIR}/${FILENAME}" -C "$TMPDIR"

info "Installing to ${INSTALL_DIR}..."
$SUDO install -m 755 "${TMPDIR}/opencrabs" "${INSTALL_DIR}/opencrabs"

# Poppler (pdftoppm) renders PDF pages to images so the agent can SEE scanned
# PDFs. Best-effort: install via the system package manager, skip if already
# present, and just print a hint if we don't recognise the package manager.
install_poppler() {
  if command -v pdftoppm >/dev/null 2>&1; then
    info "poppler (pdftoppm) already installed — PDF rendering ready"
    return
  fi
  info "Installing poppler (pdftoppm) for PDF page rendering..."
  if command -v brew >/dev/null 2>&1; then
    brew install poppler || warn "brew install poppler failed — install it manually for PDF rendering"
  elif command -v apt-get >/dev/null 2>&1; then
    $SUDO apt-get update -qq && $SUDO apt-get install -y poppler-utils \
      || warn "apt-get install poppler-utils failed — install it manually for PDF rendering"
  elif command -v dnf >/dev/null 2>&1; then
    $SUDO dnf install -y poppler-utils || warn "dnf install poppler-utils failed"
  elif command -v pacman >/dev/null 2>&1; then
    $SUDO pacman -S --noconfirm poppler || warn "pacman -S poppler failed"
  elif command -v apk >/dev/null 2>&1; then
    $SUDO apk add poppler-utils || warn "apk add poppler-utils failed"
  else
    warn "Couldn't detect a package manager. Install poppler manually for PDF rendering:"
    warn "  macOS: brew install poppler   Debian/Ubuntu: apt install poppler-utils"
  fi
}
install_poppler

info "OpenCrabs ${TAG} installed to ${INSTALL_DIR}/opencrabs"
info "Run 'opencrabs' to get started!"

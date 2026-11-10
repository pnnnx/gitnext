#!/bin/sh
# Install gitnext: a prebuilt binary from GitHub Releases when there is one for
# this machine, otherwise build from source with cargo.
#
#   curl -fsSL https://raw.githubusercontent.com/pnnnx/gitnext/main/scripts/install.sh | sh
#   sh scripts/install.sh            # from a clone: builds this checkout
#
# Environment: GITNEXT_REPO=owner/gitnext  GITNEXT_VERSION=v0.1.0  GITNEXT_BIN_DIR=~/.local/bin
set -eu

REPO="${GITNEXT_REPO:-pnnnx/gitnext}"
VERSION="${GITNEXT_VERSION:-latest}"
BIN_DIR="${GITNEXT_BIN_DIR:-$HOME/.local/bin}"

say() { printf '%s\n' "$*"; }
fail() { say "error: $*" >&2; exit 1; }

os=$(uname -s); arch=$(uname -m)
case "$os" in
  Darwin) os=apple-darwin ;;
  Linux) os=unknown-linux-gnu ;;
  *) os="" ;;
esac
case "$arch" in
  arm64|aarch64) arch=aarch64 ;;
  x86_64|amd64) arch=x86_64 ;;
  *) arch="" ;;
esac
target="$arch-$os"

from_source() {
  command -v cargo >/dev/null 2>&1 || fail "no prebuilt binary for $target and no Rust toolchain. Install Rust first: https://rustup.rs (curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh)"
  if [ -f "$(dirname "$0")/../crates/gitnext-cli/Cargo.toml" ]; then
    say "building gitnext from this checkout (takes a few minutes)…"
    cargo install --locked --path "$(dirname "$0")/../crates/gitnext-cli"
  else
    say "building gitnext from https://github.com/$REPO (takes a few minutes)…"
    cargo install --locked --git "https://github.com/$REPO" gitnext-cli
  fi
  BIN="$HOME/.cargo/bin/gitnext"
}

BIN=""
if [ -n "$arch" ] && [ -n "$os" ] && [ "$REPO" != "pnnnx/gitnext" ] && command -v curl >/dev/null 2>&1; then
  if [ "$VERSION" = latest ]; then
    url="https://github.com/$REPO/releases/latest/download/gitnext-$target.tar.gz"
  else
    url="https://github.com/$REPO/releases/download/$VERSION/gitnext-$target.tar.gz"
  fi
  tmp=$(mktemp -d)
  if curl -fsSL "$url" -o "$tmp/gitnext.tar.gz" 2>/dev/null; then
    if curl -fsSL "$url.sha256" -o "$tmp/sum" 2>/dev/null; then
      want=$(cut -d' ' -f1 "$tmp/sum")
      got=$( (command -v sha256sum >/dev/null && sha256sum "$tmp/gitnext.tar.gz" || shasum -a 256 "$tmp/gitnext.tar.gz") | cut -d' ' -f1)
      [ "$want" = "$got" ] || fail "checksum mismatch for $url"
    fi
    tar -xzf "$tmp/gitnext.tar.gz" -C "$tmp"
    mkdir -p "$BIN_DIR"
    mv "$tmp/gitnext" "$BIN_DIR/gitnext"
    chmod +x "$BIN_DIR/gitnext"
    BIN="$BIN_DIR/gitnext"
    say "installed $("$BIN" --version) → $BIN"
  fi
  rm -rf "$tmp"
fi
[ -n "$BIN" ] || from_source

case ":$PATH:" in
  *":$(dirname "$BIN"):"*) ;;
  *) say "add to your shell profile:  export PATH=\"$(dirname "$BIN"):\$PATH\"" ;;
esac
say ""
say "next, in your project:"
say "  gitnext init      # index the code, write the map"
say "  gitnext setup     # connect Claude Code / Cursor / loca (llama.cpp)"
say "  gitnext doctor    # check everything is in place"

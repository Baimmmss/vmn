#!/bin/sh
# One-time installer for vmn. No Rust, Cargo, or other tools needed —
# it just copies the prebuilt universal (Intel + Apple Silicon) binary
# onto your PATH. Afterwards, run `vmn` from any terminal.
set -e

HERE="$(cd "$(dirname "$0")" && pwd)"
SRC="$HERE/vmn"

if [ ! -f "$SRC" ]; then
    echo "error: vmn binary not found next to install.sh" >&2
    exit 1
fi

# Prefer /usr/local/bin (already on PATH on every Mac); fall back to
# ~/.local/bin without sudo if /usr/local/bin isn't writable.
if [ -w /usr/local/bin ] || sudo -n true 2>/dev/null; then
    DEST=/usr/local/bin
    if [ -w "$DEST" ]; then
        install -m 755 "$SRC" "$DEST/vmn"
    else
        sudo install -m 755 "$SRC" "$DEST/vmn"
    fi
else
    echo "Installing to /usr/local/bin needs your password (sudo)."
    if sudo mkdir -p /usr/local/bin && sudo install -m 755 "$SRC" /usr/local/bin/vmn; then
        DEST=/usr/local/bin
    else
        DEST="$HOME/.local/bin"
        mkdir -p "$DEST"
        install -m 755 "$SRC" "$DEST/vmn"
        RC="$HOME/.zshrc"
        if ! grep -q '.local/bin' "$RC" 2>/dev/null; then
            echo 'export PATH="$HOME/.local/bin:$PATH"' >> "$RC"
            echo "Added ~/.local/bin to PATH in $RC — open a new terminal."
        fi
    fi
fi

# Downloaded files get a quarantine flag; clear it so macOS doesn't block vmn.
xattr -d com.apple.quarantine "$DEST/vmn" 2>/dev/null || true

echo "✓ vmn installed to $DEST/vmn"
echo "  Run it with:  vmn"

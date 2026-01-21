#!/bin/bash
# Log Analyzer Installation Script
# Downloads and installs the latest release binary

set -e

REPO="eirenik0/log-analyzer"
BINARY_NAME="log-analyzer"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"

# Detect OS and architecture
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$OS" in
    linux)
        case "$ARCH" in
            x86_64)
                TARGET="x86_64-unknown-linux-gnu"
                # Check if musl-based
                if ldd --version 2>&1 | grep -q musl; then
                    TARGET="x86_64-unknown-linux-musl"
                fi
                ;;
            aarch64|arm64)
                TARGET="aarch64-unknown-linux-gnu"
                ;;
            *)
                echo "Unsupported architecture: $ARCH"
                exit 1
                ;;
        esac
        EXT="tar.gz"
        ;;
    darwin)
        case "$ARCH" in
            x86_64)
                TARGET="x86_64-apple-darwin"
                ;;
            arm64|aarch64)
                TARGET="aarch64-apple-darwin"
                ;;
            *)
                echo "Unsupported architecture: $ARCH"
                exit 1
                ;;
        esac
        EXT="tar.gz"
        ;;
    mingw*|msys*|cygwin*)
        TARGET="x86_64-pc-windows-msvc"
        EXT="zip"
        ;;
    *)
        echo "Unsupported OS: $OS"
        exit 1
        ;;
esac

echo "Detected: $OS ($ARCH)"
echo "Target: $TARGET"
echo ""

# Get latest version
echo "Fetching latest release..."
LATEST_RELEASE=$(curl -s "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | sed -E 's/.*"v([^"]+)".*/\1/')

if [ -z "$LATEST_RELEASE" ]; then
    echo "Could not determine latest version. Using 'latest'."
    DOWNLOAD_URL="https://github.com/$REPO/releases/latest/download/${BINARY_NAME}-latest-${TARGET}.${EXT}"
else
    echo "Latest version: $LATEST_RELEASE"
    DOWNLOAD_URL="https://github.com/$REPO/releases/download/v${LATEST_RELEASE}/${BINARY_NAME}-${LATEST_RELEASE}-${TARGET}.${EXT}"
fi

# Create temp directory
TMP_DIR=$(mktemp -d)
trap "rm -rf $TMP_DIR" EXIT

echo "Downloading from: $DOWNLOAD_URL"
curl -L -o "$TMP_DIR/archive.$EXT" "$DOWNLOAD_URL"

# Extract
cd "$TMP_DIR"
if [ "$EXT" = "tar.gz" ]; then
    tar xzf "archive.$EXT"
elif [ "$EXT" = "zip" ]; then
    unzip -q "archive.$EXT"
fi

# Install
echo ""
echo "Installing to $INSTALL_DIR..."
if [ -w "$INSTALL_DIR" ]; then
    mv "$BINARY_NAME" "$INSTALL_DIR/"
else
    echo "Need sudo to install to $INSTALL_DIR"
    sudo mv "$BINARY_NAME" "$INSTALL_DIR/"
fi

chmod +x "$INSTALL_DIR/$BINARY_NAME"

echo ""
echo "Installation complete!"
echo ""
echo "Verify installation:"
echo "  $BINARY_NAME --version"
echo ""
echo "Get help:"
echo "  $BINARY_NAME --help"

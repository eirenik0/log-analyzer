#!/bin/bash
# Install log-analyzer skill to Claude Code
# Can install locally (per-project) or globally (all projects)

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(dirname "$SCRIPT_DIR")"
SKILL_NAME="analyze-logs"
SKILL_SOURCE="$REPO_DIR/.claude/skills/$SKILL_NAME"

usage() {
    echo "Usage: $0 [--global]"
    echo ""
    echo "Install the log-analyzer skill for Claude Code."
    echo ""
    echo "Options:"
    echo "  --global    Install to ~/.claude/skills (available in all projects)"
    echo "              Default: Install to current project's .claude/skills"
    echo ""
    echo "After installation, use in Claude Code with:"
    echo "  /analyze-logs <command> [options]"
    exit 1
}

INSTALL_GLOBAL=false

while [[ $# -gt 0 ]]; do
    case $1 in
        --global|-g)
            INSTALL_GLOBAL=true
            shift
            ;;
        --help|-h)
            usage
            ;;
        *)
            echo "Unknown option: $1"
            usage
            ;;
    esac
done

if [ ! -d "$SKILL_SOURCE" ]; then
    echo "Error: Skill source not found at $SKILL_SOURCE"
    echo "Make sure you're running this from the log-analyzer repository."
    exit 1
fi

if [ "$INSTALL_GLOBAL" = true ]; then
    INSTALL_DIR="$HOME/.claude/skills/$SKILL_NAME"
    echo "Installing globally to: $INSTALL_DIR"
else
    INSTALL_DIR="$(pwd)/.claude/skills/$SKILL_NAME"
    echo "Installing to project: $INSTALL_DIR"
fi

# Create directory and copy files
mkdir -p "$INSTALL_DIR"
cp -r "$SKILL_SOURCE"/* "$INSTALL_DIR/"

echo ""
echo "Skill installed successfully!"
echo ""
echo "Usage in Claude Code:"
echo "  /analyze-logs diff file1.log file2.log"
echo "  /analyze-logs perf test.log"
echo "  /analyze-logs info test.log --samples"
echo ""

# Check if binary is installed
if command -v log-analyzer &> /dev/null; then
    echo "log-analyzer binary found at: $(which log-analyzer)"
else
    echo "Note: log-analyzer binary not found in PATH."
    echo "The skill will use 'cargo run' for development."
    echo ""
    echo "To install the binary:"
    echo "  cargo build --release"
    echo "  sudo cp target/release/log-analyzer /usr/local/bin/"
    echo ""
    echo "Or download from releases:"
    echo "  ./scripts/install.sh"
fi

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
    exit "${1:-0}"
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
            usage 1
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
    INSTALL_DIR="$PWD/.claude/skills/$SKILL_NAME"
    echo "Installing to project: $INSTALL_DIR"
fi

# Create directory and copy files
mkdir -p "$INSTALL_DIR"
if [ "$SKILL_SOURCE" -ef "$INSTALL_DIR" ]; then
    echo "Skill is already available in this project."
else
    cp -R "$SKILL_SOURCE/." "$INSTALL_DIR/"
fi

echo ""
echo "Skill installed successfully!"
echo ""
echo "Usage in Claude Code:"
echo "  /analyze-logs What failed in this capture? test.log --preset eyes"
echo "  /analyze-logs perf test.log --config ./config/profiles/my-team.toml"
echo "Choose a profile for the actual format and validate it against the capture."
echo ""
echo "Create a custom analyzer profile from template:"
echo "  mkdir -p ./config/profiles"
echo "  cp \"$INSTALL_DIR/templates/custom-start.toml\" ./config/profiles/my-team.toml"
echo "  log-analyzer --config ./config/profiles/my-team.toml info ./logs/test.log"
echo ""

# Check if binary is installed
if command -v log-analyzer &> /dev/null; then
    echo "log-analyzer binary found at: $(which log-analyzer)"
else
    echo "Note: log-analyzer binary not found in PATH."
    echo "Install or build the binary before running an investigation."
    echo ""
    echo "To install the binary:"
    echo "  cargo build --release --manifest-path \"$REPO_DIR/Cargo.toml\""
    echo "Then supply the built executable or install it on PATH."
    echo ""
    echo "Or download from releases:"
    echo "  \"$REPO_DIR/scripts/install.sh\""
fi

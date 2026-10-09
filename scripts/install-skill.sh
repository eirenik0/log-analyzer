#!/bin/bash
# Install the shared investigation skill; the Rust executable is separate.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_DIR="$(dirname "$SCRIPT_DIR")"
SKILL_NAME="analyze-logs"

usage() {
    cat <<EOF
Usage: $0 [--host claude|codex|pi] [--scope project|user] [--global]

Install the Log Analyzer investigation skill. Defaults: claude, project.
  --host HOST      Claude uses .claude/skills; Codex and Pi use .agents/skills
  --scope SCOPE    project uses the current directory; user uses your home
  --global, -g     Legacy alias for --scope user (works with every host)
  --help, -h       Show this help

The skill does not install the separately required log-analyzer executable.
EOF
    exit "${1:-0}"
}
fail() { echo "Error: $*" >&2; exit 1; }
HOST=claude
SCOPE=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --host)
            [[ $# -ge 2 ]] || fail '--host requires claude, codex, or pi'
            HOST="$2"; shift 2 ;;
        --scope)
            [[ $# -ge 2 ]] || fail '--scope requires project or user'
            [[ -z "$SCOPE" || "$SCOPE" == "$2" ]] || fail 'conflicting scope options'
            SCOPE="$2"; shift 2 ;;
        --global|-g)
            [[ -z "$SCOPE" || "$SCOPE" == user ]] || fail 'conflicting scope options'
            SCOPE=user; shift ;;
        --help|-h) usage ;;
        *) fail "Unknown option: $1" ;;
    esac
done
SCOPE="${SCOPE:-project}"
case "$HOST" in claude) SKILL_ROOT=.claude ;; codex|pi) SKILL_ROOT=.agents ;; *) fail "Unsupported host: $HOST" ;; esac
case "$SCOPE" in project) BASE_DIR="$PWD" ;; user) BASE_DIR="$HOME" ;; *) fail "Unsupported scope: $SCOPE" ;; esac
SKILL_SOURCE="$REPO_DIR/$SKILL_ROOT/skills/$SKILL_NAME"
[[ -d "$SKILL_SOURCE" ]] || fail "Skill source not found at $SKILL_SOURCE"
INSTALL_DIR="$BASE_DIR/$SKILL_ROOT/skills/$SKILL_NAME"

# Resolve existing ancestors before mkdir/cp, including symlinks and paths with spaces.
resolve_directory() {
    local candidate="$1" suffix="" resolved
    while [[ ! -d "$candidate" ]]; do
        [[ ! -e "$candidate" && ! -L "$candidate" ]] || fail "Not an accessible directory: $candidate"
        suffix="/$(basename "$candidate")$suffix"
        candidate="$(dirname "$candidate")"
    done
    resolved="$(cd "$candidate" && pwd -P)" || return 1
    printf '%s%s\n' "${resolved%/}" "$suffix"
}
SKILL_SOURCE="$(resolve_directory "$SKILL_SOURCE")"
INSTALL_DIR="$(resolve_directory "$INSTALL_DIR")"
# Protect both the canonical source and the generated Claude bundle.
for SOURCE_ROOT in "$REPO_DIR/.agents/skills/$SKILL_NAME" "$REPO_DIR/.claude/skills/$SKILL_NAME"; do
    [[ -d "$SOURCE_ROOT" ]] || continue
    SOURCE_ROOT="$(resolve_directory "$SOURCE_ROOT")"
    if [[ "$INSTALL_DIR" == "$SOURCE_ROOT" ]]; then
        [[ "$INSTALL_DIR" == "$SKILL_SOURCE" ]] || fail 'installation destination overlaps the skill source'
        continue
    fi
    case "${INSTALL_DIR%/}/" in "${SOURCE_ROOT%/}/"*) fail 'installation destination overlaps the skill source' ;; esac
    case "${SOURCE_ROOT%/}/" in "${INSTALL_DIR%/}/"*) fail 'installation destination overlaps the skill source' ;; esac
done
# Refuse destination child symlinks: cp must not write outside the chosen bundle.
if [[ -d "$INSTALL_DIR" && "$SKILL_SOURCE" != "$INSTALL_DIR" ]]; then
    [[ -z "$(find "$INSTALL_DIR" -type l -print -quit)" ]] || fail 'destination contains a symlink; remove it before reinstalling'
fi
mkdir -p "$INSTALL_DIR"
if [[ "$SKILL_SOURCE" -ef "$INSTALL_DIR" ]]; then
    echo 'Skill is already available in this project.'
else
    cp -RL "$SKILL_SOURCE/." "$INSTALL_DIR/"
fi
printf 'Skill installed at: %s\n' "$INSTALL_DIR"
case "$HOST" in
    claude) echo 'Invoke: /analyze-logs <question> <absolute input paths> --config <profile>' ;;
    codex) echo 'Invoke: $analyze-logs <question> <absolute input paths> --config <profile>' ;;
    pi) echo 'Invoke: /skill:analyze-logs <question> <absolute input paths> --config <profile>' ;;
esac
echo 'See hosts.md in the installed bundle for discovery, compatibility and profile selection.'
if command -v log-analyzer >/dev/null 2>&1; then
    printf 'Executable on PATH: %s\n' "$(command -v log-analyzer)"
    echo 'Run log-analyzer capabilities before investigating; finding a binary does not prove compatibility.'
else
    echo 'Note: log-analyzer binary not found in PATH. The skill is installed, but cannot investigate yet.'
    echo 'Install the executable separately: cargo install log-analyzer --locked'
    printf 'Or build: cargo build --release --manifest-path "%s/Cargo.toml"\n' "$REPO_DIR"
    echo 'Supply the absolute built executable path to the agent, or add it to PATH.'
fi

#!/usr/bin/env bash
set -euo pipefail

# Ensure we run from the git repo root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Target directory in Windows
WIN_DIR="${WIN_PROJECT_DIR:-}"
if [ -z "$WIN_DIR" ]; then
    if [ -d "/mnt/c/Users/paul/Projects/yt-dlp" ]; then
        WIN_DIR="/mnt/c/Users/paul/Projects/yt-dlp"
    elif [ -d "C:/Users/paul/Projects/yt-dlp" ]; then
        WIN_DIR="C:/Users/paul/Projects/yt-dlp"
    elif [ -d "/c/Users/paul/Projects/yt-dlp" ]; then
        WIN_DIR="/c/Users/paul/Projects/yt-dlp"
    else
        echo "==> Error: Target directory C:\\Users\\paul\\Projects\\yt-dlp not found!" >&2
        exit 1
    fi
fi

# Parse optional arguments:
# ./sync-build.sh [optional-commit-message] [extra tauri build args...]
COMMIT_MSG=""
EXTRA_ARGS=()

if [ $# -gt 0 ]; then
    if [[ "$1" == -* ]]; then
        EXTRA_ARGS=("$@")
    else
        COMMIT_MSG="$1"
        shift
        EXTRA_ARGS=("$@")
    fi
fi

# Generate random commit message if none provided
if [ -z "$COMMIT_MSG" ]; then
    RAND_PREFIXES=("df" "sd" "d" "f" "sync" "wip" "update" "patch")
    RAND_PREFIX="${RAND_PREFIXES[$((RANDOM % ${#RAND_PREFIXES[@]}))]}"
    RAND_HEX="$(od -An -N2 -tx1 /dev/urandom 2>/dev/null | tr -d ' \n' || printf '%04x' "$RANDOM")"
    COMMIT_MSG="${RAND_PREFIX} ${RAND_HEX}"
fi

echo "========================================================"
echo "==> 1. Checking and committing changes in current repo..."
echo "========================================================"

git add -A

if ! git diff-index --quiet HEAD -- 2>/dev/null; then
    echo "==> Committing with message: \"$COMMIT_MSG\""
    git commit -m "$COMMIT_MSG"
else
    echo "==> No unstaged/uncommitted changes detected."
fi

echo "========================================================"
echo "==> 2. Pushing changes to remote..."
echo "========================================================"

# Determine upstream tracking branch safely (handles master -> origin/main tracking)
UPSTREAM="$(git rev-parse --abbrev-ref --symbolic-full-name @{u} 2>/dev/null || true)"
if [ -n "$UPSTREAM" ]; then
    REMOTE="${UPSTREAM%%/*}"
    REMOTE_BRANCH="${UPSTREAM#*/}"
    echo "==> Pushing HEAD to $REMOTE ($REMOTE_BRANCH)..."
    git push "$REMOTE" "HEAD:$REMOTE_BRANCH"
else
    echo "==> Pushing HEAD to origin..."
    git push origin HEAD
fi

echo "========================================================"
echo "==> 3. Pulling changes in $WIN_DIR..."
echo "========================================================"

git -C "$WIN_DIR" pull --autostash

echo "========================================================"
echo "==> 4. Running Tauri build in Windows..."
echo "========================================================"

BUILD_CMD="${BUILD_CMD:-pnpm run tauri:build}"
if [ ${#EXTRA_ARGS[@]} -gt 0 ]; then
    BUILD_CMD="$BUILD_CMD ${EXTRA_ARGS[*]}"
fi

echo "==> Executing: $BUILD_CMD"

if [ -x "/mnt/c/Windows/System32/cmd.exe" ]; then
    # Close any running yt-dlp-gui.exe so Windows file locks don't block replacing the binary
    (cd /mnt/c 2>/dev/null || true; /mnt/c/Windows/System32/cmd.exe /c "taskkill /F /IM yt-dlp-gui.exe >nul 2>&1" < /dev/null || true)
    # Running from WSL: invoke Windows cmd.exe from /mnt/c to avoid UNC warnings
    (cd /mnt/c 2>/dev/null || true; /mnt/c/Windows/System32/cmd.exe /c "cd /d C:\Users\paul\Projects\yt-dlp && $BUILD_CMD" < /dev/null)
elif command -v cmd.exe &>/dev/null; then
    cmd.exe /c "taskkill /F /IM yt-dlp-gui.exe >nul 2>&1" < /dev/null || true
    # Running from Git Bash or Windows environment
    (cd "$WIN_DIR" && cmd.exe /c "cd /d C:\Users\paul\Projects\yt-dlp && $BUILD_CMD" < /dev/null)
else
    # Fallback to direct execution
    (cd "$WIN_DIR" && $BUILD_CMD)
fi

echo "========================================================"
echo "==> Done!"
echo "========================================================"


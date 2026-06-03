#!/usr/bin/env bash
set -euo pipefail

# Self-healing entrypoint for auxloclaw service.
#
# On container runtime resets, ephemeral filesystems (like /usr/local/bin) get
# wiped. This wrapper detects the missing binary and auto-reinstalls from
# GitHub Releases before running the requested command.
#
# Works for any user, any container environment. No hardcoded paths beyond
# the install location. Passes all arguments through to the binary.
#
# Usage (as service entrypoint):
#   /usr/local/bin/auxloclaw_entrypoint.sh gateway --port 18789
#   /usr/local/bin/auxloclaw_entrypoint.sh chat "hello"

REPO="Auxlo-xyz/auxloclaw"
BINARY="auxloclaw"
INSTALL_DIR="${AUXLOCLAW_INSTALL_DIR:-/usr/local/bin}"
INSTALL_PATH="${INSTALL_DIR}/${BINARY}"
MAX_RETRIES=3
RETRY_DELAY=2

log() {
    printf '[auxloclaw-entrypoint] %s %s\n' "$(date -u +'%Y-%m-%dT%H:%M:%SZ')" "$*" >&2
}

detect_target() {
    local os arch
    os="$(uname -s)"
    arch="$(uname -m)"
    case "$os" in
        Linux)  os="unknown-linux-musl" ;;
        Darwin) os="apple-darwin" ;;
        *) log "Unsupported OS: $os"; return 1 ;;
    esac
    case "$arch" in
        x86_64)  arch="x86_64" ;;
        aarch64|arm64) arch="aarch64" ;;
        *) log "Unsupported architecture: $arch"; return 1 ;;
    esac
    echo "${arch}-${os}"
}

ensure_binary() {
    if [ -x "$INSTALL_PATH" ]; then
        return 0
    fi

    log "Binary not found at ${INSTALL_PATH}, reinstalling..."

    local target
    target="$(detect_target)" || exit 1

    local url="https://github.com/${REPO}/releases/latest/download/${BINARY}-${target}"
    local tmp_path="/tmp/${BINARY}.entrypoint.$$"
    local attempt=0

    while [ "$attempt" -lt "$MAX_RETRIES" ]; do
        attempt=$((attempt + 1))
        log "Download attempt ${attempt}/${MAX_RETRIES}: ${url}"

        if curl -fsSL --connect-timeout 10 --max-time 120 "$url" -o "$tmp_path" 2>/dev/null; then
            chmod +x "$tmp_path"

            # Verify the downloaded binary runs
            if "$tmp_path" --version >/dev/null 2>&1; then
                mkdir -p "$INSTALL_DIR"
                mv "$tmp_path" "$INSTALL_PATH"
                log "Installed: $($INSTALL_PATH --version)"
                return 0
            else
                log "WARNING: Downloaded binary failed verification"
                rm -f "$tmp_path"
            fi
        else
            log "WARNING: Download attempt ${attempt} failed"
            rm -f "$tmp_path"
        fi

        if [ "$attempt" -lt "$MAX_RETRIES" ]; then
            log "Retrying in ${RETRY_DELAY}s..."
            sleep "$RETRY_DELAY"
        fi
    done

    log "ERROR: Failed to install ${BINARY} after ${MAX_RETRIES} attempts"
    exit 1
}

# Ensure binary exists (install if missing -- covers container reset)
ensure_binary

# Pass all arguments to the real binary
exec "$INSTALL_PATH" "$@"

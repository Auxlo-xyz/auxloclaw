#!/usr/bin/env bash

set -euo pipefail

LOCKFILE="/tmp/auxloclaw_watchdog.lock"
LOGFILE="/tmp/auxloclaw_watchdog.log"
REPO="Auxlo-xyz/auxloclaw"
BINARY="auxloclaw"
INSTALL_DIR="/usr/local/bin"
INSTALL_PATH="${INSTALL_DIR}/${BINARY}"

exec 9>"$LOCKFILE"
if ! flock -n 9; then
  exit 0
fi

log() {
    printf '%s %s\n' "$(date -u +"%Y-%m-%dT%H:%M:%SZ")" "$*" >> "$LOGFILE"
}

detect_platform() {
    local os arch
    os="$(uname -s)"
    arch="$(uname -m)"
    case "$os" in
        Linux)  os="unknown-linux-musl" ;;
        Darwin) os="apple-darwin" ;;
        *) echo "unsupported" ;;
    esac
    case "$arch" in
        x86_64)  arch="x86_64" ;;
        aarch64|arm64) arch="aarch64" ;;
        *) echo "unsupported" ;;
    esac
    echo "${arch}-${os}"
}

ensure_binary() {
    if [ -x "$INSTALL_PATH" ]; then
        return 0
    fi

    log "WARN: Binary not found at ${INSTALL_PATH}, reinstalling after container reset..."
    local target
    target="$(detect_platform)"
    if [ "$target" = "unsupported-unsupported" ]; then
        log "ERROR: Cannot detect platform for reinstall"
        return 1
    fi

    local url="https://github.com/${REPO}/releases/latest/download/${BINARY}-${target}"
    local tmp="/tmp/${BINARY}.watchdog.$$"

    if ! curl -fsSL "$url" -o "$tmp" 2>/dev/null; then
        log "ERROR: Failed to download binary from ${url}"
        rm -f "$tmp"
        return 1
    fi

    chmod +x "$tmp"
    if ! "$tmp" --version >/dev/null 2>&1; then
        log "ERROR: Downloaded binary failed verification"
        rm -f "$tmp"
        return 1
    fi

    mkdir -p "$INSTALL_DIR"
    mv "$tmp" "$INSTALL_PATH"
    log "REINSTALLED: $($INSTALL_PATH --version 2>/dev/null)"
    return 0
}

gateway_up() {
    timeout 12s auxloclaw status 2>/dev/null | grep -q "Gateway:"
}

# Attempt reinstall if binary is missing (container reset scenario)
if ! ensure_binary; then
    log "ERROR: Could not ensure auxloclaw binary exists"
    exit 1
fi

if gateway_up; then
  log "OK: gateway already running"
  exit 0
fi

log "WARN: gateway down, restarting"
auxloclaw stop >/dev/null 2>&1 || true
auxloclaw gateway >/tmp/auxloclaw_gateway.out 2>&1 &

for _ in {1..12}; do
  sleep 1
  if gateway_up; then
    log "RECOVERED: gateway restarted"
    exit 0
  fi
done

log "FAIL: restart attempted but gateway is still down"
exit 1

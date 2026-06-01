#!/usr/bin/env bash

set -euo pipefail

LOCKFILE="/tmp/auxloclaw_watchdog.lock"
LOGFILE="/tmp/auxloclaw_watchdog.log"

exec 9>"$LOCKFILE"
if ! flock -n 9; then
  exit 0
fi

log() {
  printf '%s %s
' "$(date -u +"%Y-%m-%dT%H:%M:%SZ")" "$*" >> "$LOGFILE"
}

gateway_up() {
  timeout 12s auxloclaw status 2>/dev/null | grep -q "Gateway:"
}

if ! command -v auxloclaw >/dev/null 2>&1; then
  log "ERROR: auxloclaw binary not found in PATH"
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

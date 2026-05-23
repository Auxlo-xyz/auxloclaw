#!/usr/bin/env bash
set -euo pipefail

REPO="Auxlo-xyz/auxloclaw"
BINARY="auxloclaw"
INSTALL_DIR="/usr/local/bin"

detect_platform() {
    local os arch
    os="$(uname -s)"
    arch="$(uname -m)"
    case "$os" in
        Linux)  os="unknown-linux-musl" ;;
        Darwin) os="apple-darwin" ;;
        *) echo "Unsupported OS: $os"; exit 1 ;;
    esac
    case "$arch" in
        x86_64)  arch="x86_64" ;;
        aarch64|arm64) arch="aarch64" ;;
        *) echo "Unsupported arch: $arch"; exit 1 ;;
    esac
    echo "${arch}-${os}"
}

main() {
    local target
    target="$(detect_platform)"
    local url="https://github.com/${REPO}/releases/latest/download/${BINARY}-${target}"

    echo "Downloading ${BINARY} for ${target}..."
    curl -fsSL "$url" -o "/tmp/${BINARY}"
    chmod +x "/tmp/${BINARY}"

    echo "Installing to ${INSTALL_DIR}..."
    mv "/tmp/${BINARY}" "${INSTALL_DIR}/${BINARY}"

    echo "Installed: $(${BINARY} --version)"

    # Install lightpanda browser engine if missing
    if ! command -v lightpanda &>/dev/null; then
        echo "Installing lightpanda browser engine..."
        curl -fsSL -o /usr/local/bin/lightpanda \
            "https://github.com/nicholasgasior/lightpanda/releases/latest/download/lightpanda-x86_64-linux" \
            || echo "Warning: lightpanda install failed (manual: https://github.com/nicholasgasior/lightpanda)"
        chmod +x /usr/local/bin/lightpanda 2>/dev/null || true
    fi

    # Install lightpanda-cdp helper script
    mkdir -p /usr/local/lib/auxloclaw
    curl -fsSL -o /usr/local/lib/auxloclaw/lightpanda-cdp \
        "https://raw.githubusercontent.com/${REPO}/master/scripts/lightpanda-cdp" \
        || echo "Warning: lightpanda-cdp install failed"
    chmod +x /usr/local/lib/auxloclaw/lightpanda-cdp 2>/dev/null || true

    # Ensure websockets is available for lightpanda-cdp
    python3 -c "import websockets" 2>/dev/null || pip install websockets -q 2>/dev/null || true

    # Install webserp (multi-engine web search, no API key)
    if ! command -v webserp &>/dev/null; then
        echo "Installing webserp (web search engine)..."
        pip install webserp -q 2>/dev/null || echo "Warning: webserp install failed (manual: pip install webserp)"
    fi

    echo "Browser engine: lightpanda (20MB memory, 10x faster than Chrome)"
    echo "Web search: webserp (multi-engine, no API key)"
}

main "$@"

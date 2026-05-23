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

    # Install agent-browser (Vercel) if missing
    if ! command -v agent-browser &>/dev/null; then
        echo "Installing agent-browser (by Vercel)..."
        curl -fsSL https://media.zocomputer.com/install/agentbrowser2.sh | bash \
            || echo "Warning: agent-browser install failed (manual: curl -fsSL https://media.zocomputer.com/install/agentbrowser2.sh | bash)"
    fi

    # Install webserp (multi-engine web search, no API key)
    if ! command -v webserp &>/dev/null; then
        echo "Installing webserp (web search engine)..."
        pip install webserp -q 2>/dev/null || echo "Warning: webserp install failed (manual: pip install webserp)"
    fi

    echo "Browser: agent-browser (by Vercel)"
    echo "Web search: webserp (multi-engine, no API key)"
}

main "$@"

#!/usr/bin/env bash
set -euo pipefail

REPO="larsontrey720/auxloclaw"
BINARY="auxloclaw"
INSTALL_DIR="/usr/local/bin"

detect_platform() {
    local os arch
    os="$(uname -s)"
    arch="$(uname -m)"
    case "$os" in
        Linux)  os="unknown-linux-gnu" ;;
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
}

main "$@"

#!/usr/bin/env bash
set -euo pipefail

# AUXLOCLAW Installer
# Usage: curl -sSL https://install.auxloclaw.sh | bash
# Options:
#   AUXLOCLAW_VERSION  - specific version tag (default: latest)
#   AUXLOCLAW_DIR      - install directory (default: /usr/local/bin)
#   AUXLOCLAW_SKIP_CONFIRM - set to 1 to skip confirmation prompt

REPO="larsontrey720/auxloclaw"
BINARY="auxloclaw"
INSTALL_DIR="${AUXLOCLAW_DIR:-/usr/local/bin}"
GITHUB_API="https://api.github.com/repos/${REPO}/releases"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

info()  { echo -e "${CYAN}[info]${NC} $*"; }
ok()    { echo -e "${GREEN}[ok]${NC} $*"; }
warn()  { echo -e "${YELLOW}[warn]${NC} $*"; }
err()   { echo -e "${RED}[error]${NC} $*" >&2; }
die()   { err "$*"; exit 1; }

detect_arch() {
    local arch
    arch="$(uname -m)"
    case "$arch" in
        x86_64|amd64)   echo "x86_64" ;;
        aarch64|arm64)   echo "aarch64" ;;
        armv7l|armhf)    echo "armv7" ;;
        *)               die "Unsupported architecture: $arch" ;;
    esac
}

detect_os() {
    local os
    os="$(uname -s)"
    case "$os" in
        Linux)   echo "linux" ;;
        Darwin)  echo "macos" ;;
        *)       die "Unsupported OS: $os. AUXLOCLAW currently supports Linux and macOS." ;;
    esac
}

need_cmd() {
    command -v "$1" >/dev/null 2>&1 || die "Required command not found: $1"
}

check_deps() {
    need_cmd curl
    need_cmd tar
}

get_latest_version() {
    local version
    version="$(curl -sL "${GITHUB_API}/latest" 2>/dev/null | grep '"tag_name"' | head -1 | sed -E 's/.*"tag_name":\s*"([^"]+)".*/\1/')"
    if [ -z "$version" ]; then
        # No releases yet, fall back to source build
        echo ""
    else
        echo "$version"
    fi
}

download_binary() {
    local version="$1" os="$2" arch="$3"
    local url asset_name

    if [ "$os" = "macos" ]; then
        if [ "$arch" = "aarch64" ]; then
            asset_name="${BINARY}-${version}-aarch64-apple-darwin.tar.gz"
        else
            asset_name="${BINARY}-${version}-x86_64-apple-darwin.tar.gz"
        fi
    else
        if [ "$arch" = "aarch64" ]; then
            asset_name="${BINARY}-${version}-aarch64-unknown-linux-gnu.tar.gz"
        elif [ "$arch" = "armv7" ]; then
            asset_name="${BINARY}-${version}-armv7-unknown-linux-gnueabihf.tar.gz"
        else
            asset_name="${BINARY}-${version}-x86_64-unknown-linux-gnu.tar.gz"
        fi
    fi

    url="https://github.com/${REPO}/releases/download/${version}/${asset_name}"
    info "Downloading ${BINARY} ${version} for ${os}/${arch}..."

    local tmp_dir
    tmp_dir="$(mktemp -d)"
    trap 'rm -rf "$tmp_dir"' EXIT

    if ! curl -sL --fail "$url" -o "${tmp_dir}/${asset_name}"; then
        return 1
    fi

    tar -xzf "${tmp_dir}/${asset_name}" -C "$tmp_dir"
    local binary_path
    binary_path="$(find "$tmp_dir" -name "$BINARY" -type f | head -1)"

    if [ -z "$binary_path" ]; then
        return 1
    fi

    chmod +x "$binary_path"
    echo "$binary_path"
}

build_from_source() {
    info "No pre-built binary available. Building from source..."

    need_cmd git

    # Check for Rust toolchain
    if ! command -v cargo >/dev/null 2>&1; then
        info "Installing Rust toolchain..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        source "$HOME/.cargo/env"
    fi

    local tmp_dir
    tmp_dir="$(mktemp -d)"
    trap 'rm -rf "$tmp_dir"' EXIT

    info "Cloning repository..."
    git clone --depth 1 "https://github.com/${REPO}.git" "${tmp_dir}/auxloclaw"

    info "Building release binary (this may take a few minutes)..."
    (
        cd "${tmp_dir}/auxloclaw"
        cargo build --release 2>&1 | tail -5
    )

    local binary_path="${tmp_dir}/auxloclaw/target/release/${BINARY}"
    if [ ! -f "$binary_path" ]; then
        die "Build failed. Binary not found at ${binary_path}"
    fi

    chmod +x "$binary_path"
    echo "$binary_path"
}

install_binary() {
    local binary_path="$1"
    mkdir -p "$INSTALL_DIR"
    cp "$binary_path" "${INSTALL_DIR}/${BINARY}"
    chmod +x "${INSTALL_DIR}/${BINARY}"
    ok "Installed ${BINARY} to ${INSTALL_DIR}/${BINARY}"
}

post_install() {
    # Check if install dir is in PATH
    if ! echo "$PATH" | tr ':' '\n' | grep -qx "$INSTALL_DIR"; then
        warn "${INSTALL_DIR} is not in your PATH."
        echo ""
        echo "  Add it to your shell profile:"
        echo ""
        if [ -f "$HOME/.zshrc" ]; then
            echo "    echo 'export PATH=\"${INSTALL_DIR}:\$PATH\"' >> ~/.zshrc"
            echo "    source ~/.zshrc"
        elif [ -f "$HOME/.bashrc" ]; then
            echo "    echo 'export PATH=\"${INSTALL_DIR}:\$PATH\"' >> ~/.bashrc"
            echo "    source ~/.bashrc"
        else
            echo "    export PATH=\"${INSTALL_DIR}:\$PATH\""
        fi
        echo ""
    fi

    echo ""
    echo -e "${BOLD}Next steps:${NC}"
    echo ""
    echo "  1. Run the setup wizard:"
    echo -e "     ${CYAN}auxloclaw setup${NC}"
    echo ""
    echo "  2. Or quick setup with defaults:"
    echo -e "     ${CYAN}auxloclaw setup --quick${NC}"
    echo ""
    echo "  3. Add MCP integrations (GitHub, etc):"
    echo -e "     ${CYAN}auxloclaw mcp add github${NC}"
    echo -e "     ${CYAN}auxloclaw token set GITHUB_TOKEN your-token-here${NC}"
    echo ""
    echo "  4. Start the gateway:"
    echo -e "     ${CYAN}auxloclaw gateway${NC}"
    echo ""
    echo "  5. Or start a chat:"
    echo -e "     ${CYAN}auxloclaw chat${NC}"
    echo ""
    echo -e "  Docs: ${CYAN}https://github.com/${REPO}${NC}"
    echo ""
}

main() {
    echo ""
    echo -e "${BOLD}AUXLOCLAW Installer${NC}"
    echo -e "  Ultra-High-Performance AI Agent Framework"
    echo ""

    check_deps

    local os arch
    os="$(detect_os)"
    arch="$(detect_arch)"
    info "Detected: ${os}/${arch}"

    # Check for existing install
    if command -v "$BINARY" >/dev/null 2>&1; then
        local current_version
        current_version="$($BINARY --version 2>/dev/null || echo 'unknown')"
        warn "Existing installation found: ${current_version}"
        if [ "${AUXLOCLAW_SKIP_CONFIRM:-0}" != "1" ]; then
            read -r -p "  Overwrite? [y/N] " answer
            case "$answer" in
                [yY]*) ;;
                *) info "Cancelled."; exit 0 ;;
            esac
        fi
    fi

    local version
    version="${AUXLOCLAW_VERSION:-$(get_latest_version)}"

    local binary_path=""
    if [ -n "$version" ]; then
        binary_path="$(download_binary "$version" "$os" "$arch" 2>/dev/null)" || true
    fi

    if [ -z "$binary_path" ]; then
        binary_path="$(build_from_source)"
    fi

    install_binary "$binary_path"

    # Verify
    if command -v "$BINARY" >/dev/null 2>&1 || [ -x "${INSTALL_DIR}/${BINARY}" ]; then
        local installed_version
        installed_version="$("${INSTALL_DIR}/${BINARY}" --version 2>/dev/null || echo 'installed')"
        ok "AUXLOCLAW ${installed_version} installed successfully!"
    else
        warn "Binary installed but not found in PATH. You may need to restart your shell."
    fi

    post_install
}

main "$@"

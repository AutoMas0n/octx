#!/usr/bin/env bash
set -euo pipefail

# octx — one-liner installer
# Usage: curl -fsSL https://octx.sh/install.sh | bash

OCTX_REPO="https://github.com/AutoMas0n/octx"
OCTX_BINARY="octx"

# ── helpers ──────────────────────────────────────────────────────────────────
die() {
    echo "Error: $*" >&2
    exit 1
}

info() {
    echo "==> $*"
}

prompt_yn() {
    # $1: prompt text; returns 0 for yes, 1 for no
    local reply
    read -r -p "$1 (Y/n) " reply
    case "${reply,,}" in
        "" | y | yes) return 0 ;;
        *) return 1 ;;
    esac
}

# ── detect target triple ─────────────────────────────────────────────────────
detect_target() {
    local arch os kernel
    os="$(uname -s | tr '[:upper:]' '[:lower:]')"
    arch="$(uname -m)"
    kernel="$(uname -o | tr '[:upper:]' '[:lower:]')"

    # Detect Android (Termux or similar Linux-on-Android environments)
    if [ "$kernel" = "android" ]; then
        case "$arch" in
            aarch64|arm64) echo "aarch64-linux-android" ;;
            armv7l)        echo "armv7-linux-androideabi" ;;
            *)
                die "unsupported Android architecture: $arch — run 'uname -m' and open an issue"
                ;;
        esac
        return
    fi

    # Standard Linux (musl/glibc)
    if [ "$os" != "linux" ]; then
        die "unsupported OS: $os — only Linux and Android are supported. Run 'uname -m' and open an issue."
    fi

    case "$arch" in
        x86_64)  echo "x86_64-unknown-linux-musl" ;;
        aarch64) echo "aarch64-unknown-linux-musl" ;;
        armv6l)  echo "armv6-unknown-linux-gnueabihf" ;;
        armv7l)  echo "armv7-unknown-linux-gnueabihf" ;;
        arm64)   echo "aarch64-unknown-linux-musl" ;;
        *)
            die "unsupported architecture: $arch — run 'uname -m' and open an issue with the output"
            ;;
    esac
}

# ── check prerequisites ──────────────────────────────────────────────────────
check_deps() {
    for cmd in curl gunzip chmod mv; do
        if ! command -v "$cmd" >/dev/null 2>&1; then
            die "required command not found: $cmd — please install it and try again"
        fi
    done
}

# ── find install directory ───────────────────────────────────────────────────
find_prefix() {
    if [ -w "/usr/local/bin" ]; then
        echo "/usr/local/bin"
    else
        local home_bin="${HOME}/.local/bin"
        mkdir -p "$home_bin"
        echo "$home_bin"
    fi
}

# ── handle existing installation ─────────────────────────────────────────────
handle_existing() {
    local prefix="$1"
    local existing="${prefix}/${OCTX_BINARY}"

    if [ -f "$existing" ]; then
        if prompt_yn "octx is already installed at ${existing}. Reinstall?"; then
            info "Renaming old binary to ${existing}.bak"
            mv "$existing" "${existing}.bak" || die "failed to rename existing binary"
            return 0
        else
            info "Installation cancelled by user."
            exit 0
        fi
    fi
}

# ── download & install ───────────────────────────────────────────────────────
download_and_install() {
    local target="$1"
    local prefix="$2"
    local url="${OCTX_REPO}/releases/latest/download/${OCTX_BINARY}-${target}.gz"
    local tmp_dir
    tmp_dir="$(mktemp -d)"
    local gz_file="${tmp_dir}/${OCTX_BINARY}.gz"
    local bin_file="${tmp_dir}/${OCTX_BINARY}"

    # Cleanup on exit
    trap 'rm -rf "$tmp_dir"' EXIT

    info "Downloading octx for ${target}..."
    if ! curl -fsSL "$url" -o "$gz_file"; then
        die "download failed — could not fetch ${url}"
    fi

    # Optional: try to fetch and verify SHA256
    local sha_url="${OCTX_REPO}/releases/latest/download/${OCTX_BINARY}-${target}.sha256"
    if curl -fsSL "$sha_url" -o "${tmp_dir}/checksum.sha256" 2>/dev/null; then
        info "Verifying checksum..."
        local expected actual
        expected="$(cut -d' ' -f1 < "${tmp_dir}/checksum.sha256")"
        actual="$(sha256sum "$gz_file" | cut -d' ' -f1)"
        if [ "$expected" != "$actual" ]; then
            die "checksum mismatch — expected ${expected}, got ${actual}"
        fi
        info "Checksum verified."
    else
        info "No checksum file found — skipping verification."
    fi

    info "Extracting..."
    gunzip -c "$gz_file" > "$bin_file" || die "extraction failed"
    chmod +x "$bin_file" || die "failed to set executable permission"

    info "Installing to ${prefix}/${OCTX_BINARY}..."
    mv "$bin_file" "${prefix}/${OCTX_BINARY}" || die "installation failed"
}

# ── run octx init ────────────────────────────────────────────────────────────
run_octx_init() {
    local prefix="$1"
    local octx_cmd="${prefix}/${OCTX_BINARY}"

    if [ -x "$octx_cmd" ]; then
        info "Running octx init to set up shell PATH..."
        "$octx_cmd" init || info "octx init exited non-zero — you may need to add ${prefix} to your PATH manually."
    fi
}

# ── optional GitHub token ────────────────────────────────────────────────────
add_github_token() {
    local prefix="$1"
    local octx_cmd="${prefix}/${OCTX_BINARY}"

    if prompt_yn "Add a GitHub token?"; then
        local token
        read -r -p "GitHub token: " token
        if [ -n "$token" ]; then
            "$octx_cmd" creds add github <<< "$token" || info "Failed to store token — run 'octx creds add github' manually."
        fi
    fi
}

# ── main ─────────────────────────────────────────────────────────────────────
main() {
    info "Installing octx — the Octopus CLI"

    check_deps

    local target
    target="$(detect_target)"

    local prefix
    prefix="$(find_prefix)"

    handle_existing "$prefix"

    download_and_install "$target" "$prefix"

    run_octx_init "$prefix"

    add_github_token "$prefix"

    echo ""
    echo "  ✅ Done! octx is installed at ${prefix}/${OCTX_BINARY}"
    echo ""
    echo "     Run \`octx update\` anytime to keep octx and all tools current."
    echo ""
}

main
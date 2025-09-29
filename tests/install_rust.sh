#!/bin/bash

function arch_to_rust() {
	local -r arch="$(uname -m)"

	case "${arch}" in
		aarch64) echo "${arch}";;
		ppc64le) echo "powerpc64le";;
		x86_64) echo "${arch}";;
		s390x) echo "${arch}";;
		*) die "unsupported architecture: ${arch}";;
	esac
}

set -o errexit
set -o nounset
set -o pipefail

RUSTARCH=$(arch_to_rust)
VERSION="${1:-"1.85.1"}"

echo "Install rust ${VERSION}"

if ! command -v rustup > /dev/null; then
	curl https://sh.rustup.rs -sSf | sh -s -- -y --default-toolchain "${VERSION}"
fi

export PATH="${PATH}:${HOME}/.cargo/bin"

rustup toolchain install "${VERSION}"
rustup default "${VERSION}"

rustup target add "${RUSTARCH}-unknown-linux-musl"

rustup component add rustfmt
rustup component add clippy
rustup component add miri


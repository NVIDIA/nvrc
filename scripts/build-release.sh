#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright (c) NVIDIA CORPORATION

# Builds a release binary whose sha256 is independent of the build machine
# (ARCHITECTURE.md "Provenance & Supply-Chain Security", VERIFY.md
# "Reproduce the build"). Two machine-specific paths reach .rodata via panic
# Location strings and are remapped:
#
#   $CARGO_HOME/registry/src/...  dependency code
#   $SYSROOT/lib/rustlib/src/...  monomorphized std when the rust-src
#                                 component is installed; mapped onto the
#                                 /rustc/<commit> form embedded without it
#
# Workspace paths are compiled relative and do not leak.
#
# The remaps use `cargo --config`, which joins same-key arrays with
# .cargo/config.toml. RUSTFLAGS would replace them: Cargo reads exactly one
# of CARGO_ENCODED_RUSTFLAGS, RUSTFLAGS, target.*.rustflags,
# build.rustflags (Cargo book, "build.rustflags").
set -euo pipefail

target="${1:?usage: $0 <target-triple>}"

# A stray dev stamp would change release bytes; release identity is the bare
# Cargo version (see src/hash.rs).
unset GIT_REV

# Owned here so the release build and the reproducibility check share one
# environment; the release tarball step consumes it.
SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git log -1 --pretty=%ct)}"
export SOURCE_DATE_EPOCH

# rustc embeds absolute paths; a relative CARGO_HOME would not match
cargo_home="$(realpath -m "${CARGO_HOME:-$HOME/.cargo}")"
sysroot="$(rustc --print sysroot)"
rustc_commit="$(rustc -vV | sed -n 's/^commit-hash: //p')"
# An empty hash would remap onto /rustc/ and produce unmatchable bytes.
[ -n "$rustc_commit" ] || { echo "$0: cannot parse commit-hash from rustc -vV" >&2; exit 1; }

exec cargo build --release --locked --target "$target" --config \
  "target.\"$target\".rustflags=[\"--remap-path-prefix=$cargo_home=/cargo\",\"--remap-path-prefix=$sysroot/lib/rustlib/src/rust=/rustc/$rustc_commit\"]"

# NVRC Development Guide

## Project Overview
NVRC is a minimal init process (PID 1) for ephemeral confidential VMs with
NVIDIA GPUs. It configures GPU resources, starts required daemons
(nvidia-persistenced, nv-hostengine, etc.), and hands off to kata-agent.

## hardened_std
Security-hardened std replacement with whitelist-only access to filesystem,
processes, and sockets.

**Core Principles:**
- Fresh filesystem on every boot - if a path exists, it's an error (fail-fast)
- No `remove_file` - we setup clean state, not fix bad state
- Whitelist-only: paths, binaries, socket paths must be explicitly allowed
- Static arguments: `&'static str` only (no runtime injection)
- Minimal surface: implement only what NVRC actually needs

## Guidelines

1. **API Compatibility**: Keep std-compatible interfaces for easy exchange
2. **Tests**: Minimal meaningful coverage. Tests can use std.
3. **After completion**: Run `cargo fmt` and `cargo clippy`

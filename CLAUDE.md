# NVRC Development Guide

**Read [ARCHITECTURE.md](ARCHITECTURE.md) first** to understand the security
model, threat landscape, and design rationale before making changes.

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
- Single-threaded: NVRC is PID 1 (init) with no threads - no thread::sleep, no
  mutexes, no thread-safe synchronization needed in production code

## Guidelines

1. **API Compatibility**: Keep std-compatible interfaces for easy exchange
2. **Tests**: Minimal meaningful coverage. Tests can use std.
3. **After completion**: Run `cargo fmt` and `cargo clippy`

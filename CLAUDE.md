# NVRC Development Guide

**Read [ARCHITECTURE.md](ARCHITECTURE.md) first** to understand the security
model, threat landscape, and design rationale before making changes.

## Project Overview

NVRC is a minimal init process (PID 1) for ephemeral confidential VMs with
NVIDIA GPUs. It configures GPU resources, starts required daemons
(nvidia-persistenced, nv-hostengine, etc.), and hands off to kata-agent.

## Long-term Goal: no_std

The goal is to eventually run NVRC as `no_std`. We are slowly transitioning
by building `hardened_std` as our security-hardened std replacement. This
enables:

- Smaller binary size
- No libstd dependency (pure syscall interface)
- Reduced attack surface
- Better control over all system interactions

**no_std Transition Roadmap:**

1. [ ] `hardened_std::fs` - File operations with path whitelisting
2. [ ] `hardened_std::process` - Process execution with binary whitelisting
3. [ ] `hardened_std::os::unix::net` - Unix sockets with path whitelisting
4. [ ] Replace `std::sync::Once` with `once_cell` (no_std compatible)
5. [ ] `hardened_std::fs::exists()` - Replace `std::path::Path::exists()`
6. [ ] `hardened_std::panic` - Panic hook and power_off for VM shutdown
7. [ ] Implement `std::os::fd::AsFd` for hardened types (nix poll integration)
8. [ ] `hardened_std::fs::copy()` - Copy with source/destination whitelisting
9. [ ] `hardened_std::fs::set_permissions()` - Permission control with path whitelisting
10. [ ] `hardened_std::fs::seal()` - Make file immutable (FS_IMMFL ioctl)
11. [ ] Audit all dependencies for no_std compatibility

**Runtime-writable config pattern:**

The rootfs image is read-only. Config files that need modification at runtime
(e.g. fabricmanager.cfg) must be copied to a writable tmpfs (`/run`) before
editing. Since `/run` stays writable after `mount::readonly("/")`, the file
must be sealed after writing: `copy()` -> `write()` -> `set_permissions(0o400)`
-> `seal()`. The `seal()` sets the immutable flag via `FS_IMMFL` ioctl so even
root cannot modify or delete the file without first clearing the flag.
`hardened_std` should enforce this copy-write-seal pattern for any file written
to tmpfs.

**Dependencies no_std status:**

- `anyhow` - yes, supports no_std (disable default features)
- `log` - yes, supports no_std
- `nix` - REMOVED, replaced with direct libc syscalls
- `once_cell` - yes, supports no_std
- `kernlog` - no, requires std (may need replacement)
- `rlimit` - needs investigation

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
3. **Functional style**: Prefer functional programming idioms over imperative
   control flow. Use combinators (`map`, `and_then`, `inspect`, `filter`,
   iterators) instead of `if`/`else` and mutable state where possible.
4. **After completion**: Run `cargo fmt` and `cargo clippy`
5. **Minimize Rust Crates**: Suggest smaller crates rather then  pulling in huge
   crates like nix
6. **KISS**: Minimize the cyclomatic complexity, keep code simple and stupid
7. **Self-describing code**: Names carry the WHAT so comments can focus on the
   WHY. If you reach for a comment to explain what a function, type, or
   variable does, rename it instead. Reserve comments for non-obvious WHY:
   hidden constraints, subtle invariants, source citations for magic constants
   (e.g. NIST test vectors), or workarounds for specific bugs. Don't restate
   what well-named code already says.

# Contribute to the NVIDIA `nvrc` Project

Want to contribute to the NVIDIA `nvrc` project? Awesome!
We only require you to sign your work as described in the following section.

## Sign your work

The sign-off is a simple signature at the end of the description for the patch.
Your signature certifies that you wrote the patch or otherwise have the right
to pass it on as an open-source patch.

The rules are pretty simple, and sign-off means that you certify the DCO below
(from [developercertificate.org](http://developercertificate.org/)):

```text
Developer Certificate of Origin
Version 1.1

Copyright (C) 2004, 2006 The Linux Foundation and its contributors.
1 Letterman Drive
Suite D4700
San Francisco, CA, 94129

Everyone is permitted to copy and distribute verbatim copies of this
license document, but changing it is not allowed.

Developer's Certificate of Origin 1.1

By making a contribution to this project, I certify that:

(a) The contribution was created in whole or in part by me and I
    have the right to submit it under the open source license
    indicated in the file; or

(b) The contribution is based upon previous work that, to the best
    of my knowledge, is covered under an appropriate open source
    license and I have the right under that license to submit that
    work with modifications, whether created in whole or in part
    by me, under the same open source license (unless I am
    permitted to submit under a different license), as indicated
    in the file; or

(c) The contribution was provided directly to me by some other
    person who certified (a), (b) or (c) and I have not modified
    it.

(d) I understand and agree that this project and the contribution
    are public and that a record of the contribution (including all
    personal information I submit with it, including my sign-off) is
    maintained indefinitely and may be redistributed consistent with
    this project or the open source license(s) involved.
```

To sign off, you just add the following line to every git commit message:

```text
Signed-off-by: Joe Smith <joe.smith@email.com>
```

You must use your real name (sorry, no pseudonyms or anonymous contributions).

If you set your `user.name` and `user.email` using git config, you can sign
your commit automatically with `git commit -s`.

## For AI Assistants (Claude Code, Cursor, GitHub Copilot, etc.)

If you're an AI coding assistant helping with this project, here's critical
context to avoid mistakes and understand the codebase architecture.

### Project Overview

NVRC is a minimal init system (PID 1) for ephemeral NVIDIA GPU-enabled VMs
running under Kata Containers in confidential computing environments. It sets
up GPU drivers, spawns management daemons, and hands off to kata-agent.

**Fail-Fast Philosophy**: This project intentionally panics and powers off the
VM on any error. Do NOT add error recovery, try/catch, or fallback logic.
Panics prevent undefined states in confidential VMs and are the correct design
choice here.

### Critical Warnings

1. **NEVER run `cargo test --lib`** - This specific test command can reboot
   the system. Use `cargo test` without the `--lib` flag.

2. **Testing Constraints**:
   - Many tests require root privileges (`sudo`)
   - Some tests require actual GPU hardware
   - Tests marked with `#[serial]` need exclusive hardware access
   - Check test names - they often indicate requirements (e.g.,
     `test_requires_root`)

3. **Fail-Fast is Intentional**:
   - DO NOT add error recovery mechanisms
   - Panics are designed to power off the VM safely
   - This prevents undefined states in confidential computing
   - The orchestrator (Kubernetes/Kata) handles retries

4. **Security Implications**:
   - Runs as PID 1 in confidential VMs
   - Changes can affect VM security posture
   - Minimal dependencies (9 direct deps) are critical
   - Must maintain static linking (musl target)
   - Read-only root filesystem after init

5. **Dependency Constraints**:
   - Keep dependencies minimal
   - All deps must be musl-compatible (static linking requirement)
   - Check `.cargo/config.toml` for build requirements

### Architecture Quick Reference

**Initialization Flow**:

```text
mount → logging → parse kernel params → mode dispatch → daemon spawn →
kata-agent fork → syslog poll
```

**Operation Modes**:

- `gpu` (default): Full GPU initialization with drivers and daemons
- `cpu`: Skip GPU setup, jump to kata-agent
- `nvswitch-nvl4`: NVSwitch mode for H100/H200/H800
- `nvswitch-nvl5`: NVSwitch mode for B200/B300/B100

**Configuration**: Via kernel parameters only (no config files) - see
`/proc/cmdline`

### Code Patterns to Follow

- **Use `must!` macro**: For init-critical operations that should panic on
  failure
- **Logging**: All output goes to kmsg via `kernlog` crate
- **Command execution**:
  - `foreground()` for synchronous commands
  - `background()` for daemons
- **Unsafe code**: Requires comprehensive `SAFETY:` comments explaining why
  it's safe
- **Testing**: Use `#[serial]` attribute for tests requiring exclusive
  hardware access

### Before Making Changes

1. Read [README.md](README.md) for full context on the fail-fast philosophy
2. Understand the security model (see README Security Model section)
3. Check if changes affect static linking requirements
4. Consider confidential computing implications
5. Verify changes don't add unnecessary dependencies

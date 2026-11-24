# Security Policy: NVRC

NVIDIA is dedicated to the security and trust of our software products and
services, including all source code repositories managed through our
organization.

NVRC is the init process (PID 1) of the guest VMs that Kata Containers
launches for NVIDIA GPU workloads, with or without confidential computing. If
you believe you have found a security vulnerability in NVRC, report it
privately. **Do not open a public
GitHub issue, pull request, or discussion for a potential vulnerability.**

## Reporting a Vulnerability

- **Web (preferred):** [NVIDIA Vulnerability Disclosure Program](https://www.nvidia.com/en-us/security/report-vulnerability/)
- **Email:** [psirt@nvidia.com](mailto:psirt@nvidia.com). Please encrypt with
  the [NVIDIA public PGP key](https://www.nvidia.com/en-us/security/pgp-key).
- **GitHub:** this repository's **Security** tab, **Report a vulnerability**.

Please include:

- The NVRC release tag or commit. With `nvrc.log=info` or higher, the guest
  boot log carries a line of the form `NVRC version=<version> sha256=<digest>`
  that identifies the binary; the default log level suppresses it.
- The Kata Containers release and guest image the binary was embedded in.
- GPU model, system type (PCIe, HGX, NVSwitch, ConnectX management), and the
  mode NVRC detected, visible with `nvrc.log=debug`.
- The kernel command line and the guest extension images in use.
- Vulnerability type, step-by-step reproduction, proof of concept if
  available, and the potential impact.

NVIDIA's Product Security Incident Response Team (PSIRT) acknowledges receipt,
validates severity, and coordinates remediation and disclosure under NVIDIA's
[coordinated vulnerability disclosure policy](https://www.nvidia.com/en-us/security/psirt-policies/).
NVIDIA may publish security bulletins and acknowledge reporters. If a potential
vulnerability is reported through a public channel, maintainers may limit the
discussion and redirect the reporter to a private channel.

### Coordination with Related Projects

NVRC ships inside Kata Containers guest images and starts the NVIDIA driver
stack. A single report often touches more than one project. NVIDIA PSIRT
coordinates with the relevant teams; please still report to PSIRT first.

- **Kata Containers** (kata-agent, runtime, guest image build): the Kata
  Containers Vulnerability Management Team, following the
  [Kata Containers security policy](https://github.com/kata-containers/kata-containers/blob/main/SECURITY.md).
  Fixes that require a new NVRC release are coordinated so the next Kata
  release picks them up.
- **NVIDIA GPU driver and daemons** (kernel modules, nvidia-persistenced,
  Fabric Manager, NVLSM, DCGM, NVIDIA Container Toolkit): NVIDIA PSIRT as
  the respective product.
- **Third-party Rust dependencies:** report to the upstream project. CI runs
  `cargo audit` and `cargo deny`; tell PSIRT if NVRC's use of a dependency
  is affected.

## Supported Versions

NVRC has no maintenance branches. Security fixes land on `main` and are
published as a new release tag; Kata Containers picks the new release up in
its next guest image build.

| Version            | Supported                                    |
| ------------------ | -------------------------------------------- |
| Latest release tag | Yes                                          |
| `main`             | Development; fixes land here first           |
| Older release tags | No. Rebuild guest images with the latest tag |

Published advisories appear on this repository's Security tab and, where
applicable, in NVIDIA security bulletins.

## Security Architecture and Context

NVRC is a statically linked Rust binary that runs as PID 1 in an ephemeral
VM. On boot it mounts the guest filesystems, verifies and mounts
guest extension images, discovers the NVIDIA devices assigned to the VM
through sysfs, loads the NVIDIA kernel modules, starts the daemons the
detected mode needs, generates the CDI specification, disables further kernel
module loading, and then execs kata-agent, which inherits PID 1. A forked
NVRC child stays behind for the guest lifetime to drain `/dev/log` into
`/run/syslog.log`; it keeps NVRC's privileges and is part of the
post-handoff attack surface. The root filesystem is read-only by construction
of the guest image; NVRC relies on that property and does not remount it.
NVRC has no configuration files, no shell, and no network listeners. The only
socket it keeps open is the `/dev/log` Unix datagram socket it binds for
daemon syslog; bringing up the loopback interface uses a transient ioctl
socket that is closed at once. Configuration comes solely from the kernel
command line: `nvrc.*` parameters for NVRC's own settings and
`kata.extension.<name>.verity_params` for extension verification.

The same binary serves confidential and non-confidential Kata guests. The
threat model below is written for the confidential case, where the host is
untrusted, because it is the stricter one. In a non-confidential guest the
host is trusted as in any Kata VM, so the host-boundary scenarios do not
apply there; NVRC behaves identically in both.

**Repository Exposure Classification:** Public. Basis: the canonical source is
publicly readable at `github.com/NVIDIA/nvrc`.

**Service Exposure Classification:** External / Regulated (high confidence).
Basis: release binaries are published with signatures and build provenance
and are embedded in externally distributed Kata Containers guest images,
including those used for confidential computing. This describes distribution
context, not the severity of any vulnerability.

The main trust boundaries are:

- **Host and hypervisor boundary.** In a confidential guest the host is
  untrusted. Everything the host presents to the guest is attacker-controlled
  input: virtual PCI configuration space and Vital Product Data exposed
  through sysfs, virtio devices, and extension block devices. NVRC matches
  PCI identity exactly and fails closed: an attribute or VPD it cannot read
  or parse, or a mix of direct NVSwitch and ConnectX management devices,
  panics and powers the VM off instead of guessing.
- **Measured inputs.** The kernel, root filesystem, and kernel command line
  are part of the launch measurement verified by remote attestation. The
  `nvrc.*` parameters and the per-extension dm-verity parameters are trusted
  only to the extent that measurement is enforced.
- **Extension image boundary.** Guest extension images, including the GPU
  driver stack, arrive as virtio-blk devices and are opened with dm-verity
  using root hashes from the measured command line. An extension without
  verity parameters is rejected.
- **NVIDIA daemon boundary.** NVRC starts daemons shipped in the GPU
  extension. nvidia-persistenced and Fabric Manager signal readiness through
  syslog messages forwarded into `/run/syslog.log`, waited for under a
  timeout that ends in power off; the other daemons get a single exit-status
  check right after spawning, so only a non-zero exit seen at that moment
  aborts boot. The daemons are trusted code, and their syslog output keeps
  being forwarded after handoff at a fixed drain rate.
- **kata-agent handoff.** After handoff, kata-agent is the guest control
  plane and handles untrusted container images and network input. The state
  NVRC leaves behind, with module loading disabled and the OOM score of
  kata-agent adjusted, on top of the read-only root image, is what the rest
  of the guest lifetime builds on. The retained syslog child holds the
  `/dev/log` socket and the log file with NVRC's privileges; it reads only
  the socket and writes only that file, plus the kernel log when debug
  logging is enabled, so daemon-controlled text can reach dmesg.
- **Build and release boundary.** Release binaries are built by GitHub
  Actions with build provenance attestations, signed with Sigstore, and
  published with an SBOM. With info logging enabled the binary logs its own
  SHA-256 at boot so a running guest can be matched against a published
  release.

### Threat Model

1. **Malicious host-presented device data.** A host crafts PCI vendor, class,
   or VPD attributes so that NVRC selects the wrong mode, skips fabric
   services, treats an unrelated NIC as a management port, or crashes a
   parser. **Security stance:** in scope. sysfs and VPD are untrusted input;
   NVRC matches exactly and fails closed: unreadable or malformed attributes
   and VPD panic rather than being treated as absent, and only a missing VPD
   attribute means a device is not a management port. A host that withholds
   devices gets a CPU-only boot, which is intended and not a vulnerability.
   Crafted data that makes NVRC start services it should not, or select a
   mode that misrepresents the hardware, is in scope.

2. **Extension image tampering.** The host substitutes or modifies an
   extension block device or strips its verity parameters. **Security
   stance:** defects in NVRC's verification are in scope, for example
   accepting an extension without verity, reading extension content before
   verity is active, or path traversal through an extension name. Tampering
   that dm-verity detects and that ends the boot is working as intended.

3. **Kernel command line abuse.** A parameter value reaches a daemon or tool
   as a raw argument, or changes behaviour beyond its documented meaning.
   **Security stance:** in scope. `nvrc.*` values are parsed into typed
   values, and the clock and power limits reach `nvidia-smi` as formatted
   numbers. The one raw string forwarded today, `nvrc.smi.srs`, is passed as
   a single argument vector element with no shell involved, so it cannot add
   arguments or commands; validating it into a closed set is in progress. A
   parameter reaching a tool by any other route is in scope. The command line is
   measured, so an operator choosing a documented value is configuration, not
   a vulnerability.

4. **Misbehaving or compromised NVIDIA daemon.** A daemon started by NVRC
   floods syslog, never signals readiness, or exits. **Security stance:**
   for nvidia-persistenced and Fabric Manager, a missing readiness marker is
   bounded by the wait timeout, which ends in power off, and that is
   accepted. NVLSM, nv-hostengine and dcgm-exporter get one exit-status check
   right after spawning; a later exit or hang is not detected by NVRC and is
   left to the workload and the host to notice. After handoff the syslog
   child keeps appending accepted messages to `/run/syslog.log` at a fixed
   drain rate for the guest lifetime, so a flooding daemon grows that tmpfs
   file slowly rather than hitting a timeout; a size cap on the file is
   planned hardening. Defects that let daemon output influence anything other
   than readiness are in scope. Vulnerabilities in the daemons themselves are
   reported to PSIRT as their own products.

5. **Lockdown bypass.** A path to load kernel modules after lockdown, make
   the root filesystem writable, or turn the retained syslog child into more
   than a log drain. **Security stance:** in scope. The ordering verify
   extensions, load modules, lock down, hand off is a security invariant, and
   so is the syslog child doing nothing but drain the socket into the file
   and, with debug logging on, the kernel log.

6. **Supply chain.** A tampered dependency, toolchain, or release artifact.
   **Security stance:** the release pipeline and published artifacts are in
   scope, including provenance, signatures, SBOM, and dependency pinning.
   Vulnerabilities in third-party crates are reported upstream and tracked
   through `cargo audit`.

7. **Fail-fast as denial of service.** Any input that triggers a panic powers
   the VM off. **Security stance:** a panic caused by host-controlled input is
   the intended fail-closed response; the host can stop the VM at any time
   regardless. A panic reachable from inside a container after handoff, for
   example through the `/dev/log` socket, is in scope.

### Critical Security Assumptions

- In confidential deployments the hardware root of trust, the TEE
  attestation flow, and where present the DPU firmware are trusted; the host,
  hypervisor, and virtual devices are not. In non-confidential deployments the
  host is trusted, as in any Kata guest.
- Confidential deployments measure the kernel, root filesystem, and command
  line and verify that measurement before releasing secrets. Without
  attestation the command line is only as trustworthy as the host that
  supplied it.
- Guest extension images are produced by the same build pipeline as the
  kernel and root filesystem, with dm-verity, and their root hashes arrive on
  the measured command line.
- The NVIDIA kernel modules and daemons shipped in the GPU extension are
  trusted code. NVRC does not sandbox them.
- kata-agent and the guest kernel enforce container isolation after handoff.
  NVRC does not take part in runtime container security.
- Consumers verify release artifacts against the published Sigstore
  signatures and provenance before embedding them in images.

## Out of Scope

- Vulnerabilities in the NVIDIA GPU driver, open kernel modules, Fabric
  Manager, NVLSM, nvidia-persistenced, DCGM, or the NVIDIA Container Toolkit.
  Report them to NVIDIA PSIRT as those products.
- kata-agent, the Kata runtime, the guest kernel configuration, and the guest
  image build. Report them under the Kata Containers security policy.
- Hypervisor, host operating system, firmware, and attestation services.
  Report them to the respective vendor.
- Behaviour on configurations outside the supported build pipeline, such as
  custom kernels or extensions without dm-verity.

For all other security-related concerns, visit NVIDIA's Product Security
portal at <https://www.nvidia.com/en-us/security>.

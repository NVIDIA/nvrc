# NVRC Release Process

This document describes how to create a new NVRC release.

## Prerequisites

- Write access to the repository
- Ability to merge PRs to `main` branch

## Important: Immutable Releases

This repository has **Immutable Releases** enabled as a security measure. Once a
release is published:

- Release assets cannot be modified or added
- The tag cannot be reused, even if deleted
- Each release attempt requires a unique version number

If a release workflow fails after creating the tag, you must bump the version
and try again. There is no way to "retry" with the same version.

**Note:** Even if you disable immutable releases after a failed release attempt,
you still cannot reuse the same version number. GitHub permanently remembers
that the tag/release existed. You must always bump to a new version.

## Release Workflow

### Step 1: Bump Version in Cargo.toml

Create a PR that updates the version in `Cargo.toml`:

```toml
[package]
name = "NVRC"
version = "X.Y.Z"  # Update this
```

The version in `Cargo.toml` is the **source of truth** for release tags. The
workflow automatically derives the tag name as `vX.Y.Z` from this version.

Merge the PR to `main` after review.

### Step 2: Trigger the Release Workflow

1. Go to **Actions** in the GitHub repository
2. Select **Release NVRC** workflow from the left sidebar
3. Click **Run workflow**
4. Ensure **main** branch is selected (this is the default and required)
5. Click the green **Run workflow** button

**Important:** Always run from `main` branch. The signature verification step
validates that artifacts were built from `main` using certificate constraints
like `--certificate-github-workflow-ref`.

### Step 3: Monitor the Workflow

The release workflow performs these steps:

1. **preflight** - Derives tag from `Cargo.toml`, checks no existing release,
   creates git tag
2. **build-and-release** - Builds binaries for x86_64 and aarch64, signs with
   Sigstore/cosign, creates tarballs
3. **create-release** - Creates a **draft** GitHub release with tarballs
4. **provenance** - Generates SLSA Level 3 provenance attestations
5. **provenance-publish** - Uploads provenance to the draft release
6. **release-notes** - Adds VERIFY.md content to release body
7. **publish-release** - Publishes the draft (makes it immutable)
8. **verify-signatures** - Verifies all signatures and provenance

The release remains a draft until all assets are uploaded, then gets published
in a single atomic operation. This ensures the immutable release contains all
required artifacts.

## Troubleshooting

### "Release already exists" Error

If preflight fails with "Release already exists", a previous release attempt
partially succeeded. With immutable releases enabled, you cannot reuse the
version. Bump the version in `Cargo.toml` and try again.

### "Cannot upload assets to an immutable release" Error

This occurs if the release was published before all assets were uploaded. The
workflow uses draft releases to prevent this, but if it happens:

1. Delete the release: `gh release delete vX.Y.Z --repo NVIDIA/nvrc --yes`
2. Delete the tag: `gh api -X DELETE repos/NVIDIA/nvrc/git/refs/tags/vX.Y.Z`
3. Bump version and retry (required if immutable releases is enabled)

### Tag Creation Fails

If tag creation fails with repository rule violations, there may be org-level
restrictions. Contact the repository administrator.

### Signature Verification Fails

The verify-signatures job validates:

- Cosign keyless signatures (online via Rekor)
- Cosign bundle signatures (offline verification)
- SLSA provenance (via slsa-verifier)

Failures here indicate a problem with the signing process or certificate
constraints. Check that the workflow ran from `main` branch.

## Release Artifacts

Each release includes per-architecture:

| File                                       | Description                              |
| ------------------------------------------ | ---------------------------------------- |
| `NVRC-{arch}.tar.xz`                       | Tarball containing binary and signatures |
| `NVRC-{arch}.tar.xz.sig`                   | Cosign signature for tarball             |
| `NVRC-{arch}.tar.xz.cert`                  | Cosign certificate for tarball           |
| `NVRC-{arch}.tar.xz.bundle.json`           | Rekor bundle for offline verification    |
| `NVRC-{arch}.intoto.jsonl`                 | SLSA provenance attestation              |

Inside each tarball:

| File                                       | Description          |
| ------------------------------------------ | -------------------- |
| `NVRC-{arch}`                              | The binary           |
| `NVRC-{arch}.sig`                          | Binary signature     |
| `NVRC-{arch}.cert`                         | Binary certificate   |
| `NVRC-{arch}.bundle.json`                  | Binary Rekor bundle  |
| `sbom-NVRC-{arch}.spdx.json`               | SBOM in SPDX format  |
| `sbom-NVRC-{arch}.spdx.json.sig`           | SBOM signature       |
| `sbom-NVRC-{arch}.spdx.json.cert`          | SBOM certificate     |
| `sbom-NVRC-{arch}.spdx.json.bundle.json`   | SBOM Rekor bundle    |

## Verifying a Release

The release workflow automatically runs signature verification as the final step
(`verify-signatures` job) to ensure all artifacts are correctly signed before
the release is considered complete. This serves as a sanity check that the
signing and publishing process succeeded.

For manual verification of downloaded releases, see [VERIFY.md](VERIFY.md) for
instructions on verifying release signatures and provenance.

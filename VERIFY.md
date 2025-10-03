# Verify NVRC release artifacts

This guide shows how to verify the **binary**, **checksum**, **SBOM**, and **SLSA provenance** for a given release.

## Prerequisites

- [Cosign](https://docs.sigstore.dev/) v2+ (signature & bundle verification)
- [SLSA Verifier](https://github.com/slsa-framework/slsa-verifier) v2.7.1+
- (Optional) [GitHub CLI](https://cli.github.com/) `gh` to download assets

---

## 1) Set variables

```bash
# Choose your target:
export TARGET=x86_64-unknown-linux-musl     # or: aarch64-unknown-linux-musl

# Set the release tag and repository:
export TAG="vX.Y.Z"
export REPO="owner/repo"
```

**Example:**
```bash
export TARGET=x86_64-unknown-linux-musl
export TAG="v0.0.1"
export REPO="NVIDIA/nvrc"
```

---

## 2) Download the release assets

```bash
gh release download "$TAG" --repo "$REPO"   --pattern "NVRC-$TARGET"   --pattern "NVRC-$TARGET.*"   --pattern "sbom-NVRC-$TARGET.*"   --pattern "NVRC-$TARGET.intoto.jsonl"   --dir .
```

You should now have (per target):

```
NVRC-$TARGET
NVRC-$TARGET.sig
NVRC-$TARGET.cert
NVRC-$TARGET.bundle.json

NVRC-$TARGET.sha256
NVRC-$TARGET.sha256.sig
NVRC-$TARGET.sha256.cert
NVRC-$TARGET.sha256.bundle.json

sbom-NVRC-$TARGET.spdx.json
sbom-NVRC-$TARGET.spdx.json.sig
sbom-NVRC-$TARGET.spdx.json.cert
sbom-NVRC-$TARGET.spdx.json.bundle.json

NVRC-$TARGET.intoto.jsonl
```

---

## 3) Verify the checksum

```bash
sha256sum -c "NVRC-$TARGET.sha256"
```

---

## 4) Verify signatures (online with Rekor)

This project’s releases are **built from `main`** (the workflow ref in the signing certificate ends with `@refs/heads/main`). The commands below pin identity accordingly.

```bash
# Binary
cosign verify-blob   --rekor-url https://rekor.sigstore.dev   --certificate "NVRC-$TARGET.cert"   --signature   "NVRC-$TARGET.sig"   --certificate-identity-regexp "^https://github.com/$REPO/.github/workflows/.+@refs/heads/main$"   --certificate-oidc-issuer "https://token.actions.githubusercontent.com"   "NVRC-$TARGET"

# Checksum file
cosign verify-blob   --rekor-url https://rekor.sigstore.dev   --certificate "NVRC-$TARGET.sha256.cert"   --signature   "NVRC-$TARGET.sha256.sig"   --certificate-identity-regexp "^https://github.com/$REPO/.github/workflows/.+@refs/heads/main$"   --certificate-oidc-issuer "https://token.actions.githubusercontent.com"   "NVRC-$TARGET.sha256"

# SBOM (SPDX JSON)
cosign verify-blob   --rekor-url https://rekor.sigstore.dev   --certificate "sbom-NVRC-$TARGET.spdx.json.cert"   --signature   "sbom-NVRC-$TARGET.spdx.json.sig"   --certificate-identity-regexp "^https://github.com/$REPO/.github/workflows/.+@refs/heads/main$"   --certificate-oidc-issuer "https://token.actions.githubusercontent.com"   "sbom-NVRC-$TARGET.spdx.json"
```

> Tip: if you later switch to tag-triggered builds, change the regex to:
> `^https://github.com/$REPO/.github/workflows/.+@refs/tags/$TAG$`

---

## 5) Verify signatures (offline using bundles)

Bundles embed Rekor proofs so you can verify without network access.

```bash
cosign verify-blob --bundle "NVRC-$TARGET.bundle.json" "NVRC-$TARGET"
cosign verify-blob --bundle "NVRC-$TARGET.sha256.bundle.json" "NVRC-$TARGET.sha256"
cosign verify-blob --bundle "sbom-NVRC-$TARGET.spdx.json.bundle.json" "sbom-NVRC-$TARGET.spdx.json"
```

---

## 6) Verify SLSA provenance

Releases are built from `main`, so verify against the branch.

```bash
PROV="NVRC-$TARGET.intoto.jsonl"

slsa-verifier verify-artifact "NVRC-$TARGET"   --provenance-path "$PROV"   --source-uri "github.com/$REPO"   --source-branch "main"
```

---

## Notes & troubleshooting

- If **identity check fails** in step 4, ensure `REPO` and the regex (main vs tag) match how the release was built.
- If `NVRC-$TARGET.intoto.jsonl` is missing, confirm the release includes the per-target provenance asset.
- If you prefer to verify just one artifact, you can skip unrelated commands (e.g., SBOM).
- For more detail, use `--verbose` and consider setting `COSIGN_EXPERIMENTAL=1` when experimenting locally.
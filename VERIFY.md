# Verify NVRC release artifacts

This guide shows how to verify the **tarball**, the **extracted binary**, the
**SBOM**, and the **SLSA provenance** for a given release.

## Prerequisites

- [Cosign](https://docs.sigstore.dev/) v2+ (signature & bundle verification)
- [SLSA Verifier](https://github.com/slsa-framework/slsa-verifier) v2.7.1+
- (Optional) [GitHub CLI](https://cli.github.com/) `gh` to download assets

---

## 1) Set variables

```bash
# Choose your target architecture:
export TARGET=x86_64-unknown-linux-musl   # or: aarch64-unknown-linux-musl

# Set the release tag and repository:
export TAG="vX.Y.Z"
export REPO="NVIDIA/nvrc"
```

**Example:**

```bash
export TARGET=x86_64-unknown-linux-musl
export TAG="v0.1.0"
export REPO="NVIDIA/nvrc"
```

---

## 2) Download the release assets

The release contains a **tarball** per target architecture and a
per-artifact **SLSA provenance** file.

```bash
gh release download "$TAG" --repo "$REPO" \
  --pattern "NVRC-${TARGET}.tar.xz" \
  --pattern "NVRC-${TARGET}.tar.xz.*" \
  --pattern "NVRC-${TARGET}.intoto.jsonl" \
  --dir .
```

You should now have:

```text
NVRC-${TARGET}.tar.xz
NVRC-${TARGET}.tar.xz.sig
NVRC-${TARGET}.tar.xz.cert
NVRC-${TARGET}.tar.xz.bundle.json

NVRC-${TARGET}.intoto.jsonl
```

---

## 3) Verify the **tarball**

These commands assert the tarball was signed by this repository's **GitHub
Actions** workflow on **main** and is recorded in Rekor.
(Online verification talks to Rekor; offline verification uses the embedded
bundle.)

```bash
# Online verification (Rekor)
cosign verify-blob \
  --rekor-url https://rekor.sigstore.dev \
  --certificate "NVRC-${TARGET}.tar.xz.cert" \
  --signature   "NVRC-${TARGET}.tar.xz.sig" \
  --certificate-identity-regexp "^https://github.com/$REPO/.github/workflows/.+@refs/heads/main$" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
  "NVRC-${TARGET}.tar.xz"

# Offline verification (bundle)
cosign verify-blob \
  --bundle "NVRC-${TARGET}.tar.xz.bundle.json" \
  --certificate-identity-regexp "^https://github.com/$REPO/.github/workflows/.+@refs/heads/main$" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
  "NVRC-${TARGET}.tar.xz"
```

Extract files for further checks:

```bash
tar -xf "NVRC-${TARGET}.tar.xz"
```

After extraction, you should see:

```text
NVRC-${TARGET}
NVRC-${TARGET}.sig
NVRC-${TARGET}.cert
NVRC-${TARGET}.bundle.json

sbom-NVRC-${TARGET}.spdx.json
sbom-NVRC-${TARGET}.spdx.json.sig
sbom-NVRC-${TARGET}.spdx.json.cert
sbom-NVRC-${TARGET}.spdx.json.bundle.json
```

---

## 4) Verify the **binary** and **SBOM** (online with Rekor)

```bash
# Binary
cosign verify-blob \
  --rekor-url https://rekor.sigstore.dev \
  --certificate "NVRC-${TARGET}.cert" \
  --signature   "NVRC-${TARGET}.sig" \
  --certificate-identity-regexp "^https://github.com/$REPO/.github/workflows/.+@refs/heads/main$" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
  "NVRC-${TARGET}"

# SBOM (SPDX JSON)
cosign verify-blob \
  --rekor-url https://rekor.sigstore.dev \
  --certificate "sbom-NVRC-${TARGET}.spdx.json.cert" \
  --signature   "sbom-NVRC-${TARGET}.spdx.json.sig" \
  --certificate-identity-regexp "^https://github.com/$REPO/.github/workflows/.+@refs/heads/main$" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
  "sbom-NVRC-${TARGET}.spdx.json"
```

### (Optional) Offline verification using bundles

```bash
cosign verify-blob \
  --bundle "NVRC-${TARGET}.bundle.json" \
  --certificate-identity-regexp "^https://github.com/$REPO/.github/workflows/.+@refs/heads/main$" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
  "NVRC-${TARGET}"

cosign verify-blob \
  --bundle "sbom-NVRC-${TARGET}.spdx.json.bundle.json" \
  --certificate-identity-regexp "^https://github.com/$REPO/.github/workflows/.+@refs/heads/main$" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
  "sbom-NVRC-${TARGET}.spdx.json"
```

> Tip: if you later switch to tag-triggered builds, change the regex to:
> `^https://github.com/$REPO/.github/workflows/.+@refs/tags/$TAG$`

---

## 5) Verify **SLSA provenance**

Releases are built from `main`, so verify against the branch.

```bash
slsa-verifier verify-artifact "NVRC-${TARGET}" \
  --provenance-path "NVRC-${TARGET}.intoto.jsonl" \
  --source-uri "github.com/$REPO" \
  --source-branch "main"
```

---

## 6) (Optional) Rekor references

If the release includes `REKOR-REFERENCES.md` and `rekor-references.json`,
they list each artifact's **UUID** and **logIndex** in Rekor:

- `https://rekor.sigstore.dev/api/v1/log/entries/<uuid>`

These are extracted from the published **bundle** files for precise,
reproducible auditing.

---

## 7) Advanced: strict identity pinning (recommended)

For maximum assurance, pin additional **GitHub-specific** claims embedded in
the Fulcio certificate. If you know your workflow file and branch, use:

```bash
WF_FILE="release.yaml"              # workflow filename under .github/workflows/
WF_REF="refs/heads/main"            # release branch
WF_REPO="$REPO"                     # e.g., NVIDIA/nvrc

# Tarball (same flags also apply to binary/SBOM verifies)
cosign verify-blob \
  --rekor-url https://rekor.sigstore.dev \
  --certificate-identity "https://github.com/$WF_REPO/.github/workflows/$WF_FILE@$WF_REF" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
  --certificate-github-workflow-repository "$WF_REPO" \
  --certificate-github-workflow-ref "$WF_REF" \
  "NVRC-${TARGET}.tar.xz"

# Binary
cosign verify-blob \
  --rekor-url https://rekor.sigstore.dev \
  --certificate-identity "https://github.com/$WF_REPO/.github/workflows/$WF_FILE@$WF_REF" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
  --certificate-github-workflow-repository "$WF_REPO" \
  --certificate-github-workflow-ref "$WF_REF" \
  "NVRC-${TARGET}"

# SBOM
cosign verify-blob \
  --rekor-url https://rekor.sigstore.dev \
  --certificate-identity "https://github.com/$WF_REPO/.github/workflows/$WF_FILE@$WF_REF" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
  --certificate-github-workflow-repository "$WF_REPO" \
  --certificate-github-workflow-ref "$WF_REF" \
  "sbom-NVRC-${TARGET}.spdx.json"
```

Don't know the exact workflow filename? Stick with the **regex** form used
above (it already pins to your repo and branch).

If you run releases via `workflow_dispatch`, you can also pin the trigger:

```text
--certificate-github-workflow-trigger "workflow_dispatch"
```

---

## Notes & troubleshooting

- If **identity check fails**, ensure `REPO` and the regex
  (`@refs/heads/main` vs `@refs/tags/$TAG`) match how the release was built.
- If `NVRC-${TARGET}.intoto.jsonl` is missing, confirm the release includes
  per-target provenance.
- For more detail, add `--verbose` and optionally set `COSIGN_EXPERIMENTAL=1`
  when experimenting locally.

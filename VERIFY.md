# Verify NVRC release artifacts

This guide shows how to verify the **tarball**, the **extracted binary**, the
**SBOM**, and the **SLSA provenance** for a given release.
Releases ship **two flavors** per target:

- `NVRC` (standard)
- `NVRC-confidential` (built with `--features=confidential`)

## Prerequisites

- [Cosign](https://docs.sigstore.dev/) v2+ (signature & bundle verification)
- [SLSA Verifier](https://github.com/slsa-framework/slsa-verifier) v2.7.1+
- (Optional) [GitHub CLI](https://cli.github.com/) `gh` to download assets

---

## 1) Set variables

```bash
# Choose your flavor (binary name prefix) and target:
export BIN=NVRC                 # or: NVRC-confidential
export TARGET=x86_64-unknown-linux-musl   # or: aarch64-unknown-linux-musl

# Set the release tag and repository:
export TAG="vX.Y.Z"
export REPO="owner/repo"
```

**Example:**

```bash
export BIN=NVRC-confidential
export TARGET=x86_64-unknown-linux-musl
export TAG="v0.0.1"
export REPO="NVIDIA/nvrc"
```

---

## 2) Download the release assets

The release contains a **single tarball** per (flavor, target) and a
per-artifact **SLSA provenance** file.

```bash
gh release download "$TAG" --repo "$REPO"   --pattern "${BIN}-${TARGET}.tar.xz"   --pattern "${BIN}-${TARGET}.tar.xz.*"   --pattern "${BIN}-${TARGET}.intoto.jsonl"   --dir .
```

You should now have:

```text
${BIN}-${TARGET}.tar.xz
${BIN}-${TARGET}.tar.xz.sig
${BIN}-${TARGET}.tar.xz.cert
${BIN}-${TARGET}.tar.xz.bundle.json

${BIN}-${TARGET}.intoto.jsonl
```

---

## 3) Verify the **tarball**

These commands assert the tarball was signed by this repository's **GitHub
Actions** workflow on **main** and is recorded in Rekor.
(Online verification talks to Rekor; offline verification uses the embedded
bundle.)

```bash
# Online verification (Rekor)
cosign verify-blob   --rekor-url https://rekor.sigstore.dev   --certificate "${BIN}-${TARGET}.tar.xz.cert"   --signature   "${BIN}-${TARGET}.tar.xz.sig"   --certificate-identity-regexp "^https://github.com/$REPO/.github/workflows/.+@refs/heads/main$"   --certificate-oidc-issuer "https://token.actions.githubusercontent.com"   "${BIN}-${TARGET}.tar.xz"

# Offline verification (bundle)
cosign verify-blob --bundle "${BIN}-${TARGET}.tar.xz.bundle.json"   --certificate-identity-regexp "^https://github.com/$REPO/.github/workflows/.+@refs/heads/main$"   --certificate-oidc-issuer "https://token.actions.githubusercontent.com"   "${BIN}-${TARGET}.tar.xz"
```

Extract files for further checks:

```bash
tar -xf "${BIN}-${TARGET}.tar.xz"
```

After extraction, you should see (per (flavor, target)):

```text
${BIN}-${TARGET}
${BIN}-${TARGET}.sig
${BIN}-${TARGET}.cert
${BIN}-${TARGET}.bundle.json

sbom-${BIN}-${TARGET}.spdx.json
sbom-${BIN}-${TARGET}.spdx.json.sig
sbom-${BIN}-${TARGET}.spdx.json.cert
sbom-${BIN}-${TARGET}.spdx.json.bundle.json
```

---

## 4) Verify the **binary** and **SBOM** (online with Rekor)

```bash
# Binary
cosign verify-blob   --rekor-url https://rekor.sigstore.dev   --certificate "${BIN}-${TARGET}.cert"   --signature   "${BIN}-${TARGET}.sig"   --certificate-identity-regexp "^https://github.com/$REPO/.github/workflows/.+@refs/heads/main$"   --certificate-oidc-issuer "https://token.actions.githubusercontent.com"   "${BIN}-${TARGET}"

# SBOM (SPDX JSON)
cosign verify-blob   --rekor-url https://rekor.sigstore.dev   --certificate "sbom-${BIN}-${TARGET}.spdx.json.cert"   --signature   "sbom-${BIN}-${TARGET}.spdx.json.sig"   --certificate-identity-regexp "^https://github.com/$REPO/.github/workflows/.+@refs/heads/main$"   --certificate-oidc-issuer "https://token.actions.githubusercontent.com"   "sbom-${BIN}-${TARGET}.spdx.json"
```

### (Optional) Offline verification using bundles

```bash
cosign verify-blob --bundle "${BIN}-${TARGET}.bundle.json" "${BIN}-${TARGET}"
cosign verify-blob --bundle "sbom-${BIN}-${TARGET}.spdx.json.bundle.json" "sbom-${BIN}-${TARGET}.spdx.json"
```

> Tip: if you later switch to tag-triggered builds, change the regex to:
> `^https://github.com/$REPO/.github/workflows/.+@refs/tags/$TAG$`

---

## 5) Verify **SLSA provenance**

Releases are built from `main`, so verify against the branch.

```bash
PROV="${BIN}-${TARGET}.intoto.jsonl"

slsa-verifier verify-artifact "${BIN}-${TARGET}"   --provenance-path "$PROV"   --source-uri "github.com/$REPO"   --source-branch "main"
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
WF_FILE="release.yml"               # your workflow filename under .github/workflows/
WF_REF="refs/heads/main"            # your release branch
WF_REPO="$REPO"                     # e.g., NVIDIA/nvrc

# Tarball (same flags also apply to binary/SBOM verifies)
cosign verify-blob   --rekor-url https://rekor.sigstore.dev   --certificate-identity "https://github.com/$WF_REPO/.github/workflows/$WF_FILE@$WF_REF"   --certificate-oidc-issuer "https://token.actions.githubusercontent.com"   --certificate-github-workflow-repository "$WF_REPO"   --certificate-github-workflow-ref "$WF_REF"   "${BIN}-${TARGET}.tar.xz"

# Binary
cosign verify-blob   --rekor-url https://rekor.sigstore.dev   --certificate-identity "https://github.com/$WF_REPO/.github/workflows/$WF_FILE@$WF_REF"   --certificate-oidc-issuer "https://token.actions.githubusercontent.com"   --certificate-github-workflow-repository "$WF_REPO"   --certificate-github-workflow-ref "$WF_REF"   "${BIN}-${TARGET}"

# SBOM
cosign verify-blob   --rekor-url https://rekor.sigstore.dev   --certificate-identity "https://github.com/$WF_REPO/.github/workflows/$WF_FILE@$WF_REF"   --certificate-oidc-issuer "https://token.actions.githubusercontent.com"   --certificate-github-workflow-repository "$WF_REPO"   --certificate-github-workflow-ref "$WF_REF"   "sbom-${BIN}-${TARGET}.spdx.json"
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
- If `${BIN}-${TARGET}.intoto.jsonl` is missing, confirm the release includes
  per-(flavor,target) provenance.
- For more detail, add `--verbose` and optionally set `COSIGN_EXPERIMENTAL=1`
  when experimenting locally.

#!/usr/bin/env bash
# Check that every `uses:` reference a pull request adds is permitted by the
# repository's allowed-actions policy (Settings → Actions → General →
# "Allow select actions and reusable workflows").
#
# Usage: check-actions-allowlist.sh <pr-number>
#
# Environment:
#   GH_REPO         owner/name of the repository (read by the gh CLI)
#   GH_TOKEN        token with pull request read access, used for `gh pr diff`
#   ALLOWLIST_JSON  path to the JSON returned by
#                   GET /repos/{owner}/{repo}/actions/permissions/selected-actions
#
# Only lines the pull request adds are inspected: everything else already
# lives on the base branch and was vetted when it landed there.
#
# Matching mirrors GitHub's policy: actions owned by `actions` or `github`
# pass when github_owned_allowed is set, `*` is the only wildcard in
# patterns_allowed, and names compare case-insensitively. GitHub also runs
# actions from Marketplace verified creators without listing them, but that
# status is not exposed through the API, so such references are reported as
# denied and left to a human.
#
# Output: one `allowed<TAB>ref` or `denied<TAB>ref` line per reference on
# stdout; progress and errors go to stderr.
#
# Exit status: 0 when every added reference is allowed, 1 when at least one
# is denied, 2 on usage or input errors.

set -euo pipefail

pr="${1:?usage: $0 <pr-number>}"
: "${ALLOWLIST_JSON:?ALLOWLIST_JSON must point to the selected-actions JSON}"

if ! jq -e 'has("patterns_allowed")' "${ALLOWLIST_JSON}" >/dev/null 2>&1; then
    echo "::error::${ALLOWLIST_JSON} is not a selected-actions policy document" >&2
    exit 2
fi

github_owned_allowed=$(jq -r '.github_owned_allowed // false' "${ALLOWLIST_JSON}")
mapfile -t patterns < <(jq -r '.patterns_allowed[]?' "${ALLOWLIST_JSON}")

# Added `uses:` lines, stripped of list markers, trailing comments and quotes.
# `+++ b/path` headers do not match because `++` follows the leading `+`.
mapfile -t refs < <(
    gh pr diff "${pr}" \
        | { grep -E '^\+[[:space:]]*-?[[:space:]]*uses:' || true; } \
        | sed -E 's/^\+[[:space:]]*-?[[:space:]]*uses:[[:space:]]*//; s/[[:space:]]+#.*$//; s/["'"'"']//g; s/[[:space:]]+$//' \
        | sort -u
)

shopt -s nocasematch

is_allowed() {
    local ref="$1"
    local owner="${ref%%/*}"
    local pattern

    case "${ref}" in
        ./*) return 0 ;;        # local action: part of this repository
        docker://*) return 1 ;; # container images are outside the policy; review by hand
    esac

    if [[ "${github_owned_allowed}" == "true" ]] &&
        [[ "${owner}" == "actions" || "${owner}" == "github" ]]; then
        return 0
    fi

    for pattern in "${patterns[@]}"; do
        # shellcheck disable=SC2053  # unquoted on purpose: `*` in the policy is a glob
        [[ "${ref}" == ${pattern} ]] && return 0
    done
    return 1
}

denied=0
if ((${#refs[@]} == 0)); then
    echo "PR #${pr}: no added 'uses:' references, nothing to check" >&2
fi
for ref in "${refs[@]}"; do
    if is_allowed "${ref}"; then
        printf 'allowed\t%s\n' "${ref}"
    else
        printf 'denied\t%s\n' "${ref}"
        denied=$((denied + 1))
    fi
done

if ((denied > 0)); then
    echo "PR #${pr}: ${denied} reference(s) not covered by the allowed-actions policy" >&2
    exit 1
fi
echo "PR #${pr}: all ${#refs[@]} added reference(s) are allowed" >&2

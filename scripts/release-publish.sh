#!/bin/sh
# SPDX-License-Identifier: FSL-1.1-ALv2

set -eu

usage() {
  cat <<'USAGE'
Usage: mise run release-publish [--dry-run] [vX.Y.Z|X.Y.Z]

Dispatches the stable release workflow for the current workspace version.
If no version is provided, the script uses the version in Cargo.toml.
USAGE
}

die() {
  echo "release-publish: $*" >&2
  exit 1
}

current_version() {
  # Keep this in sync with scripts/release-prep.sh.
  awk -F'"' '/^version = / { print $2; exit }' Cargo.toml
}

is_stable_semver() {
  # Keep this in sync with scripts/release-prep.sh.
  echo "$1" | grep -Eq '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'
}

confirm() {
  mode="publish"
  if [ "$DRY_RUN" = "true" ]; then
    mode="dry-run"
  fi

  cat <<EOF
Current version: $CURRENT_VERSION
Release tag: v$RELEASE_VERSION
Commit: $LOCAL_HEAD
Workflow mode: $mode

Ready to dispatch the stable release workflow for v$RELEASE_VERSION? [y/N]
EOF

  read -r answer
  case "$answer" in
    y|Y|yes|YES)
      ;;
    *)
      echo "Aborted."
      exit 0
      ;;
  esac
}

DRY_RUN=false
REQUEST=""

while [ "$#" -gt 0 ]; do
  case "$1" in
    -h|--help)
      usage
      exit 0
      ;;
    --dry-run)
      DRY_RUN=true
      ;;
    v[0-9]*.[0-9]*.[0-9]*)
      [ -z "$REQUEST" ] || die "only one version may be provided"
      REQUEST="${1#v}"
      ;;
    [0-9]*.[0-9]*.[0-9]*)
      [ -z "$REQUEST" ] || die "only one version may be provided"
      REQUEST="$1"
      ;;
    *)
      usage >&2
      die "expected --dry-run, vX.Y.Z, or X.Y.Z"
      ;;
  esac
  shift
done

command -v gh >/dev/null 2>&1 || die "GitHub CLI is required"
gh auth status >/dev/null 2>&1 || die "gh is not authenticated; run 'gh auth login'"
REPO="$(gh repo view --json nameWithOwner --jq .nameWithOwner)"
[ -n "$REPO" ] || die "could not resolve GitHub repository"

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

[ -z "$(git status --porcelain)" ] || die "worktree must be clean"

BRANCH="$(git branch --show-current)"
[ "$BRANCH" = "main" ] || die "stable release workflow must be dispatched from main, currently on $BRANCH"

git fetch origin main --tags >/dev/null
LOCAL_HEAD="$(git rev-parse main)"
REMOTE_HEAD="$(git rev-parse origin/main)"
[ "$LOCAL_HEAD" = "$REMOTE_HEAD" ] || die "main must match origin/main before release publishing"

UPSTREAM="$(git rev-parse --abbrev-ref --symbolic-full-name '@{upstream}' 2>/dev/null || true)"
[ "$UPSTREAM" = "origin/main" ] || die "main must track origin/main before release publishing"

CURRENT_VERSION="$(current_version)"
is_stable_semver "$CURRENT_VERSION" || die "current version must be stable X.Y.Z, got $CURRENT_VERSION"

if [ -n "$REQUEST" ]; then
  is_stable_semver "$REQUEST" || die "requested version must be stable X.Y.Z, got $REQUEST"
  [ "$REQUEST" = "$CURRENT_VERSION" ] || die "requested version $REQUEST does not match Cargo.toml version $CURRENT_VERSION"
fi

RELEASE_VERSION="$CURRENT_VERSION"
RELEASE_TAG="v$RELEASE_VERSION"

if git show-ref --verify --quiet "refs/tags/$RELEASE_TAG"; then
  LOCAL_TAG_COMMIT="$(git rev-list -n 1 "$RELEASE_TAG")"
  [ "$LOCAL_TAG_COMMIT" = "$LOCAL_HEAD" ] || die "local tag $RELEASE_TAG already exists at $LOCAL_TAG_COMMIT, not $LOCAL_HEAD"
fi
if git ls-remote --exit-code --tags origin "refs/tags/$RELEASE_TAG" >/dev/null 2>&1; then
  REMOTE_TAG_COMMIT="$(git ls-remote --tags origin "refs/tags/$RELEASE_TAG^{}" | awk '{ print $1 }')"
  if [ -z "$REMOTE_TAG_COMMIT" ]; then
    REMOTE_TAG_COMMIT="$(git ls-remote --tags origin "refs/tags/$RELEASE_TAG" | awk '{ print $1 }')"
  fi
  [ "$REMOTE_TAG_COMMIT" = "$LOCAL_HEAD" ] || die "remote tag $RELEASE_TAG already exists at $REMOTE_TAG_COMMIT, not $LOCAL_HEAD"
fi

gh workflow view release.yml --repo "$REPO" >/dev/null 2>&1 \
  || die "release.yml workflow is not available on GitHub yet"

confirm

gh workflow run release.yml \
  --repo "$REPO" \
  --ref main \
  -f "version=$RELEASE_VERSION" \
  -f "dry_run=$DRY_RUN"

echo "Dispatched release workflow for $RELEASE_TAG."
echo "View runs with: gh run list --workflow release.yml --limit 1"

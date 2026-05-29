#!/bin/sh
# SPDX-License-Identifier: FSL-1.1-ALv2

set -eu

usage() {
  cat <<'USAGE'
Usage: mise run release-tag [vX.Y.Z|X.Y.Z]

Creates and pushes the stable release tag for the current workspace version.
If no version is provided, the script uses the version in Cargo.toml.
USAGE
}

die() {
  echo "release-tag: $*" >&2
  exit 1
}

current_version() {
  awk -F'"' '/^version = / { print $2; exit }' Cargo.toml
}

is_stable_semver() {
  echo "$1" | grep -Eq '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'
}

confirm() {
  cat <<EOF
Current version: $CURRENT_VERSION
Release tag: v$RELEASE_VERSION
Commit: $LOCAL_HEAD

Ready to create and push tag v$RELEASE_VERSION? [y/N]
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

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
  usage
  exit 0
fi

if [ "$#" -gt 1 ]; then
  usage >&2
  exit 2
fi

REQUEST="${1:-}"
case "$REQUEST" in
  "")
    ;;
  v[0-9]*.[0-9]*.[0-9]*)
    REQUEST="${REQUEST#v}"
    ;;
  [0-9]*.[0-9]*.[0-9]*)
    ;;
  *)
    usage >&2
    die "expected vX.Y.Z or X.Y.Z"
    ;;
esac

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

[ -z "$(git status --porcelain)" ] || die "worktree must be clean"

BRANCH="$(git branch --show-current)"
[ "$BRANCH" = "main" ] || die "release tag must be created from main, currently on $BRANCH"

git fetch origin main --tags >/dev/null
LOCAL_HEAD="$(git rev-parse main)"
REMOTE_HEAD="$(git rev-parse origin/main)"
[ "$LOCAL_HEAD" = "$REMOTE_HEAD" ] || die "main must match origin/main before tagging"

UPSTREAM="$(git rev-parse --abbrev-ref --symbolic-full-name '@{upstream}' 2>/dev/null || true)"
[ "$UPSTREAM" = "origin/main" ] || die "main must track origin/main before tagging"

CURRENT_VERSION="$(current_version)"
is_stable_semver "$CURRENT_VERSION" || die "current version must be stable X.Y.Z, got $CURRENT_VERSION"

if [ -n "$REQUEST" ]; then
  is_stable_semver "$REQUEST" || die "requested version must be stable X.Y.Z, got $REQUEST"
  [ "$REQUEST" = "$CURRENT_VERSION" ] || die "requested version $REQUEST does not match Cargo.toml version $CURRENT_VERSION"
fi

RELEASE_VERSION="$CURRENT_VERSION"
RELEASE_TAG="v$RELEASE_VERSION"

if git show-ref --verify --quiet "refs/tags/$RELEASE_TAG"; then
  die "local tag already exists: $RELEASE_TAG"
fi
if git ls-remote --exit-code --tags origin "refs/tags/$RELEASE_TAG" >/dev/null 2>&1; then
  die "remote tag already exists: $RELEASE_TAG"
fi

confirm

git tag -a "$RELEASE_TAG" -m "Release $RELEASE_TAG" "$LOCAL_HEAD"
git push origin "$RELEASE_TAG"

echo "Pushed $RELEASE_TAG."

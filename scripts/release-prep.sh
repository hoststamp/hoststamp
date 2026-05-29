#!/bin/sh
# SPDX-License-Identifier: FSL-1.1-ALv2

set -eu

usage() {
  cat <<'USAGE'
Usage: mise run release-prep [patch|minor|major|vX.Y.Z|X.Y.Z]

Defaults to patch. Creates a release prep branch, updates the workspace
version, runs focused checks, commits the version bump, pushes the branch, and
opens a pull request.
USAGE
}

die() {
  echo "release-prep: $*" >&2
  exit 1
}

current_version() {
  # Keep this in sync with scripts/release-publish.sh.
  awk -F'"' '/^version = / { print $2; exit }' Cargo.toml
}

is_stable_semver() {
  # Keep this in sync with scripts/release-publish.sh.
  echo "$1" | grep -Eq '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'
}

greater_than() {
  old="$1"
  new="$2"
  awk -v old="$old" -v new="$new" '
    BEGIN {
      split(old, o, ".")
      split(new, n, ".")
      for (i = 1; i <= 3; i++) {
        if ((n[i] + 0) > (o[i] + 0)) exit 0
        if ((n[i] + 0) < (o[i] + 0)) exit 1
      }
      exit 1
    }
  '
}

bump_version() {
  version="$1"
  level="$2"
  IFS=. read -r major minor patch <<EOF
$version
EOF

  case "$level" in
    patch)
      patch=$((patch + 1))
      ;;
    minor)
      minor=$((minor + 1))
      patch=0
      ;;
    major)
      major=$((major + 1))
      minor=0
      patch=0
      ;;
    *)
      die "unsupported bump level: $level"
      ;;
  esac

  printf '%s.%s.%s\n' "$major" "$minor" "$patch"
}

confirm() {
  new_version="$1"
  bump_label="$2"

  cat <<EOF
Current version: $CURRENT_VERSION
Requested release: $bump_label
New version: $new_version
Branch: release/v$new_version

Ready to create release prep PR for v$new_version? [y/N]
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

REQUEST="${1:-patch}"
case "$REQUEST" in
  patch|minor|major)
    ;;
  v[0-9]*.[0-9]*.[0-9]*)
    REQUEST="${REQUEST#v}"
    ;;
  [0-9]*.[0-9]*.[0-9]*)
    ;;
  *)
    usage >&2
    die "expected patch, minor, major, vX.Y.Z, or X.Y.Z"
    ;;
esac

command -v cargo >/dev/null 2>&1 || die "cargo is required"
command -v gh >/dev/null 2>&1 || die "GitHub CLI is required"
gh auth status >/dev/null 2>&1 || die "gh is not authenticated; run 'gh auth login'"
cargo set-version --version >/dev/null 2>&1 || die "cargo-edit is required; run mise install --locked"

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

[ -z "$(git status --porcelain)" ] || die "worktree must be clean"

BRANCH="$(git branch --show-current)"
[ "$BRANCH" = "main" ] || die "release prep must start from main, currently on $BRANCH"

git fetch origin main >/dev/null
LOCAL_HEAD="$(git rev-parse main)"
REMOTE_HEAD="$(git rev-parse origin/main)"
[ "$LOCAL_HEAD" = "$REMOTE_HEAD" ] || die "main must match origin/main before release prep"

CURRENT_VERSION="$(current_version)"
is_stable_semver "$CURRENT_VERSION" || die "current version must be stable X.Y.Z, got $CURRENT_VERSION"

if [ "$REQUEST" = "patch" ] || [ "$REQUEST" = "minor" ] || [ "$REQUEST" = "major" ]; then
  NEW_VERSION="$(bump_version "$CURRENT_VERSION" "$REQUEST")"
  BUMP_LABEL="$REQUEST"
else
  NEW_VERSION="$REQUEST"
  is_stable_semver "$NEW_VERSION" || die "target version must be stable X.Y.Z, got $NEW_VERSION"
  greater_than "$CURRENT_VERSION" "$NEW_VERSION" || die "target version $NEW_VERSION must be greater than $CURRENT_VERSION"
  BUMP_LABEL="explicit"
fi

RELEASE_BRANCH="release/v$NEW_VERSION"
if git show-ref --verify --quiet "refs/heads/$RELEASE_BRANCH"; then
  die "local branch already exists: $RELEASE_BRANCH"
fi
if git ls-remote --exit-code --heads origin "$RELEASE_BRANCH" >/dev/null 2>&1; then
  die "remote branch already exists: $RELEASE_BRANCH"
fi

confirm "$NEW_VERSION" "$BUMP_LABEL"

git switch -c "$RELEASE_BRANCH"
cargo set-version --workspace "$NEW_VERSION"

# Refresh Cargo.lock package versions if the workspace version changed entries.
cargo metadata --format-version 1 --no-deps >/dev/null
cargo metadata --locked --format-version 1 --no-deps >/dev/null
cargo fmt --all -- --check
cargo test --all-targets

git add Cargo.toml Cargo.lock
git diff --staged --quiet && die "version update produced no staged changes"
git commit -m "build: prepare v$NEW_VERSION release"
git push -u origin "$RELEASE_BRANCH"

gh pr create \
  --base main \
  --head "$RELEASE_BRANCH" \
  --title "build: prepare v$NEW_VERSION release" \
  --body "## Summary
- bump workspace version to $NEW_VERSION for release prep

## Verification
- cargo metadata --locked --format-version 1 --no-deps
- cargo fmt --all -- --check
- cargo test --all-targets"

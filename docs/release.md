# Release Process

Hoststamp releases are intentionally review-first. Version bumps happen in a
normal pull request, while a manually dispatched GitHub Actions workflow creates
the stable release tag and publishes artifacts from reviewed `main`.

## Release Prep PR

Create a release prep pull request with:

```sh
mise run release-prep
mise run release-prep patch
mise run release-prep minor
mise run release-prep major
mise run release-prep v1.1.1
```

When no argument is provided, `release-prep` defaults to `patch`. The script
prints the current version, requested bump, computed release version, and target
branch, then asks for confirmation before it creates a branch or edits files.

Before prompting, it:

- requires a clean `main` branch synced with `origin/main`
- requires the current workspace version to be a stable `X.Y.Z` version
- accepts `patch`, `minor`, `major`, `vX.Y.Z`, or `X.Y.Z`
- rejects explicit target versions that are not greater than the current
  version
- rejects release branches that already exist locally or on `origin`

After confirmation, it:

- creates `release/vX.Y.Z` from `main`
- updates the workspace version with `cargo set-version --workspace X.Y.Z`
- validates unlocked and locked Cargo metadata
- runs `cargo fmt --all -- --check`
- runs `cargo test --all-targets`
- stages `Cargo.toml` and `Cargo.lock`
- commits `build: prepare vX.Y.Z release`
- pushes the branch to `origin`
- opens a pull request with `gh pr create`

The script intentionally stops before publishing. Stable releases are published
only after the release prep PR has been reviewed and merged.

In short, `release-prep`:

- changes files: yes, after confirmation
- creates a branch: yes, after confirmation
- commits: yes, after checks pass
- pushes: yes, after commit
- tags: no
- opens a PR: yes

Review and merge the release prep PR like any other change. GitHub Actions do
not commit release prep changes back to the repository.

## Stable Release

After the release prep PR is merged, publish the stable release with:

```sh
mise run release-publish
mise run release-publish vX.Y.Z
mise run release-publish --dry-run vX.Y.Z
```

When no argument is provided, `release-publish` uses the workspace version in
`Cargo.toml`. The script requires a clean `main` branch synced with
`origin/main`, verifies that the local and remote tag do not already exist, and
asks for confirmation before dispatching the manual `Release` workflow.

Before prompting, it:

- requires a clean worktree
- requires the current branch to be `main`
- requires `main` to track `origin/main`
- fetches `origin/main` and tags
- requires local `main` and `origin/main` to point at the same commit
- requires the workspace version to be a stable `X.Y.Z` version
- rejects a provided `vX.Y.Z` or `X.Y.Z` argument if it does not match
  `Cargo.toml`
- accepts existing local or remote tags only when they already point at the
  current `main` commit, which allows re-dispatch after partial publish failures
- rejects existing tags that point anywhere else
- requires the `release.yml` workflow to be available on GitHub

After confirmation, it:

- dispatches the manual `Release` workflow on `main`
- passes the stable version and dry-run setting to the workflow

In short, `release-publish`:

- changes files: no
- creates a branch: no
- commits: no
- pushes: no
- tags: no, the remote workflow does that after checks pass
- opens a PR: no

The `Release` workflow is manual-only. It verifies that it is running from
`main`, checks that `vX.Y.Z` matches the workspace package version `X.Y.Z`,
runs the standard CI gate, verifies that the workflow did not modify source
files, verifies release Git credentials with a non-mutating dry-run tag push,
builds and smoke-tests the arm64 image under QEMU, publishes the
multi-architecture GHCR image, creates or reuses annotated Git tag `vX.Y.Z`,
and creates or updates the GitHub Release.

Dry runs run the release checks, the release Git credential preflight, the arm64
smoke test, and the multi-architecture image build. They do not publish image
tags, create Git tags, or create GitHub Releases.

The workflow publishes Docker image tags:

- `vX.Y.Z`
- `vX.Y`
- `vX` for 1.0 and later

Only `vX.Y.Z` is a Git tag. The moving `vX.Y` and `vX` names are Docker image
tags only, so they do not trigger duplicate Git tag workflows. While Hoststamp
is pre-1.0, the release workflow suppresses the `v0` Docker image tag because
0.x minor releases may contain breaking changes. The exact `v0.Y.Z` tag and
minor-line `v0.Y` tag are still published.

Release publishing must not rewrite source files, commit version changes, or
push branches. The workflow needs `contents: write` to create the release tag
and GitHub Release, plus `packages: write` to publish to GHCR. Create a
`stable-release` environment in GitHub repository settings. Add required
reviewers to that environment if stable publishing should require manual
approval before the job runs.

The first supported stable release artifact is the multi-architecture GHCR
image. Native binary packaging, SBOMs, provenance, and checksum artifacts are
separate release-scope decisions.

The release gate intentionally re-runs advisory and filesystem security scans
against the current RustSec, gitleaks, and Trivy data. A byte-identical commit
can fail release checks if a new advisory or finding appears after merge. Treat
that as a current-release blocker: fix or explicitly accept the finding on
`main`, then re-dispatch the release workflow.

If a release fails after the image and Git tag are published but before the
GitHub Release is created, re-dispatch before merging anything new to `main`. If
`main` has already advanced, delete the published tag and orphaned image tags,
then cut the next version.

## Nightly Release

The `publish-nightly` workflow runs on pushes to `main` and can also be started
manually. It publishes multi-architecture nightly images to GHCR tagged as:

- `nightly`
- `sha-<short>`
- `vX.Y.Z-nightly.YYYYMMDD.N`

Before building the image, the workflow computes a nightly version from the
workspace version in `Cargo.toml`, increments the patch component, and runs:

```sh
cargo set-version --workspace "$NIGHTLY_VERSION"
```

That version edit is only in the workflow checkout. It is not committed back to
the repository, but the built image reports the nightly version through the
normal Cargo package metadata used by the CLI.

Nightly publishing is separate from stable publishing: nightly images answer
"what is the current reviewed state of `main`?", while stable release tags
answer "what did we intentionally release?".

## Release Checks

Before publishing a stable release, run the standard CI gate:

```sh
MISE_TRUSTED_CONFIG_PATHS=$PWD mise run ci
```

For Docker-specific release confidence, also run:

```sh
MISE_TRUSTED_CONFIG_PATHS=$PWD mise run docker-smoke
```

Use `MISE_TRUSTED_CONFIG_PATHS=$PWD` if the local shell refuses to load the
project `mise.toml` because it has not been globally trusted.

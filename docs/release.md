# Release Process

Hoststamp releases are intentionally review-first. Version bumps happen in a
normal pull request, while GitHub Actions publish artifacts from reviewed
commits and tags.

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

The script intentionally stops before tagging. Stable tags are created only
after the release prep PR has been reviewed and merged.

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

After the release prep PR is merged, create the stable release tag with:

```sh
mise run release-tag
mise run release-tag vX.Y.Z
```

When no argument is provided, `release-tag` uses the workspace version in
`Cargo.toml`. The script requires a clean `main` branch synced with
`origin/main`, verifies that the local and remote tag do not already exist, and
asks for confirmation before creating and pushing `vX.Y.Z`.

Before prompting, it:

- requires a clean worktree
- requires the current branch to be `main`
- requires `main` to track `origin/main`
- fetches `origin/main` and tags
- requires local `main` and `origin/main` to point at the same commit
- requires the workspace version to be a stable `X.Y.Z` version
- rejects a provided `vX.Y.Z` or `X.Y.Z` argument if it does not match
  `Cargo.toml`
- rejects tags that already exist locally or on `origin`

After confirmation, it:

- creates annotated tag `vX.Y.Z` at the current `main` commit
- pushes `vX.Y.Z` to `origin`

In short, `release-tag`:

- changes files: no
- creates a branch: no
- commits: no
- pushes: yes, the tag only
- tags: yes, after confirmation
- opens a PR: no

A future stable release workflow will publish from the pushed tag and must
verify that `vX.Y.Z` matches the workspace package version `X.Y.Z`. Release
publishing must not rewrite source files, commit version changes, or push
branches.

The first supported stable release artifact is the multi-architecture GHCR
image. Native binary packaging, SBOMs, provenance, and checksum artifacts are
separate release-scope decisions.

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

Before tagging a stable release, run the standard CI gate:

```sh
MISE_TRUSTED_CONFIG_PATHS=$PWD mise run ci
```

For Docker-specific release confidence, also run:

```sh
MISE_TRUSTED_CONFIG_PATHS=$PWD mise run docker-smoke
```

Use `MISE_TRUSTED_CONFIG_PATHS=$PWD` if the local shell refuses to load the
project `mise.toml` because it has not been globally trusted.

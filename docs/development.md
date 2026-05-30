# Development

## Project Commands

```sh
cargo check --all-targets
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo llvm-cov --all-targets --locked --summary-only --fail-under-lines 90
cargo build --release --locked
docker build -t hoststamp:dev .
mise run docker-smoke
```

The same local checks are available through `mise`:

```sh
mise install --locked
mise run check
mise run ci
```

Run individual `mise` tasks when you only need one check. Tool versions are
pinned in `mise.toml` and locked in `mise.lock`.

## Local Dev Loop

Use the watched dev server when working on the API or local UX:

```sh
mise run dev
```

The task runs `cargo run -p hoststamp -- serve` under `watchexec` and restarts
the server whenever Rust source or Cargo metadata changes. The wrapper in
`scripts/dev-env.sh` uses the throwaway SQLite database at
`target/dev/hoststamp.db`, creates local admin and token-hash key files under
`target/dev/` when they are missing, and sets
`HOSTSTAMP_UX_STATIC_DIR=crates/hoststamp-ux/static` so the debug server reads
the admin HTML, CSS, and JavaScript from disk on each request. Edits to those
files only need a browser refresh. The admin bearer token for the browser
prompt is stored at `target/dev/admin-token`.

Two narrower server loops are also available:

```sh
mise run dev-api
mise run dev-ux
```

Stop the dev server before resetting its local state:

```sh
mise run dev-reset
```

For a fast compile-only feedback loop, run:

```sh
mise run check
mise run watch-check
```

Production builds still embed the admin shell from
`crates/hoststamp-ux/static/` and keep a strict content security policy. The
disk-served path is debug-only, so the release binary does not depend on loose
asset files.

## Crate Layout

The workspace is split so applications can reuse Hoststamp without running it
as a microservice:

| Crate | Purpose |
| --- | --- |
| `hoststamp-core` | reusable generator, dictionary, profile, storage, config, auth, and notices code |
| `hoststamp-api` | Axum API server routes and serving helpers |
| `hoststamp-ux` | local UX shell assets and route handler |
| `hoststamp` | CLI binary that composes the core, API, and UX crates |

## Automation

CI validates formatting, clippy, tests with coverage, release builds,
third-party notice drift, workflow syntax, dependency advisories, secret leaks,
filesystem vulnerability/misconfiguration scans, and the Docker image. Pull
requests run a fast amd64 Docker smoke build and start the image with hardened
runtime flags. Cargo audit and Dependabot also run weekly. Release preparation,
stable tags, and nightly images are documented in
[Release](./release.md).

## Commit Messages

Use Conventional Commit-style subjects:

```text
<type>: <imperative summary>
```

Common prefixes:

- `docs`: documentation and repo guidance
- `feat`: user-facing features
- `fix`: bug fixes
- `ci`: CI and release automation
- `build`: build system, packaging, and dependency tooling
- `deps`: dependency updates
- `docker`: Docker image and base image updates
- `test`: tests and test infrastructure
- `refactor`: behavior-preserving code changes

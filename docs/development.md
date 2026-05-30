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
the server whenever `Cargo.toml`, `Cargo.lock`, or files under `crates/` change.
Open `http://127.0.0.1:8080/` and refresh the browser after each restart.

Two narrower server loops are also available:

```sh
mise run dev-api
mise run dev-ux
```

For a fast compile-only feedback loop, run:

```sh
mise run check
mise run watch-check
```

The admin shell is currently embedded into the binary from
`crates/hoststamp-ux/static/index.html`, so HTML, CSS, and JavaScript edits
still rebuild the `hoststamp-ux` crate. If that becomes too slow, the next
workflow improvement should be a dev-only file-serving path for local UX assets.

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

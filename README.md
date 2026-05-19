Hoststamp
---

Hoststamp is a Rust CLI, API server, and local UX.

## Development

### Run locally

```sh
cargo run -p hoststamp -- serve
```

The server binds to `127.0.0.1:8080` by default. Set `HOSTSTAMP_ADDR`
or pass `--addr` to choose another bind address.

```sh
cargo run -p hoststamp -- serve --addr 0.0.0.0:8080
cargo run -p hoststamp -- health
cargo run -p hoststamp -- --version
cargo run -p hoststamp -- --credits
cargo run -p hoststamp -- generate
```

Generate hostnames:

```sh
cargo run -p hoststamp -- generate
cargo run -p hoststamp -- generate --words 3 --dictionary eff_short --count 10
cargo run -p hoststamp -- generate --dictionary eff_short_2
cargo run -p hoststamp -- generate --word-length 6
cargo run -p hoststamp -- generate --suffix-len 10
cargo run -p hoststamp -- generate --no-suffix-hash
```

Generated hostnames use the shape `word-word-hash` by default. The default
output uses two 5-character words and a 5-character suffix hash, so the default
shape is `5/5/5`. With the default `eff_short` dictionary, 5-character words
provide the largest filtered two-word combination pool available from that
dictionary: 631 available words and 397,530 ordered two-word names without
repeats. Words never repeat within a single hostname. `--dictionary` accepts
`eff_short`, `eff_short_2`, or `eff_large`, and defaults to `eff_short`;
`--count` is capped at 50. `--word-length` filters words to an exact character
length. If no matching words are available, generation fails.

Dictionary stats after Hoststamp's curated server-name filter:

| Dictionary | Available entries | Word lengths |
| --- | ---: | --- |
| `eff_short` | 1,128 | 3-5 |
| `eff_short_2` | 1,256 | 3-10 |
| `eff_large` | 7,450 | 3-9 |

The bundled EFF large list is EFF's 2016 five-dice long wordlist. EFF describes
that list as using words between 3 and 9 characters, so `--dictionary eff_large
--word-length 10` correctly has no matches. `eff_short` maps to EFF Short
Wordlist #1, a separate four-dice list featuring only short words. EFF Short
Wordlist #2 is available as `eff_short_2` and includes 10-character words.
Hoststamp keeps the EFF source files unchanged, then removes a curated set of
words that make poor server names.

Exact-length availability:

| Length | `eff_short` words | `eff_short` 2-word names | `eff_short_2` words | `eff_short_2` 2-word names | `eff_large` words | `eff_large` 2-word names |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 3 | 81 | 6,480 | 6 | 30 | 81 | 6,480 |
| 4 | 416 | 172,640 | 46 | 2,070 | 452 | 203,852 |
| 5 | 631 | 397,530 | 137 | 18,632 | 793 | 628,056 |
| 6 | 0 | 0 | 220 | 48,180 | 1,334 | 1,778,222 |
| 7 | 0 | 0 | 243 | 58,806 | 1,546 | 2,388,570 |
| 8 | 0 | 0 | 268 | 71,556 | 1,731 | 2,994,630 |
| 9 | 0 | 0 | 217 | 46,872 | 1,513 | 2,287,656 |
| 10 | 0 | 0 | 119 | 14,042 | 0 | 0 |

The 2-word counts assume order matters and words do not repeat. The hash
suffix adds additional uniqueness beyond these word-only counts.

Local endpoints:

- UX: `http://127.0.0.1:8080/`
- API health: `http://127.0.0.1:8080/api/health`
- Container health: `http://127.0.0.1:8080/healthz`

### Configuration

Hoststamp reads configuration from the first available source:

1. `--config <path>`
2. `HOSTSTAMP_CONFIG=<path>`
3. `$XDG_CONFIG_HOME/hoststamp/config.toml`
4. `~/.config/hoststamp/config.toml`
5. Built-in defaults

CLI flags override environment variables, environment variables override
the config file, and the config file overrides built-in defaults.

```toml
[server]
addr = "127.0.0.1:8080"
```

For containers, mount a config file and set `HOSTSTAMP_CONFIG`:

```sh
docker run --rm -p 8080:8080 \
  -e HOSTSTAMP_CONFIG=/etc/hoststamp/config.toml \
  -v "$PWD/config.example.toml:/etc/hoststamp/config.toml:ro" \
  hoststamp:dev
```

### Project commands

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo build --release --locked
docker build -t hoststamp:dev .
```

### Docker

```sh
docker build -t hoststamp:dev .
docker run --rm -p 8080:8080 hoststamp:dev
```

### Automation

CI validates formatting, clippy, tests with coverage, release builds,
workflow syntax, and the Docker image. Pushes to `main` publish multi-arch
nightly images to GHCR tagged as `nightly` and `sha-<short>`. Cargo audit
and Dependabot run weekly.

## License

Hoststamp source is licensed under the Functional Source License 1.1,
ALv2 Future License (`FSL-1.1-ALv2`). See [LICENSE](./LICENSE).

Third-party notices for bundled datasets are in
[THIRD-PARTY-NOTICES.md](./THIRD-PARTY-NOTICES.md) and are also available
from the CLI:

```sh
cargo run -p hoststamp -- --credits
```

### Commit messages

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

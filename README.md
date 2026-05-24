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
cargo run -p hoststamp -- --list-categories
cargo run -p hoststamp -- generate
cargo run -p hoststamp -- config show
cargo run -p hoststamp -- --profile staging generate
```

Generate hostnames:

```sh
cargo run -p hoststamp -- generate
cargo run -p hoststamp -- generate --count 10
cargo run -p hoststamp -- generate --word1-categories adjective --word2-categories animal
cargo run -p hoststamp -- generate --word1-categories adjective,noun --word2-categories animal,name
cargo run -p hoststamp -- generate --word1-lengths 4,5,6 --word2-lengths 4,5,6
cargo run -p hoststamp -- generate --word1-lengths any --word2-lengths any
cargo run -p hoststamp -- generate --suffix-min-length 10
cargo run -p hoststamp -- generate --no-suffix
cargo run -p hoststamp -- generate --no-word2 --no-suffix
cargo run -p hoststamp -- --profile team-a generate
cargo run -p hoststamp -- --capacity --word1-lengths 5 --word2-lengths 5
```

Hostnames are assembled from three positions: `word1`, `word2`, and `suffix`.
The selected profile supplies the default generator settings. The built-in
profile seed is `word1-word2-suffix` (e.g. `5/5/5`) with `adjective,adverb`
for `word1` and all non-`adjective`, non-`adverb`, non-`diceware` categories
for `word2`. Each word position has independent
disable, lengths, and categories controls (`--no-word1`, `--word1-lengths`,
`--word1-categories`, and the matching `word2` flags). The suffix has
`--no-suffix` and `--suffix-min-length`. Words never repeat within a single
hostname. `--count` is capped at 50.

`--wordN-categories` accepts a comma-separated category list. `--wordN-lengths`
accepts a comma-separated list of exact lengths or the literal `any` for no
length filter. Selection across selected categories and length buckets is
weighted by available word count so every candidate word has an even chance.
If the selected categories do not contain enough matching words, generation
fails loudly.

Use `--capacity` with any generator option set to report the available name
space without generating or modifying a profile. The report includes the
candidate count for each word position, overlap removed by the no-repeat rule,
unique word combinations, suffix variants, suffix bits, and total variants.
Suffixes are Sqids-encoded lowercase base36 (`0-9a-z`) values with a pinned
Sqids blocklist. `--suffix-min-length` is bounded to `[4, 13]` and is a minimum:
suffixes can grow longer as the encoded number passes the fixed-length space
for that minimum. The fixed-length suffix space is `36^suffix_min_length`; with
the default minimum length of `5`, that space is `60,466,176`.

With profile storage, Hoststamp increments the selected profile's database
counter and encodes that atomic value with Sqids. The profile UUID is used to
derive a deterministic profile-specific alphabet, so each profile gets a
different-looking sequence while keeping the uniqueness guarantee scoped to the
active profile row. Without profile storage, Hoststamp encodes a random number
from `1..=(36^suffix_min_length / 2)`. That fallback keeps the suffix inside
the requested minimum length range, but it is not uniqueness-tracked or
reproducible.

Sqids can expand past the configured minimum length. For example,
`--suffix-min-length 5` keeps profile-backed atomic values `1..=60,466,176`
within at least five suffix characters; larger atomic values may require six or
more suffix characters. Length `13` covers the full signed SQLite counter range
used by Hoststamp profile storage (`1..=9,223,372,036,854,775,807`).

| Suffix min length | Approx fixed-length atomic values* | Approx random fallback range* |
| ---: | ---: | ---: |
| 3 | ~1-46,656 | ~1-23,328 |
| 4 | ~1-1,679,616 | ~1-839,808 |
| 5 | ~1-60,466,176 | ~1-30,233,088 |
| 6 | ~1-2,176,782,336 | ~1-1,088,391,168 |
| 7 | ~1-78,364,164,096 | ~1-39,182,082,048 |
| 8 | ~1-2,821,109,907,456 | ~1-1,410,554,953,728 |
| 9 | ~1-101,559,956,668,416 | ~1-50,779,978,334,208 |
| 10 | ~1-3,656,158,440,062,976 | ~1-1,828,079,220,031,488 |
| 11 | ~1-131,621,703,842,267,136 | ~1-65,810,851,921,133,568 |
| 12 | ~1-4,738,381,338,321,616,896 | ~1-2,369,190,669,160,808,448 |
| 13 | ~1-9,223,372,036,854,775,807 | ~1-4,611,686,018,427,387,903 |

*Approximate base36 space before Sqids blocklist filtering. The pinned Sqids
blocklist can skip some encoded values, so expansion may happen a few values
earlier for a given profile alphabet. Length `3` is shown for planning math;
the CLI accepts suffix minimum lengths `4..=13`.

Category stats from the generated artifact:

| Category | Available entries | Word lengths |
| --- | ---: | --- |
| `adjective` | 584 | 3-12 |
| `adverb` | 257 | 4-10 |
| `animal` | 448 | 3-8 |
| `deity` | 151 | 3-11 |
| `diceware` | 8,026 | 3-10 |
| `element` | 117 | 3-12 |
| `gemstone` | 312 | 3-12 |
| `metal` | 91 | 3-12 |
| `monster` | 20 | 5-11 |
| `name` | 652 | 3-12 |
| `noun` | 95 | 3-10 |
| `ocean` | 5 | 6-8 |
| `phonetic` | 26 | 4-8 |
| `planet` | 13 | 4-8 |
| `river` | 187 | 3-12 |
| `scientist` | 241 | 4-12 |
| `star` | 435 | 3-12 |
| `stone` | 48 | 4-12 |
| `tolkien` | 398 | 3-11 |
| `wind` | 90 | 3-12 |

Run `hoststamp --list-categories` for the category names and total counts
compiled into the binary.

Local endpoints:

- UX: `http://127.0.0.1:8080/`
- API health: `http://127.0.0.1:8080/api/health`
- API generate: `http://127.0.0.1:8080/api/generate?count=3`
- Container health: `http://127.0.0.1:8080/healthz`

`/api/generate` returns JSON with a `hostnames` array. Query parameters mirror
the generator option names, including `count`, `word1_lengths`,
`word1_categories`, `word2_lengths`, `word2_categories`, `suffix_enabled`,
and `suffix_min_length`.

### Configuration

Hoststamp reads bootstrap configuration from the first available source:

1. `--config <path>`
2. `HOSTSTAMP_CONFIG=<path>`
3. `$XDG_CONFIG_HOME/hoststamp/config.toml`
4. `~/.config/hoststamp/config.toml`
5. Built-in defaults

The bootstrap config handles server and storage settings. Generator profile
defaults are stored in the profile database. The default database path is
`$XDG_CONFIG_HOME/hoststamp/hoststamp.db`, falling back to
`~/.config/hoststamp/hoststamp.db`; it sits next to the default config file.

```toml
[server]
addr = "127.0.0.1:8080"

[storage]
# url = "sqlite:///home/hoststamp/.config/hoststamp/hoststamp.db"
```

Environment variables (all `HOSTSTAMP_*`):

| Env var | Maps to |
| --- | --- |
| `HOSTSTAMP_CONFIG` | path to the config file |
| `HOSTSTAMP_ADDR` | `server.addr` |
| `HOSTSTAMP_DATABASE_URL` | `storage.url` |

Profiles are selected with `--profile <slug>`. The default profile slug is `_`,
which is reserved and cannot be used as a normal user slug. User slugs use
lowercase ASCII letters, digits, and hyphens, and must start and end with a
letter or digit. Missing profiles are seeded from the built-in `5/5/5`
generator defaults on first use.

Use `hoststamp config show` to print the resolved bootstrap settings, selected
profile metadata, stored profile config, and effective generator config after
CLI request options are applied. Database URLs that could contain secrets are
redacted.

Profile-backed suffix generation treats the selected profile config as part of
the identity used for deterministic suffixes. If suffixes are enabled and CLI
options differ from the stored profile config, Hoststamp asks for two
confirmations before replacing the active profile row. Replacement creates a
new profile UUID and resets that profile's atomic counter. `--count` is a
request option only and does not trigger profile replacement. API requests
cannot provide interactive confirmation, so profile config overrides are
rejected; use the CLI to confirm a profile replacement first.

SQLite storage is implemented for local profiles. `postgres://` and
`postgresql://` URLs are recognized as planned remote storage backends, but
Postgres execution is not implemented yet.

For containers, mount a config file and set `HOSTSTAMP_CONFIG`:

```sh
docker run --rm -p 8080:8080 \
  -e HOSTSTAMP_CONFIG=/etc/hoststamp/config.toml \
  -e HOSTSTAMP_DATABASE_URL=sqlite:///home/hoststamp/.config/hoststamp/hoststamp.db \
  -v hoststamp-data:/home/hoststamp/.config/hoststamp \
  -v "$PWD/config.example.toml:/etc/hoststamp/config.toml:ro" \
  hoststamp:dev
```

### Project commands

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo llvm-cov --all-targets --locked --summary-only --fail-under-lines 90
cargo build --release --locked
docker build -t hoststamp:dev .
```

The same local checks are available through `mise`:

```sh
mise install --locked
mise run ci
```

Run individual `mise` tasks when you only need one check. Tool versions are
pinned in `mise.toml` and locked in `mise.lock`.

### Docker

```sh
docker build -t hoststamp:dev .
docker run --rm -p 8080:8080 hoststamp:dev
```

### Automation

CI validates formatting, clippy, tests with coverage, release builds,
third-party notice drift, workflow syntax, dependency advisories, secret
leaks, filesystem vulnerability/misconfiguration scans, and the Docker image.
Pull requests run a fast amd64 Docker smoke build. Pushes to `main` publish
multi-arch nightly images to GHCR tagged as `nightly`, `sha-<short>`, and
`vX.Y.Z-nightly.YYYYMMDD.N`. Cargo audit and Dependabot also run weekly.

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

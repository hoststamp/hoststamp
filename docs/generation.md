# Generation

Hoststamp assembles hostnames from three positions: `word1`, `word2`, and
`suffix`.

## Commands

```sh
cargo run -p hoststamp -- generate
cargo run -p hoststamp -- generate --count 10
cargo run -p hoststamp -- generate --count 10 --json
cargo run -p hoststamp -- --capacity --json
cargo run -p hoststamp -- random
cargo run -p hoststamp -- random --count 10
cargo run -p hoststamp -- random --word1-lengths 4 --word2-lengths 4
cargo run -p hoststamp -- random --word1-categories adjective --word2-categories animal
cargo run -p hoststamp -- random --suffix-min-length 8
cargo run -p hoststamp -- random --json
cargo run -p hoststamp -- --profile team-a generate
cargo run -p hoststamp -- --profile team-a lookup brief-cobra-db50d
cargo run -p hoststamp -- --profile team-a lookup brief-cobra-db50d --json
cargo run -p hoststamp -- --profile team-a regenerate --atomic-value 42
cargo run -p hoststamp -- --profile team-a regenerate --atomic-value 42 --count 3 --json
cargo run -p hoststamp -- regenerate --profile-id <uuid> --atomic-value 42
cargo run -p hoststamp -- --profile team-a --capacity
cargo run -p hoststamp -- --profile team-a --capacity --json
```

`generate` uses the selected profile's stored generator settings and atomic
counter. `random` is stateless: it never opens or mutates the profile database,
and it starts from the built-in `5/5/5` defaults unless ad hoc generation
options are passed on the command line.

The built-in profile seed is `word1-word2-suffix` with `adjective,adverb` for
`word1` and all non-`adjective`, non-`adverb`, non-`diceware` categories for
`word2`.

Each word position has independent enable, lengths, and categories controls
stored on the selected profile with `hoststamp config set`:

- `--word1-enabled`
- `--word1-lengths`
- `--word1-categories`
- `--word2-enabled`
- `--word2-lengths`
- `--word2-categories`

The same generation controls can be passed to `hoststamp random` without
changing a profile. The suffix has `--suffix-enabled` and
`--suffix-min-length`. Words never repeat within a single hostname. `--count`
is a request option and is capped at 50.

`config set --wordN-categories` accepts a comma-separated category list.
`config set --wordN-lengths` accepts a comma-separated list of exact lengths or
the literal `any` for no length filter. Selection across selected categories
and length buckets is weighted by available word count so every candidate word
has an even chance. If the selected categories do not contain enough matching
words, configuration fails loudly before it is stored.

## Capacity

Use `--capacity` to report the available name space for the selected profile
without generating or modifying that profile. The report includes the candidate
count for each word position, overlap removed by the no-repeat rule, unique
word combinations, suffix variants, suffix bits, and total variants.

Suffixes are Sqids-encoded lowercase base36 (`0-9a-z`) values with a pinned
Sqids blocklist. `config set --suffix-min-length` is bounded to `[3, 13]` and
is a minimum: suffixes can grow longer as the encoded number passes the
fixed-length space for that minimum. The fixed-length suffix space is
`36^suffix_min_length`; with the default minimum length of `5`, that space is
`60,466,176`.

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
earlier for a given profile alphabet.

## Deterministic Profiles

With profile storage, Hoststamp increments the selected profile's database
counter and derives the full hostname from the profile UUID, profile config
hash, and atomic value. Stored profile configs include `engine = "atomic-v1"`.

That engine freezes the deterministic generation contract: word-pair
permutation, no-repeat word handling, suffix encoding, profile-specific suffix
alphabet derivation, and `word1-word2-suffix` formatting. Word choices walk a
deterministic permutation of the valid word space, so each valid word pair is
used once before that profile cycle repeats. The suffix encodes the same atomic
value with Sqids.

The profile UUID also derives a deterministic profile-specific suffix
alphabet, so each profile gets a different-looking sequence while keeping the
uniqueness guarantee scoped to the active profile row. Future algorithm changes
must use a new engine value instead of changing `atomic-v1`.

For stateless random generation, Hoststamp encodes a random number from
`1..=(36^suffix_min_length / 2)`. That fallback keeps the suffix inside the
requested minimum length range, but it is not uniqueness-tracked or
reproducible.

Sqids can expand past the configured minimum length. For example,
`--suffix-min-length 5` keeps profile-backed atomic values `1..=60,466,176`
within at least five suffix characters; larger atomic values may require six or
more suffix characters. Length `13` covers the full signed SQLite counter range
used by Hoststamp profile storage (`1..=9,223,372,036,854,775,807`).

## Regeneration

Use `hoststamp regenerate --atomic-value <n>` to reproduce the hostname for a
stored profile atomic value. Regeneration uses only the selected profile
(`--profile`, default `_`) and an atomic range; it does not increment the
counter. Use `--profile-id <uuid>` to regenerate from a replaced or deleted
profile row listed by `hoststamp --profile <slug> profile history`.

Pass `--count <n>` to regenerate a contiguous range starting at
`--atomic-value`. Plain output is one hostname per line, and `--json` returns
each hostname with `profile` and `atomic_value` metadata. The requested atomic
range must already have been issued by the selected profile row. For example, a
profile with `last_atomic_value = 10` rejects
`--atomic-value 10 --count 2` because that includes value `11`.

Regeneration requires suffixes to be enabled for the stored profile because
atomic values are tracked only for profile-backed suffix generation. Stored
profiles include the generation engine, selected dictionary and blocklist
versions, those version hashes, and resolved word-pool hashes. Hoststamp will
not regenerate if the engine, selected version content, or resolved pools drift
from what this binary supports.

## Lookup

Use `hoststamp lookup <hostname>` to validate a profile-backed hostname against
the selected profile (`--profile`, default `_`). Lookup decodes the Sqids suffix
to an atomic value, regenerates that profile hostname, and returns `valid =
true` only when the hostname matches and the atomic value has already been
issued by the profile.

Plain output reports `valid`, `profile`, and `atomic_value`. `--json` returns
the same fields as JSON. Tampered names return `valid = false`; when the suffix
can still be decoded, `atomic_value` is included to help diagnose which issued
value was altered. Lookup requires suffixes and the current deterministic
generation contract for the stored profile. It only applies to profile-backed
atomic hostnames; stateless random hostnames and profiles with suffixes
disabled cannot be reverse-looked-up.

For CI and bulk checks, use `hoststamp validate <hostname>` or
`hoststamp validate --file <path>`. File input is newline-delimited, blank lines
are ignored, and any invalid hostname makes the command exit non-zero. Pass
`--json` to print a `results` array with `hostname`, `profile`, `atomic_value`,
and `valid` fields. See [Integrations](./integrations.md) for a GitHub Actions
validation workflow example.

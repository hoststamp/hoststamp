# Dictionaries

Hoststamp embeds its generated dictionary artifact into the binary. Run
`hoststamp --list-categories` for the category names and total counts compiled
into the current build. Global inspection flags such as `--list-categories`
and `--credits` are handled before subcommands, similar to `--version`.

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
| `river` | 186 | 3-12 |
| `scientist` | 241 | 4-12 |
| `star` | 435 | 3-12 |
| `stone` | 48 | 4-12 |
| `tolkien` | 398 | 3-11 |
| `wind` | 90 | 3-12 |

Stored profiles include dictionary and blocklist version hashes plus resolved
word-pool hashes. If a newer Hoststamp binary changes unrelated dictionary
versions, old profiles can continue to run. If the selected dictionary version,
selected blocklist version, or resolved pools for the profile change,
profile-backed `generate`, `serve`, and `regenerate` fail closed so they do not
emit names that cannot later be regenerated under the recorded profile state.

Create a new profile, delete and recreate the existing profile, or use
`config set` to replace the active profile row with the current dictionary and
blocklist versions.

Third-party notices for bundled datasets are in
[THIRD-PARTY-NOTICES.md](../THIRD-PARTY-NOTICES.md) and are also available
from the CLI:

```sh
cargo run -p hoststamp -- --credits
```

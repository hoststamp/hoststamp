// SPDX-License-Identifier: FSL-1.1-ALv2

use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs, panic,
    path::PathBuf,
    process,
};

const ARTIFACT_ENV: &str = "HOSTSTAMP_ARTIFACT_PATH";
const DEFAULT_ARTIFACT_PATH: &str = "data/artifact.json";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    schema_version: u32,
    generated: String,
    generator: String,
    normalization: Normalization,
    default_dictionary_version: u32,
    default_blocklist_version: u32,
    words: Words,
    dictionary_versions: BTreeMap<String, DictionaryVersion>,
    blocklist_versions: BTreeMap<String, BlocklistVersion>,
    sources: BTreeMap<String, Source>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Normalization {
    ascii_fold: bool,
    charset: String,
    length_max: usize,
    length_min: usize,
    rfc1123_label: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Words {
    allowed: Vec<String>,
    blocked: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DictionaryVersion {
    label: String,
    sources: Vec<String>,
    categories: BTreeMap<String, Vec<u16>>,
    hash: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BlocklistVersion {
    label: String,
    sources: BTreeMap<String, Vec<u16>>,
    hash: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    title: String,
    url: String,
    license: String,
    license_url: String,
    attribution: String,
    retrieved: String,
    sha256: String,
    changes: String,
    notice_required: bool,
}

fn main() {
    if let Err(message) = try_main() {
        println!("cargo::error={message}");
        process::exit(1);
    }
}

fn try_main() -> Result<(), String> {
    println!("cargo::rerun-if-env-changed={ARTIFACT_ENV}");

    let manifest_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR")
            .map_err(|error| format!("failed to read CARGO_MANIFEST_DIR: {error}"))?,
    );
    let artifact_path = env::var_os(ARTIFACT_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest_dir.join(DEFAULT_ARTIFACT_PATH));
    println!("cargo::rerun-if-changed={}", artifact_path.display());

    let contents = fs::read_to_string(&artifact_path).map_err(|error| {
        format!(
            "failed to read dictionary artifact {}: {error}",
            artifact_path.display()
        )
    })?;
    let artifact_sha256 = hex_digest(Sha256::digest(contents.as_bytes()).as_slice());
    let artifact: Artifact = serde_json::from_str(&contents).map_err(|error| {
        format!(
            "failed to parse dictionary artifact {}: {error}",
            artifact_path.display()
        )
    })?;

    let blocked_words = catch_validation_error(|| validate_artifact(&artifact))?;

    let out_path = PathBuf::from(
        env::var("OUT_DIR").map_err(|error| format!("failed to read OUT_DIR: {error}"))?,
    )
    .join("dictionary_generated.rs");
    fs::write(
        &out_path,
        render(&artifact, &blocked_words, &artifact_sha256),
    )
    .map_err(|error| format!("failed to write {}: {error}", out_path.display()))?;

    Ok(())
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else if let Some(message) = payload.downcast_ref::<&'static str>() {
        (*message).to_owned()
    } else {
        "dictionary artifact validation failed".to_owned()
    }
}

fn catch_validation_error(
    function: impl FnOnce() -> Vec<String> + panic::UnwindSafe,
) -> Result<Vec<String>, String> {
    let hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let result = panic::catch_unwind(function).map_err(panic_message);
    panic::set_hook(hook);
    result
}

fn validate_artifact(artifact: &Artifact) -> Vec<String> {
    assert_eq!(
        artifact.schema_version, 1,
        "unsupported artifact schema_version"
    );
    assert!(!artifact.generated.is_empty(), "generated is required");
    assert!(!artifact.generator.is_empty(), "generator is required");
    assert!(
        artifact.normalization.ascii_fold,
        "normalization.ascii_fold must be true"
    );
    assert_eq!(
        artifact.normalization.charset, "^[a-z]+$",
        "normalization.charset must be ^[a-z]+$"
    );
    assert!(
        artifact.normalization.rfc1123_label,
        "normalization.rfc1123_label must be true"
    );
    assert!(
        artifact.normalization.length_min > 0,
        "normalization length_min must be positive"
    );
    assert!(
        artifact.normalization.length_min <= artifact.normalization.length_max,
        "normalization length_min must be <= length_max"
    );
    assert!(
        !artifact.words.allowed.is_empty(),
        "words.allowed must not be empty"
    );
    assert!(
        !artifact.words.blocked.is_empty(),
        "words.blocked must not be empty"
    );
    assert!(
        !artifact.dictionary_versions.is_empty(),
        "dictionary_versions must not be empty"
    );
    assert!(
        !artifact.blocklist_versions.is_empty(),
        "blocklist_versions must not be empty"
    );
    assert!(!artifact.sources.is_empty(), "sources must not be empty");
    assert!(
        artifact.words.allowed.len() <= usize::from(u16::MAX) + 1,
        "words.allowed exceeds u16 word id capacity"
    );

    validate_allowed_words(&artifact.words.allowed);
    let blocked_words = validate_blocked_words(&artifact.words.blocked);
    validate_sources(&artifact.sources);
    validate_dictionary_versions(artifact);
    validate_blocklist_versions(artifact, &blocked_words);

    assert!(
        artifact
            .dictionary_versions
            .contains_key(&artifact.default_dictionary_version.to_string()),
        "default_dictionary_version is missing from dictionary_versions"
    );
    assert!(
        artifact
            .blocklist_versions
            .contains_key(&artifact.default_blocklist_version.to_string()),
        "default_blocklist_version is missing from blocklist_versions"
    );

    blocked_words
}

fn validate_allowed_words(words: &[String]) {
    let mut previous: Option<&str> = None;
    for word in words {
        assert!(
            word.chars().all(|c| c.is_ascii_lowercase()),
            "words.allowed contains non-lowercase word {word}"
        );
        assert!(
            (3..=12).contains(&word.chars().count()),
            "words.allowed contains unsupported length word {word}"
        );
        if let Some(previous) = previous {
            assert!(
                previous < word.as_str(),
                "words.allowed must be sorted and unique"
            );
        }
        previous = Some(word);
    }
}

fn validate_blocked_words(encoded_words: &[String]) -> Vec<String> {
    let mut decoded_words = Vec::with_capacity(encoded_words.len());
    for encoded in encoded_words {
        let decoded = decode_base64url(encoded).unwrap_or_else(|error| {
            panic!("words.blocked contains invalid token {encoded}: {error}")
        });
        assert!(
            decoded
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
            "words.blocked token is not lowercase base36: {decoded}"
        );
        decoded_words.push(decoded);
    }
    assert!(
        decoded_words
            == decoded_words
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>(),
        "words.blocked must decode to sorted unique tokens"
    );
    decoded_words
}

fn validate_sources(sources: &BTreeMap<String, Source>) {
    for (source_id, source) in sources {
        assert!(!source_id.is_empty(), "source ids must not be empty");
        assert!(
            !source.title.is_empty(),
            "source {source_id} title is required"
        );
        assert!(!source.url.is_empty(), "source {source_id} url is required");
        assert!(
            !source.license.is_empty(),
            "source {source_id} license is required"
        );
        assert!(
            !source.license_url.is_empty(),
            "source {source_id} license_url is required"
        );
        assert!(
            !source.attribution.is_empty(),
            "source {source_id} attribution is required"
        );
        assert!(
            !source.retrieved.is_empty(),
            "source {source_id} retrieved is required"
        );
        assert!(
            is_sha256(&source.sha256),
            "source {source_id} sha256 must be lowercase sha256"
        );
        assert!(
            !source.changes.is_empty(),
            "source {source_id} changes is required"
        );
    }
}

fn validate_dictionary_versions(artifact: &Artifact) {
    for (version_key, version) in &artifact.dictionary_versions {
        let version_number = parse_version_key(version_key, "dictionary_versions");
        assert!(
            !version.label.is_empty(),
            "dictionary_versions.{version_key}.label is required"
        );
        assert!(
            version.sources == sorted_unique_strings(&version.sources),
            "dictionary_versions.{version_key}.sources must be sorted and unique"
        );
        for source_id in &version.sources {
            assert!(
                artifact.sources.contains_key(source_id),
                "dictionary_versions.{version_key} references unknown source {source_id}"
            );
        }
        assert!(
            !version.categories.is_empty(),
            "dictionary_versions.{version_key}.categories must not be empty"
        );

        let mut logical_categories = BTreeMap::new();
        for (category_name, word_ids) in &version.categories {
            assert!(
                !category_name.is_empty(),
                "dictionary_versions.{version_key} has empty category name"
            );
            validate_word_ids(
                word_ids,
                artifact.words.allowed.len(),
                &format!("dictionary_versions.{version_key}.categories.{category_name}"),
            );
            logical_categories.insert(
                category_name.clone(),
                word_ids
                    .iter()
                    .map(|word_id| artifact.words.allowed[usize::from(*word_id)].clone())
                    .collect::<Vec<_>>(),
            );
        }

        let expected_hash = dictionary_version_hash(
            version_number,
            &version.label,
            &version.sources,
            &logical_categories,
        );
        assert_eq!(
            version.hash, expected_hash,
            "dictionary_versions.{version_key}.hash mismatch"
        );
    }
}

fn validate_blocklist_versions(artifact: &Artifact, blocked_words: &[String]) {
    for (version_key, version) in &artifact.blocklist_versions {
        let version_number = parse_version_key(version_key, "blocklist_versions");
        assert!(
            !version.label.is_empty(),
            "blocklist_versions.{version_key}.label is required"
        );
        assert!(
            !version.sources.is_empty(),
            "blocklist_versions.{version_key}.sources must not be empty"
        );

        let mut logical_sources = BTreeMap::new();
        for (source_id, word_ids) in &version.sources {
            assert!(
                artifact.sources.contains_key(source_id),
                "blocklist_versions.{version_key} references unknown source {source_id}"
            );
            validate_word_ids(
                word_ids,
                blocked_words.len(),
                &format!("blocklist_versions.{version_key}.sources.{source_id}"),
            );
            logical_sources.insert(
                source_id.clone(),
                word_ids
                    .iter()
                    .map(|word_id| blocked_words[usize::from(*word_id)].clone())
                    .collect::<Vec<_>>(),
            );
        }

        let expected_hash =
            blocklist_version_hash(version_number, &version.label, &logical_sources);
        assert_eq!(
            version.hash, expected_hash,
            "blocklist_versions.{version_key}.hash mismatch"
        );
    }
}

fn validate_word_ids(word_ids: &[u16], word_count: usize, context: &str) {
    assert!(!word_ids.is_empty(), "{context} must not be empty");
    let sorted_unique = word_ids.iter().copied().collect::<BTreeSet<_>>();
    assert!(
        word_ids.iter().copied().eq(sorted_unique.iter().copied()),
        "{context} must be sorted and unique"
    );
    for word_id in word_ids {
        assert!(
            usize::from(*word_id) < word_count,
            "{context} contains out-of-range word id {word_id}"
        );
    }
}

fn parse_version_key(version_key: &str, context: &str) -> u32 {
    let version = version_key
        .parse::<u32>()
        .unwrap_or_else(|error| panic!("{context} key {version_key:?} is invalid: {error}"));
    assert!(version > 0, "{context} version numbers must be positive");
    version
}

fn sorted_unique_strings(values: &[String]) -> Vec<String> {
    values
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn dictionary_version_hash(
    version: u32,
    label: &str,
    sources: &[String],
    categories: &BTreeMap<String, Vec<String>>,
) -> String {
    let mut payload = BTreeMap::new();
    payload.insert("kind", Value::String("dictionary".to_owned()));
    payload.insert("version", Value::from(version));
    payload.insert("label", Value::String(label.to_owned()));
    payload.insert(
        "sources",
        Value::Array(sources.iter().cloned().map(Value::String).collect()),
    );
    payload.insert("categories", serde_json::to_value(categories).unwrap());
    logical_hash(&payload)
}

fn blocklist_version_hash(
    version: u32,
    label: &str,
    sources: &BTreeMap<String, Vec<String>>,
) -> String {
    let mut payload = BTreeMap::new();
    payload.insert("kind", Value::String("blocklist".to_owned()));
    payload.insert("version", Value::from(version));
    payload.insert("label", Value::String(label.to_owned()));
    payload.insert("sources", serde_json::to_value(sources).unwrap());
    logical_hash(&payload)
}

fn logical_hash(payload: &BTreeMap<&str, Value>) -> String {
    hex_digest(Sha256::digest(serde_json::to_vec(payload).unwrap()).as_slice())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn render(artifact: &Artifact, blocked_words: &[String], artifact_sha256: &str) -> String {
    let mut generated = String::from("// @generated by crates/hoststamp-core/build.rs\n\n");
    generated.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq)]\n");
    generated.push_str("pub struct SourceMeta {\n");
    generated.push_str("    pub id: &'static str,\n");
    generated.push_str("    pub title: &'static str,\n");
    generated.push_str("    pub url: &'static str,\n");
    generated.push_str("    pub license: &'static str,\n");
    generated.push_str("    pub license_url: &'static str,\n");
    generated.push_str("    pub attribution: &'static str,\n");
    generated.push_str("    pub retrieved: &'static str,\n");
    generated.push_str("    pub sha256: &'static str,\n");
    generated.push_str("    pub changes: &'static str,\n");
    generated.push_str("    pub notice_required: bool,\n");
    generated.push_str("}\n\n");
    generated.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq)]\n");
    generated.push_str("pub struct CategoryWordIds {\n");
    generated.push_str("    pub category: &'static str,\n");
    generated.push_str("    pub word_ids: &'static [u16],\n");
    generated.push_str("}\n\n");
    generated.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq)]\n");
    generated.push_str("pub struct DictionaryVersionMeta {\n");
    generated.push_str("    pub version: u32,\n");
    generated.push_str("    pub label: &'static str,\n");
    generated.push_str("    pub hash: &'static str,\n");
    generated.push_str("    pub sources: &'static [&'static str],\n");
    generated.push_str("    pub categories: &'static [CategoryWordIds],\n");
    generated.push_str("}\n\n");
    generated.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq)]\n");
    generated.push_str("pub struct BlocklistSourceWordIds {\n");
    generated.push_str("    pub source_id: &'static str,\n");
    generated.push_str("    pub word_ids: &'static [u16],\n");
    generated.push_str("}\n\n");
    generated.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq)]\n");
    generated.push_str("pub struct BlocklistVersionMeta {\n");
    generated.push_str("    pub version: u32,\n");
    generated.push_str("    pub label: &'static str,\n");
    generated.push_str("    pub hash: &'static str,\n");
    generated.push_str("    pub sources: &'static [BlocklistSourceWordIds],\n");
    generated.push_str("}\n\n");

    generated.push_str(&format!(
        "pub const SCHEMA_VERSION: u32 = {};\n",
        artifact.schema_version
    ));
    generated.push_str(&format!(
        "pub const GENERATED_AT: &str = {};\n",
        string_literal(&artifact.generated)
    ));
    generated.push_str(&format!(
        "pub const GENERATOR: &str = {};\n",
        string_literal(&artifact.generator)
    ));
    generated.push_str(&format!(
        "pub const ARTIFACT_SHA256: &str = {};\n",
        string_literal(artifact_sha256)
    ));
    generated.push_str(&format!(
        "pub const DEFAULT_DICTIONARY_VERSION: u32 = {};\n",
        artifact.default_dictionary_version
    ));
    generated.push_str(&format!(
        "pub const DEFAULT_BLOCKLIST_VERSION: u32 = {};\n\n",
        artifact.default_blocklist_version
    ));

    generated.push_str("pub const ALLOWED_WORDS: &[&str] = &[\n");
    for word in &artifact.words.allowed {
        generated.push_str("    ");
        generated.push_str(&string_literal(word));
        generated.push_str(",\n");
    }
    generated.push_str("];\n\n");

    generated.push_str("pub const BLOCKED_WORDS: &[&str] = &[\n");
    for word in blocked_words {
        generated.push_str("    ");
        generated.push_str(&string_literal(word));
        generated.push_str(",\n");
    }
    generated.push_str("];\n\n");

    generated.push_str("pub const DICTIONARY_VERSIONS: &[DictionaryVersionMeta] = &[\n");
    for (version_key, version) in &artifact.dictionary_versions {
        generated.push_str("    DictionaryVersionMeta {\n");
        generated.push_str(&format!(
            "        version: {},\n",
            parse_version_key(version_key, "dictionary_versions")
        ));
        generated.push_str(&format!(
            "        label: {},\n",
            string_literal(&version.label)
        ));
        generated.push_str(&format!(
            "        hash: {},\n",
            string_literal(&version.hash)
        ));
        generated.push_str("        sources: &[\n");
        for source_id in &version.sources {
            generated.push_str("            ");
            generated.push_str(&string_literal(source_id));
            generated.push_str(",\n");
        }
        generated.push_str("        ],\n");
        generated.push_str("        categories: &[\n");
        for (category_name, word_ids) in &version.categories {
            generated.push_str("            CategoryWordIds {\n");
            generated.push_str(&format!(
                "                category: {},\n",
                string_literal(category_name)
            ));
            generated.push_str("                word_ids: &[\n");
            for word_id in word_ids {
                generated.push_str(&format!("                    {word_id}u16,\n"));
            }
            generated.push_str("                ],\n");
            generated.push_str("            },\n");
        }
        generated.push_str("        ],\n");
        generated.push_str("    },\n");
    }
    generated.push_str("];\n\n");

    generated.push_str("pub const BLOCKLIST_VERSIONS: &[BlocklistVersionMeta] = &[\n");
    for (version_key, version) in &artifact.blocklist_versions {
        generated.push_str("    BlocklistVersionMeta {\n");
        generated.push_str(&format!(
            "        version: {},\n",
            parse_version_key(version_key, "blocklist_versions")
        ));
        generated.push_str(&format!(
            "        label: {},\n",
            string_literal(&version.label)
        ));
        generated.push_str(&format!(
            "        hash: {},\n",
            string_literal(&version.hash)
        ));
        generated.push_str("        sources: &[\n");
        for (source_id, word_ids) in &version.sources {
            generated.push_str("            BlocklistSourceWordIds {\n");
            generated.push_str(&format!(
                "                source_id: {},\n",
                string_literal(source_id)
            ));
            generated.push_str("                word_ids: &[\n");
            for word_id in word_ids {
                generated.push_str(&format!("                    {word_id}u16,\n"));
            }
            generated.push_str("                ],\n");
            generated.push_str("            },\n");
        }
        generated.push_str("        ],\n");
        generated.push_str("    },\n");
    }
    generated.push_str("];\n\n");

    generated.push_str("pub const SOURCES: &[SourceMeta] = &[\n");
    for (source_id, source) in &artifact.sources {
        generated.push_str("    SourceMeta {\n");
        generated.push_str(&format!("        id: {},\n", string_literal(source_id)));
        generated.push_str(&format!(
            "        title: {},\n",
            string_literal(&source.title)
        ));
        generated.push_str(&format!("        url: {},\n", string_literal(&source.url)));
        generated.push_str(&format!(
            "        license: {},\n",
            string_literal(&source.license)
        ));
        generated.push_str(&format!(
            "        license_url: {},\n",
            string_literal(&source.license_url)
        ));
        generated.push_str(&format!(
            "        attribution: {},\n",
            string_literal(&source.attribution)
        ));
        generated.push_str(&format!(
            "        retrieved: {},\n",
            string_literal(&source.retrieved)
        ));
        generated.push_str(&format!(
            "        sha256: {},\n",
            string_literal(&source.sha256)
        ));
        generated.push_str(&format!(
            "        changes: {},\n",
            string_literal(&source.changes)
        ));
        generated.push_str(&format!(
            "        notice_required: {},\n",
            source.notice_required
        ));
        generated.push_str("    },\n");
    }
    generated.push_str("];\n\n");

    let default_dictionary = artifact
        .dictionary_versions
        .get(&artifact.default_dictionary_version.to_string())
        .expect("default dictionary was validated");
    generated.push_str("pub const DEFAULT_CATEGORY_NAMES: &[&str] = &[\n");
    for category_name in default_dictionary.categories.keys() {
        generated.push_str("    ");
        generated.push_str(&string_literal(category_name));
        generated.push_str(",\n");
    }
    generated.push_str("];\n");

    generated
}

fn decode_base64url(value: &str) -> Result<String, String> {
    let mut bits = 0u32;
    let mut bit_count = 0u8;
    let mut bytes = Vec::new();
    for byte in value.bytes() {
        let digit = match byte {
            b'A'..=b'Z' => u32::from(byte - b'A'),
            b'a'..=b'z' => u32::from(byte - b'a') + 26,
            b'0'..=b'9' => u32::from(byte - b'0') + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return Err("token is not base64url".to_owned()),
        };
        bits = (bits << 6) | digit;
        bit_count += 6;
        while bit_count >= 8 {
            bit_count -= 8;
            bytes.push(((bits >> bit_count) & 0xff) as u8);
        }
    }
    if bit_count > 0 && (bits & ((1 << bit_count) - 1)) != 0 {
        return Err("token has non-zero trailing bits".to_owned());
    }
    let decoded =
        String::from_utf8(bytes).map_err(|error| format!("token is not utf-8: {error}"))?;
    if encode_base64url(decoded.as_bytes()) != value {
        return Err("token is not canonical base64url".to_owned());
    }
    Ok(decoded)
}

fn encode_base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut encoded = String::new();
    let mut index = 0;
    while index < bytes.len() {
        let first = bytes[index];
        let second = bytes.get(index + 1).copied();
        let third = bytes.get(index + 2).copied();
        let chunk = (u32::from(first) << 16)
            | (u32::from(second.unwrap_or(0)) << 8)
            | u32::from(third.unwrap_or(0));
        encoded.push(ALPHABET[((chunk >> 18) & 0x3f) as usize] as char);
        encoded.push(ALPHABET[((chunk >> 12) & 0x3f) as usize] as char);
        if second.is_some() {
            encoded.push(ALPHABET[((chunk >> 6) & 0x3f) as usize] as char);
        }
        if third.is_some() {
            encoded.push(ALPHABET[(chunk & 0x3f) as usize] as char);
        }
        index += 3;
    }
    encoded
}

fn string_literal(value: &str) -> String {
    format!("{value:?}")
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(value, "{byte:02x}");
    }
    value
}

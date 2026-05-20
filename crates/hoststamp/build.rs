// SPDX-License-Identifier: FSL-1.1-ALv2

use serde::Deserialize;
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
    meta: Meta,
    categories: BTreeMap<String, Category>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Meta {
    generated: String,
    generator: String,
    normalization: Normalization,
    sources: Vec<Source>,
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
struct Source {
    id: String,
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Category {
    source_ids: Vec<String>,
    lengths: BTreeMap<String, Vec<String>>,
}

fn main() {
    if let Err(message) = try_main() {
        println!("cargo::error={message}");
        process::exit(1);
    }
}

fn try_main() -> Result<(), String> {
    println!("cargo:rerun-if-env-changed={ARTIFACT_ENV}");

    let manifest_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR")
            .map_err(|error| format!("failed to read CARGO_MANIFEST_DIR: {error}"))?,
    );
    let artifact_path = env::var_os(ARTIFACT_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest_dir.join(DEFAULT_ARTIFACT_PATH));
    println!("cargo:rerun-if-changed={}", artifact_path.display());

    let contents = fs::read_to_string(&artifact_path).map_err(|error| {
        format!(
            "failed to read dictionary artifact {}: {error}",
            artifact_path.display()
        )
    })?;
    let artifact: Artifact = serde_json::from_str(&contents).map_err(|error| {
        format!(
            "failed to parse dictionary artifact {}: {error}",
            artifact_path.display()
        )
    })?;

    catch_validation_error(|| validate_artifact(&artifact))?;

    let out_path = PathBuf::from(
        env::var("OUT_DIR").map_err(|error| format!("failed to read OUT_DIR: {error}"))?,
    )
    .join("dictionary_generated.rs");
    fs::write(&out_path, render(&artifact))
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

fn catch_validation_error(function: impl FnOnce() + panic::UnwindSafe) -> Result<(), String> {
    let hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let result = panic::catch_unwind(function).map_err(panic_message);
    panic::set_hook(hook);
    result
}

fn validate_artifact(artifact: &Artifact) {
    assert_eq!(
        artifact.schema_version, 1,
        "unsupported artifact schema_version"
    );
    assert!(
        !artifact.meta.generated.is_empty(),
        "meta.generated is required"
    );
    assert!(
        !artifact.meta.generator.is_empty(),
        "meta.generator is required"
    );
    assert!(
        artifact.meta.normalization.ascii_fold,
        "meta.normalization.ascii_fold must be true"
    );
    assert_eq!(
        artifact.meta.normalization.charset, "^[a-z]+$",
        "meta.normalization.charset must be ^[a-z]+$"
    );
    assert!(
        artifact.meta.normalization.rfc1123_label,
        "meta.normalization.rfc1123_label must be true"
    );
    assert!(
        artifact.meta.normalization.length_min > 0,
        "normalization length_min must be positive"
    );
    assert!(
        artifact.meta.normalization.length_min <= artifact.meta.normalization.length_max,
        "normalization length_min must be <= length_max"
    );
    assert!(
        !artifact.meta.sources.is_empty(),
        "meta.sources must not be empty"
    );
    assert!(
        !artifact.categories.is_empty(),
        "categories must not be empty"
    );

    let mut source_ids = BTreeSet::new();
    for source in &artifact.meta.sources {
        assert!(!source.id.is_empty(), "source.id is required");
        assert!(
            source_ids.insert(source.id.as_str()),
            "duplicate source id {}",
            source.id
        );
        assert!(
            !source.title.is_empty(),
            "source {} title is required",
            source.id
        );
        assert!(
            !source.url.is_empty(),
            "source {} url is required",
            source.id
        );
        assert!(
            !source.license.is_empty(),
            "source {} license is required",
            source.id
        );
        assert!(
            !source.license_url.is_empty(),
            "source {} license_url is required",
            source.id
        );
        assert!(
            !source.attribution.is_empty(),
            "source {} attribution is required",
            source.id
        );
        assert!(
            !source.retrieved.is_empty(),
            "source {} retrieved is required",
            source.id
        );
        assert!(
            !source.sha256.is_empty(),
            "source {} sha256 is required",
            source.id
        );
        assert!(
            !source.changes.is_empty(),
            "source {} changes is required",
            source.id
        );
    }

    for (category_name, category) in &artifact.categories {
        assert!(
            !category_name.is_empty(),
            "category names must not be empty"
        );
        assert!(
            !category.source_ids.is_empty(),
            "category {category_name} must include source_ids"
        );
        for source_id in &category.source_ids {
            assert!(
                source_ids.contains(source_id.as_str()),
                "category {category_name} references unknown source {source_id}"
            );
        }

        assert!(
            !category.lengths.is_empty(),
            "category {category_name} must include length buckets"
        );
        for (length_key, words) in &category.lengths {
            let length = length_key.parse::<usize>().unwrap_or_else(|error| {
                panic!("category {category_name} has invalid length key {length_key}: {error}")
            });
            assert!(
                (artifact.meta.normalization.length_min..=artifact.meta.normalization.length_max)
                    .contains(&length),
                "category {category_name} length {length} is outside normalization bounds"
            );
            assert!(
                !words.is_empty(),
                "category {category_name} length {length} is empty"
            );

            let mut previous: Option<&str> = None;
            for word in words {
                assert!(
                    word.chars().all(|c| c.is_ascii_lowercase()),
                    "category {category_name} length {length} contains non-lowercase word {word}"
                );
                assert_eq!(
                    word.chars().count(),
                    length,
                    "category {category_name} length {length} contains wrong-length word {word}"
                );
                if let Some(previous) = previous {
                    assert!(
                        previous < word.as_str(),
                        "category {category_name} length {length} words must be sorted and unique"
                    );
                }
                previous = Some(word);
            }
        }
    }
}

fn render(artifact: &Artifact) -> String {
    let mut generated = String::from("// @generated by crates/hoststamp/build.rs\n\n");
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
    generated.push_str(&format!(
        "pub const SCHEMA_VERSION: u32 = {};\n",
        artifact.schema_version
    ));
    generated.push_str(&format!(
        "pub const GENERATED_AT: &str = {};\n\n",
        string_literal(&artifact.meta.generated)
    ));

    generated.push_str("pub const CATEGORY_NAMES: &[&str] = &[\n");
    for category_name in artifact.categories.keys() {
        generated.push_str("    ");
        generated.push_str(&string_literal(category_name));
        generated.push_str(",\n");
    }
    generated.push_str("];\n\n");

    generated.push_str("#[allow(clippy::type_complexity)]\n");
    generated.push_str("pub const CATEGORIES: &[(&str, &[(usize, &[&str])])] = &[\n");
    for (category_name, category) in &artifact.categories {
        generated.push_str("    (");
        generated.push_str(&string_literal(category_name));
        generated.push_str(", &[\n");
        for (length_key, words) in &category.lengths {
            generated.push_str("        (");
            generated.push_str(length_key);
            generated.push_str("usize, &[\n");
            for word in words {
                generated.push_str("            ");
                generated.push_str(&string_literal(word));
                generated.push_str(",\n");
            }
            generated.push_str("        ]),\n");
        }
        generated.push_str("    ]),\n");
    }
    generated.push_str("];\n\n");

    generated.push_str("pub const SOURCES: &[SourceMeta] = &[\n");
    for source in &artifact.meta.sources {
        generated.push_str("    SourceMeta {\n");
        generated.push_str(&format!("        id: {},\n", string_literal(&source.id)));
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

    generated.push_str("pub const CATEGORY_SOURCE_IDS: &[(&str, &[&str])] = &[\n");
    for (category_name, category) in &artifact.categories {
        generated.push_str("    (");
        generated.push_str(&string_literal(category_name));
        generated.push_str(", &[\n");
        for source_id in &category.source_ids {
            generated.push_str("        ");
            generated.push_str(&string_literal(source_id));
            generated.push_str(",\n");
        }
        generated.push_str("    ]),\n");
    }
    generated.push_str("];\n");

    generated
}

fn string_literal(value: &str) -> String {
    format!("{value:?}")
}

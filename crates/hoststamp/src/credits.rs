// SPDX-License-Identifier: FSL-1.1-ALv2

use crate::dictionary;
use std::fmt::Write as _;

pub fn text() -> String {
    let mut credits = format!(
        "\
Hoststamp {version}

Copyright (c) 2026 Michael Stutz
License: Functional Source License 1.1, ALv2 Future License (FSL-1.1-ALv2)
Future license: Apache License 2.0

External data:
Generated: {generated_at}
",
        version = env!("CARGO_PKG_VERSION"),
        generated_at = dictionary::GENERATED_AT,
    );

    for source in dictionary::sources() {
        writeln!(credits, "- {}", source.title).expect("write to string");
        writeln!(credits, "  Attribution: {}", source.attribution).expect("write to string");
        writeln!(credits, "  Source: {}", source.url).expect("write to string");
        writeln!(credits, "  License: {}", source.license).expect("write to string");
        writeln!(credits, "  License URL: {}", source.license_url).expect("write to string");
        writeln!(credits, "  Retrieved: {}", source.retrieved).expect("write to string");
        writeln!(credits, "  SHA-256: {}", source.sha256).expect("write to string");
        writeln!(credits, "  Changes: {}", source.changes).expect("write to string");
    }

    credits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credits_include_generated_source_metadata() {
        let credits = text();

        assert!(credits.contains("FSL-1.1-ALv2"));
        assert!(credits.contains("golang-petname"));
        assert!(credits.contains("EFF large Diceware wordlist"));
        assert!(credits.contains("CC-BY-3.0-US"));
        assert!(credits.contains("SHA-256:"));
        assert!(credits.contains(dictionary::GENERATED_AT));
    }
}

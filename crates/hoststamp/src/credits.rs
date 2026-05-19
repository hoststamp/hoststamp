// SPDX-License-Identifier: FSL-1.1-ALv2

use crate::wordlists::{EFF_DICE_PAGE_URL, EFF_LICENSE_NAME, EFF_LICENSE_URL, EFF_WORDLISTS};
use std::fmt::Write as _;

pub fn text() -> String {
    let mut credits = format!(
        "\
Hoststamp {version}

Copyright (c) 2026 Michael Stutz
License: Functional Source License 1.1, ALv2 Future License (FSL-1.1-ALv2)
Future license: Apache License 2.0

External data:
- EFF wordlists, created by Joseph Bonneau for the Electronic Frontier
  Foundation (EFF)
  Project page: {dice_page}
  License: {license_name}
  License URL: {license_url}
  Changes: none; bundled as downloaded text files.
",
        version = env!("CARGO_PKG_VERSION"),
        dice_page = EFF_DICE_PAGE_URL,
        license_name = EFF_LICENSE_NAME,
        license_url = EFF_LICENSE_URL,
    );

    for wordlist in EFF_WORDLISTS {
        writeln!(
            credits,
            "  - {} ({} entries, {} dice): {}",
            wordlist.title,
            wordlist.entry_count(),
            wordlist.dice,
            wordlist.source_url
        )
        .expect("write to string");
    }

    credits
}

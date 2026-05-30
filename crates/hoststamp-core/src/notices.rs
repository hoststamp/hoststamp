// SPDX-License-Identifier: FSL-1.1-ALv2

use crate::dictionary;
use std::fmt::Write as _;

pub fn text() -> String {
    let mut notices = String::from("# Third-Party Notices\n\n");
    let _ = writeln!(
        notices,
        "Generated from Hoststamp dictionary artifact schema {} at {}.\n",
        dictionary::SCHEMA_VERSION,
        dictionary::GENERATED_AT
    );

    for source in dictionary::sources() {
        let _ = writeln!(notices, "## {}", source.title);
        let _ = writeln!(notices);
        let _ = writeln!(notices, "- Source ID: `{}`", source.id);
        let _ = writeln!(notices, "- Attribution: {}", source.attribution);
        let _ = writeln!(notices, "- Source: <{}>", source.url);
        let _ = writeln!(notices, "- License: {}", source.license);
        let _ = writeln!(notices, "- License URL: <{}>", source.license_url);
        let _ = writeln!(notices, "- Retrieved: {}", source.retrieved);
        let _ = writeln!(notices, "- SHA-256: `{}`", source.sha256);
        let _ = writeln!(notices, "- Notice required: {}", source.notice_required);
        let _ = writeln!(notices, "- Changes: {}", source.changes);
        let _ = writeln!(notices);
    }

    if notices.ends_with("\n\n") {
        notices.pop();
    }

    notices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn third_party_notices_are_current() {
        assert_eq!(text(), include_str!("../../../THIRD-PARTY-NOTICES.md"));
    }
}

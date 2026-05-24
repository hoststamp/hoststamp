// SPDX-License-Identifier: FSL-1.1-ALv2

use crate::dictionary;
use std::fmt::Write as _;

pub fn text() -> String {
    let mut notices = String::from("# Third-Party Notices\n\n");
    writeln!(
        notices,
        "Generated from Hoststamp dictionary artifact schema {} at {}.\n",
        dictionary::SCHEMA_VERSION,
        dictionary::GENERATED_AT
    )
    .expect("write to string");

    for source in dictionary::sources() {
        writeln!(notices, "## {}", source.title).expect("write to string");
        writeln!(notices).expect("write to string");
        writeln!(notices, "- Source ID: `{}`", source.id).expect("write to string");
        writeln!(notices, "- Attribution: {}", source.attribution).expect("write to string");
        writeln!(notices, "- Source: <{}>", source.url).expect("write to string");
        writeln!(notices, "- License: {}", source.license).expect("write to string");
        writeln!(notices, "- License URL: <{}>", source.license_url).expect("write to string");
        writeln!(notices, "- Retrieved: {}", source.retrieved).expect("write to string");
        writeln!(notices, "- SHA-256: `{}`", source.sha256).expect("write to string");
        writeln!(notices, "- Notice required: {}", source.notice_required)
            .expect("write to string");
        writeln!(notices, "- Changes: {}", source.changes).expect("write to string");
        writeln!(notices).expect("write to string");
    }

    notices.push_str(
        "## Sqids default blocklist\n\n\
- Source ID: `sqids-default-blocklist`\n\
- Attribution: Sqids maintainers\n\
- Source: <https://github.com/sqids/sqids-rust/blob/v0.4.2/src/blocklist.json>\n\
- License: MIT\n\
- License URL: <https://opensource.org/license/mit>\n\
- Retrieved: via pinned `sqids` crate 0.4.2\n\
- Notice required: true\n\
- Changes: used through the pinned sqids crate; filtered by lowercase base36 alphabet at runtime\n\n",
    );

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

// SPDX-License-Identifier: FSL-1.1-ALv2

pub fn text() -> String {
    format!(
        "\
Hoststamp {version}

Copyright (c) 2026 Michael Stutz
License: Functional Source License 1.1, ALv2 Future License (FSL-1.1-ALv2)
Future license: Apache License 2.0

External data:
- EFF wordlists: not bundled in this build. Attribution will be included here
  when wordlists are embedded.
",
        version = env!("CARGO_PKG_VERSION")
    )
}

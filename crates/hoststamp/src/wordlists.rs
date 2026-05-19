// SPDX-License-Identifier: FSL-1.1-ALv2

pub const EFF_DICE_PAGE_URL: &str = "https://www.eff.org/dice";
pub const EFF_LICENSE_NAME: &str = "Creative Commons Attribution 3.0 United States";
pub const EFF_LICENSE_URL: &str = "https://creativecommons.org/licenses/by/3.0/us/";

pub const EFF_WORDLISTS: &[Wordlist] = &[EFF_LARGE, EFF_SHORT_2];

pub const EFF_LARGE: Wordlist = Wordlist {
    title: "EFF Long Wordlist",
    filename: "eff_large_wordlist.txt",
    dice: 5,
    source_url: "https://www.eff.org/files/2016/07/18/eff_large_wordlist.txt",
    contents: include_str!("../data/eff/eff_large_wordlist.txt"),
};

pub const EFF_SHORT_2: Wordlist = Wordlist {
    title: "EFF Short Wordlist #2",
    filename: "eff_short_wordlist_2_0.txt",
    dice: 4,
    source_url: "https://www.eff.org/files/2016/09/08/eff_short_wordlist_2_0.txt",
    contents: include_str!("../data/eff/eff_short_wordlist_2_0.txt"),
};

#[derive(Debug, Clone, Copy)]
pub struct Wordlist {
    pub title: &'static str,
    pub filename: &'static str,
    pub dice: u8,
    pub source_url: &'static str,
    pub contents: &'static str,
}

impl Wordlist {
    pub fn entry_count(self) -> usize {
        self.entries().count()
    }

    pub fn entries(self) -> impl Iterator<Item = WordlistEntry<'static>> {
        self.contents.lines().filter_map(parse_entry)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WordlistEntry<'a> {
    pub key: &'a str,
    pub word: &'a str,
}

fn parse_entry(line: &str) -> Option<WordlistEntry<'_>> {
    let (key, word) = line.split_once(char::is_whitespace)?;
    Some(WordlistEntry {
        key,
        word: word.trim(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eff_large_wordlist_has_expected_shape() {
        assert_eq!(EFF_LARGE.entry_count(), 7776);
        assert_eq!(
            EFF_LARGE.entries().next(),
            Some(WordlistEntry {
                key: "11111",
                word: "abacus",
            })
        );
    }

    #[test]
    fn eff_short_2_wordlist_has_expected_shape() {
        assert_eq!(EFF_SHORT_2.entry_count(), 1296);
        assert_eq!(
            EFF_SHORT_2.entries().next(),
            Some(WordlistEntry {
                key: "1111",
                word: "aardvark",
            })
        );
    }
}

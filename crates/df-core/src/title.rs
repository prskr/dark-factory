//! Task title sources, sanitizing and the placeholder derived from a description
//! (`specs/task-management/spec.md`, "Task title sources").

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::domain::ParseEnumError;

/// Maximum length of a title in characters.
pub const TITLE_MAX_CHARS: usize = 80;

/// Title used when neither a title nor a usable description exists.
pub const FALLBACK_TITLE: &str = "Untitled task";

/// Where a task's title came from, ranked `User` > `Generated` > `Agent` > `Placeholder`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TitleSource {
    Placeholder,
    Agent,
    Generated,
    User,
}

impl TitleSource {
    pub const ALL: &'static [Self] = &[Self::Placeholder, Self::Agent, Self::Generated, Self::User];

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Placeholder => "placeholder",
            Self::Agent => "agent",
            Self::Generated => "generated",
            Self::User => "user",
        }
    }

    /// Whether a title of source `self` may replace a title of source `current`: a user title
    /// always wins, agent and generated titles only replace a placeholder (the generated one
    /// also an agent title).
    pub const fn may_replace(&self, current: Self) -> bool {
        match self {
            Self::User => true,
            Self::Generated => matches!(current, Self::Placeholder | Self::Agent),
            Self::Agent => matches!(current, Self::Placeholder),
            Self::Placeholder => false,
        }
    }
}

impl fmt::Display for TitleSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl TryFrom<String> for TitleSource {
    type Error = ParseEnumError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl FromStr for TitleSource {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .iter()
            .copied()
            .find(|v| v.as_str() == s)
            .ok_or_else(|| ParseEnumError {
                kind: "TitleSource",
                value: s.to_string(),
            })
    }
}

/// Whether `raw` is longer than [`TITLE_MAX_CHARS`] after control characters and whitespace are
/// normalized; [`sanitize_title`] would cut it.
pub fn title_too_long(raw: &str) -> bool {
    collapse(raw).chars().count() > TITLE_MAX_CHARS
}

/// Makes `raw` a plain-text title: control characters and line breaks become spaces,
/// whitespace is collapsed, the result is trimmed and cut to [`TITLE_MAX_CHARS`] characters.
/// Returns `None` when nothing is left.
pub fn sanitize_title(raw: &str) -> Option<String> {
    let flat = collapse(raw);
    let cut: String = flat.chars().take(TITLE_MAX_CHARS).collect();
    let cut = cut.trim_end();
    (!cut.is_empty()).then(|| cut.to_string())
}

/// The placeholder title of a description: its first sentence (ends at `. `, `! `, `? ` or a
/// line break), cut at a word boundary with `…` when longer than [`TITLE_MAX_CHARS`].
pub fn placeholder_title(description: &str) -> Option<String> {
    let text = description.trim();
    let sentence = first_sentence(text);
    let flat = collapse(sentence);
    if flat.chars().count() <= TITLE_MAX_CHARS {
        return (!flat.is_empty()).then_some(flat);
    }
    let head: String = flat.chars().take(TITLE_MAX_CHARS - 1).collect();
    // Cut back to the last word when the next character does not continue a word.
    let next_is_space = flat
        .chars()
        .nth(TITLE_MAX_CHARS - 1)
        .is_some_and(char::is_whitespace);
    let words = match head.rfind(' ') {
        Some(i) if !next_is_space => &head[..i],
        _ => &head,
    };
    Some(format!("{}…", words.trim_end()))
}

fn collapse(raw: &str) -> String {
    let spaced: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    spaced.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn first_sentence(text: &str) -> &str {
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == '\n' || c == '\r' {
            return &text[..i];
        }
        if matches!(c, '.' | '!' | '?') && chars.peek().is_some_and(|(_, n)| n.is_whitespace()) {
            return &text[..i + c.len_utf8()];
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_breaks_and_control_characters_become_single_spaces() {
        assert_eq!(
            sanitize_title("Fix login\n\nand logout").as_deref(),
            Some("Fix login and logout")
        );
        assert_eq!(sanitize_title("  a\tb\u{7}c  ").as_deref(), Some("a b c"));
    }

    #[test]
    fn too_long_is_measured_after_normalizing() {
        assert!(!title_too_long(&format!(
            "{}\n\n{}",
            "a".repeat(40),
            "b".repeat(39)
        )));
        assert!(title_too_long(&"a".repeat(TITLE_MAX_CHARS + 1)));
    }

    #[test]
    fn empty_titles_are_rejected() {
        assert_eq!(sanitize_title(" \n\t "), None);
        assert_eq!(sanitize_title(""), None);
        assert_eq!(placeholder_title("  "), None);
    }

    #[test]
    fn long_titles_are_cut_on_a_character_boundary() {
        let title = sanitize_title(&"ä".repeat(200)).unwrap();
        assert_eq!(title.chars().count(), TITLE_MAX_CHARS);
    }

    #[test]
    fn placeholder_is_the_first_sentence() {
        assert_eq!(
            placeholder_title("The nightly backup job sometimes runs twice. It started last week.")
                .as_deref(),
            Some("The nightly backup job sometimes runs twice.")
        );
        assert_eq!(
            placeholder_title("Fix login\nthen logout").as_deref(),
            Some("Fix login")
        );
        assert_eq!(
            placeholder_title("Fix v1.2 parsing").as_deref(),
            Some("Fix v1.2 parsing")
        );
    }

    #[test]
    fn long_placeholder_is_cut_at_a_word_with_an_ellipsis() {
        let description = "word ".repeat(40);
        let title = placeholder_title(&description).unwrap();
        assert!(title.ends_with("word…"), "{title}");
        assert!(title.chars().count() <= TITLE_MAX_CHARS);

        let unbroken = placeholder_title(&"x".repeat(200)).unwrap();
        assert_eq!(unbroken.chars().count(), TITLE_MAX_CHARS);
        assert!(unbroken.ends_with('…'));
    }

    #[test]
    fn precedence_matrix() {
        use TitleSource::*;
        let allowed = [
            (User, Placeholder),
            (User, Agent),
            (User, Generated),
            (User, User),
            (Generated, Placeholder),
            (Generated, Agent),
            (Agent, Placeholder),
        ];
        for new in TitleSource::ALL {
            for current in TitleSource::ALL {
                assert_eq!(
                    new.may_replace(*current),
                    allowed.contains(&(*new, *current)),
                    "{new} over {current}"
                );
            }
        }
    }

    #[test]
    fn source_strings_roundtrip() {
        for s in TitleSource::ALL {
            assert_eq!(s.as_str().parse::<TitleSource>().unwrap(), *s);
        }
        assert!("other".parse::<TitleSource>().is_err());
    }
}

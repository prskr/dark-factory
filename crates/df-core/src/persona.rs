//! The persona identity: handle, display name and commit email
//! (`specs/persona-identity/spec.md`, "Persona identity fields").

use thiserror::Error;

/// Longest handle, in characters.
pub const HANDLE_MAX_CHARS: usize = 39;
/// Longest display name, in characters.
pub const DISPLAY_NAME_MAX_CHARS: usize = 100;
/// Longest commit email, in characters.
pub const EMAIL_MAX_CHARS: usize = 254;

/// Author and committer of a run's commits: the display name and commit email its persona had
/// when the run was created. It travels from the run through `Assign` and the gateway's Git route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitIdentity {
    pub name: String,
    pub email: String,
}

/// Why a part of a persona identity is invalid. The message is shown at the form field.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum IdentityError {
    #[error(
        "Use 1 to {HANDLE_MAX_CHARS} lowercase letters, digits or hyphens, starting with a letter or digit."
    )]
    Handle,
    #[error("Use 1 to {DISPLAY_NAME_MAX_CHARS} characters without line breaks, `<` or `>`.")]
    DisplayName,
    #[error("Enter one address like name@example.org.")]
    Email,
}

/// Who a persona appears as in commits and in the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonaIdentity {
    pub handle: String,
    pub display_name: String,
    pub commit_email: String,
}

impl PersonaIdentity {
    /// Validates the three parts; the display name is trimmed and the email lowercased.
    pub fn new(
        handle: &str,
        display_name: &str,
        commit_email: &str,
    ) -> Result<Self, IdentityError> {
        Ok(Self {
            handle: parse_handle(handle)?,
            display_name: parse_display_name(display_name)?,
            commit_email: parse_email(commit_email)?,
        })
    }
}

/// `^[a-z0-9][a-z0-9-]{0,38}$`.
pub fn parse_handle(handle: &str) -> Result<String, IdentityError> {
    let mut chars = handle.chars();
    let valid_start = chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    let valid_rest = chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if valid_start && valid_rest && handle.len() <= HANDLE_MAX_CHARS {
        Ok(handle.to_owned())
    } else {
        Err(IdentityError::Handle)
    }
}

/// 1 to 100 characters, no control characters, `<` or `>`; surrounding whitespace is trimmed.
pub fn parse_display_name(name: &str) -> Result<String, IdentityError> {
    let name = name.trim();
    let count = name.chars().count();
    let forbidden = |c: char| c.is_control() || c == '<' || c == '>';
    if (1..=DISPLAY_NAME_MAX_CHARS).contains(&count) && !name.chars().any(forbidden) {
        Ok(name.to_owned())
    } else {
        Err(IdentityError::DisplayName)
    }
}

/// One `local@domain` address without whitespace, `<`, `>` or `,`; stored lowercased.
pub fn parse_email(email: &str) -> Result<String, IdentityError> {
    let email = email.trim();
    let forbidden = |c: char| c.is_whitespace() || c.is_control() || matches!(c, '<' | '>' | ',');
    let well_formed = email.chars().count() <= EMAIL_MAX_CHARS
        && !email.chars().any(forbidden)
        && email.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty() && !domain.is_empty() && !domain.contains('@')
        });
    if well_formed {
        Ok(email.to_lowercase())
    } else {
        Err(IdentityError::Email)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_valid_identity_is_stored_with_a_lowercased_email() {
        let identity =
            PersonaIdentity::new("reviewer", "Code Reviewer", "Reviewer@Example.org").unwrap();

        assert_eq!(identity.commit_email, "reviewer@example.org");
        assert_eq!(identity.display_name, "Code Reviewer");
    }

    #[test]
    fn handles_follow_the_pattern() {
        for ok in ["a", "0", "code-reviewer", "a-", &"a".repeat(39)] {
            assert!(parse_handle(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "-a",
            "Code Reviewer",
            "Ada",
            "a_b",
            "é",
            "a b",
            &"a".repeat(40),
        ] {
            assert_eq!(parse_handle(bad), Err(IdentityError::Handle), "{bad}");
        }
    }

    #[test]
    fn display_names_reject_markup_control_characters_and_bad_lengths() {
        assert_eq!(parse_display_name("  Linus ").unwrap(), "Linus");
        assert!(parse_display_name(&"é".repeat(100)).is_ok());
        for bad in [
            "",
            "   ",
            "Bot <root@example.org>",
            "a>b",
            "two\nlines",
            "tab\there",
            "nul\0",
            &"a".repeat(101),
        ] {
            assert_eq!(
                parse_display_name(bad),
                Err(IdentityError::DisplayName),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn emails_are_a_single_plain_address() {
        for bad in [
            "",
            "linus",
            "@example.org",
            "linus@",
            "a@b@c",
            "a b@example.org",
            "Linus <a@example.org>",
            "a@example.org,b@example.org",
            &format!("{}@example.org", "a".repeat(250)),
        ] {
            assert_eq!(parse_email(bad), Err(IdentityError::Email), "{bad}");
        }
    }
}

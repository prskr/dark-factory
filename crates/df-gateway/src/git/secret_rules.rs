//! The push checks' secret scan (design D3): Kingfisher's rules, vendored in `rules/secrets.yml`
//! (attribution in `rules/NOTICE`), compiled once into a [`RegexSet`] that selects the rules
//! matching a blob, plus one [`Regex`] per rule to locate and mask the matches.

use gix::bstr::{BStr, ByteSlice as _};
use regex::bytes::{Regex, RegexBuilder, RegexSet, RegexSetBuilder};
use serde::Deserialize;

use super::checks::{ScanError, SecretMatch, SecretMatcher};
use super::view::{AddedBlob, escape};
use crate::upstream::BoxError;

const BUILTIN: &str = include_str!("../../rules/secrets.yml");
/// Lazy DFA cache of the rule set; halves the scan time compared to the default 2 MiB (README).
const SET_DFA_SIZE_LIMIT: usize = 8 << 20;
/// Characters of a match shown unmasked.
const MASK_KEEP: usize = 4;
/// Most characters of a masked match.
const MASK_WIDTH: usize = 12;

/// A compiled set of secret rules.
pub struct SecretRules {
    set: RegexSet,
    rules: Vec<SecretRule>,
}

/// One compiled secret rule.
pub struct SecretRule {
    id: String,
    regex: Regex,
}

/// The rules could not be loaded.
#[derive(Debug, thiserror::Error)]
pub enum SecretRulesError {
    #[error("the secret rules are not valid YAML: {0}")]
    Parse(String),
    #[error("secret rule {id} does not compile: {source}")]
    Rule { id: String, source: regex::Error },
    #[error("the secret rule set does not compile: {0}")]
    Set(regex::Error),
}

impl SecretRules {
    /// The vendored rules.
    pub fn builtin() -> Result<Self, SecretRulesError> {
        Self::from_yaml(BUILTIN)
    }

    /// Rules in the format of `rules/secrets.yml`. Patterns are compiled without Unicode: `\w`,
    /// `\s` and `\b` are ASCII, and a match may sit between bytes that are not UTF-8.
    pub fn from_yaml(yaml: &str) -> Result<Self, SecretRulesError> {
        let file: RuleFile =
            serde_saphyr::from_str(yaml).map_err(|e| SecretRulesError::Parse(e.to_string()))?;
        let rules = file
            .rules
            .into_iter()
            .map(|entry| {
                let regex = RegexBuilder::new(&entry.pattern)
                    .unicode(false)
                    .build()
                    .map_err(|source| SecretRulesError::Rule {
                        id: entry.id.clone(),
                        source,
                    })?;
                Ok(SecretRule {
                    id: entry.id,
                    regex,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let set = RegexSetBuilder::new(rules.iter().map(|rule| rule.regex.as_str()))
            .unicode(false)
            .dfa_size_limit(SET_DFA_SIZE_LIMIT)
            .build()
            .map_err(SecretRulesError::Set)?;
        Ok(Self { set, rules })
    }

    /// Rules with at least one match in `blob`, in file order.
    pub fn matching<'a>(&'a self, blob: &[u8]) -> impl Iterator<Item = &'a SecretRule> {
        self.set
            .matches(blob)
            .into_iter()
            .filter_map(|index| self.rules.get(index))
    }

    /// The secrets in `content`, at most one per rule and line. The line is counted over `\n`
    /// bytes; the match itself only appears masked.
    pub fn scan(&self, content: &[u8]) -> Vec<SecretMatch> {
        let mut found = Vec::new();
        for rule in self.matching(content) {
            let mut line: u32 = 1;
            let mut counted = 0;
            let mut last_line = None;
            for matched in rule.regex.find_iter(content) {
                let newlines = content
                    .get(counted..matched.start())
                    .unwrap_or_default()
                    .find_iter("\n")
                    .count();
                line = line.saturating_add(u32::try_from(newlines).unwrap_or(u32::MAX));
                counted = matched.start();
                if last_line.replace(line) == Some(line) {
                    continue;
                }
                found.push(SecretMatch {
                    rule: rule.id.clone(),
                    line,
                    masked: mask(matched.as_bytes()),
                });
            }
        }
        found
    }
}

impl SecretRule {
    pub fn id(&self) -> &str {
        &self.id
    }
}

/// Reads the blobs of a push.
pub trait BlobSource {
    /// The content of the blob `id`.
    fn read_blob(&self, id: gix::ObjectId) -> Result<Vec<u8>, BoxError>;
}

impl BlobSource for gix::Repository {
    fn read_blob(&self, id: gix::ObjectId) -> Result<Vec<u8>, BoxError> {
        Ok(self.find_blob(id)?.detach().data)
    }
}

/// The secret matcher of the push checks: scans every blob, binary ones included, with `rules`.
pub struct RuleMatcher<'a> {
    pub rules: &'a SecretRules,
    pub blobs: &'a dyn BlobSource,
}

impl SecretMatcher for RuleMatcher<'_> {
    fn find(&self, blob: &AddedBlob) -> Result<Vec<SecretMatch>, ScanError> {
        let content = self.blobs.read_blob(blob.id).map_err(|source| ScanError {
            blob: blob.id,
            source,
        })?;
        Ok(self.rules.scan(&content))
    }
}

#[derive(Deserialize)]
struct RuleFile {
    rules: Vec<RuleEntry>,
}

#[derive(Deserialize)]
struct RuleEntry {
    id: String,
    pattern: String,
}

/// `matched` escaped like a path (bytes that are not UTF-8 and control characters as `\xNN`),
/// with all but its first [`MASK_KEEP`] characters as `*` and at most [`MASK_WIDTH`] long.
fn mask(matched: &[u8]) -> String {
    let text = escape(BStr::new(matched));
    let width = text.chars().count().min(MASK_WIDTH);
    text.chars()
        .take(MASK_KEEP)
        .chain(std::iter::repeat('*'))
        .take(width)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use df_proto::gateway::v1::{CheckMode, MaskedSecret, PushPolicy};

    use super::*;
    use crate::git::checks::{Check, CheckPolicy, Identity, Target, Verdict, check_push, verdict};
    use crate::git::view::PushView;

    fn matching_ids(rules: &SecretRules, blob: &[u8]) -> Vec<String> {
        rules.matching(blob).map(|r| r.id().to_owned()).collect()
    }

    /// A PEM private key; assembled at runtime so this file holds no complete key.
    fn private_key() -> String {
        let label = ["PRIVATE", "KEY"].join(" ");
        format!(
            "-----BEGIN {label}-----\nMIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQC7\n-----END {label}-----\n"
        )
    }

    #[test]
    fn builtin_rules_compile_with_unique_ids() {
        let rules = SecretRules::builtin().unwrap();

        let ids: HashSet<_> = rules.rules.iter().map(SecretRule::id).collect();
        assert_eq!(rules.rules.len(), 56);
        assert_eq!(ids.len(), rules.rules.len());
        assert_eq!(rules.set.len(), rules.rules.len());
    }

    #[test]
    fn github_token_in_source_matches_the_github_rule() {
        let rules = SecretRules::builtin().unwrap();
        let token = ["ghp_", &"A1b2".repeat(9)].concat();
        let blob = format!("import os\n\nGITHUB_TOKEN = \"{token}\"\n");

        assert_eq!(
            matching_ids(&rules, blob.as_bytes()),
            ["betterleaks.github-pat"]
        );
    }

    #[test]
    fn private_key_after_nul_and_non_utf8_bytes_matches_the_private_key_rule() {
        let rules = SecretRules::builtin().unwrap();
        let mut blob = vec![0_u8; 32];
        blob.extend_from_slice(&[0xff, 0xfe, 0x80]);
        blob.extend_from_slice(private_key().as_bytes());
        blob.extend_from_slice(&[0xc3, 0x28, 0]);

        assert_eq!(matching_ids(&rules, &blob), ["betterleaks.private-key"]);
    }

    #[test]
    fn ordinary_source_matches_no_rule() {
        let rules = SecretRules::builtin().unwrap();
        let blob = b"fn main() {\n    let commit = \"9fceb02d0ae598e95dc970b74767f19372d61af8\";\n    let id = \"0190f6a5-8c3e-7b1d-9f2a-4c6e8d0b2a1f\";\n    println!(\"{commit} {id}\");\n}\n";

        assert!(matching_ids(&rules, blob).is_empty());
    }

    #[test]
    fn a_broken_pattern_names_its_rule() {
        let yaml = "rules:\n  - id: broken\n    pattern: 'ghp_('\n";

        let error = SecretRules::from_yaml(yaml).err().unwrap();

        assert!(matches!(error, SecretRulesError::Rule { ref id, .. } if id == "broken"));
    }

    fn github_token() -> String {
        ["ghp_", &"A1b2".repeat(9)].concat()
    }

    fn blob_id(n: u8) -> gix::ObjectId {
        gix::ObjectId::from_bytes_or_panic(&[n; 20])
    }

    /// Blobs by id; reading any other blob fails.
    struct FakeBlobs(HashMap<gix::ObjectId, Vec<u8>>);

    impl BlobSource for FakeBlobs {
        fn read_blob(&self, id: gix::ObjectId) -> Result<Vec<u8>, BoxError> {
            self.0
                .get(&id)
                .cloned()
                .ok_or_else(|| format!("{id} missing").into())
        }
    }

    #[test]
    fn scan_reports_rule_line_and_a_masked_preview_of_a_github_token() {
        let rules = SecretRules::builtin().unwrap();
        let blob = format!("import os\n\nGITHUB_TOKEN = \"{}\"\n", github_token());

        let found = rules.scan(blob.as_bytes());

        assert_eq!(
            found,
            [SecretMatch {
                rule: "betterleaks.github-pat".to_owned(),
                line: 3,
                masked: "ghp_********".to_owned(),
            }]
        );
    }

    #[test]
    fn scan_finds_a_private_key_after_nul_bytes_in_a_binary_blob() {
        let rules = SecretRules::builtin().unwrap();
        let mut blob = vec![0_u8; 64];
        blob.extend_from_slice(b"\x01\n\xff\xfe\n");
        blob.extend_from_slice(private_key().as_bytes());
        blob.extend_from_slice(&[0, 0, 0x80]);

        let found = rules.scan(&blob);

        assert_eq!(
            found,
            [SecretMatch {
                rule: "betterleaks.private-key".to_owned(),
                line: 3,
                masked: "----********".to_owned(),
            }]
        );
    }

    #[test]
    fn scan_reports_one_match_per_rule_and_line() {
        let rules = SecretRules::builtin().unwrap();
        let token = github_token();
        let blob = format!("a = \"{token}\" b = \"{token}\"\n\nc = \"{token}\"\n");

        let lines: Vec<u32> = rules.scan(blob.as_bytes()).iter().map(|m| m.line).collect();

        assert_eq!(lines, [1, 3]);
    }

    #[test]
    fn scan_of_ordinary_source_reports_nothing() {
        let rules = SecretRules::builtin().unwrap();

        assert!(rules.scan(b"fn main() {}\n").is_empty());
    }

    #[test]
    fn mask_shows_bytes_that_are_not_utf8_as_escapes() {
        let rules =
            SecretRules::from_yaml("rules:\n  - id: raw\n    pattern: '\\xff\\xfeKEY[0-9]{8}'\n")
                .unwrap();

        let found = rules.scan(b"head \xff\xfeKEY12345678 tail");

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].masked, "\\xff********");
    }

    #[test]
    fn mask_keeps_four_characters_and_masks_the_rest_of_a_short_match() {
        let rules =
            SecretRules::from_yaml("rules:\n  - id: short\n    pattern: 'key[0-9]{3}'\n").unwrap();

        let found = rules.scan(b"key123");

        assert_eq!(found[0].masked, "key1**");
    }

    #[test]
    fn mask_escapes_control_characters_of_a_match() {
        let rules =
            SecretRules::from_yaml("rules:\n  - id: multi\n    pattern: '\\nk\\tey[0-9]{9}'\n")
                .unwrap();

        let found = rules.scan(b"x\nk\tey123456789");

        assert_eq!(found[0].masked, "\\x0a********");
        assert_eq!(found[0].line, 1);
    }

    #[test]
    fn rule_matcher_reads_the_blob_from_its_source() {
        let rules = SecretRules::builtin().unwrap();
        let content = format!("TOKEN={}\n", github_token());
        let blobs = FakeBlobs(HashMap::from([(blob_id(1), content.into_bytes())]));
        let matcher = RuleMatcher {
            rules: &rules,
            blobs: &blobs,
        };
        let blob = AddedBlob {
            id: blob_id(1),
            size: 0,
            paths: vec!["config.py".to_owned()],
        };

        let found = matcher.find(&blob).unwrap();

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].rule, "betterleaks.github-pat");
    }

    #[test]
    fn rule_matcher_fails_for_a_blob_it_cannot_read() {
        let rules = SecretRules::builtin().unwrap();
        let blobs = FakeBlobs(HashMap::new());
        let matcher = RuleMatcher {
            rules: &rules,
            blobs: &blobs,
        };
        let blob = AddedBlob {
            id: blob_id(2),
            size: 0,
            paths: Vec::new(),
        };

        let error = matcher.find(&blob).err().unwrap();

        assert_eq!(error.blob, blob_id(2));
    }

    #[test]
    fn a_committed_api_key_rejects_the_push_with_only_the_masked_preview() {
        let rules = SecretRules::builtin().unwrap();
        let token = github_token();
        let content = format!("import os\n\nGITHUB_TOKEN = \"{token}\"\n");
        let blobs = FakeBlobs(HashMap::from([(blob_id(3), content.into_bytes())]));
        let matcher = RuleMatcher {
            rules: &rules,
            blobs: &blobs,
        };
        let view = PushView {
            commits: Vec::new(),
            blobs: vec![AddedBlob {
                id: blob_id(3),
                size: 60,
                paths: vec!["config.py".to_owned()],
            }],
        };
        let policy = CheckPolicy::new(&PushPolicy {
            secret_mode: CheckMode::Approval.into(),
            max_blob_bytes: 1 << 20,
            ..PushPolicy::default()
        })
        .unwrap();

        let findings = check_push(
            &view,
            &policy,
            &Identity {
                email: "agent@factory.test",
            },
            Target::TaskBranch,
            &matcher,
        )
        .unwrap();

        assert_eq!(findings.len(), 1);
        let found = &findings[0];
        assert_eq!(found.check, Check::SecretScan);
        assert_eq!((found.path.as_str(), found.line), ("config.py", Some(3)));
        assert_eq!(found.detail, "betterleaks.github-pat ghp_********");
        assert_eq!(
            found.secret,
            Some(MaskedSecret {
                rule: "betterleaks.github-pat".to_owned(),
                masked: "ghp_********".to_owned(),
            })
        );
        assert_eq!(
            found.fingerprint,
            Some(format!("secret:betterleaks.github-pat:{}:3", blob_id(3)))
        );
        assert!(!format!("{findings:?}").contains(&token[4..]));
        assert!(matches!(
            verdict(findings, &[]),
            Verdict::Reject {
                approval_requested: true,
                ..
            }
        ));
    }

    #[test]
    fn a_repository_reads_its_blobs() {
        let dir = tempfile::tempdir().unwrap();
        let init = std::process::Command::new("git")
            .args(["init", "-q", "--bare", "repo.git"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        assert!(init.success());
        let repo =
            gix::open_opts(dir.path().join("repo.git"), gix::open::Options::isolated()).unwrap();
        let id = repo.write_blob(b"\0binary\n").unwrap().detach();

        assert_eq!(repo.read_blob(id).unwrap(), b"\0binary\n");
        assert!(repo.read_blob(blob_id(4)).is_err());
    }
}

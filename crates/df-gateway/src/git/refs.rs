//! Ref rules of a push: which ref updates a sandbox may send upstream.
//!
//! A sandbox may create or fast-forward its task branch and change its WIP branch in any way.
//! Everything else is rejected, and one rejected command rejects the whole push.

use std::fmt;

/// Prefix of the task branch ref of a task key.
#[cfg(test)]
const TASK_BRANCH_PREFIX: &str = "refs/heads/factory/task/";
/// Prefix of the WIP branch ref of a task key.
#[cfg(test)]
const WIP_BRANCH_PREFIX: &str = "refs/heads/factory/wip/";

/// A Git object id: 40 (SHA-1) or 64 (SHA-256) lowercase hex characters.
///
/// The all-zero id means "no object": the old id of a create and the new id of a delete.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ObjectId(String);

/// A string that is not 40 or 64 lowercase hex characters.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("object id must be 40 or 64 lowercase hex characters")]
pub struct InvalidObjectId;

impl ObjectId {
    /// Parses a hex object id as Git sends it in a ref command.
    pub fn parse(hex: &str) -> Result<Self, InvalidObjectId> {
        let valid_len = hex.len() == 40 || hex.len() == 64;
        let lower_hex = hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        if valid_len && lower_hex {
            Ok(Self(hex.to_owned()))
        } else {
            Err(InvalidObjectId)
        }
    }

    /// Whether this is the all-zero id ("no object").
    pub fn is_zero(&self) -> bool {
        self.0.bytes().all(|b| b == b'0')
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One ref update of a push (`<old> <new> <name>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefCommand {
    pub old: ObjectId,
    pub new: ObjectId,
    /// Full ref name, e.g. `refs/heads/factory/task/DF-7`.
    pub name: String,
}

/// The refs a run may push to, as full ref names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefScope {
    /// `refs/heads/factory/task/<KEY>`
    pub task_branch: String,
    /// `refs/heads/factory/wip/<KEY>`
    pub wip_branch: String,
}

impl RefScope {
    /// The scope of the task with the key `key`, e.g. `DF-7`.
    #[cfg(test)]
    pub fn for_task(key: &str) -> Self {
        Self {
            task_branch: format!("{TASK_BRANCH_PREFIX}{key}"),
            wip_branch: format!("{WIP_BRANCH_PREFIX}{key}"),
        }
    }
}

/// Why a ref update was rejected; reported to the Git client as `ng <ref> <reason>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// The ref is outside the run's scope, or the update is a task branch deletion.
    RefNotAllowed,
    /// A task branch update whose old commit is not an ancestor of the new one.
    NonFastForward,
    /// The update is allowed on its own, but another update of the same push was rejected.
    OtherRefRejected,
    /// The push could not be recorded with the control plane (design D6).
    AuditUnavailable,
    /// A push check rejected the update; the `df:` reason comes from the check.
    Check(&'static str),
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RefNotAllowed => "df:ref-not-allowed",
            Self::NonFastForward => "df:non-fast-forward",
            Self::OtherRefRejected => "df:other-ref-rejected",
            Self::AuditUnavailable => "df:audit-unavailable",
            Self::Check(reason) => reason,
        }
    }
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The outcome for one ref update of a push.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefVerdict {
    pub name: String,
    /// `None` if the update is accepted.
    pub reason: Option<Reason>,
}

impl RefVerdict {
    pub fn is_accepted(&self) -> bool {
        self.reason.is_none()
    }
}

/// Commit ancestry lookup, backed by the mirror plus the quarantined pack of the push.
pub trait Ancestry {
    type Error;

    /// Whether `ancestor` is reachable from `descendant` (a commit is its own ancestor).
    fn is_ancestor(&self, ancestor: &ObjectId, descendant: &ObjectId) -> Result<bool, Self::Error>;
}

/// Applies the ref rules to every command of a push and returns one verdict per command, in
/// command order. If any command is rejected, every other command is rejected with
/// [`Reason::OtherRefRejected`].
pub fn check_refs<A: Ancestry>(
    scope: &RefScope,
    commands: &[RefCommand],
    ancestry: &A,
) -> Result<Vec<RefVerdict>, A::Error> {
    let mut verdicts = commands
        .iter()
        .map(|command| {
            Ok(RefVerdict {
                name: command.name.clone(),
                reason: rejection(scope, command, ancestry)?,
            })
        })
        .collect::<Result<Vec<_>, A::Error>>()?;
    if verdicts.iter().any(|verdict| !verdict.is_accepted()) {
        for verdict in &mut verdicts {
            verdict.reason.get_or_insert(Reason::OtherRefRejected);
        }
    }
    Ok(verdicts)
}

/// Why `command` is rejected on its own, if it is.
fn rejection<A: Ancestry>(
    scope: &RefScope,
    command: &RefCommand,
    ancestry: &A,
) -> Result<Option<Reason>, A::Error> {
    if command.name == scope.wip_branch {
        return Ok(None);
    }
    if command.name != scope.task_branch || command.new.is_zero() {
        return Ok(Some(Reason::RefNotAllowed));
    }
    if command.old.is_zero() || ancestry.is_ancestor(&command.old, &command.new)? {
        Ok(None)
    } else {
        Ok(Some(Reason::NonFastForward))
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    #[derive(Debug, PartialEq)]
    struct LookupFailed;

    /// Answers every ancestry lookup with `answer` and records the calls.
    struct FakeAncestry {
        answer: Result<bool, LookupFailed>,
        calls: RefCell<Vec<(ObjectId, ObjectId)>>,
    }

    impl FakeAncestry {
        fn answering(answer: Result<bool, LookupFailed>) -> Self {
            Self {
                answer,
                calls: RefCell::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<(ObjectId, ObjectId)> {
            self.calls.borrow().clone()
        }
    }

    impl Ancestry for FakeAncestry {
        type Error = LookupFailed;

        fn is_ancestor(
            &self,
            ancestor: &ObjectId,
            descendant: &ObjectId,
        ) -> Result<bool, LookupFailed> {
            self.calls
                .borrow_mut()
                .push((ancestor.clone(), descendant.clone()));
            match &self.answer {
                Ok(answer) => Ok(*answer),
                Err(LookupFailed) => Err(LookupFailed),
            }
        }
    }

    fn oid(digit: char) -> ObjectId {
        ObjectId::parse(&digit.to_string().repeat(40)).unwrap()
    }

    fn zero() -> ObjectId {
        oid('0')
    }

    fn command(old: ObjectId, new: ObjectId, name: &str) -> RefCommand {
        RefCommand {
            old,
            new,
            name: name.to_owned(),
        }
    }

    fn scope() -> RefScope {
        RefScope::for_task("DF-7")
    }

    fn reasons(verdicts: &[RefVerdict]) -> Vec<Option<Reason>> {
        verdicts.iter().map(|verdict| verdict.reason).collect()
    }

    #[test]
    fn scope_for_task_holds_full_ref_names() {
        assert_eq!(
            scope(),
            RefScope {
                task_branch: "refs/heads/factory/task/DF-7".to_owned(),
                wip_branch: "refs/heads/factory/wip/DF-7".to_owned(),
            }
        );
    }

    #[test]
    fn push_to_main_is_rejected_as_ref_not_allowed() {
        let ancestry = FakeAncestry::answering(Ok(true));
        let commands = [command(oid('1'), oid('2'), "refs/heads/main")];

        let verdicts = check_refs(&scope(), &commands, &ancestry).unwrap();

        assert_eq!(
            verdicts,
            [RefVerdict {
                name: "refs/heads/main".to_owned(),
                reason: Some(Reason::RefNotAllowed),
            }]
        );
        assert_eq!(
            verdicts[0].reason.map(Reason::as_str),
            Some("df:ref-not-allowed")
        );
    }

    #[test]
    fn task_branch_create_is_allowed_without_ancestry_lookup() {
        let ancestry = FakeAncestry::answering(Ok(false));
        let commands = [command(zero(), oid('1'), "refs/heads/factory/task/DF-7")];

        let verdicts = check_refs(&scope(), &commands, &ancestry).unwrap();

        assert!(verdicts[0].is_accepted());
        assert!(ancestry.calls().is_empty());
    }

    #[test]
    fn fast_forward_task_push_is_allowed() {
        let ancestry = FakeAncestry::answering(Ok(true));
        let commands = [command(oid('1'), oid('2'), "refs/heads/factory/task/DF-7")];

        let verdicts = check_refs(&scope(), &commands, &ancestry).unwrap();

        assert_eq!(reasons(&verdicts), [None]);
        assert_eq!(ancestry.calls(), [(oid('1'), oid('2'))]);
    }

    #[test]
    fn non_fast_forward_task_push_is_rejected() {
        let ancestry = FakeAncestry::answering(Ok(false));
        let commands = [command(oid('1'), oid('2'), "refs/heads/factory/task/DF-7")];

        let verdicts = check_refs(&scope(), &commands, &ancestry).unwrap();

        assert_eq!(reasons(&verdicts), [Some(Reason::NonFastForward)]);
        assert_eq!(
            verdicts[0].reason.map(Reason::as_str),
            Some("df:non-fast-forward")
        );
    }

    #[test]
    fn task_branch_deletion_is_rejected() {
        let ancestry = FakeAncestry::answering(Ok(true));
        let commands = [command(oid('1'), zero(), "refs/heads/factory/task/DF-7")];

        let verdicts = check_refs(&scope(), &commands, &ancestry).unwrap();

        assert_eq!(reasons(&verdicts), [Some(Reason::RefNotAllowed)]);
        assert!(ancestry.calls().is_empty());
    }

    #[test]
    fn wip_force_push_is_allowed_without_ancestry_lookup() {
        let ancestry = FakeAncestry::answering(Ok(false));
        let commands = [command(oid('1'), oid('2'), "refs/heads/factory/wip/DF-7")];

        let verdicts = check_refs(&scope(), &commands, &ancestry).unwrap();

        assert_eq!(reasons(&verdicts), [None]);
        assert!(ancestry.calls().is_empty());
    }

    #[test]
    fn wip_create_and_delete_are_allowed() {
        let ancestry = FakeAncestry::answering(Ok(false));
        let create = [command(zero(), oid('1'), "refs/heads/factory/wip/DF-7")];
        let delete = [command(oid('1'), zero(), "refs/heads/factory/wip/DF-7")];

        let created = check_refs(&scope(), &create, &ancestry).unwrap();
        let deleted = check_refs(&scope(), &delete, &ancestry).unwrap();

        assert_eq!(reasons(&created), [None]);
        assert_eq!(reasons(&deleted), [None]);
        assert!(ancestry.calls().is_empty());
    }

    #[test]
    fn refs_of_another_task_are_rejected() {
        let ancestry = FakeAncestry::answering(Ok(true));
        let commands = [
            command(zero(), oid('1'), "refs/heads/factory/task/DF-8"),
            command(oid('1'), oid('2'), "refs/heads/factory/wip/DF-8"),
        ];

        let verdicts = check_refs(&scope(), &commands, &ancestry).unwrap();

        assert_eq!(
            reasons(&verdicts),
            [Some(Reason::RefNotAllowed), Some(Reason::RefNotAllowed)]
        );
    }

    #[test]
    fn mixed_push_rejects_every_update() {
        let ancestry = FakeAncestry::answering(Ok(true));
        let commands = [
            command(oid('1'), oid('2'), "refs/heads/factory/task/DF-7"),
            command(zero(), oid('3'), "refs/tags/v1"),
        ];

        let verdicts = check_refs(&scope(), &commands, &ancestry).unwrap();

        assert_eq!(
            verdicts,
            [
                RefVerdict {
                    name: "refs/heads/factory/task/DF-7".to_owned(),
                    reason: Some(Reason::OtherRefRejected),
                },
                RefVerdict {
                    name: "refs/tags/v1".to_owned(),
                    reason: Some(Reason::RefNotAllowed),
                },
            ]
        );
        assert_eq!(Reason::OtherRefRejected.as_str(), "df:other-ref-rejected");
    }

    #[test]
    fn ancestry_error_propagates() {
        let ancestry = FakeAncestry::answering(Err(LookupFailed));
        let commands = [command(oid('1'), oid('2'), "refs/heads/factory/task/DF-7")];

        assert_eq!(
            check_refs(&scope(), &commands, &ancestry),
            Err(LookupFailed)
        );
    }

    #[test]
    fn object_id_accepts_sha1_and_sha256_lowercase_hex() {
        let sha1 = "0123456789abcdef0123456789abcdef01234567";
        let sha256 = "0123456789abcdef".repeat(4);

        assert_eq!(ObjectId::parse(sha1).unwrap().as_str(), sha1);
        assert_eq!(ObjectId::parse(&sha256).unwrap().as_str(), sha256);
        assert!(ObjectId::parse(&"0".repeat(64)).unwrap().is_zero());
        assert!(!oid('1').is_zero());
    }

    #[test]
    fn object_id_rejects_uppercase_wrong_length_and_non_hex() {
        for invalid in [
            "0123456789ABCDEF0123456789ABCDEF01234567".to_owned(),
            "0".repeat(39),
            "0".repeat(41),
            "0".repeat(63),
            String::new(),
            "g".repeat(40),
            format!("{} ", "0".repeat(39)),
        ] {
            assert_eq!(
                ObjectId::parse(&invalid),
                Err(InvalidObjectId),
                "{invalid:?}"
            );
        }
    }
}

//! Pure CI verdict gate; validates statuses and commit binding, not network APIs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status { Success, Failure, Pending, Skipped }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateError { MissingChecks, StaleCommit, Failed, Pending, Skipped }
pub fn require_all(required: &[Status], checked_commit: &str, target_commit: &str)
    -> Result<(), GateError>
{
    if checked_commit.is_empty() || checked_commit != target_commit {
        return Err(GateError::StaleCommit);
    }
    if required.is_empty() { return Err(GateError::MissingChecks); }
    // Denial is deterministic and does not depend on result ordering.
    for status in required {
        if *status == Status::Failure { return Err(GateError::Failed); }
    }
    for status in required {
        if *status == Status::Pending { return Err(GateError::Pending); }
    }
    for status in required {
        if *status == Status::Skipped { return Err(GateError::Skipped); }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_same_commit_all_green() {
        assert_eq!(require_all(&[Status::Success, Status::Success], "abc", "abc"), Ok(()));
    }
    #[test]
    fn refuses_failed_skipped_and_pending() {
        assert_eq!(require_all(&[Status::Success, Status::Failure], "abc", "abc"),
            Err(GateError::Failed));
        assert_eq!(require_all(&[Status::Skipped], "abc", "abc"), Err(GateError::Skipped));
        assert_eq!(require_all(&[Status::Pending], "abc", "abc"), Err(GateError::Pending));
    }
    #[test]
    fn no_green_checks_and_stale_sha_are_not_approval() {
        assert_eq!(require_all(&[], "abc", "abc"), Err(GateError::MissingChecks));
        assert_eq!(require_all(&[Status::Success], "abc", "def"), Err(GateError::StaleCommit));
        assert_eq!(require_all(&[Status::Success], "", ""), Err(GateError::StaleCommit));
    }
}

/// A merged change must have every named mandatory check on exactly the
/// target commit, with no duplicate or unexpected check names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NamedCheck<'a> { pub name: &'a str, pub sha: &'a str, pub status: Status }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamedGateError { Missing, Duplicate, Unexpected, Stale, Incomplete }
pub fn require_named_checks(checks: &[NamedCheck<'_>], required: &[&str],
    target_sha: &str) -> Result<(), NamedGateError>
{
    if target_sha.is_empty() || required.is_empty() { return Err(NamedGateError::Missing); }
    for (i, name) in required.iter().enumerate() {
        if name.is_empty() { return Err(NamedGateError::Missing); }
        if required[..i].contains(name) { return Err(NamedGateError::Duplicate); }
    }
    for (i, check) in checks.iter().enumerate() {
        if check.name.is_empty() || !required.contains(&check.name) {
            return Err(NamedGateError::Unexpected);
        }
        if checks[..i].iter().any(|old| old.name == check.name) {
            return Err(NamedGateError::Duplicate);
        }
        if check.sha != target_sha { return Err(NamedGateError::Stale); }
        if check.status != Status::Success { return Err(NamedGateError::Incomplete); }
    }
    if checks.len() != required.len() { return Err(NamedGateError::Missing); }
    Ok(())
}
#[cfg(test)]
mod named_gate_tests {
    use super::*;
    fn check<'a>(name: &'a str, sha: &'a str, status: Status) -> NamedCheck<'a> {
        NamedCheck { name, sha, status }
    }
    #[test]
    fn exact_required_checks_are_accepted() {
        assert_eq!(require_named_checks(&[check("kernel","a",Status::Success),
            check("source","a",Status::Success)], &["kernel","source"], "a"), Ok(()));
    }
    #[test]
    fn missing_skipped_and_stale_reject_merge() {
        assert_eq!(require_named_checks(&[], &["kernel"], "a"), Err(NamedGateError::Missing));
        assert_eq!(require_named_checks(&[check("kernel","a",Status::Skipped)],
            &["kernel"],"a"), Err(NamedGateError::Incomplete));
        assert_eq!(require_named_checks(&[check("kernel","old",Status::Success)],
            &["kernel"],"a"), Err(NamedGateError::Stale));
    }
    #[test]
    fn duplicate_and_unknown_checks_cannot_replace_required() {
        assert_eq!(require_named_checks(&[check("kernel","a",Status::Success),
            check("kernel","a",Status::Success)], &["kernel","source"],"a"),
            Err(NamedGateError::Duplicate));
        assert_eq!(require_named_checks(&[check("other","a",Status::Success)],
            &["kernel"],"a"), Err(NamedGateError::Unexpected));
    }
}

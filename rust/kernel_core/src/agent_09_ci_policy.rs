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

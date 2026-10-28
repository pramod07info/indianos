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


// R007: fail-closed integration policy. Inputs must be read from GitHub's
// authenticated API; this is not a signature verifier or CI runner.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct ReviewerEvidence<'a>{
    pub reviewer:&'a str,pub sha:&'a str,pub approved:bool,
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum ReviewGateError{
    Checks(NamedGateError),Missing,SelfReview,Duplicate,Stale,Rejected,
}
pub fn require_reviewed_merge(checks:&[NamedCheck<'_>],required:&[&str],
    target_sha:&str,author:&str,reviews:&[ReviewerEvidence<'_>],
    minimum_independent:usize)->Result<usize,ReviewGateError>{
    require_named_checks(checks,required,target_sha).map_err(ReviewGateError::Checks)?;
    if author.is_empty()||minimum_independent==0||minimum_independent>10||
       reviews.len()>20{return Err(ReviewGateError::Missing);}
    for (i,review) in reviews.iter().enumerate(){
        if review.reviewer.is_empty(){return Err(ReviewGateError::Missing);}
        if review.sha!=target_sha{return Err(ReviewGateError::Stale);}
        if review.reviewer==author{return Err(ReviewGateError::SelfReview);}
        if !review.approved{return Err(ReviewGateError::Rejected);}
        if reviews[..i].iter().any(|old|old.reviewer==review.reviewer){
            return Err(ReviewGateError::Duplicate);
        }
    }
    if reviews.len()<minimum_independent{return Err(ReviewGateError::Missing);}
    Ok(reviews.len())
}
#[cfg(test)]mod r007_review_gate_tests{
    use super::*;
    fn checks()->[NamedCheck<'static>;2]{
        [NamedCheck{name:"kernel",sha:"abc",status:Status::Success},
         NamedCheck{name:"security",sha:"abc",status:Status::Success}]
    }
    fn review(user:&'static str,sha:&'static str,approved:bool)
       ->ReviewerEvidence<'static>{
        ReviewerEvidence{reviewer:user,sha,approved}
    }
    fn check(reviews:&[ReviewerEvidence<'_>],min:usize)
       ->Result<usize,ReviewGateError>{
        require_reviewed_merge(&checks(),&["kernel","security"],"abc",
            "author",reviews,min)
    }
    #[test]fn two_independent_approvals_bound_to_exact_sha(){
        assert_eq!(check(&[review("alice","abc",true),
            review("bob","abc",true)],2),Ok(2));
    }
    #[test]fn cannot_self_approve_duplicate_or_reuse_stale_sha(){
        assert_eq!(check(&[review("author","abc",true)],1),Err(ReviewGateError::SelfReview));
        assert_eq!(check(&[review("alice","old",true)],1),Err(ReviewGateError::Stale));
        assert_eq!(check(&[review("alice","abc",true),
            review("alice","abc",true)],2),Err(ReviewGateError::Duplicate));
    }
    #[test]fn pending_checks_and_changes_requested_block_approval(){
        assert_eq!(check(&[review("alice","abc",false)],1),Err(ReviewGateError::Rejected));
        assert_eq!(check(&[review("alice","abc",true)],2),Err(ReviewGateError::Missing));
        let pending=[NamedCheck{name:"kernel",sha:"abc",status:Status::Pending},
            NamedCheck{name:"security",sha:"abc",status:Status::Success}];
        assert_eq!(require_reviewed_merge(&pending,&["kernel","security"],"abc",
            "author",&[review("alice","abc",true)],1),
            Err(ReviewGateError::Checks(NamedGateError::Incomplete)));
    }
}


// R008: release provenance binds the tested source, reviewed source and
// release artifact to the same immutable SHA. External API authenticity is
// required; caller-supplied strings are not signed attestations.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct BuildArtifact<'a>{
 pub name:&'a str,pub source_sha:&'a str,pub digest:&'a str,
 pub checks_passed:bool,
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum ReleaseFault{Invalid,Missing,Duplicate,Stale,Failed,Review(ReviewGateError)}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct ReleaseVerdict{pub artifacts:usize,pub reviews:usize}
pub fn check_release<'a>(target:&str,author:&str,
 checks:&[NamedCheck<'a>],required_checks:&[&str],
 reviews:&[ReviewerEvidence<'a>],required_reviewers:usize,
 artifacts:&[BuildArtifact<'a>],required_artifacts:&[&str])
 ->Result<ReleaseVerdict,ReleaseFault>{
 let reviews_count=require_reviewed_merge(checks,required_checks,target,author,
  reviews,required_reviewers).map_err(ReleaseFault::Review)?;
 if target.is_empty()||required_artifacts.is_empty()||
  artifacts.len()!=required_artifacts.len()||artifacts.len()>16{
  return Err(ReleaseFault::Missing)
 }
 for (i,artifact) in artifacts.iter().enumerate(){
  if artifact.name.is_empty()||artifact.digest.len()<16||
   artifact.digest.bytes().any(|c|!c.is_ascii_hexdigit()){
   return Err(ReleaseFault::Invalid)
  }
  if artifact.source_sha!=target{return Err(ReleaseFault::Stale)}
  if !artifact.checks_passed{return Err(ReleaseFault::Failed)}
  if artifacts[..i].iter().any(|p|p.name==artifact.name){
   return Err(ReleaseFault::Duplicate)
  }
  if !required_artifacts.contains(&artifact.name){return Err(ReleaseFault::Invalid)}
 }
 for required in required_artifacts{
  if !artifacts.iter().any(|a|a.name==*required){return Err(ReleaseFault::Missing)}
 }
 Ok(ReleaseVerdict{artifacts:artifacts.len(),reviews:reviews_count})
}
pub fn allow_promote(old_version:u64,next_version:u64,rollback_floor:u64,
 release:ReleaseVerdict)->Result<u64,ReleaseFault>{
 if next_version<=old_version||next_version<rollback_floor||
  release.artifacts==0||release.reviews==0{return Err(ReleaseFault::Invalid)}
 Ok(next_version)
}
#[cfg(test)]mod r008_release_provenance_tests{
 use super::*;
 fn check()->[NamedCheck<'static>;2]{[
  NamedCheck{name:"kernel",sha:"abc",status:Status::Success},
  NamedCheck{name:"security",sha:"abc",status:Status::Success}
 ]}
 fn review()->[ReviewerEvidence<'static>;1]{[
  ReviewerEvidence{reviewer:"independent",sha:"abc",approved:true}
 ]}
 fn artifacts()->[BuildArtifact<'static>;2]{[
  BuildArtifact{name:"kernel-elf",source_sha:"abc",
   digest:"abcdeffedcba1234",checks_passed:true},
  BuildArtifact{name:"qemu-report",source_sha:"abc",
   digest:"0123456789abcdef",checks_passed:true}
 ]}
 fn run<'a>(artifacts:&[BuildArtifact<'a>])->Result<ReleaseVerdict,ReleaseFault>{
  check_release("abc","author",&check(),&["kernel","security"],
   &review(),1,artifacts,&["kernel-elf","qemu-report"])
 }
 #[test]fn all_green_exact_sha_reviewed_artifacts_pass(){
  assert_eq!(run(&artifacts()),Ok(ReleaseVerdict{artifacts:2,reviews:1}));
 }
 #[test]fn stale_or_duplicated_provenance_is_blocked(){
  let mut data=artifacts();data[0].source_sha="old";
  assert_eq!(run(&data),Err(ReleaseFault::Stale));
  data=artifacts();data[1].name="kernel-elf";
  assert_eq!(run(&data),Err(ReleaseFault::Duplicate));
 }
 #[test]fn unsigned_fake_digest_or_failed_attestation_is_blocked(){
  let mut data=artifacts();data[0].digest="unsigned";
  assert_eq!(run(&data),Err(ReleaseFault::Invalid));
  data=artifacts();data[0].checks_passed=false;
  assert_eq!(run(&data),Err(ReleaseFault::Failed));
  assert_eq!(allow_promote(10,10,9,ReleaseVerdict{artifacts:2,reviews:1}),
   Err(ReleaseFault::Invalid));
 }
}

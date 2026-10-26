//! R037/30: exact-SHA fail-closed release/security evidence gate.
//! This cannot replace an external reviewer or independent security test.
pub const REQUIRED_CONTROL_COUNT:usize=7;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Control{MemorySafety,ProcessIsolation,PackageTrust,
    DriverIsolation,UpdateRollback,DependencyPolicy,MainQemuBuild}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum ResultState{Passed,Failed,NotRun}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Finding{pub control:Control,pub revision:[u8;20],pub state:ResultState}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum AuditError{InvalidRevision,Incomplete,Duplicate,Failed,Unreviewed}
fn index(control:Control)->usize{match control{
    Control::MemorySafety=>0,Control::ProcessIsolation=>1,
    Control::PackageTrust=>2,Control::DriverIsolation=>3,
    Control::UpdateRollback=>4,Control::DependencyPolicy=>5,
    Control::MainQemuBuild=>6
}}
pub fn audit(findings:&[Finding],sha:[u8;20],independent_review:bool)
    ->Result<(),AuditError>{
    if sha==[0;20]{return Err(AuditError::InvalidRevision);}
    if findings.len()!=REQUIRED_CONTROL_COUNT{return Err(AuditError::Incomplete);}
    let mut seen=[false;REQUIRED_CONTROL_COUNT];
    for item in findings{
        if item.revision!=sha{return Err(AuditError::InvalidRevision);}
        let id=index(item.control);
        if seen[id]{return Err(AuditError::Duplicate);}
        seen[id]=true;
        if item.state!=ResultState::Passed{return Err(AuditError::Failed);}
    }
    if seen.iter().any(|&found|!found){return Err(AuditError::Incomplete);}
    if !independent_review{return Err(AuditError::Unreviewed);}
    Ok(())
}
#[cfg(test)]
mod tests{
    use super::*;
    fn all()->[Finding;7]{
        let c=[Control::MemorySafety,Control::ProcessIsolation,
            Control::PackageTrust,Control::DriverIsolation,
            Control::UpdateRollback,Control::DependencyPolicy,Control::MainQemuBuild];
        core::array::from_fn(|i|Finding{
            control:c[i],revision:[1;20],state:ResultState::Passed,
        })
    }
    #[test]fn complete_reviewed_evidence_only(){
        assert_eq!(audit(&all(),[1;20],true),Ok(()));
        assert_eq!(audit(&all(),[1;20],false),Err(AuditError::Unreviewed));
    }
    #[test]fn rejects_stale_duplicate_or_missing_checks(){
        assert_eq!(audit(&all(),[2;20],true),Err(AuditError::InvalidRevision));
        let mut duplicated=all();duplicated[1]=duplicated[0];
        assert_eq!(audit(&duplicated,[1;20],true),Err(AuditError::Duplicate));
        assert_eq!(audit(&all()[..6],[1;20],true),Err(AuditError::Incomplete));
    }
    #[test]fn failing_checks_cannot_be_green(){
        let mut checks=all();checks[4].state=ResultState::NotRun;
        assert_eq!(audit(&checks,[1;20],true),Err(AuditError::Failed));
    }
}

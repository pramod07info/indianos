//! Offline app-install policy decision. No download, install or privilege changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Architecture { X86_64, AArch64 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub catalog_trusted: bool,
    pub manifest_signed: bool,
    pub architecture: Architecture,
    pub requested_permissions: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallError { UntrustedCatalog, Unsigned, ArchitectureMismatch, PermissionDenied }

/// Explicit consent is mandatory even if no special permissions are requested.
pub fn approve(candidate: Candidate, host: Architecture, granted: u32, consent: bool)
    -> Result<(), InstallError>
{
    if !candidate.catalog_trusted { return Err(InstallError::UntrustedCatalog); }
    if !candidate.manifest_signed { return Err(InstallError::Unsigned); }
    if candidate.architecture != host { return Err(InstallError::ArchitectureMismatch); }
    if !consent || (candidate.requested_permissions & granted) != candidate.requested_permissions {
        return Err(InstallError::PermissionDenied);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn candidate() -> Candidate {
        Candidate { catalog_trusted: true, manifest_signed: true,
            architecture: Architecture::X86_64, requested_permissions: 0b011 }
    }
    #[test]
    fn authorized_install_policy_passes() {
        assert_eq!(approve(candidate(), Architecture::X86_64, 0b111, true), Ok(()));
    }
    #[test]
    fn denies_missing_signature_catalog_and_architecture() {
        let mut c = candidate();
        c.catalog_trusted = false;
        assert_eq!(approve(c, Architecture::X86_64, 7, true), Err(InstallError::UntrustedCatalog));
        c.catalog_trusted = true; c.manifest_signed = false;
        assert_eq!(approve(c, Architecture::X86_64, 7, true), Err(InstallError::Unsigned));
        c.manifest_signed = true;
        assert_eq!(approve(c, Architecture::AArch64, 7, true), Err(InstallError::ArchitectureMismatch));
    }
    #[test]
    fn permission_mask_and_explicit_consent_are_required() {
        assert_eq!(approve(candidate(), Architecture::X86_64, 0b001, true),
            Err(InstallError::PermissionDenied));
        assert_eq!(approve(candidate(), Architecture::X86_64, u32::MAX, false),
            Err(InstallError::PermissionDenied));
    }
}

/// Evaluate the root application and every declared dependency as a unit.
/// Only a caller-held consent bit and permission mask can authorize the plan.
pub fn approve_plan(candidates: &[Candidate], host: Architecture,
    granted: u32, consent: bool) -> Result<u32, InstallError>
{
    if candidates.is_empty() { return Err(InstallError::PermissionDenied); }
    let mut requested = 0u32;
    for &candidate in candidates {
        approve(candidate, host, granted, consent)?;
        requested |= candidate.requested_permissions;
    }
    Ok(requested)
}
#[cfg(test)]
mod dependency_tests {
    use super::*;
    fn candidate(permissions: u32) -> Candidate {
        Candidate { catalog_trusted: true, manifest_signed: true,
            architecture: Architecture::X86_64, requested_permissions: permissions }
    }
    #[test]
    fn returns_complete_preapproved_permission_union() {
        assert_eq!(approve_plan(&[candidate(1), candidate(2)],
            Architecture::X86_64, 3, true), Ok(3));
    }
    #[test]
    fn unapproved_dependency_blocks_entire_plan() {
        assert_eq!(approve_plan(&[candidate(1), candidate(4)],
            Architecture::X86_64, 3, true), Err(InstallError::PermissionDenied));
    }
    #[test]
    fn unsigned_or_empty_plan_does_not_install() {
        let mut unverified = candidate(1);
        unverified.manifest_signed = false;
        assert_eq!(approve_plan(&[candidate(1),unverified],Architecture::X86_64,1,true),
            Err(InstallError::Unsigned));
        assert_eq!(approve_plan(&[],Architecture::X86_64,0,true),
            Err(InstallError::PermissionDenied));
    }
}


/// Batch 1: bounded dependency graph over a trusted, immutable catalog.
/// This produces an atomic dry-run install order, not a package installer.
pub const MAX_DEPENDENCY_NODES:usize=32;
#[derive(Clone,Copy)]
pub struct Dependency<'a>{
    pub id:u16, pub dependencies:&'a[u16], pub candidate:Candidate,
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum PlanError{Empty,TooMany,Duplicate,Unknown,Cycle,OutputFull,Policy(InstallError)}
fn visit_dependency(index:usize,items:&[Dependency<'_>],
    state:&mut[u8;MAX_DEPENDENCY_NODES],order:&mut[u16;MAX_DEPENDENCY_NODES],
    count:&mut usize,host:Architecture,permissions:u32,consent:bool)
    ->Result<(),PlanError> {
    if state[index]==1{return Err(PlanError::Cycle);}
    if state[index]==2{return Ok(());}
    state[index]=1;
    let entry=items[index];
    approve(entry.candidate,host,permissions,consent).map_err(PlanError::Policy)?;
    for dependency in entry.dependencies {
        let next=items.iter().position(|d|d.id==*dependency)
            .ok_or(PlanError::Unknown)?;
        visit_dependency(next,items,state,order,count,host,permissions,consent)?;
    }
    state[index]=2;
    order[*count]=entry.id;
    *count+=1;
    Ok(())
}
pub fn plan_dependencies(items:&[Dependency<'_>],root:u16,host:Architecture,
    granted_permissions:u32,consent:bool,out:&mut[u16])->Result<usize,PlanError>{
    if items.is_empty()||root==0{return Err(PlanError::Empty);}
    if items.len()>MAX_DEPENDENCY_NODES{return Err(PlanError::TooMany);}
    for (index,item) in items.iter().enumerate(){
        if item.id==0{return Err(PlanError::Empty);}
        if items[..index].iter().any(|prev|prev.id==item.id){
            return Err(PlanError::Duplicate);
        }
    }
    let index=items.iter().position(|d|d.id==root).ok_or(PlanError::Unknown)?;
    let mut state=[0u8;MAX_DEPENDENCY_NODES];
    let mut order=[0u16;MAX_DEPENDENCY_NODES];
    let mut count=0;
    visit_dependency(index,items,&mut state,&mut order,&mut count,
        host,granted_permissions,consent)?;
    if count>out.len(){return Err(PlanError::OutputFull);}
    out[..count].copy_from_slice(&order[..count]);
    Ok(count)
}
#[cfg(test)]
mod batch1_dependency_graph_tests{
    use super::*;
    fn c(mask:u32)->Candidate{
        Candidate{catalog_trusted:true,manifest_signed:true,
            architecture:Architecture::X86_64,requested_permissions:mask}
    }
    #[test]
    fn sorts_dependencies_before_root_and_checks_permissions(){
        let items=[
            Dependency{id:1,dependencies:&[2,3],candidate:c(1)},
            Dependency{id:2,dependencies:&[3],candidate:c(2)},
            Dependency{id:3,dependencies:&[],candidate:c(4)},
        ];
        let mut out=[0;3];
        assert_eq!(plan_dependencies(&items,1,Architecture::X86_64,7,true,&mut out),Ok(3));
        assert_eq!(out,[3,2,1]);
        assert_eq!(plan_dependencies(&items,1,Architecture::X86_64,3,true,&mut out),
            Err(PlanError::Policy(InstallError::PermissionDenied)));
    }
    #[test]
    fn cycles_unknown_and_duplicate_ids_are_rejected(){
        let cycle=[
            Dependency{id:1,dependencies:&[2],candidate:c(0)},
            Dependency{id:2,dependencies:&[1],candidate:c(0)},
        ];
        assert_eq!(plan_dependencies(&cycle,1,Architecture::X86_64,0,true,&mut[0;2]),
            Err(PlanError::Cycle));
        let unknown=[Dependency{id:1,dependencies:&[9],candidate:c(0)}];
        assert_eq!(plan_dependencies(&unknown,1,Architecture::X86_64,0,true,&mut[0;2]),
            Err(PlanError::Unknown));
        let duplicate=[
            Dependency{id:1,dependencies:&[],candidate:c(0)},
            Dependency{id:1,dependencies:&[],candidate:c(0)},
        ];
        assert_eq!(plan_dependencies(&duplicate,1,Architecture::X86_64,0,true,&mut[0;2]),
            Err(PlanError::Duplicate));
    }
    #[test]
    fn insufficient_buffer_and_untrusted_dependency_preserve_output(){
        let items=[
            Dependency{id:1,dependencies:&[2],candidate:c(0)},
            Dependency{id:2,dependencies:&[],candidate:c(0)}
        ];
        let mut out=[77u16;1];
        assert_eq!(plan_dependencies(&items,1,Architecture::X86_64,0,true,&mut out),
            Err(PlanError::OutputFull));
        assert_eq!(out,[77]);
        let mut untrusted=c(0);untrusted.catalog_trusted=false;
        let bad=[Dependency{id:1,dependencies:&[],candidate:untrusted}];
        assert_eq!(plan_dependencies(&bad,1,Architecture::X86_64,0,true,&mut out),
            Err(PlanError::Policy(InstallError::UntrustedCatalog)));
        assert_eq!(out,[77]);
    }
}


// R007: multiple application roots planned as one dependency-ordered,
// permission-checked dry run. A separate trusted installer must persist
// signed payloads, perform compatibility checks and execute transactions.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct MultiAppPlan{pub count:usize,pub requested_permissions:u32}
pub fn plan_multi_roots(items:&[Dependency<'_>],roots:&[u16],
    host:Architecture,granted:u32,consent:bool,
    output:&mut[u16])->Result<MultiAppPlan,PlanError>{
    if roots.is_empty()||roots.len()>MAX_DEPENDENCY_NODES{
        return Err(PlanError::Empty);
    }
    if items.len()>MAX_DEPENDENCY_NODES{return Err(PlanError::TooMany);}
    let mut ordered=[0u16;MAX_DEPENDENCY_NODES];
    let mut total=0usize;
    let mut permissions=0u32;
    for (i,root) in roots.iter().enumerate(){
        if *root==0{return Err(PlanError::Empty);}
        if roots[..i].contains(root){return Err(PlanError::Duplicate);}
        let mut scratch=[0u16;MAX_DEPENDENCY_NODES];
        let n=plan_dependencies(items,*root,host,granted,consent,&mut scratch)?;
        for &id in &scratch[..n]{
            if !ordered[..total].contains(&id){
                let entry=items.iter().find(|node|node.id==id)
                    .ok_or(PlanError::Unknown)?;
                permissions|=entry.candidate.requested_permissions;
                ordered[total]=id;total+=1;
            }
        }
    }
    if total>output.len(){return Err(PlanError::OutputFull);}
    output[..total].copy_from_slice(&ordered[..total]);
    Ok(MultiAppPlan{count:total,requested_permissions:permissions})
}
#[cfg(test)]mod r007_multi_application_tests{
    use super::*;
    fn c(rights:u32)->Candidate{
        Candidate{catalog_trusted:true,manifest_signed:true,
            architecture:Architecture::X86_64,requested_permissions:rights}
    }
    #[test]fn shared_dependencies_are_installed_once_before_multiple_apps(){
        let items=[
            Dependency{id:1,dependencies:&[3],candidate:c(1)},
            Dependency{id:2,dependencies:&[3],candidate:c(2)},
            Dependency{id:3,dependencies:&[],candidate:c(4)}];
        let mut out=[0u16;3];
        assert_eq!(plan_multi_roots(&items,&[1,2],Architecture::X86_64,7,
            true,&mut out),Ok(MultiAppPlan{count:3,requested_permissions:7}));
        assert_eq!(out,[3,1,2]);
    }
    #[test]fn failed_permission_or_small_buffer_does_not_write_output(){
        let items=[
            Dependency{id:1,dependencies:&[2],candidate:c(1)},
            Dependency{id:2,dependencies:&[],candidate:c(4)}];
        let mut out=[77u16;1];
        assert_eq!(plan_multi_roots(&items,&[1],Architecture::X86_64,7,
            true,&mut out),Err(PlanError::OutputFull));
        assert_eq!(out,[77]);
        assert_eq!(plan_multi_roots(&items,&[1],Architecture::X86_64,1,
            true,&mut out),
            Err(PlanError::Policy(InstallError::PermissionDenied)));
        assert_eq!(out,[77]);
    }
    #[test]fn duplicate_roots_and_unknown_dependencies_fail_closed(){
        let items=[Dependency{id:1,dependencies:&[4],candidate:c(0)}];
        assert_eq!(plan_multi_roots(&items,&[1],Architecture::X86_64,0,
            true,&mut [0;2]),Err(PlanError::Unknown));
        assert_eq!(plan_multi_roots(&items,&[1,1],Architecture::X86_64,0,
            true,&mut [0;2]),Err(PlanError::Unknown));
        let safe=[Dependency{id:1,dependencies:&[],candidate:c(0)}];
        assert_eq!(plan_multi_roots(&safe,&[1,1],Architecture::X86_64,0,
            true,&mut [0;2]),Err(PlanError::Duplicate));
    }
}

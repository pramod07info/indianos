//! Persistent discretionary owner/group/other permissions. Credentials are
//! issued by the kernel; the native ABI never accepts a caller-supplied UID.
pub const READ:u16=4;
pub const WRITE:u16=2;
pub const SEARCH:u16=1;
#[cfg(test)]
mod tests{
 use super::*;
 #[test]fn owner_group_other_and_root_are_distinct(){
  let p=Permissions::new(1000,42,0o640).unwrap();
  assert!(p.allows(Credentials::kernel(1000,7),READ|WRITE));
  assert!(p.allows(Credentials::kernel(2000,42),READ));
  assert!(!p.allows(Credentials::kernel(2000,42),WRITE));
  assert!(!p.allows(Credentials::kernel(2000,7),READ));
  assert!(p.allows(Credentials::kernel(0,7),READ|WRITE|SEARCH));
  assert!(!p.allows(Credentials::kernel(0,7),8));
  assert!(Permissions::new(1,1,0o1000).is_none());
 }
}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct Credentials{uid:u16,gid:u16}
impl Credentials{
 pub(crate) const fn kernel(uid:u16,gid:u16)->Self{Self{uid,gid}}
 pub const fn uid(self)->u16{self.uid}
 pub const fn gid(self)->u16{self.gid}
}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct Permissions{pub owner:u16,pub group:u16,pub mode:u16}
impl Permissions{
 /// Legacy v1 volumes had no discretionary permissions. Upgrade preserves
 /// their existing access until an authorized owner explicitly restricts it.
 pub const LEGACY:Self=Self{owner:0,group:0,mode:0o777};
 pub fn new(owner:u16,group:u16,mode:u16)->Option<Self>{
  if mode&!0o777!=0{None}else{Some(Self{owner,group,mode})}
 }
 /// Restrict or update mode only through a validated kernel-issued owner/root
 /// credential; a forged public mode must not be laundered into a valid ACL.
 pub fn with_mode(self,credentials:Credentials,mode:u16)->Option<Self>{
  if self.mode&!0o777!=0||mode&!0o777!=0||
    (credentials.uid!=0&&credentials.uid!=self.owner){return None;}
  Some(Self{mode,..self})
 }
 pub fn allows(self,credentials:Credentials,access:u16)->bool{
  // Corrupt persisted modes must never grant access, even to root.
  if self.mode&!0o777!=0||access&!7!=0{return false;}
  if credentials.uid==0{return true;}
  let shift=if credentials.uid==self.owner{6}else if credentials.gid==self.group{3}else{0};
  ((self.mode>>shift)&access)==access
 }
}

#[cfg(test)]
mod g7_acl_integrity_tests {
 use super::*;
 #[test] fn forged_modes_fail_closed_even_for_root() {
  for mode in [0o1000,0o1777,0o7777,u16::MAX] {
   assert!(Permissions::new(1000,77,mode).is_none());
   let forged=Permissions{owner:1000,group:77,mode};
   for who in [Credentials::kernel(0,0),Credentials::kernel(1000,9),
               Credentials::kernel(99,77),Credentials::kernel(99,8)] {
    for access in [0,READ,WRITE,SEARCH,READ|WRITE|SEARCH] {
     assert!(!forged.allows(who,access));
    }
   }
  }
 }
 #[test] fn valid_acl_and_unknown_access_flags_preserved() {
  let p=Permissions::new(1000,77,0o750).unwrap();
  assert!(p.allows(Credentials::kernel(1000,8),READ|WRITE|SEARCH));
  assert!(p.allows(Credentials::kernel(33,77),READ|SEARCH));
  assert!(!p.allows(Credentials::kernel(33,77),WRITE));
  assert!(!p.allows(Credentials::kernel(33,8),SEARCH));
  assert!(p.allows(Credentials::kernel(0,8),READ|WRITE|SEARCH));
  for who in [Credentials::kernel(0,0),Credentials::kernel(1000,77)] {
   assert!(!p.allows(who,8));
  }
 }
}

#[cfg(test)]
mod m1_ws07_acl_mutation_tests {
 use super::*;
 #[test] fn only_owner_or_root_can_change_valid_discretionary_modes() {
  let p=Permissions::new(100,7,0o640).unwrap();
  let owner=Credentials::kernel(100,1);
  let group=Credentials::kernel(101,7);
  assert_eq!(p.with_mode(owner,0o600),Some(Permissions{mode:0o600,..p}));
  assert_eq!(p.with_mode(Credentials::kernel(0,5),0o400),Some(Permissions{mode:0o400,..p}));
  assert_eq!(p.with_mode(group,0o777),None);
  assert_eq!(p.with_mode(owner,0o1000),None);
  assert_eq!(Permissions{mode:0o1777,..p}.with_mode(owner,0o600),None);
 }
}


// PR782 WS78-81: trusted owner/group policy transitions for persisted files.
// Caller MUST obtain Credentials from the kernel, not from a guest request.
impl Permissions {
 /// WS78: Ownership changes require root and a valid existing persisted ACL.
 /// This does not change any actual filesystem metadata until a caller commits.
 pub fn with_owner(self,caller:Credentials,new_owner:u16)->Option<Self>{
  if self.mode&!0o777!=0||caller.uid()!=0{return None;}
  Some(Self{owner:new_owner,..self})
 }
 /// WS79: An ordinary owner can only select the caller's own group; root
 /// can choose any group. A forged ACL or untrusted caller is never accepted.
 pub fn with_group(self,caller:Credentials,new_group:u16)->Option<Self>{
  if self.mode&!0o777!=0{return None;}
  if caller.uid()!=0&&(caller.uid()!=self.owner||new_group!=caller.gid()){
   return None;
  }
  Some(Self{group:new_group,..self})
 }
 /// WS80: Restriction-only chmod; no principal class may gain new rights.
 pub fn restrict_mode(self,caller:Credentials,mode:u16)->Option<Self>{
  if self.mode&!0o777!=0||mode&!0o777!=0||
   (mode&!self.mode)!=0{return None;}
  self.with_mode(caller,mode)
 }
 /// WS81: Walk-to-leaf authorization: every ancestor requires SEARCH.
 /// Empty ancestor lists represent a direct root-directory child.
 pub fn authorize_path(ancestors:&[Permissions],leaf:Permissions,
  caller:Credentials,requested:u16)->bool{
  if requested==0||requested&!7!=0{return false;}
  ancestors.iter().all(|dir|dir.allows(caller,SEARCH))&&
   leaf.allows(caller,requested)
 }
}
#[cfg(test)]
mod pr782_ws78_81_tests{
 use super::*;
 #[test]fn ws78_only_trusted_root_can_change_file_owner(){
  let p=Permissions::new(100,7,0o640).unwrap();
  assert_eq!(p.with_owner(Credentials::kernel(100,7),200),None);
  assert_eq!(p.with_owner(Credentials::kernel(0,0),200),
   Some(Permissions{owner:200,..p}));
  let corrupt=Permissions{mode:0o1777,..p};
  assert_eq!(corrupt.with_owner(Credentials::kernel(0,0),200),None);
 }
 #[test]fn ws79_owner_may_use_own_group_only(){
  let p=Permissions::new(100,7,0o640).unwrap();
  assert_eq!(p.with_group(Credentials::kernel(100,9),9),
   Some(Permissions{group:9,..p}));
  assert_eq!(p.with_group(Credentials::kernel(100,9),12),None);
  assert_eq!(p.with_group(Credentials::kernel(102,9),9),None);
  assert_eq!(p.with_group(Credentials::kernel(0,1),12),
   Some(Permissions{group:12,..p}));
 }
 #[test]fn ws80_restriction_only_chmod_denies_escalation(){
  let p=Permissions::new(100,7,0o750).unwrap();
  assert_eq!(p.restrict_mode(Credentials::kernel(100,9),0o500),
   Some(Permissions{mode:0o500,..p}));
  assert_eq!(p.restrict_mode(Credentials::kernel(100,9),0o770),None);
  assert_eq!(p.restrict_mode(Credentials::kernel(101,7),0o500),None);
 }
 #[test]fn ws81_directory_search_is_required_for_leaf_access(){
  let user=Credentials::kernel(100,7);
  let dir=Permissions::new(100,7,0o700).unwrap();
  let leaf=Permissions::new(100,7,0o600).unwrap();
  assert!(Permissions::authorize_path(&[dir],leaf,user,READ));
  let blocked=Permissions::new(100,7,0o600).unwrap();
  assert!(!Permissions::authorize_path(&[blocked],leaf,user,READ));
  assert!(!Permissions::authorize_path(&[dir],leaf,user,0));
  assert!(!Permissions::authorize_path(&[dir],leaf,user,8));
  assert!(!Permissions::authorize_path(&[Permissions{mode:0o1000,..dir}],
    leaf,Credentials::kernel(0,0),READ));
 }
}


// M1 gate 11 / G6: permission guard for a cross-directory rename.
// The caller MUST separately pin namespace, mount and file generations.
// This checks both parent directories and every ancestor without allocating;
// it is a policy primitive until the live VFS rename entrypoint uses it.
impl Permissions {
 pub fn authorize_rename(
  source_ancestors:&[Permissions],source_parent:Permissions,
  destination_ancestors:&[Permissions],destination_parent:Permissions,
  caller:Credentials,
 )->bool {
  source_ancestors.iter().all(|dir|dir.allows(caller,SEARCH)) &&
  destination_ancestors.iter().all(|dir|dir.allows(caller,SEARCH)) &&
  source_parent.allows(caller,WRITE|SEARCH) &&
  destination_parent.allows(caller,WRITE|SEARCH)
 }
}
#[cfg(test)]
mod m1_gate11_rename_acl_tests {
 use super::*;
 #[test]fn rename_checks_both_parents_and_all_ancestors() {
  let user=Credentials::kernel(100,7);
  let writable=Permissions::new(100,7,0o700).unwrap();
  let searchable=Permissions::new(100,7,0o500).unwrap();
  let denied=Permissions::new(200,8,0o700).unwrap();
  assert!(Permissions::authorize_rename(&[searchable],writable,
    &[searchable],writable,user));
  assert!(!Permissions::authorize_rename(&[denied],writable,
    &[searchable],writable,user));
  assert!(!Permissions::authorize_rename(&[searchable],writable,
    &[denied],writable,user));
  assert!(!Permissions::authorize_rename(&[searchable],searchable,
    &[searchable],writable,user));
  assert!(!Permissions::authorize_rename(&[searchable],writable,
    &[searchable],searchable,user));
  assert!(!Permissions::authorize_rename(&[searchable],writable,
    &[Permissions{mode:0o1777,..searchable}],writable,
    Credentials::kernel(0,0)));
 }
}


#[cfg(test)]
mod m1_batch01_w5_acl_tests{
 use super::*;
 #[test]fn b01_12_rename_checks_both_writable_parent_directories(){
  let u=Credentials::kernel(5,5);
  let rw=Permissions::new(5,5,0o700).unwrap();
  let ro=Permissions::new(5,5,0o500).unwrap();
  assert!(Permissions::authorize_rename(&[],rw,&[],rw,u));
  assert!(!Permissions::authorize_rename(&[],rw,&[],ro,u));
 }
 #[test]fn b01_13_rename_denies_unsearchable_ancestor(){
  let u=Credentials::kernel(5,5);
  let rw=Permissions::new(5,5,0o700).unwrap();
  let nox=Permissions::new(5,5,0o600).unwrap();
  assert!(!Permissions::authorize_rename(&[nox],rw,&[],rw,u));
 }
 #[test]fn b01_14_invalid_persisted_mode_denies_root(){
  let bad=Permissions{owner:5,group:5,mode:0o1000};
  assert!(!bad.allows(Credentials::kernel(0,0),WRITE|SEARCH));
 }
}
#[cfg(test)]
mod m1_gate20_w3_acl_tests {
 use super::*;
 #[test]fn rename_requires_search_and_write_on_both_parents(){
  let parent=Permissions::new(1,1,0o700).unwrap();
  let ancestor=Permissions::new(1,1,0o500).unwrap();
  let peer=Credentials::kernel(2,2);
  let owner=Credentials::kernel(1,1);
  assert!(!Permissions::authorize_rename(&[ancestor],parent,&[],parent,peer));
  assert!(Permissions::authorize_rename(&[ancestor],parent,&[],parent,owner));
  assert!(!Permissions::authorize_rename(&[Permissions::new(1,1,0o400).unwrap()],
   parent,&[],parent,owner));
 }
}

/// Two bounded, persisted named-user exceptions. The existing mode's group
/// bits are the named-entry mask; matching named users do not fall through to
/// group/other rights. This convention is intentionally narrower than POSIX
/// extended ACLs (no named groups, default inheritance or independent mask).
/// UID zero is reserved for an unused slot; root uses its existing override.
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct NamedUsers{entries:[(u16,u16);2]}
impl NamedUsers{
 pub const EMPTY:Self=Self{entries:[(0,0);2]};
 /// Canonical storage has increasing, nonzero UIDs followed by empty slots.
 /// Reject malformed input instead of sorting away duplicate/ambiguous grants.
 pub fn from_entries(entries:[(u16,u16);2])->Option<Self>{
  let result=Self{entries};result.validate().then_some(result)
 }
 pub const fn entries(self)->[(u16,u16);2]{self.entries}
 pub fn validate(self)->bool{
  let mut previous=0;let mut ended=false;
  for (uid,rights) in self.entries{
   if rights&!7!=0{return false;}
   if uid==0{if rights!=0{return false;}ended=true;}
   else{
    if ended||uid<=previous{return false;}previous=uid;
   }
  }
  true
 }
 pub fn allows(self,permissions:Permissions,caller:Credentials,access:u16)->bool{
  // Validate persisted ACL and mode before root or owner precedence, so
  // corrupt metadata cannot regain authority through the privileged branch.
  if !self.validate()||permissions.mode&!0o777!=0||access&!7!=0{return false;}
  if caller.uid()==0||caller.uid()==permissions.owner{
   return permissions.allows(caller,access);
  }
  if let Some((_,rights))=self.entries.iter().find(|(uid,_)|*uid==caller.uid()){
   let effective=rights&((permissions.mode>>3)&7);
   return effective&access==access;
  }
  permissions.allows(caller,access)
 }
 /// Caller identity must be kernel-issued. Only root or the current owner
 /// may set/remove an entry; explicit rights=0 denies matching-user access.
 /// Removing an absent UID is idempotent, while a full table rejects additions.
 pub fn update(self,permissions:Permissions,caller:Credentials,uid:u16,
  rights:Option<u16>)->Option<Self>{
  if !self.validate()||permissions.mode&!0o777!=0||uid==0||
   (caller.uid()!=0&&caller.uid()!=permissions.owner)||
   rights.is_some_and(|value|value&!7!=0){return None;}
  let mut entries=self.entries;
  let found=entries.iter().position(|(entry,_)|*entry==uid);
  match (found,rights){
   (Some(index),Some(value))=>entries[index].1=value,
   (Some(index),None)=>{
    for i in index..entries.len()-1{entries[i]=entries[i+1];}
    entries[1]=(0,0);
   },
   (None,None)=>return Some(self),
   (None,Some(value))=>{
    let index=entries.iter().position(|(entry,_)|*entry==0)?;
    entries[index]=(uid,value);
    if index==1&&entries[0].0>uid{entries.swap(0,1);}
   },
  }
  Self::from_entries(entries)
 }
}

#[cfg(test)]
mod named_user_acl_tests{
 use super::*;
 #[test]fn empty_acl_preserves_owner_group_other_and_root_semantics(){
  let permissions=Permissions::new(100,7,0o640).unwrap();
  for caller in [Credentials::kernel(0,0),Credentials::kernel(100,9),
   Credentials::kernel(200,7),Credentials::kernel(200,9)]{
   for access in 0..16{
    assert_eq!(NamedUsers::EMPTY.allows(permissions,caller,access),permissions.allows(caller,access));
   }
  }
 }
 #[test]fn matching_user_uses_mask_without_group_or_other_fallback(){
  let permissions=Permissions::new(100,7,0o647).unwrap();
  let acl=NamedUsers::from_entries([(200,READ|WRITE),(300,0)]).unwrap();
  assert!(acl.allows(permissions,Credentials::kernel(200,9),READ));
  assert!(!acl.allows(permissions,Credentials::kernel(200,7),WRITE));
  assert!(!acl.allows(permissions,Credentials::kernel(200,9),SEARCH));
  assert!(!acl.allows(permissions,Credentials::kernel(300,7),READ));
  assert!(acl.allows(permissions,Credentials::kernel(300,7),0));
  assert!(acl.allows(permissions,Credentials::kernel(400,9),READ|WRITE|SEARCH));
  assert!(acl.allows(permissions,Credentials::kernel(0,0),READ|WRITE|SEARCH));
  let owner_entry=NamedUsers::from_entries([(100,0),(200,0)]).unwrap();
  assert!(owner_entry.allows(permissions,Credentials::kernel(100,9),READ|WRITE));
 }
 #[test]fn owner_admin_is_bounded_canonical_and_removal_is_idempotent(){
  let permissions=Permissions::new(100,7,0o660).unwrap();let owner=Credentials::kernel(100,9);
  let first=NamedUsers::EMPTY.update(permissions,owner,300,Some(READ)).unwrap();
  let full=first.update(permissions,owner,200,Some(WRITE)).unwrap();
  assert_eq!(full.entries(),[(200,WRITE),(300,READ)]);
  assert!(full.update(permissions,owner,400,Some(READ)).is_none());
  assert_eq!(full.update(permissions,owner,400,None),Some(full));
  assert_eq!(full.update(permissions,owner,200,None).unwrap().entries(),[(300,READ),(0,0)]);
  let updated=full.update(permissions,owner,300,Some(0)).unwrap();
  assert_eq!(updated.entries(),[(200,WRITE),(300,0)]);
  assert!(full.update(permissions,Credentials::kernel(200,7),300,None).is_none());
  assert!(full.update(permissions,owner,0,None).is_none());
  assert!(full.update(permissions,owner,200,Some(8)).is_none());
  assert!(full.update(permissions,Credentials::kernel(0,0),200,None).is_some());
 }
 #[test]fn corrupt_acl_or_mode_cannot_be_laundered_even_by_root(){
  let permissions=Permissions::new(100,7,0o777).unwrap();let root=Credentials::kernel(0,0);
  for entries in [[(1,4),(1,2)],[(2,4),(1,2)],[(0,0),(1,4)],[(0,4),(0,0)],[(1,8),(0,0)]]{
   assert!(NamedUsers::from_entries(entries).is_none());
   let corrupt=NamedUsers{entries};assert!(!corrupt.validate());
   assert!(!corrupt.allows(permissions,root,READ));
   assert!(corrupt.update(permissions,root,1,None).is_none());
  }
  let corrupt=Permissions{mode:0o1777,..permissions};
  assert!(!NamedUsers::EMPTY.allows(corrupt,root,0));
  assert!(NamedUsers::EMPTY.update(corrupt,root,1,Some(READ)).is_none());
 }
 #[test]fn chmod_masks_retained_grant_and_owner_transfer_has_owner_precedence(){
  let owner=Credentials::kernel(100,7);let user=Credentials::kernel(200,9);
  let permissions=Permissions::new(100,7,0o660).unwrap();
  let acl=NamedUsers::EMPTY.update(permissions,owner,200,Some(READ|WRITE)).unwrap();
  assert!(acl.allows(permissions,user,READ|WRITE));
  let restricted=permissions.with_mode(owner,0o600).unwrap();
  assert!(!acl.allows(restricted,user,READ));
  let transferred=restricted.with_owner(Credentials::kernel(0,0),200).unwrap();
  assert!(acl.allows(transferred,user,READ|WRITE));
  assert!(acl.update(transferred,owner,300,Some(READ)).is_none());
 }
}

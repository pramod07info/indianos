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

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
 pub fn allows(self,credentials:Credentials,access:u16)->bool{
  if access&!7!=0{return false;}
  if credentials.uid==0{return true;}
  let shift=if credentials.uid==self.owner{6}else if credentials.gid==self.group{3}else{0};
  ((self.mode>>shift)&access)==access
 }
}

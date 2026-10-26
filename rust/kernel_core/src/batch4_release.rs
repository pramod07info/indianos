//! R038/40: exact-SHA release evidence and independent-review gate.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub enum Status{Pass,Fail,Missing,Skipped}
#[derive(Clone,Copy)]pub struct Evidence{pub sha:[u8;20],pub kernel:Status,pub filesystem:Status,
 pub security:Status,pub update:Status,pub compatibility:Status,pub platform_mask:u8,
 pub independently_reviewed:bool}
#[derive(Debug,PartialEq,Eq)]pub enum Error{Stale,Failed,Incomplete,Unreviewed}
pub fn qualify(e:Evidence,expected:[u8;20],required_platforms:u8)->Result<(),Error>{
 if expected==[0;20]||e.sha!=expected{return Err(Error::Stale);}
 let checks=[e.kernel,e.filesystem,e.security,e.update,e.compatibility];
 if checks.contains(&Status::Fail){return Err(Error::Failed);}
 if checks.iter().any(|s|*s!=Status::Pass)||required_platforms==0||
  e.platform_mask&required_platforms!=required_platforms{return Err(Error::Incomplete);}
 if !e.independently_reviewed{return Err(Error::Unreviewed);}
 Ok(())
}
#[cfg(test)]mod tests{use super::*;
 #[test]fn release_requires_exact_sha_and_review(){
  let e=Evidence{sha:[2;20],kernel:Status::Pass,filesystem:Status::Pass,
   security:Status::Pass,update:Status::Pass,compatibility:Status::Pass,
   platform_mask:1,independently_reviewed:false};
  assert_eq!(qualify(e,[2;20],1),Err(Error::Unreviewed));
  assert_eq!(qualify(e,[1;20],1),Err(Error::Stale));
 }
}
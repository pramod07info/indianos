//! R022/15: fail-closed typed developer command admission and audit.
//! Does not run a shell. Only trusted executor can acknowledge completion.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Command{Compile,Test,Lint,Package,Preview,Debug}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Status{Requested,Granted,Running,Success,Failure,Denied}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Request{pub id:u64,pub owner:u32,pub rights:u32,pub command:Command,
 pub artifact:u64,pub expires:u64,pub status:Status}
pub struct Gateway<const N:usize>{requests:[Option<Request>;N]}
impl<const N:usize> Gateway<N>{
 pub const fn new()->Self{Self{requests:[None;N]}}
 fn idx(&self,id:u64)->Option<usize>{
  self.requests.iter().position(|r|r.is_some_and(|r|r.id==id))
 }
 pub fn request(&mut self,item:Request)->Result<(),Error>{
  if item.id==0||item.owner==0||item.rights==0||
   item.artifact==0||item.expires==0{return Err(Error::Invalid);}
  if self.idx(item.id).is_some(){return Err(Error::Conflict);}
  let at=self.requests.iter().position(Option::is_none).ok_or(Error::Full)?;
  let mut value=item;value.status=Status::Requested;
  self.requests[at]=Some(value);Ok(())
 }
 pub fn authorize(&mut self,id:u64,actor:u32,granted:u32,
   now:u64)->Result<(),Error>{
  let i=self.idx(id).ok_or(Error::Missing)?;
  let mut r=self.requests[i].ok_or(Error::Missing)?;
  if r.owner!=actor||actor==0||r.rights&granted!=r.rights||
    now>=r.expires{return Err(Error::Denied);}
  if r.status!=Status::Requested{return Err(Error::Conflict);}
  r.status=Status::Granted;self.requests[i]=Some(r);Ok(())
 }
 pub fn execute_start(&mut self,trusted_executor:bool,id:u64,
  now:u64)->Result<(),Error>{
  if !trusted_executor{return Err(Error::Denied);}
  let i=self.idx(id).ok_or(Error::Missing)?;
  let mut r=self.requests[i].ok_or(Error::Missing)?;
  if r.status!=Status::Granted||now>=r.expires{return Err(Error::Stale);}
  r.status=Status::Running;self.requests[i]=Some(r);Ok(())
 }
 pub fn complete(&mut self,trusted_executor:bool,id:u64,
  success:bool)->Result<(),Error>{
  if !trusted_executor{return Err(Error::Denied);}
  let i=self.idx(id).ok_or(Error::Missing)?;
  let mut r=self.requests[i].ok_or(Error::Missing)?;
  if r.status!=Status::Running{return Err(Error::Conflict);}
  r.status=if success{Status::Success}else{Status::Failure};
  self.requests[i]=Some(r);Ok(())
 }
 pub fn revoke_owner(&mut self,owner:u32)->usize{
  if owner==0{return 0;}
  let mut n=0;for r in &mut self.requests{
   if r.is_some_and(|v|v.owner==owner){*r=None;n+=1;}
  }n
 }
 pub fn status(&self,actor:u32,id:u64)->Result<Status,Error>{
  let r=self.idx(id).and_then(|i|self.requests[i]).ok_or(Error::Missing)?;
  if actor!=r.owner||actor==0{return Err(Error::Denied);}Ok(r.status)
 }
}
#[cfg(test)]mod tests{
 use super::*;
 fn req()->Request{Request{id:1,owner:7,rights:0b11,
  command:Command::Compile,artifact:11,expires:100,status:Status::Running}}
 #[test]fn authorized_build_request_records_actual_trusted_lifecycle(){
  let mut g=Gateway::<1>::new();g.request(req()).unwrap();
  g.authorize(1,7,3,1).unwrap();
  g.execute_start(true,1,2).unwrap();g.complete(true,1,true).unwrap();
  assert_eq!(g.status(7,1),Ok(Status::Success));
 }
 #[test]fn forged_executor_and_expired_job_do_not_run(){
  let mut g=Gateway::<1>::new();g.request(req()).unwrap();
  assert_eq!(g.authorize(1,7,3,100),Err(Error::Denied));
  g.authorize(1,7,3,1).unwrap();
  assert_eq!(g.execute_start(false,1,2),Err(Error::Denied));
 }
 #[test]fn capability_subset_is_not_sufficient_for_shell_permission(){
  let mut g=Gateway::<1>::new();g.request(req()).unwrap();
  assert_eq!(g.authorize(1,7,1,1),Err(Error::Denied));
  assert_eq!(g.status(8,1),Err(Error::Denied));
 }
}

//! R020/12 local source-control branch and snapshot ancestry planner.
//! This is metadata only; no external Git remote or branch checkout.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Branch{pub owner:u32,pub id:u32,pub head:u64,pub base:u64,
 pub dirty:bool,pub protected:bool}
pub struct Repository<const N:usize>{branches:[Option<Branch>;N]}
impl<const N:usize> Repository<N>{
 pub const fn new()->Self{Self{branches:[None;N]}}
 fn index(&self,owner:u32,id:u32)->Option<usize>{
  self.branches.iter().position(|x|x.is_some_and(
   |x|x.owner==owner&&x.id==id))
 }
 pub fn create(&mut self,owner:u32,id:u32,
  base:u64,protected:bool)->Result<(),Error>{
  if owner==0||id==0||base==0{return Err(Error::Invalid);}
  if self.index(owner,id).is_some(){return Err(Error::Conflict);}
  let free=self.branches.iter_mut().find(|s|s.is_none()).ok_or(Error::Full)?;
  *free=Some(Branch{owner,id,base,head:base,dirty:false,protected});Ok(())
 }
 pub fn working_change(&mut self,owner:u32,id:u32,changed:bool)
  ->Result<(),Error>{
  let i=self.index(owner,id).ok_or(Error::Missing)?;
  let mut b=self.branches[i].ok_or(Error::Missing)?;
  b.dirty=changed;self.branches[i]=Some(b);Ok(())
 }
 pub fn commit(&mut self,owner:u32,id:u32,expected:u64,next:u64,
  trusted_writer:bool)->Result<(),Error>{
  let i=self.index(owner,id).ok_or(Error::Missing)?;
  let mut b=self.branches[i].ok_or(Error::Missing)?;
  if b.protected||!trusted_writer{return Err(Error::Denied);}
  if !b.dirty||b.head!=expected||next==0||next==expected{
   return Err(Error::Stale);
  }
  b.head=next;b.dirty=false;self.branches[i]=Some(b);Ok(())
 }
 pub fn fast_forward(&mut self,owner:u32,target:u32,source:u32,
   acknowledged_ancestor:u64,reviewed:bool)->Result<(),Error>{
  if !reviewed||target==source{return Err(Error::Denied);}
  let ti=self.index(owner,target).ok_or(Error::Missing)?;
  let si=self.index(owner,source).ok_or(Error::Missing)?;
  let mut t=self.branches[ti].ok_or(Error::Missing)?;
  let s=self.branches[si].ok_or(Error::Missing)?;
  if t.dirty||s.dirty||t.head!=acknowledged_ancestor||
   s.base!=acknowledged_ancestor{return Err(Error::Conflict);}
  t.head=s.head;self.branches[ti]=Some(t);Ok(())
 }
 pub fn get(&self,owner:u32,id:u32)->Result<Branch,Error>{
  self.index(owner,id).and_then(|i|self.branches[i]).ok_or(Error::Missing)
 }
 pub fn remove(&mut self,owner:u32,id:u32)->Result<(),Error>{
  let i=self.index(owner,id).ok_or(Error::Missing)?;
  let b=self.branches[i].ok_or(Error::Missing)?;
  if b.protected||b.dirty{return Err(Error::Denied);}
  self.branches[i]=None;Ok(())
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn reviewed_feature_branch_fast_forwards_clean_main(){
  let mut r=Repository::<2>::new();
  r.create(7,1,100,true).unwrap();r.create(7,2,100,false).unwrap();
  r.working_change(7,2,true).unwrap();
  r.commit(7,2,100,101,true).unwrap();
  r.fast_forward(7,1,2,100,true).unwrap();
  assert_eq!(r.get(7,1).unwrap().head,101);
 }
 #[test]fn protected_branch_or_missing_review_cannot_commit(){
  let mut r=Repository::<2>::new();
  r.create(7,1,100,true).unwrap();r.create(7,2,100,false).unwrap();
  r.working_change(7,1,true).unwrap();
  assert_eq!(r.commit(7,1,100,101,true),Err(Error::Denied));
  assert_eq!(r.fast_forward(7,1,2,100,false),Err(Error::Denied));
 }
 #[test]fn non_ancestor_and_dirty_target_refuse_merge(){
  let mut r=Repository::<2>::new();
  r.create(7,1,100,false).unwrap();r.create(7,2,200,false).unwrap();
  r.working_change(7,1,true).unwrap();
  assert_eq!(r.fast_forward(7,1,2,100,true),Err(Error::Conflict));
  assert_eq!(r.get(7,1).unwrap().head,100);
 }
}

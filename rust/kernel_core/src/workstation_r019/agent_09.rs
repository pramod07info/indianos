//! R019/09: owner-scoped version-control staging index.
//! This records explicit file digests; it never invokes a host Git binary.
use super::Error;
pub const PATH:usize=80;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Entry{pub owner:u32,pub path:[u8;PATH],pub len:usize,
 pub old:u64,pub new:u64,pub staged:bool}
pub struct Index<const N:usize>{items:[Option<Entry>;N]}
impl<const N:usize> Index<N>{
 pub const fn new()->Self{Self{items:[None;N]}}
 fn index(&self,owner:u32,path:&[u8])->Option<usize>{
  self.items.iter().position(|e|e.is_some_and(
   |e|e.owner==owner&&&e.path[..e.len]==path))
 }
 fn valid(path:&[u8])->bool{
  !path.is_empty()&&path.len()<=PATH&&path[0]==b'/'&&
   !path.windows(2).any(|p|p==b"..")&&
   path.iter().all(|b|b.is_ascii_graphic()&&*b!=b'\\')
 }
 pub fn track(&mut self,owner:u32,path:&[u8],old:u64,new:u64)
  ->Result<(),Error>{
  if owner==0||!Self::valid(path)||old==new{return Err(Error::Invalid);}
  let i=self.index(owner,path).or_else(||
   self.items.iter().position(Option::is_none)).ok_or(Error::Full)?;
  let mut name=[0u8;PATH];name[..path.len()].copy_from_slice(path);
  self.items[i]=Some(Entry{owner,path:name,len:path.len(),old,new,staged:false});Ok(())
 }
 pub fn stage(&mut self,owner:u32,path:&[u8],expected_new:u64)
  ->Result<(),Error>{
  let i=self.index(owner,path).ok_or(Error::Missing)?;
  let mut e=self.items[i].ok_or(Error::Missing)?;
  if e.new!=expected_new{return Err(Error::Stale);}
  e.staged=true;self.items[i]=Some(e);Ok(())
 }
 pub fn unstage(&mut self,owner:u32,path:&[u8])->Result<(),Error>{
  let i=self.index(owner,path).ok_or(Error::Missing)?;
  let mut e=self.items[i].ok_or(Error::Missing)?;
  e.staged=false;self.items[i]=Some(e);Ok(())
 }
 pub fn snapshot_staged(&self,owner:u32,out:&mut[Entry])->Result<usize,Error>{
  if owner==0{return Err(Error::Denied);}
  let n=self.items.iter().flatten().filter(|e|e.owner==owner&&e.staged).count();
  if n>out.len(){return Err(Error::Full);}
  let mut at=0;
  for e in self.items.iter().flatten(){
   if e.owner==owner&&e.staged{out[at]=*e;at+=1;}
  }Ok(at)
 }
 pub fn commit_index(&mut self,owner:u32)->usize{
  let mut n=0;
  for e in &mut self.items{
   if e.is_some_and(|e|e.owner==owner&&e.staged){*e=None;n+=1;}
  }n
 }
}
#[cfg(test)]mod tests{
 use super::*;
 fn empty()->Entry{Entry{owner:0,path:[0;PATH],len:0,old:0,new:0,staged:false}}
 #[test]fn staged_digest_is_required_before_commit(){
  let mut i=Index::<2>::new();
  i.track(1,b"/src/a",10,11).unwrap();
  assert_eq!(i.stage(1,b"/src/a",10),Err(Error::Stale));
  i.stage(1,b"/src/a",11).unwrap();
  let mut out=[empty();1];
  assert_eq!(i.snapshot_staged(1,&mut out),Ok(1));
  assert_eq!(out[0].new,11);assert_eq!(i.commit_index(1),1);
 }
 #[test]fn foreign_owner_commit_does_not_erase_changes(){
  let mut i=Index::<1>::new();i.track(1,b"/a",1,2).unwrap();
  assert_eq!(i.stage(2,b"/a",2),Err(Error::Missing));
  i.stage(1,b"/a",2).unwrap();assert_eq!(i.commit_index(2),0);
  let mut out=[empty();1];assert_eq!(i.snapshot_staged(1,&mut out),Ok(1));
 }
 #[test]fn malicious_path_and_short_view_are_atomic(){
  let mut i=Index::<2>::new();
  assert_eq!(i.track(1,b"/../etc",1,2),Err(Error::Invalid));
  i.track(1,b"/a",1,2).unwrap();i.stage(1,b"/a",2).unwrap();
  let mut out: [Entry;0]=[];
  assert_eq!(i.snapshot_staged(1,&mut out),Err(Error::Full));
 }
}

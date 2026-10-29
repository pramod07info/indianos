//! R026/13: source-revision-aware incremental build planner over real project files.
//! This computes stale compiler inputs; a separate trusted worker executes build steps.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Input{pub path:[u8;PATH_MAX],pub len:usize,pub revision:u64,pub digest:u64}
pub const EMPTY:Input=Input{path:[0;PATH_MAX],len:0,revision:0,digest:0};
fn hash(bytes:&[u8])->u64{
 let mut h=0xcbf29ce484222325u64;
 for &b in bytes{h=(h^(b as u64)).wrapping_mul(0x100000001b);}h
}
pub struct Planner<const N:usize>{owner:u32,toolchain:u64,
 inputs:[Input;N],count:usize,baseline:u64}
impl<const N:usize> Planner<N>{
 pub const fn new(owner:u32,toolchain:u64)->Self{
  Self{owner,toolchain,inputs:[EMPTY;N],count:0,baseline:0}
 }
 pub fn capture<const F:usize>(&mut self,owner:u32,fs:&RamFs<F>,
 root:&[u8])->Result<usize,Error>{
  if owner==0||owner!=self.owner||self.toolchain==0{return Err(Error::Denied);}
  if root.first()!=Some(&b'/'){return Err(Error::Invalid);}
  let mut stage=[EMPTY;N];let mut n=0;
  for i in 0..fs.count(){
   let mut path=[0u8;PATH_MAX];
   let (len,size,revision)=fs.entry(i,&mut path).map_err(|_|Error::Invalid)?
    .ok_or(Error::Missing)?;
   if !path[..len].starts_with(root)||!path[..len].ends_with(b".rs"){continue;}
   if n==N{return Err(Error::Full);}
   let mut bytes=[0u8;CONTENT_MAX];
   fs.read(&path[..len],0,&mut bytes[..size]).map_err(|_|Error::Missing)?;
   stage[n]=Input{path,len,revision,digest:hash(&bytes[..size])};n+=1;
  }
  self.inputs=stage;self.count=n;self.baseline=fs.version();Ok(n)
 }
 pub fn dirty<const F:usize>(&self,owner:u32,fs:&RamFs<F>,
  out:&mut[Input])->Result<usize,Error>{
  if owner==0||owner!=self.owner{return Err(Error::Denied);}
  let mut count=0;let mut stage=[EMPTY;N];
  for entry in &self.inputs[..self.count]{
   let changed=match fs.metadata(&entry.path[..entry.len]){
    Err(_)=>true,
    Ok((rev,size))=>{
     let mut bytes=[0u8;CONTENT_MAX];
     rev!=entry.revision||
     fs.read(&entry.path[..entry.len],0,&mut bytes[..size]).is_err()||
     hash(&bytes[..size])!=entry.digest
    }
   };
   if changed{
    if count==N{return Err(Error::Full);}
    stage[count]=*entry;count+=1;
   }
  }
  if count>out.len(){return Err(Error::Full);}
  out[..count].copy_from_slice(&stage[..count]);Ok(count)
 }
 pub fn toolchain_changed(&self,new:u64)->bool{self.toolchain!=new}
 pub fn baseline(&self)->u64{self.baseline}
 pub fn count(&self)->usize{self.count}
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn only_changed_real_source_file_is_marked_for_rebuild(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/p/a.rs",b"a").unwrap();fs.replace(b"/p/b.rs",b"b").unwrap();
  let mut p=Planner::<2>::new(7,21);p.capture(7,&fs,b"/p").unwrap();
  fs.replace(b"/p/b.rs",b"new").unwrap();
  let mut out=[EMPTY;2];assert_eq!(p.dirty(7,&fs,&mut out),Ok(1));
  assert_eq!(&out[0].path[..out[0].len],b"/p/b.rs");
 }
 #[test]fn clean_source_tree_results_in_incremental_cache_hit(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a.rs",b"a").unwrap();
  let mut p=Planner::<1>::new(7,21);p.capture(7,&fs,b"/p").unwrap();
  assert_eq!(p.dirty(7,&fs,&mut [EMPTY;1]),Ok(0));
 }
 #[test]fn toolchain_and_owner_changes_invalidate_build_reuse(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a.rs",b"a").unwrap();
  let mut p=Planner::<1>::new(7,21);p.capture(7,&fs,b"/p").unwrap();
  assert!(p.toolchain_changed(22));
  assert_eq!(p.dirty(8,&fs,&mut [EMPTY;1]),Err(Error::Denied));
 }
}

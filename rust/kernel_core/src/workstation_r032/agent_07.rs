//! R032/07: revision-bound, trusted compiler diagnostics attached to real project files.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Severity{Error,Warning,Info}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Diagnostic{pub path:[u8;PATH_MAX],pub len:usize,pub revision:u64,
 pub line:usize,pub column:usize,pub code:u32,pub severity:Severity}
pub struct Diagnostics<const N:usize>{owner:u64,records:[Option<Diagnostic>;N]}
impl<const N:usize> Diagnostics<N>{
 pub fn new(owner:u64)->Self{Self{owner,records:[None;N]}}
 pub fn record<const F:usize>(&mut self,owner:u64,trusted:bool,fs:&RamFs<F>,
  path:&[u8],line:usize,column:usize,code:u32,severity:Severity)
  ->Result<(),Error>{
  if owner==0||owner!=self.owner||!trusted{return Err(Error::Denied);}
  if line==0||column==0||path.len()>PATH_MAX||code==0{return Err(Error::Invalid);}
  let(rev,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
  let mut data=[0u8;CONTENT_MAX];fs.read(path,0,&mut data[..size])
   .map_err(|_|Error::Missing)?;
  let target=data[..size].split(|b|*b==b'\n').nth(line-1).ok_or(Error::Invalid)?;
  if column>target.len()+1{return Err(Error::Invalid);}
  let i=self.records.iter().position(Option::is_none).ok_or(Error::Full)?;
  let mut name=[0u8;PATH_MAX];name[..path.len()].copy_from_slice(path);
  self.records[i]=Some(Diagnostic{path:name,len:path.len(),revision:rev,line,column,
   code,severity});Ok(())
 }
 pub fn visible<const F:usize>(&self,owner:u64,fs:&RamFs<F>,out:&mut[Diagnostic])
  ->Result<usize,Error>{
  if owner==0||owner!=self.owner{return Err(Error::Denied);}
  let mut n=0;
  for item in self.records.iter().flatten(){
   if !fs.metadata(&item.path[..item.len]).is_ok_and(|(v,_)|v==item.revision){continue;}
   if n==out.len(){return Err(Error::Full);}
   n+=1;
  }
  let mut i=0;
  for item in self.records.iter().flatten(){
   if fs.metadata(&item.path[..item.len]).is_ok_and(|(v,_)|v==item.revision){
    out[i]=*item;i+=1;
   }
  }Ok(n)
 }
 pub fn release_ready<const F:usize>(&self,owner:u64,fs:&RamFs<F>)->Result<bool,Error>{
  if owner==0||owner!=self.owner{return Err(Error::Denied);}
  Ok(self.records.iter().flatten().all(|d|
   d.severity!=Severity::Error&&
   fs.metadata(&d.path[..d.len]).is_ok_and(|(v,_)|v==d.revision)))
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn trusted_diagnostics_navigate_real_revision_and_block_release(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a.rs",b"fn main(){}\n").unwrap();
  let mut d=Diagnostics::<2>::new(7);
  d.record(7,true,&fs,b"/p/a.rs",1,4,101,Severity::Error).unwrap();
  let mut out=[Diagnostic{path:[0;PATH_MAX],len:0,revision:0,line:0,column:0,
   code:0,severity:Severity::Info};2];
  assert_eq!(d.visible(7,&fs,&mut out),Ok(1));assert!(!d.release_ready(7,&fs).unwrap());
 }
 #[test]fn changed_files_invalidate_old_diagnostics_instead_of_navigating_wrong_lines(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"bad\n").unwrap();
  let mut d=Diagnostics::<1>::new(5);
  d.record(5,true,&fs,b"/p/a",1,1,1,Severity::Error).unwrap();
  fs.replace(b"/p/a",b"new\n").unwrap();
  assert_eq!(d.visible(5,&fs,&mut[]),Ok(0));
  assert!(!d.release_ready(5,&fs).unwrap());
 }
}
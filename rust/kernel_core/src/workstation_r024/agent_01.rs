//! R024/01: real-file create, read, copy, move, remove and preflight for guest File Manager.
//! Optimistic global revision guard and actual RamFs operations; paths not invented.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Operation{Create,Copy,Move,Delete}
pub fn preflight<const N:usize>(fs:&RamFs<N>,revision:u64,op:Operation,
 from:&[u8],to:&[u8])->Result<(),Error>{
 if fs.version()!=revision{return Err(Error::Stale);}
 if from.len()<2||from.len()>PATH_MAX||from[0]!=b'/'||
 from.windows(2).any(|w|w==b".."){return Err(Error::Invalid);}
 let src=fs.metadata(from).ok();
 match op{
  Operation::Create=>{
   if to.len()>CONTENT_MAX{return Err(Error::Full);}
   if src.is_some(){return Err(Error::Conflict);}
  }
  Operation::Copy|Operation::Move=>{
   if src.is_none(){return Err(Error::Missing);}
   if to.len()<2||to.len()>PATH_MAX||to[0]!=b'/'||
    to.windows(2).any(|w|w==b".."){return Err(Error::Invalid);}
   if fs.metadata(to).is_ok()||from==to{return Err(Error::Conflict);}
  }
  Operation::Delete=>{
   if src.is_none(){return Err(Error::Missing);}
   if !to.is_empty(){return Err(Error::Invalid);}
  }
 }
 Ok(())
}
pub fn execute<const N:usize>(fs:&mut RamFs<N>,revision:u64,
 op:Operation,from:&[u8],to:&[u8])->Result<u64,Error>{
 preflight(fs,revision,op,from,to)?;
 match op{
  Operation::Create=>fs.replace(from,to).map_err(|_|Error::Full),
  Operation::Copy=>fs.copy_new(from,to).map_err(|_|Error::Full),
  Operation::Move=>{
   let (file_rev,_)=fs.metadata(from).map_err(|_|Error::Missing)?;
   fs.rename_if_version(from,to,file_rev).map_err(|_|Error::Conflict)
  }
  Operation::Delete=>{
   let (file_rev,_)=fs.metadata(from).map_err(|_|Error::Missing)?;
   fs.delete_if_version(from,file_rev).map_err(|_|Error::Stale)
  }
 }
}
pub fn contents<const N:usize>(fs:&RamFs<N>,path:&[u8],
 output:&mut[u8])->Result<usize,Error>{
 let (_,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
 if output.len()<size{return Err(Error::Full);}
 fs.read(path,0,&mut output[..size]).map_err(|_|Error::Missing)
}
pub fn exists<const N:usize>(fs:&RamFs<N>,path:&[u8])->bool{fs.metadata(path).is_ok()}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn create_copy_move_and_remove_actual_file_contents(){
  let mut fs=RamFs::<2>::new();
  execute(&mut fs,0,Operation::Create,b"/a",b"hello").unwrap();
  execute(&mut fs,1,Operation::Copy,b"/a",b"/b").unwrap();
  execute(&mut fs,2,Operation::Move,b"/b",b"/c").unwrap();
  assert!(exists(&fs,b"/c"));assert!(!exists(&fs,b"/b"));
  execute(&mut fs,3,Operation::Delete,b"/a",b"").unwrap();
  let mut out=[0;5];assert_eq!(contents(&fs,b"/c",&mut out),Ok(5));
  assert_eq!(&out,b"hello");
 }
 #[test]fn stale_view_never_removes_or_moves_newer_user_source(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"old").unwrap();
  fs.replace(b"/a",b"new").unwrap();
  assert_eq!(execute(&mut fs,1,Operation::Delete,b"/a",b""),Err(Error::Stale));
  assert!(exists(&fs,b"/a"));
 }
 #[test]fn short_read_buffer_and_copy_collision_do_not_corrupt_output(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"source").unwrap();
  let mut out=[99;2];assert_eq!(contents(&fs,b"/a",&mut out),Err(Error::Full));
  assert_eq!(out,[99;2]);
  assert_eq!(execute(&mut fs,1,Operation::Copy,b"/a",b"/a"),Err(Error::Conflict));
 }
}

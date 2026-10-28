//! R024/11: restricted developer terminal writes against actual RAM files.
//! Typed commands only: touch, write, append, rm; no shell, expansion or unsafe exec.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX,PATH_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Command{Touch,Write,Append,Remove}
pub fn run<const F:usize>(fs:&mut RamFs<F>,expected:u64,
 command:Command,path:&[u8],data:&[u8])->Result<u64,Error>{
 if expected!=fs.version(){return Err(Error::Stale);}
 if path.len()<2||path.len()>PATH_MAX||path[0]!=b'/'||
  path.windows(2).any(|w|w==b"..")||data.len()>CONTENT_MAX{
  return Err(Error::Invalid);
 }
 let existing=fs.metadata(path).ok();
 match command{
  Command::Touch=>{
   if !data.is_empty(){return Err(Error::Invalid);}
   if existing.is_some(){return Ok(fs.version());}
   fs.replace(path,b"").map_err(|_|Error::Full)
  }
  Command::Write=>fs.replace(path,data).map_err(|_|Error::Full),
  Command::Append=>{
   let (_,size)=existing.ok_or(Error::Missing)?;
   let n=size.checked_add(data.len()).ok_or(Error::Overflow)?;
   if n>CONTENT_MAX{return Err(Error::Full);}
   let mut image=[0u8;CONTENT_MAX];
   fs.read(path,0,&mut image[..size]).map_err(|_|Error::Missing)?;
   image[size..n].copy_from_slice(data);
   fs.replace(path,&image[..n]).map_err(|_|Error::Full)
  }
  Command::Remove=>{
   if !data.is_empty(){return Err(Error::Invalid);}
   let (file_rev,_)=existing.ok_or(Error::Missing)?;
   fs.delete_if_version(path,file_rev).map_err(|_|Error::Stale)
  }
 }
}
pub fn preview<const F:usize>(fs:&RamFs<F>,command:Command,
 path:&[u8],data:&[u8])->Result<usize,Error>{
 let existing=fs.metadata(path).ok();
 match command{
  Command::Touch=>Ok(existing.map(|(_,s)|s).unwrap_or(0)),
  Command::Write=>Ok(data.len()),
  Command::Append=>existing.map(|(_,s)|s.checked_add(data.len())
   .ok_or(Error::Overflow)).unwrap_or(Err(Error::Missing)),
  Command::Remove=>existing.map(|(_,s)|Ok(s)).unwrap_or(Err(Error::Missing))
 }
}
pub fn read<const F:usize>(fs:&RamFs<F>,path:&[u8],
 out:&mut[u8])->Result<usize,Error>{
 let (_,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
 if out.len()<size{return Err(Error::Full);}
 fs.read(path,0,&mut out[..size]).map_err(|_|Error::Missing)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn terminal_touch_write_append_and_rm_modify_real_files(){
  let mut fs=RamFs::<1>::new();
  run(&mut fs,0,Command::Touch,b"/src/a",b"").unwrap();
  run(&mut fs,1,Command::Write,b"/src/a",b"one").unwrap();
  run(&mut fs,2,Command::Append,b"/src/a",b"two").unwrap();
  let mut out=[0;6];read(&fs,b"/src/a",&mut out).unwrap();
  assert_eq!(&out,b"onetwo");
  run(&mut fs,3,Command::Remove,b"/src/a",b"").unwrap();
  assert!(fs.metadata(b"/src/a").is_err());
 }
 #[test]fn terminal_append_refuses_content_overflow_without_partial_changes(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",&[1;CONTENT_MAX]).unwrap();
  assert_eq!(run(&mut fs,1,Command::Append,b"/a",b"x"),Err(Error::Full));
  assert_eq!(fs.version(),1);
 }
 #[test]fn stale_writer_and_path_traversal_are_rejected(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"hello").unwrap();
  assert_eq!(run(&mut fs,0,Command::Write,b"/a",b"bad"),Err(Error::Stale));
  assert_eq!(run(&mut fs,1,Command::Remove,b"/../a",b""),Err(Error::Invalid));
 }
}

//! R032/01: transactional three-file Rust project scaffold in a real guest filesystem.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
pub fn create<const F:usize,const B:usize>(fs:&mut RamFs<F>,base:&[u8],
 expected:u64,name:&[u8])->Result<u64,Error>{
 if fs.version()!=expected||name.is_empty()||name.len()>32||
  !name[0].is_ascii_alphabetic()||!name.iter().all(|c|c.is_ascii_alphanumeric()||*c==b'_'){
  return Err(Error::Invalid);
 }
 if !base.starts_with(b"/projects/")||base.ends_with(b"/")||
  base.len()+15>PATH_MAX||fs.count()+3>F{return Err(Error::Full);}
 let parts:[(&[u8],&[u8]);3]=[
  (b"/src/main.rs",b"fn main() {\n}\n"),
  (b"/src/lib.rs",b"pub fn init() {}\n"),
  (b"/README.md",b"# IndianOS Rust workspace\n")
 ];
 let mut image=[0u8;B];let size=fs.export_snapshot(&mut image).map_err(|_|Error::Full)?;
 let mut staged=RamFs::<F>::new();staged.import_snapshot(&image[..size]).map_err(|_|Error::Invalid)?;
 for (suffix,body) in parts{
  let n=base.len()+suffix.len();let mut path=[0u8;PATH_MAX];
  path[..base.len()].copy_from_slice(base);path[base.len()..n].copy_from_slice(suffix);
  if staged.metadata(&path[..n]).is_ok(){return Err(Error::Conflict);}
  staged.replace(&path[..n],body).map_err(|_|Error::Full)?;
 }
 let mut snapshot=[0u8;B];let n=staged.export_snapshot(&mut snapshot).map_err(|_|Error::Full)?;
 fs.import_snapshot(&snapshot[..n]).map_err(|_|Error::Invalid)?;
 Ok(fs.version())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn creates_real_project_tree_and_main_source(){
  let mut fs=RamFs::<4>::new();
  assert_eq!(create::<4,8192>(&mut fs,b"/projects/demo",0,b"demo"),Ok(3));
  assert_eq!(fs.count(),3);
  let mut out=[0u8;20];let n=fs.read(b"/projects/demo/src/main.rs",0,&mut out).unwrap();
  assert_eq!(&out[..n],b"fn main() {\n}\n");
 }
 #[test]fn existing_path_blocks_all_project_files_without_partial_write(){
  let mut fs=RamFs::<4>::new();
  fs.replace(b"/projects/demo/README.md",b"old").unwrap();
  assert_eq!(create::<4,8192>(&mut fs,b"/projects/demo",1,b"demo"),Err(Error::Conflict));
  assert_eq!(fs.count(),1);assert_eq!(fs.version(),1);
 }
}
//! R031/03: multi-file rename of exact identifiers, one all-or-nothing source transaction.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
use crate::workstation_r030::agent_06;
pub struct Target<'a>{pub path:&'a[u8],pub revision:u64}
pub fn rename_project<const F:usize,const B:usize>(fs:&mut RamFs<F>,expected:u64,
 targets:&[Target<'_>],old:&[u8],new:&[u8])->Result<usize,Error>{
 if fs.version()!=expected{return Err(Error::Stale);}
 if targets.is_empty()||targets.len()>F{return Err(Error::Invalid);}
 for (i,t) in targets.iter().enumerate(){
  if !t.path.starts_with(b"/projects/")||
   targets[..i].iter().any(|p|p.path==t.path){return Err(Error::Denied);}
  if fs.metadata(t.path).map_err(|_|Error::Missing)?.0!=t.revision{
   return Err(Error::Stale);
  }
 }
 let mut bytes=[0u8;B];let n=fs.export_snapshot(&mut bytes).map_err(|_|Error::Full)?;
 let mut staged=RamFs::<F>::new();
 staged.import_snapshot(&bytes[..n]).map_err(|_|Error::Invalid)?;
 let mut changed=0;
 for t in targets{
  changed+=agent_06::rename(&mut staged,t.path,t.revision,old,new)
   .map_err(|_|Error::Conflict)?;
 }
 if changed==0{return Ok(0);}
 let mut output=[0u8;B];let n=staged.export_snapshot(&mut output).map_err(|_|Error::Full)?;
 fs.import_snapshot(&output[..n]).map_err(|_|Error::Invalid)?;Ok(changed)
}
pub fn read_file<const F:usize>(fs:&RamFs<F>,path:&[u8],out:&mut[u8;CONTENT_MAX])
 ->Result<usize,Error>{
 let (_,len)=fs.metadata(path).map_err(|_|Error::Missing)?;
 fs.read(path,0,&mut out[..len]).map_err(|_|Error::Missing)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn renames_identifier_across_two_real_source_documents(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/projects/p/a.rs",b"fn old() {}").unwrap();
  fs.replace(b"/projects/p/b.rs",b"old()").unwrap();
  let t=[Target{path:b"/projects/p/a.rs",revision:1},
    Target{path:b"/projects/p/b.rs",revision:2}];
  assert_eq!(rename_project::<2,4096>(&mut fs,2,&t,b"old",b"new"),Ok(2));
  let mut buf=[0u8;CONTENT_MAX];let n=read_file(&fs,b"/projects/p/b.rs",&mut buf).unwrap();
  assert_eq!(&buf[..n],b"new()");
 }
 #[test]fn failed_second_revision_preserves_first_file_original(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/projects/p/a.rs",b"old").unwrap();fs.replace(b"/projects/p/b.rs",b"old").unwrap();
  let t=[Target{path:b"/projects/p/a.rs",revision:1},Target{path:b"/projects/p/b.rs",revision:1}];
  assert_eq!(rename_project::<2,4096>(&mut fs,2,&t,b"old",b"new"),Err(Error::Stale));
  assert_eq!(fs.version(),2);
 }
}
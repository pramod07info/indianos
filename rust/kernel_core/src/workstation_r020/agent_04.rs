//! R020/04 atomic project-wide literal symbol rename on actual RamFs data.
//! The caller must authorize every file; byte search is not a language AST.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
pub fn rename_all<const F:usize,const IMAGE:usize>(
 fs:&mut RamFs<F>,expected_revision:u64,from:&[u8],to:&[u8])
 ->Result<usize,Error>{
 if from.is_empty()||from.len()>64||to.len()>64||from==to||
  !from.iter().all(|b|b.is_ascii_alphanumeric()||*b==b'_')||
  !to.iter().all(|b|b.is_ascii_alphanumeric()||*b==b'_'){
  return Err(Error::Invalid);
 }
 if expected_revision!=fs.version(){return Err(Error::Stale);}
 let mut image=[0u8;IMAGE];let n=fs.export_snapshot(&mut image)
  .map_err(|_|Error::Full)?;
 let mut staged=RamFs::<F>::new();
 staged.import_snapshot(&image[..n]).map_err(|_|Error::Invalid)?;
 let mut changes=0usize;
 for i in 0..staged.count(){
  let mut path=[0u8;PATH_MAX];
  let (pathlen,len,_) = staged.entry(i,&mut path)
    .map_err(|_|Error::Invalid)?.ok_or(Error::Missing)?;
  let mut text=[0u8;CONTENT_MAX];
  if staged.read(&path[..pathlen],0,&mut text[..len])
    .map_err(|_|Error::Missing)?!=len{return Err(Error::Stale);}
  let mut updated=[0u8;CONTENT_MAX];let mut src=0;let mut dst=0;
  while src<len{
   let matches=src+from.len()<=len&&&text[src..src+from.len()]==from;
   let boundary_left=src==0||!(text[src-1].is_ascii_alphanumeric()||text[src-1]==b'_');
   let right=src+from.len();
   let boundary_right=right>=len||!(text[right].is_ascii_alphanumeric()||text[right]==b'_');
   if matches&&boundary_left&&boundary_right{
    if dst+to.len()>CONTENT_MAX{return Err(Error::Full);}
    updated[dst..dst+to.len()].copy_from_slice(to);
    dst+=to.len();src+=from.len();changes+=1;
   }else{
    if dst==CONTENT_MAX{return Err(Error::Full);}
    updated[dst]=text[src];dst+=1;src+=1;
   }
  }
  if dst!=len||&updated[..dst]!=&text[..len]{
   staged.replace(&path[..pathlen],&updated[..dst])
     .map_err(|_|Error::Full)?;
  }
 }
 let mut result=[0u8;IMAGE];let used=staged.export_snapshot(&mut result)
  .map_err(|_|Error::Full)?;
 if fs.version()!=expected_revision{return Err(Error::Stale);}
 fs.import_snapshot(&result[..used]).map_err(|_|Error::Invalid)?;
 Ok(changes)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn source_refactor_updates_related_files_atomically(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/one.rs",b"fn foo(){}").unwrap();
  fs.replace(b"/two.rs",b"foo fooBar").unwrap();
  assert_eq!(rename_all::<2,4096>(&mut fs,2,b"foo",b"bar"),Ok(2));
  let mut out=[0u8;10];let n=fs.read(b"/two.rs",0,&mut out).unwrap();
  assert_eq!(&out[..n],b"bar fooBar");
 }
 #[test]fn destination_capacity_failure_does_not_edit_any_file(){
  let mut fs=RamFs::<1>::new();
  let mut tokens=[b' ';CONTENT_MAX];
  for i in (0..CONTENT_MAX).step_by(2){tokens[i]=b'a';}
  fs.replace(b"/a",&tokens).unwrap();
  assert_eq!(rename_all::<1,4096>(&mut fs,1,b"a",b"long"),
    Err(Error::Full));assert_eq!(fs.version(),1);
 }
 #[test]fn stale_revision_and_non_identifier_input_are_rejected(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"old").unwrap();
  assert_eq!(rename_all::<1,512>(&mut fs,0,b"old",b"new"),Err(Error::Stale));
  assert_eq!(rename_all::<1,512>(&mut fs,1,b"old",b"bad/path"),Err(Error::Invalid));
 }
}

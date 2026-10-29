//! R032/05: atomic multi-file UTF-8 find-and-replace over actual guest project files.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
fn apply(source:&[u8],find:&[u8],replace:&[u8],out:&mut[u8;CONTENT_MAX])
 ->Result<(usize,usize),Error>{
 if find.is_empty(){return Err(Error::Invalid);}
 let(mut i,mut w,mut hits)=(0usize,0usize,0usize);
 while i<source.len(){
  let yes=i+find.len()<=source.len()&&&source[i..i+find.len()]==find;
  let chunk=if yes{replace}else{&source[i..i+1]};
  if w.checked_add(chunk.len()).is_none_or(|n|n>CONTENT_MAX){return Err(Error::Full);}
  out[w..w+chunk.len()].copy_from_slice(chunk);w+=chunk.len();
  if yes{i+=find.len();hits+=1;}else{i+=1;}
 }
 Ok((w,hits))
}
pub fn replace_project<const F:usize,const B:usize>(fs:&mut RamFs<F>,root:&[u8],
 expected:u64,find:&[u8],replace:&[u8])->Result<(usize,usize),Error>{
 if fs.version()!=expected||root.len()<2||root[0]!=b'/'||
  find.is_empty()||core::str::from_utf8(find).is_err()||
  core::str::from_utf8(replace).is_err(){return Err(Error::Invalid);}
 let mut raw=[0u8;B];let n=fs.export_snapshot(&mut raw).map_err(|_|Error::Full)?;
 let mut staged=RamFs::<F>::new();staged.import_snapshot(&raw[..n]).map_err(|_|Error::Invalid)?;
 let(mut files,mut total)=(0usize,0usize);
 for i in 0..fs.count(){
  let mut path=[0u8;PATH_MAX];
  let (len,size,_)=fs.entry(i,&mut path).map_err(|_|Error::Invalid)?
   .ok_or(Error::Missing)?;
  if !path[..len].starts_with(root)||path.get(root.len())!=Some(&b'/')||
   !path[..len].ends_with(b".rs"){continue;}
  let mut original=[0u8;CONTENT_MAX];fs.read(&path[..len],0,&mut original[..size])
   .map_err(|_|Error::Missing)?;
  if core::str::from_utf8(&original[..size]).is_err(){return Err(Error::Invalid);}
  let mut modified=[0u8;CONTENT_MAX];
  let (bytes,hits)=apply(&original[..size],find,replace,&mut modified)?;
  if hits>0{
   staged.replace(&path[..len],&modified[..bytes]).map_err(|_|Error::Full)?;
   files+=1;total+=hits;
  }
 }
 if total==0{return Ok((0,0));}
 let mut image=[0u8;B];let n=staged.export_snapshot(&mut image).map_err(|_|Error::Full)?;
 fs.import_snapshot(&image[..n]).map_err(|_|Error::Invalid)?;Ok((files,total))
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn updates_multiple_source_documents_as_one_atomic_generation(){
  let mut fs=RamFs::<3>::new();
  fs.replace(b"/p/a.rs",b"call call").unwrap();fs.replace(b"/p/b.rs",b"call").unwrap();
  fs.replace(b"/q/hidden.rs",b"call").unwrap();
  assert_eq!(replace_project::<3,8192>(&mut fs,b"/p",3,b"call",b"run"),Ok((2,3)));
  let mut buf=[0;7];fs.read(b"/p/a.rs",0,&mut buf).unwrap();assert_eq!(&buf,b"run run");
  let mut hidden=[0;4];fs.read(b"/q/hidden.rs",0,&mut hidden).unwrap();
  assert_eq!(&hidden,b"call");
 }
 #[test]fn expansion_over_capacity_rolls_back_all_source_files(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/p/a.rs",b"a").unwrap();
  fs.replace(b"/p/b.rs",&[b'a';CONTENT_MAX]).unwrap();
  assert_eq!(replace_project::<2,8192>(&mut fs,b"/p",2,b"a",b"AAAA"),Err(Error::Full));
  assert_eq!(fs.version(),2);
 }
}
//! R019/13 hierarchical navigator backed by actual RamFs filenames.
//! It does not equate path spelling with authority or follow symlinks.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Item{pub path:[u8;PATH_MAX],pub len:usize,
 pub bytes:usize,pub version:u64,pub depth:u8}
pub const EMPTY:Item=Item{path:[0;PATH_MAX],len:0,bytes:0,version:0,depth:0};
pub fn children<const F:usize,const O:usize>(fs:&RamFs<F>,
 directory:&[u8],recursive:bool,out:&mut[Item;O])->Result<usize,Error>{
 if directory.is_empty()||directory[0]!=b'/'||
  directory.windows(2).any(|b|b==b".."){return Err(Error::Invalid);}
 if directory!=b"/"&&directory.ends_with(b"/"){return Err(Error::Invalid);}
 let mut staged=[EMPTY;O];let mut count=0;
 for index in 0..fs.count(){
  let mut path=[0;PATH_MAX];
  let (len,bytes,version)=fs.entry(index,&mut path).map_err(|_|Error::Full)?
   .ok_or(Error::Missing)?;
  let name=&path[..len];
  let suffix=if directory==b"/"{
   if len<2{continue;}&name[1..]
  }else if name.starts_with(directory)&&name.len()>directory.len()&&
     name[directory.len()]==b'/'{
   &name[directory.len()+1..]
  }else{continue};
  if suffix.is_empty(){continue;}
  let depth=suffix.iter().filter(|b|**b==b'/').count();
  if !recursive&&depth!=0{continue;}
  if depth>=u8::MAX as usize{return Err(Error::Full);}
  if count>=O{return Err(Error::Full);}
  staged[count]=Item{path,len,bytes,version,depth:depth as u8};count+=1;
 }
 *out=staged;Ok(count)
}
pub fn selected_path<'a>(item:&'a Item)->&'a[u8]{&item.path[..item.len]}
pub fn total_bytes(items:&[Item])->usize{items.iter().map(|i|i.bytes).sum()}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn folder_tree_filters_real_nested_project_paths(){
  let mut fs=RamFs::<3>::new();
  fs.replace(b"/src/main.rs",b"main").unwrap();
  fs.replace(b"/src/lib.rs",b"lib").unwrap();
  fs.replace(b"/tests/a.rs",b"x").unwrap();
  let mut out=[EMPTY;3];
  assert_eq!(children(&fs,b"/src",false,&mut out),Ok(2));
  assert_eq!(total_bytes(&out[..2]),7);
  assert_eq!(selected_path(&out[0]),b"/src/main.rs");
 }
 #[test]fn root_nonrecursive_ignores_nested_files(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/a",b"a").unwrap();fs.replace(b"/src/a",b"b").unwrap();
  let mut out=[EMPTY;2];
  assert_eq!(children(&fs,b"/",false,&mut out),Ok(1));
  assert_eq!(children(&fs,b"/",true,&mut out),Ok(2));
 }
 #[test]fn too_small_destination_never_leaks_partial_tree(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/src/a",b"a").unwrap();fs.replace(b"/src/b",b"b").unwrap();
  let mut out=[Item{bytes:99,..EMPTY};1];
  assert_eq!(children(&fs,b"/src",true,&mut out),Err(Error::Full));
  assert_eq!(out[0].bytes,99);
 }
}

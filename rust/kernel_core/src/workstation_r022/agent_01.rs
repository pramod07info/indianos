//! R022/01: project tree view over real guest RamFs files.
//! All paths and sizes come from kernel file metadata, not synthetic UI data.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Node{pub path:[u8;PATH_MAX],pub len:usize,pub size:usize,pub revision:u64}
pub const EMPTY:Node=Node{path:[0;PATH_MAX],len:0,size:0,revision:0};
fn under(path:&[u8],prefix:&[u8])->bool{
 path.starts_with(prefix)&&(prefix==b"/"||path.len()==prefix.len()||
 path.get(prefix.len())==Some(&b'/'))
}
pub fn enumerate<const F:usize,const O:usize>(
 fs:&RamFs<F>,prefix:&[u8],out:&mut[Node;O])->Result<usize,Error>{
 if prefix.is_empty()||prefix.len()>PATH_MAX||prefix[0]!=b'/'||
  prefix.windows(2).any(|p|p==b".."){return Err(Error::Invalid);}
 let mut staged=[EMPTY;O];let mut count=0;
 for i in 0..fs.count(){
  let mut path=[0u8;PATH_MAX];
  let (len,size,revision)=fs.entry(i,&mut path).map_err(|_|Error::Invalid)?
   .ok_or(Error::Missing)?;
  if under(&path[..len],prefix){
   if count==O{return Err(Error::Full);}
   staged[count]=Node{path,len,size,revision};count+=1;
  }
 }
 staged[..count].sort_unstable_by(|a,b|a.path[..a.len].cmp(&b.path[..b.len]));
 *out=staged;Ok(count)
}
pub fn project_bytes(items:&[Node])->Result<usize,Error>{
 let mut total=0usize;
 for item in items{total=total.checked_add(item.size).ok_or(Error::Overflow)?;}Ok(total)
}
pub fn extension_count(items:&[Node],extension:&[u8])->usize{
 items.iter().filter(|n|n.len>=extension.len()&&
  &n.path[n.len-extension.len()..n.len]==extension).count()
}
pub fn find<'a>(items:&'a[Node],path:&[u8])->Option<&'a Node>{
 items.iter().find(|n|&n.path[..n.len]==path)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn lists_existing_files_sorted_with_real_sizes_and_revisions(){
  let mut fs=RamFs::<3>::new();
  fs.replace(b"/src/b.rs",b"bb").unwrap();
  fs.replace(b"/src/a.rs",b"a").unwrap();
  fs.replace(b"/docs/readme",b"readme").unwrap();
  let mut out=[EMPTY;3];
  assert_eq!(enumerate(&fs,b"/src",&mut out),Ok(2));
  assert_eq!(&out[0].path[..out[0].len],b"/src/a.rs");
  assert_eq!(project_bytes(&out[..2]),Ok(3));
  assert_eq!(extension_count(&out[..2],b".rs"),2);
 }
 #[test]fn prefix_boundary_does_not_leak_sibling_project_files(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/src/a",b"x").unwrap();fs.replace(b"/srcx/secret",b"x").unwrap();
  let mut out=[EMPTY;2];assert_eq!(enumerate(&fs,b"/src",&mut out),Ok(1));
 }
 #[test]fn insufficient_result_capacity_preserves_previous_view(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/a",b"a").unwrap();fs.replace(b"/b",b"b").unwrap();
  let marker=Node{revision:99,..EMPTY};let mut out=[marker;1];
  assert_eq!(enumerate(&fs,b"/",&mut out),Err(Error::Full));
  assert_eq!(out,[marker]);
 }
}

//! R027/01: bounded real project source inventory with nested folder filtering.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Entry{pub path:[u8;PATH_MAX],pub len:usize,pub bytes:usize,
 pub revision:u64,pub depth:usize}
pub const EMPTY:Entry=Entry{path:[0;PATH_MAX],len:0,bytes:0,revision:0,depth:0};
pub fn list<const F:usize,const O:usize>(fs:&RamFs<F>,root:&[u8],
 max_depth:usize,out:&mut[Entry;O])->Result<usize,Error>{
 if root.is_empty()||root[0]!=b'/'||root.len()>PATH_MAX||
 root.windows(2).any(|x|x==b".."){return Err(Error::Invalid);}
 let mut results=[EMPTY;O];let mut count=0;
 for i in 0..fs.count(){
  let mut name=[0u8;PATH_MAX];
  let (len,size,rev)=fs.entry(i,&mut name).map_err(|_|Error::Invalid)?.ok_or(Error::Missing)?;
  if !name[..len].starts_with(root)||!(root==b"/"||
    name.get(root.len())==Some(&b'/')){continue;}
  let rest=if root==b"/"{&name[1..len]}else{&name[root.len()+1..len]};
  let depth=rest.iter().filter(|&&b|b==b'/').count()+1;
  if depth>max_depth{continue;}
  if count>=O{return Err(Error::Full);}
  results[count]=Entry{path:name,len,bytes:size,revision:rev,depth};count+=1;
 }
 results[..count].sort_unstable_by(|a,b|a.path[..a.len].cmp(&b.path[..b.len]));
 *out=results;Ok(count)
}
pub fn total_bytes(items:&[Entry])->Result<usize,Error>{
 items.iter().try_fold(0usize,|acc,x|acc.checked_add(x.bytes).ok_or(Error::Overflow))
}
pub fn source_count(items:&[Entry])->usize{
 items.iter().filter(|x|x.path[..x.len].ends_with(b".rs")).count()
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn project_inventory_separates_real_neighboring_roots(){
  let mut fs=RamFs::<3>::new();
  fs.replace(b"/p/main.rs",b"source").unwrap();
  fs.replace(b"/p/src/a.rs",b"a").unwrap();
  fs.replace(b"/p2/secret.rs",b"other").unwrap();
  let mut out=[EMPTY;3];
  assert_eq!(list(&fs,b"/p",2,&mut out),Ok(2));
  assert_eq!(total_bytes(&out[..2]),Ok(7));assert_eq!(source_count(&out[..2]),2);
 }
 #[test]fn max_depth_hides_nested_files_not_direct_ones(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/p/root",b"a").unwrap();
  fs.replace(b"/p/src/file",b"b").unwrap();
  let mut out=[EMPTY;2];assert_eq!(list(&fs,b"/p",1,&mut out),Ok(1));
 }
 #[test]fn failed_capacity_check_keeps_previous_table(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/p/a",b"a").unwrap();fs.replace(b"/p/b",b"b").unwrap();
  let mut out=[Entry{depth:77,..EMPTY};1];
  assert_eq!(list(&fs,b"/p",2,&mut out),Err(Error::Full));
  assert_eq!(out[0].depth,77);
 }
}

//! R026/05: filter, sort and summarize real guest file metadata for explorer.
//! Bounded records return exact source revisions and file lengths.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Item{pub path:[u8;PATH_MAX],pub len:usize,
 pub size:usize,pub revision:u64}
pub const EMPTY:Item=Item{path:[0;PATH_MAX],len:0,size:0,revision:0};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Sort{Name,Smallest,Largest,Newest}
pub fn query<const F:usize,const O:usize>(fs:&RamFs<F>,prefix:&[u8],
 extension:&[u8],min_bytes:usize,sort:Sort,out:&mut[Item;O])
 ->Result<usize,Error>{
 if prefix.first()!=Some(&b'/')||prefix.len()>PATH_MAX{return Err(Error::Invalid);}
 let mut stage=[EMPTY;O];let mut n=0;
 for i in 0..fs.count(){
  let mut path=[0;PATH_MAX];
  let (len,size,revision)=fs.entry(i,&mut path).map_err(|_|Error::Invalid)?
   .ok_or(Error::Missing)?;
  if !path[..len].starts_with(prefix)||size<min_bytes||
   !path[..len].ends_with(extension){continue;}
  if n==O{return Err(Error::Full);}
  stage[n]=Item{path,len,size,revision};n+=1;
 }
 stage[..n].sort_unstable_by(|a,b|match sort{
  Sort::Name=>a.path[..a.len].cmp(&b.path[..b.len]),
  Sort::Smallest=>a.size.cmp(&b.size),
  Sort::Largest=>b.size.cmp(&a.size),
  Sort::Newest=>b.revision.cmp(&a.revision)
 });
 *out=stage;Ok(n)
}
pub fn total(items:&[Item])->Result<usize,Error>{
 items.iter().try_fold(0usize,|acc,i|acc.checked_add(i.size).ok_or(Error::Overflow))
}
pub fn largest(items:&[Item])->Option<Item>{items.iter().copied().max_by_key(|i|i.size)}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn explorer_filters_real_rust_files_by_size_and_orders_largest_first(){
  let mut fs=RamFs::<3>::new();
  fs.replace(b"/p/a.rs",b"123").unwrap();
  fs.replace(b"/p/b.rs",b"123456").unwrap();
  fs.replace(b"/p/readme.md",b"ignore").unwrap();
  let mut items=[EMPTY;3];
  assert_eq!(query(&fs,b"/p",b".rs",2,Sort::Largest,&mut items),Ok(2));
  assert_eq!(items[0].size,6);assert_eq!(total(&items[..2]),Ok(9));
 }
 #[test]fn missing_extension_results_in_empty_file_table(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a.rs",b"a").unwrap();
  let mut out=[EMPTY;1];assert_eq!(query(&fs,b"/",b".txt",0,Sort::Name,&mut out),Ok(0));
 }
 #[test]fn insufficient_output_capacity_preserves_previous_metadata(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/a",b"a").unwrap();
  fs.replace(b"/b",b"b").unwrap();
  let mut out=[Item{revision:99,..EMPTY};1];
  assert_eq!(query(&fs,b"/",b"",0,Sort::Name,&mut out),Err(Error::Full));
  assert_eq!(out[0].revision,99);
 }
}

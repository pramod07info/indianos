//! R030/04: exact duplicate file detection, content equality checked to avoid hash collisions.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Pair{pub left:[u8;PATH_MAX],pub l:usize,pub right:[u8;PATH_MAX],
 pub r:usize,pub size:usize}
pub const EMPTY:Pair=Pair{left:[0;PATH_MAX],l:0,right:[0;PATH_MAX],r:0,size:0};
pub fn scan<const F:usize,const N:usize>(fs:&RamFs<F>,
 root:&[u8],out:&mut[Pair;N])->Result<usize,Error>{
 if root.first()!=Some(&b'/'){return Err(Error::Invalid);}
 let mut stage=[EMPTY;N];let mut n=0;
 for i in 0..fs.count(){
  let mut first=[0;PATH_MAX];
  let (a,asize,_)=fs.entry(i,&mut first).map_err(|_|Error::Invalid)?
   .ok_or(Error::Missing)?;
  if !first[..a].starts_with(root){continue;}
  let mut left=[0u8;CONTENT_MAX];
  fs.read(&first[..a],0,&mut left[..asize]).map_err(|_|Error::Missing)?;
  for j in i+1..fs.count(){
   let mut second=[0;PATH_MAX];
   let(b,bsize,_)=fs.entry(j,&mut second).map_err(|_|Error::Invalid)?
    .ok_or(Error::Missing)?;
   if asize!=bsize||!second[..b].starts_with(root){continue;}
   let mut right=[0u8;CONTENT_MAX];
   fs.read(&second[..b],0,&mut right[..bsize]).map_err(|_|Error::Missing)?;
   if left[..asize]!=right[..bsize]{continue;}
   if n==N{return Err(Error::Full);}
   stage[n]=Pair{left:first,l:a,right:second,r:b,size:asize};n+=1;
  }
 }
 *out=stage;Ok(n)
}
pub fn duplicate_bytes(items:&[Pair])->Result<usize,Error>{
 items.iter().try_fold(0usize,|n,p|n.checked_add(p.size).ok_or(Error::Overflow))
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn exact_duplicate_detector_finds_identical_project_file_contents(){
  let mut fs=RamFs::<3>::new();fs.replace(b"/p/a",b"abc").unwrap();
  fs.replace(b"/p/b",b"abc").unwrap();fs.replace(b"/p/c",b"other").unwrap();
  let mut out=[EMPTY;2];
  assert_eq!(scan(&fs,b"/p",&mut out),Ok(1));
  assert_eq!(duplicate_bytes(&out[..1]),Ok(3));
 }
 #[test]fn too_many_duplicates_refuse_partial_result_publication(){
  let mut fs=RamFs::<3>::new();
  fs.replace(b"/p/a",b"x").unwrap();fs.replace(b"/p/b",b"x").unwrap();
  fs.replace(b"/p/c",b"x").unwrap();
  let mut out=[Pair{size:99,..EMPTY};1];
  assert_eq!(scan(&fs,b"/p",&mut out),Err(Error::Full));
  assert_eq!(out[0].size,99);
 }
}
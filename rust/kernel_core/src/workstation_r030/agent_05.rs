//! R030/05: ranked fuzzy finder for real workspace paths (UTF-8 byte-token match).
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Match{pub path:[u8;PATH_MAX],pub len:usize,pub score:u32,pub revision:u64}
pub const EMPTY:Match=Match{path:[0;PATH_MAX],len:0,score:0,revision:0};
pub fn score(path:&[u8],query:&[u8])->Option<u32>{
 if query.is_empty()||query.len()>PATH_MAX{return None;}
 let mut pos=0;let mut gaps=0u32;let mut first=0;
 for (i,&letter) in query.iter().enumerate(){
  let offset=path.get(pos..)?.iter().position(|x|x.to_ascii_lowercase()==letter.to_ascii_lowercase())?;
  gaps=gaps.saturating_add(offset as u32);
  if i==0{first=pos+offset;}
  pos+=offset+1;
 }
 Some((1000u32).saturating_sub(gaps*5+first as u32))
}
pub fn search<const F:usize,const N:usize>(fs:&RamFs<F>,root:&[u8],
 query:&[u8],out:&mut[Match;N])->Result<usize,Error>{
 if root.first()!=Some(&b'/')||query.is_empty(){return Err(Error::Invalid);}
 let mut candidates=[EMPTY;F];let mut n=0;
 for i in 0..fs.count(){
  let mut path=[0;PATH_MAX];
  let(len,_,revision)=fs.entry(i,&mut path).map_err(|_|Error::Invalid)?
   .ok_or(Error::Missing)?;
  if !path[..len].starts_with(root){continue;}
  if let Some(value)=score(&path[..len],query){
   candidates[n]=Match{path,len,score:value,revision};n+=1;
  }
 }
 candidates[..n].sort_unstable_by(|a,b|b.score.cmp(&a.score)
  .then(a.path[..a.len].cmp(&b.path[..b.len])));
 let count=n.min(N);out[..count].copy_from_slice(&candidates[..count]);Ok(count)
}
pub fn still_current<const F:usize>(fs:&RamFs<F>,item:Match)->bool{
 fs.metadata(&item.path[..item.len]).is_ok_and(|(rev,_)|rev==item.revision)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn fuzzy_query_ranks_real_project_path_candidates(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/p/main.rs",b"a").unwrap();fs.replace(b"/p/manage.rs",b"b").unwrap();
  let mut hits=[EMPTY;2];assert_eq!(search(&fs,b"/p",b"main",&mut hits),Ok(1));
  assert_eq!(&hits[0].path[..hits[0].len],b"/p/main.rs");
 }
 #[test]fn fuzzy_result_invalidates_after_source_generation_changes(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/main.rs",b"x").unwrap();
  let mut out=[EMPTY;1];search(&fs,b"/p",b"main",&mut out).unwrap();
  fs.replace(b"/p/main.rs",b"new").unwrap();
  assert!(!still_current(&fs,out[0]));
 }
}
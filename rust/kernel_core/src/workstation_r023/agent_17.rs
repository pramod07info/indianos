//! R023/17: ranked source-file search for project explorer / command palette.
//! Uses actual paths and content size, not external search engines.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Match{pub path:[u8;PATH_MAX],pub len:usize,pub score:u32,
 pub offset:usize,pub revision:u64}
pub const EMPTY:Match=Match{path:[0;PATH_MAX],len:0,score:0,offset:0,revision:0};
fn lower(b:u8)->u8{b.to_ascii_lowercase()}
fn contains_folded(text:&[u8],pattern:&[u8])->Option<usize>{
 if pattern.len()>text.len(){return None;}
 text.windows(pattern.len()).position(|w|
  w.iter().zip(pattern.iter()).all(|(&a,&b)|lower(a)==lower(b)))
}
fn score(name:&[u8],query:&[u8],content:&[u8])->Option<(u32,usize)>{
 if name.is_empty()||query.is_empty(){return None;}
 let basename=name.iter().rposition(|b|*b==b'/')
  .map(|i|&name[i+1..]).unwrap_or(name);
 if let Some(pos)=contains_folded(basename,query){
  return Some((1000u32.saturating_sub(pos as u32),pos));
 }
 if let Some(pos)=contains_folded(name,query){
  return Some((500u32.saturating_sub(pos as u32),pos));
 }
 if let Some(pos)=contains_folded(content,query){
  return Some((100u32.saturating_sub(pos.min(99) as u32),pos));
 }
 None
}
pub fn rank<const F:usize,const O:usize>(fs:&RamFs<F>,
 query:&[u8],out:&mut[Match;O])->Result<usize,Error>{
 if query.is_empty()||query.len()>64{return Err(Error::Invalid);}
 let mut stage=[EMPTY;O];let mut count=0;
 for i in 0..fs.count(){
  let mut path=[0u8;PATH_MAX];
  let (len,size,revision)=fs.entry(i,&mut path).map_err(|_|Error::Invalid)?
   .ok_or(Error::Missing)?;
  let mut content=[0u8;CONTENT_MAX];
  fs.read(&path[..len],0,&mut content[..size]).map_err(|_|Error::Missing)?;
  if let Some((rating,offset))=score(&path[..len],query,&content[..size]){
   if count==O{return Err(Error::Full);}
   stage[count]=Match{path,len,score:rating,offset,revision};count+=1;
  }
 }
 stage[..count].sort_unstable_by(|a,b|b.score.cmp(&a.score)
  .then_with(||a.path[..a.len].cmp(&b.path[..b.len])));
 *out=stage;Ok(count)
}
pub fn first<'a>(items:&'a[Match])->Option<&'a[u8]>{
 items.first().map(|m|&m.path[..m.len])
}
pub fn current<const F:usize>(fs:&RamFs<F>,item:Match)->bool{
 fs.metadata(&item.path[..item.len]).is_ok_and(|(v,_)|v==item.revision)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn basename_match_outranks_body_only_project_search(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/src/search.rs",b"nothing").unwrap();
  fs.replace(b"/src/lib.rs",b"search source").unwrap();
  let mut out=[EMPTY;2];
  assert_eq!(rank(&fs,b"search",&mut out),Ok(2));
  assert_eq!(first(&out[..2]),Some(b"/src/search.rs".as_slice()));
 }
 #[test]fn stale_source_reference_is_not_treated_as_current(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"search").unwrap();
  let mut out=[EMPTY;1];rank(&fs,b"search",&mut out).unwrap();
  fs.replace(b"/a",b"new").unwrap();
  assert!(!current(&fs,out[0]));
 }
 #[test]fn overflowing_result_limit_preserves_previous_ranked_list(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/a",b"found").unwrap();fs.replace(b"/b",b"found").unwrap();
  let marked=Match{score:777,..EMPTY};let mut out=[marked;1];
  assert_eq!(rank(&fs,b"found",&mut out),Err(Error::Full));
  assert_eq!(out,[marked]);
 }
}

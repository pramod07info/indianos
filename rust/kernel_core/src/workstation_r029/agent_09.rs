//! R029/09: search across all live project source files and report exact byte/line hits.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Hit{pub path:[u8;PATH_MAX],pub len:usize,pub line:u32,
 pub byte:usize,pub revision:u64}
pub const EMPTY:Hit=Hit{path:[0;PATH_MAX],len:0,line:0,byte:0,revision:0};
pub fn search<const F:usize,const N:usize>(fs:&RamFs<F>,root:&[u8],
 needle:&[u8],out:&mut[Hit;N])->Result<usize,Error>{
 if root.len()<2||root[0]!=b'/'||needle.is_empty(){return Err(Error::Invalid);}
 let mut result=[EMPTY;N];let mut n=0;
 for i in 0..fs.count(){
  let mut name=[0u8;PATH_MAX];let (len,size,rev)=fs.entry(i,&mut name)
   .map_err(|_|Error::Invalid)?.ok_or(Error::Missing)?;
  if !name[..len].starts_with(root)||name.get(root.len())!=Some(&b'/')||
   !name[..len].ends_with(b".rs"){continue;}
  let mut text=[0u8;CONTENT_MAX];
  fs.read(&name[..len],0,&mut text[..size]).map_err(|_|Error::Missing)?;
  let mut line=1u32;
  if needle.len()<=size{
   for pos in 0..=size-needle.len(){
    if &text[pos..pos+needle.len()]==needle{
     if n==N{return Err(Error::Full);}
     result[n]=Hit{path:name,len,line,byte:pos,revision:rev};n+=1;
    }
    if text[pos]==b'\n'{line=line.saturating_add(1);}
   }
  }
 }
 result[..n].sort_unstable_by(|a,b|a.path[..a.len].cmp(&b.path[..b.len])
  .then(a.byte.cmp(&b.byte)));
 *out=result;Ok(n)
}
pub fn valid<const F:usize>(fs:&RamFs<F>,hit:Hit)->bool{
 fs.metadata(&hit.path[..hit.len]).is_ok_and(|(rev,_)|rev==hit.revision)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn workspace_search_returns_file_paths_and_line_numbers(){
  let mut fs=RamFs::<3>::new();
  fs.replace(b"/p/a.rs",b"one\nfind\n").unwrap();
  fs.replace(b"/p/b.rs",b"find").unwrap();fs.replace(b"/q/c.rs",b"find").unwrap();
  let mut hits=[EMPTY;3];
  assert_eq!(search(&fs,b"/p",b"find",&mut hits),Ok(2));
  assert_eq!(hits[0].line,2);
 }
 #[test]fn stale_search_hit_cannot_navigate_changed_source(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a.rs",b"foo").unwrap();
  let mut hits=[EMPTY;1];search(&fs,b"/p",b"foo",&mut hits).unwrap();
  fs.replace(b"/p/a.rs",b"bar").unwrap();assert!(!valid(&fs,hits[0]));
 }
}
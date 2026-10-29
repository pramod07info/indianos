//! R031/05: changed-source inventory against a verified project revision baseline.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
use crate::workstation_r030::agent_01;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Baseline{pub path:[u8;PATH_MAX],pub len:usize,pub generation:u64}
pub const EMPTY:Baseline=Baseline{path:[0;PATH_MAX],len:0,generation:0};
pub fn capture<const F:usize,const N:usize>(fs:&RamFs<F>,root:&[u8],
 out:&mut[Baseline;N])->Result<usize,Error>{
 let mut entries=[agent_01::EMPTY;N];
 let n=agent_01::inventory(fs,root,&mut entries).map_err(|e|match e{crate::workstation_r030::Error::Full=>Error::Full,_=>Error::Invalid})?;
 let mut b=[EMPTY;N];
 for i in 0..n{b[i]=Baseline{path:entries[i].path,len:entries[i].len,
  generation:entries[i].revision};}
 *out=b;Ok(n)
}
pub fn changed<const F:usize>(fs:&RamFs<F>,old:&[Baseline],
 out:&mut[usize])->Result<usize,Error>{
 let mut changed=0;
 for (i,b) in old.iter().enumerate(){
  if b.len==0{continue;}
  let stale=!fs.metadata(&b.path[..b.len]).is_ok_and(|(v,_)|v==b.generation);
  if stale{if changed==out.len(){return Err(Error::Full);}
   out[changed]=i;changed+=1;}
 }
 Ok(changed)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn tracks_specific_changed_file_after_real_guest_edit(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/p/a",b"a").unwrap();fs.replace(b"/p/b",b"b").unwrap();
  let mut base=[EMPTY;2];let n=capture(&fs,b"/p",&mut base).unwrap();
  fs.replace(b"/p/b",b"B").unwrap();let mut out=[0usize;2];
  assert_eq!(changed(&fs,&base[..n],&mut out),Ok(1));assert_eq!(out[0],1);
 }
 #[test]fn unchanged_project_has_no_false_dirty_records(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"a").unwrap();
  let mut base=[EMPTY;1];capture(&fs,b"/p",&mut base).unwrap();
  assert_eq!(changed(&fs,&base,&mut[0;1]),Ok(0));
 }
}
//! R024/12: terminal working-directory and canonical path resolution.
//! Disallows traversal outside root; never resolves symlinks or performs host IO.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Path{pub bytes:[u8;PATH_MAX],pub len:usize}
pub const ROOT:Path=Path{bytes:[b'/';PATH_MAX],len:1};
pub fn normalize(cwd:&[u8],input:&[u8])->Result<Path,Error>{
 if cwd.first()!=Some(&b'/')||input.is_empty()||input.len()>PATH_MAX||
  cwd.len()>PATH_MAX{return Err(Error::Invalid);}
 let mut merged=[0u8;PATH_MAX*2];let mut n=0;
 if input[0]!=b'/'{
  merged[..cwd.len()].copy_from_slice(cwd);n=cwd.len();
  if merged[n-1]!=b'/'{merged[n]=b'/';n+=1;}
 }
 if n+input.len()>merged.len(){return Err(Error::Full);}
 merged[n..n+input.len()].copy_from_slice(input);n+=input.len();
 let mut out=[0u8;PATH_MAX];out[0]=b'/';let mut p=1usize;
 for section in merged[..n].split(|b|*b==b'/'){
  if section.is_empty()||section==b"."{continue;}
  if section==b".."{
   if p==1{return Err(Error::Denied);}
   let mut start=p;while start>1&&out[start-1]!=b'/'{start-=1;}
   p=if start>1{start-1}else{1};continue;
  }
  if section.iter().any(|b|*b==0||*b==b'\\'){return Err(Error::Invalid);}
  let needed=p+(if p==1{0}else{1})+section.len();
  if needed>PATH_MAX{return Err(Error::Full);}
  if p>1{out[p]=b'/';p+=1;}
  out[p..p+section.len()].copy_from_slice(section);p+=section.len();
 }
 Ok(Path{bytes:out,len:p})
}
pub fn exists<const F:usize>(fs:&RamFs<F>,path:Path)->bool{
 fs.metadata(&path.bytes[..path.len]).is_ok()
}
pub fn parent(path:Path)->Path{
 let bytes=&path.bytes[..path.len];
 let new_len=bytes.iter().rposition(|b|*b==b'/').unwrap_or(0).max(1);
 Path{len:new_len,..path}
}
pub fn is_under(path:Path,root:Path)->bool{
 let a=&path.bytes[..path.len];let b=&root.bytes[..root.len];
 a.starts_with(b)&&(b==b"/"||a.len()==b.len()||a.get(b.len())==Some(&b'/'))
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn resolves_relative_dot_and_parent_project_paths(){
  let p=normalize(b"/home/user",b"../src/./main.rs").unwrap();
  assert_eq!(&p.bytes[..p.len],b"/home/src/main.rs");
  let pa=parent(p);assert_eq!(&pa.bytes[..pa.len],b"/home/src");
 }
 #[test]fn traversal_above_root_and_invalid_bytes_are_rejected(){
  assert_eq!(normalize(b"/",b"../../secret"),Err(Error::Denied));
  assert_eq!(normalize(b"/p",b"/a\\b"),Err(Error::Invalid));
 }
 #[test]fn normalization_binds_to_actual_existing_ram_file_paths(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/src/main.rs",b"code").unwrap();
  let p=normalize(b"/src",b"./main.rs").unwrap();
  assert!(exists(&fs,p));assert!(is_under(p,normalize(b"/",b"/src").unwrap()));
 }
}

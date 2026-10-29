//! R030/11: sandbox-scoped terminal working-directory path resolution.
use super::Error;
use crate::batch7_ramfs::PATH_MAX;
fn under(root:&[u8],path:&[u8])->bool{
 path==root||(path.starts_with(root)&&path.get(root.len())==Some(&b'/'))
}
pub fn resolve(root:&[u8],cwd:&[u8],relative:&[u8],
 out:&mut[u8;PATH_MAX])->Result<usize,Error>{
 if root.len()<2||root[0]!=b'/'||root.ends_with(b"/")||
  !under(root,cwd)||relative.is_empty()||relative[0]==b'/'{
  return Err(Error::Denied);
 }
 if relative.split(|b|*b==b'/').any(|part|part.is_empty()||
  part==b"."||part==b".."||part.contains(&0)||part.contains(&b'\\')){
  return Err(Error::Denied);
 }
 let size=cwd.len().checked_add(1).and_then(|n|n.checked_add(relative.len()))
  .ok_or(Error::Overflow)?;
 if size>PATH_MAX{return Err(Error::Full);}
 let mut stage=[0u8;PATH_MAX];stage[..cwd.len()].copy_from_slice(cwd);
 stage[cwd.len()]=b'/';stage[cwd.len()+1..size].copy_from_slice(relative);
 *out=stage;Ok(size)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn terminal_resolves_only_descendants_of_project_cwd(){
  let mut p=[0;PATH_MAX];let n=resolve(b"/p",b"/p/code",b"src/main.rs",&mut p).unwrap();
  assert_eq!(&p[..n],b"/p/code/src/main.rs");
 }
 #[test]fn traversal_and_prefix_collisions_are_rejected(){
  let mut p=[0;PATH_MAX];
  assert_eq!(resolve(b"/p",b"/p2",b"a",&mut p),Err(Error::Denied));
  assert_eq!(resolve(b"/p",b"/p",b"../secrets",&mut p),Err(Error::Denied));
  assert_eq!(resolve(b"/p",b"/p",b"/outside",&mut p),Err(Error::Denied));
 }
}
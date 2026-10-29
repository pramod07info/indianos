//! R031/02: recoverable directory trash and restore for actual project contents.
use super::Error;
use crate::batch7_ramfs::RamFs;
use crate::workstation_r028::agent_01 as move_dir;
fn safe(name:&[u8])->bool{
 !name.is_empty()&&name.len()<=24&&name.iter().all(|b|
  b.is_ascii_alphanumeric()||*b==b'-'||*b==b'_')
}
pub fn trash<const F:usize,const B:usize>(fs:&mut RamFs<F>,expected:u64,
 project:&[u8],ticket:&[u8])->Result<usize,Error>{
 if !safe(ticket)||!project.starts_with(b"/projects/")||project.len()<=10||
  project.iter().any(|b|*b==b'\\'){return Err(Error::Denied);}
 let mut dest=[0u8;64];let prefix=b"/trash/";
 let n=prefix.len()+ticket.len();dest[..prefix.len()].copy_from_slice(prefix);
 dest[prefix.len()..n].copy_from_slice(ticket);
 move_dir::move_tree::<F,B>(fs,expected,project,&dest[..n]).map_err(|e|match e{
  crate::workstation_r028::Error::Stale=>Error::Stale,
  crate::workstation_r028::Error::Missing=>Error::Missing,
  crate::workstation_r028::Error::Conflict=>Error::Conflict,_=>Error::Invalid})
}
pub fn restore<const F:usize,const B:usize>(fs:&mut RamFs<F>,expected:u64,
 ticket:&[u8],project:&[u8])->Result<usize,Error>{
 if !safe(ticket)||!project.starts_with(b"/projects/")||project.len()<=10||
 project.contains(&0){return Err(Error::Denied);}
 let mut src=[0u8;64];src[..7].copy_from_slice(b"/trash/");
 src[7..7+ticket.len()].copy_from_slice(ticket);
 move_dir::move_tree::<F,B>(fs,expected,&src[..7+ticket.len()],project)
  .map_err(|_|Error::Conflict)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn directory_and_nested_files_trash_then_restore(){
  let mut fs=RamFs::<3>::new();fs.replace(b"/projects/my/src/a",b"code").unwrap();
  fs.replace(b"/projects/my/b",b"readme").unwrap();
  assert_eq!(trash::<3,4096>(&mut fs,2,b"/projects/my",b"ticket1"),Ok(2));
  assert!(fs.metadata(b"/projects/my/b").is_err());
  assert_eq!(restore::<3,4096>(&mut fs,4,b"ticket1",b"/projects/my"),Ok(2));
  assert!(fs.metadata(b"/projects/my/src/a").is_ok());
 }
 #[test]fn unsafe_ticket_and_collision_refuse_to_delete_project(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/projects/my/a",b"A").unwrap();
  assert_eq!(trash::<1,4096>(&mut fs,1,b"/projects/my",b"../bad"),Err(Error::Denied));
  assert_eq!(fs.version(),1);
 }
}
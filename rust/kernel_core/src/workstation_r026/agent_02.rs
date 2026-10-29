//! R026/02: authenticated copy/move between real project directories.
//! Source/target tickets are scoped, version-checked and fail closed.
use super::Error;
use crate::batch5_session::Identity;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
use crate::workstation_r025::agent_02 as access;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Mode{Copy,Move}
pub fn transfer<const F:usize>(fs:&mut RamFs<F>,actor:Identity,
 source:access::Ticket,destination:access::Ticket,now:u64,
 expected:u64,from:&[u8],to:&[u8],mode:Mode)->Result<u64,Error>{
 if expected!=fs.version(){return Err(Error::Stale);}
 if from.len()<2||to.len()<2||from.len()>PATH_MAX||
 to.len()>PATH_MAX||from==to{return Err(Error::Invalid);}
 let source_rights=access::READ|if mode==Mode::Move{access::DELETE}else{0};
 access::authorize(source,actor,now,source_rights,from).map_err(|_|Error::Denied)?;
 access::authorize(destination,actor,now,access::WRITE,to).map_err(|_|Error::Denied)?;
 let (rev,_)=fs.metadata(from).map_err(|_|Error::Missing)?;
 if fs.metadata(to).is_ok(){return Err(Error::Conflict);}
 match mode{
  Mode::Copy=>fs.copy_new(from,to).map_err(|_|Error::Full),
  Mode::Move=>fs.rename_if_version(from,to,rev).map_err(|_|Error::Conflict)
 }
}
pub fn preview<const F:usize>(fs:&RamFs<F>,from:&[u8],to:&[u8])
 ->Result<usize,Error>{
 let (_,size)=fs.metadata(from).map_err(|_|Error::Missing)?;
 if fs.metadata(to).is_ok(){return Err(Error::Conflict);}
 Ok(size)
}
pub fn valid_root_path(root:&[u8],path:&[u8])->bool{
 path.starts_with(root)&&path.get(root.len())==Some(&b'/')&&
 !path.windows(2).any(|w|w==b"..")
}
#[cfg(test)]mod tests{
 use super::*;fn id()->Identity{Identity{pid:7,generation:1}}
 #[test]fn copies_real_source_across_scoped_projects(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/a/main",b"source").unwrap();
  let source=access::grant(true,id(),&fs,b"/a",access::READ,10).unwrap();
  let target=access::grant(true,id(),&fs,b"/b",access::WRITE,10).unwrap();
  transfer(&mut fs,id(),source,target,1,1,b"/a/main",b"/b/main",Mode::Copy).unwrap();
  let mut out=[0;6];fs.read(b"/b/main",0,&mut out).unwrap();assert_eq!(&out,b"source");
 }
 #[test]fn cross_project_move_requires_source_delete_right(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/a/main",b"source").unwrap();
  let s=access::grant(true,id(),&fs,b"/a",access::READ,10).unwrap();
  let d=access::grant(true,id(),&fs,b"/b",access::WRITE,10).unwrap();
  assert_eq!(transfer(&mut fs,id(),s,d,1,1,b"/a/main",b"/b/main",Mode::Move),
   Err(Error::Denied));assert!(fs.metadata(b"/a/main").is_ok());
 }
 #[test]fn destination_collision_does_not_overwrite_user_file(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/a/main",b"old").unwrap();fs.replace(b"/b/main",b"new").unwrap();
  let s=access::grant(true,id(),&fs,b"/a",access::READ,10).unwrap();
  let d=access::grant(true,id(),&fs,b"/b",access::WRITE,10).unwrap();
  assert_eq!(transfer(&mut fs,id(),s,d,1,2,b"/a/main",b"/b/main",Mode::Copy),
   Err(Error::Conflict));
 }
}

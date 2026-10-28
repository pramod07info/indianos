//! R020/20 integrated developer workstation guest-safe feature smoke.
//! Exercises real RAM files, editor history, build jobs and live window APIs.
use super::{Error,agent_01,agent_02,agent_03,agent_06,
 agent_07,agent_09,agent_14,agent_15,agent_17,agent_19};
use crate::batch5_session::Identity;
use crate::batch7_ramfs::RamFs;
use crate::workstation_r018::editor::Buffer;
use crate::batch2_windows::{Windows,Window,Rect};
pub fn smoke()->Result<(),Error>{
 let owner=Identity{pid:7,generation:1};
 let mut session=agent_19::Registry::<1>::new();
 session.login(owner,1,100)?;
 session.resource_claim(owner,1,1,3,3)?;
 let mut fs=RamFs::<1>::new();
 fs.replace(b"/src/main.rs",b"fn main").map_err(|_|Error::Full)?;
 let mut checkpoint=agent_01::Checkpoint::<256>::new();
 checkpoint.save(owner,&fs)?;
 let mut editor=Buffer::<64>::new(7);
 agent_02::indent(&mut editor,7,0,1,2)?;
 editor.append(7,1,b"fn main").map_err(|_|Error::Stale)?;
 agent_02::delete_line(&mut editor,7,2,1)?;
 let mut history=agent_03::Document::<32,2>::new(7);
 history.replace(7,0,0,0,b"let a").map_err(|_|Error::Stale)?;
 history.replace(7,1,5,5,b" = 1").map_err(|_|Error::Stale)?;
 history.undo(7,2)?;history.redo(7,3)?;
 let mut profile=agent_06::Profile::<2>::new(7,256,4);
 profile.add(7,0,1,agent_06::Edition::Developer,128,2)?;
 profile.switch(7,1,agent_06::Edition::Developer)?;
 profile.enable_batch(7,2,&[1])?;
 let mut build=agent_07::Pipeline::<3>::new(7);
 build.add(7,1,0,100)?;build.add(7,2,1,100)?;
 build.add(7,3,2,100)?;
 for id in 1..=3{build.start(7,id,1)?;build.finish(7,id,true,0)?;}
 let mut registry=agent_09::Registry::<1>::new();
 registry.publish(7,1,1,21,0,[1;32],true)?;
 if registry.match_input(7,1,1,21,0).is_none(){return Err(Error::Missing);}
 let mut windows=Windows::<3>::new();
 for id in 1..=3{
  windows.create(Window{pid:7,generation:1,id,
   rect:Rect{x:1,y:1,w:100,h:100}}).map_err(|_|Error::Full)?;
 }
 agent_15::apply(&mut windows,7,1,1,2,3,agent_15::Layout::ThreePane)?;
 let mut shortcuts=agent_14::Router::<1>::new();
 shortcuts.bind(7,1,66,1,900,1)?;shortcuts.focus(0,7,1)?;
 if shortcuts.route(7,0,66,1,1)!=Ok(900){return Err(Error::Denied);}
 let mut clipboard=agent_17::Clipboard::<16>::new();
 clipboard.copy(owner,1,10,b"source")?;
 let mut out=[0u8;16];
 if clipboard.paste(owner,2,&mut out)!=Ok(6){return Err(Error::Denied);}
 clipboard.revoke(owner)?;
 fs.replace(b"/src/main.rs",b"changed").map_err(|_|Error::Full)?;
 checkpoint.restore(owner,&mut fs,2)?;
 let mut original=[0u8;7];
 if fs.read(b"/src/main.rs",0,&mut original).is_err()||
   &original!=b"fn main"{return Err(Error::Conflict);}
 session.retire(owner)?;
 Ok(())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn actual_project_edit_profile_build_and_desktop_test(){
  assert_eq!(smoke(),Ok(()));
 }
 #[test]fn source_revision_guards_reject_older_workspace_session(){
  let owner=Identity{pid:7,generation:1};
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"x").unwrap();
  let mut cp=agent_01::Checkpoint::<128>::new();cp.save(owner,&fs).unwrap();
  fs.replace(b"/a",b"y").unwrap();
  assert_eq!(cp.restore(owner,&mut fs,1),Err(Error::Stale));
 }
 #[test]fn expired_workstation_cannot_mutate_after_logout(){
  let owner=Identity{pid:7,generation:1};
  let mut s=agent_19::Registry::<1>::new();s.login(owner,1,1).unwrap();
  assert!(!s.active(owner,2));
  assert_eq!(s.resource_claim(owner,2,1,1,1),Err(Error::Denied));
 }
}

//! R019/20 integrated in-memory developer workflow smoke with real RAM files,
//! editor buffer, source search, build task graph, stage and release checks.
//! Deliberately not a claim of a booted graphical IDE or host compiler.
use super::Error;
use super::{agent_01,agent_02,agent_03,agent_05,agent_06,agent_09,
 agent_14,agent_19};
use crate::batch5_session::Identity;
use crate::batch6_security::{Access,FILE_READ,FILE_WRITE};
use crate::batch7_ramfs::RamFs;
use crate::workstation_r018::editor::Buffer;
pub fn smoke()->Result<(),Error>{
 let actor=Identity{pid:7,generation:1};
 let mut access=Access::<1>::new();
 access.grant(actor,FILE_READ|FILE_WRITE,100).map_err(|_|Error::Denied)?;
 let mut fs=RamFs::<3>::new();
 agent_01::save(&mut fs,&access,actor,1,b"/src/main.rs",b"fn main(){}",0)?;
 let mut editor=Buffer::<64>::new(7);
 agent_02::disk_to_editor(&mut editor,&mut fs,7,0,b"/src/main.rs")?;
 agent_02::replace_one(&mut editor,7,1,b"main",b"start")?;
 agent_02::editor_to_disk(&editor,&mut fs,7,1,b"/src/main.rs")?;
 let mut hits=[agent_03::EMPTY;4];
 let count=agent_03::scan(&fs,b"start",&mut hits)?;
 if count!=1||hits[0].offset!=3{return Err(Error::Conflict);}
 let mut graph=agent_05::Graph::<3>::new();
 graph.add(7,1,0)?;graph.add(7,2,1)?;graph.add(7,3,2)?;
 for expected in [1,2,3]{
  let job=graph.start_ready(7)?;
  if job.id!=expected{return Err(Error::Conflict);}
  graph.finish(7,expected,true)?;
 }
 let mut cache=agent_06::Cache::<2>::new();
 cache.publish(agent_06::Artifact{owner:7,task:3,inputs:10,
  output_digest:[5;32],tick:1,valid:true})?;
 if cache.lookup(7,3,10).is_none(){return Err(Error::Conflict);}
 let mut staging=agent_09::Index::<2>::new();
 staging.track(7,b"/src/main.rs",10,11)?;
 staging.stage(7,b"/src/main.rs",11)?;
 let mut tabs=agent_14::Tabs::<2>::new();
 tabs.open(7,1,0)?;tabs.focus(7,1)?;
 tabs.mark_dirty(7,1,true)?;
 if tabs.close(7,1,false)!=Err(Error::Conflict){return Err(Error::Conflict);}
 let mut release=agent_19::Release::new(7,1,2,[1;32],1)?;
 release.record_checks(7,true,true)?;
 release.review(8)?;release.approve(8)?;
 let mut payload=[0u8;48];release.publish_manifest(true,&mut payload)?;
 if payload[0]!=1||staging.commit_index(7)!=1{return Err(Error::Conflict);}
 Ok(())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn compile_edit_test_stage_and_release_workflow(){
  assert_eq!(smoke(),Ok(()));
 }
 #[test]fn stale_file_revision_never_gets_to_build_step(){
  let actor=Identity{pid:7,generation:1};
  let mut acl=Access::<1>::new();acl.grant(actor,FILE_READ|FILE_WRITE,10).unwrap();
  let mut fs=RamFs::<1>::new();
  agent_01::save(&mut fs,&acl,actor,1,b"/a",b"new",0).unwrap();
  assert_eq!(agent_01::save(&mut fs,&acl,actor,1,b"/a",b"stale",0),
   Err(Error::Conflict));
 }
 #[test]fn project_session_isolation_blocks_other_process(){
  let actor=Identity{pid:7,generation:1};
  let stale=Identity{generation:2,..actor};
  let mut acl=Access::<1>::new();acl.grant(actor,FILE_READ|FILE_WRITE,10).unwrap();
  let mut fs=RamFs::<1>::new();
  assert_eq!(agent_01::save(&mut fs,&acl,stale,1,b"/a",b"x",0),Err(Error::Denied));
 }
}

//! R035/20: cross-service acceptance: real source, terminal, compiler evidence and debug.
use super::{Error,agent_01,agent_02,agent_03,agent_04,agent_05,agent_06,
agent_07,agent_08,agent_09,agent_10,agent_11,agent_12,agent_13,
agent_14,agent_15,agent_16,agent_17,agent_18,agent_19};
use crate::batch7_ramfs::RamFs;
use crate::batch5_session::Identity;
use crate::workstation_r025::agent_02 as rights;
pub fn smoke()->Result<(),Error>{
 let mut fs=RamFs::<12>::new();
 let session=agent_01::Session::new(7,1,b"/projects/demo")?;
 let root=b"/projects/demo";
 let main=b"/projects/demo/main.rs";
 let test=b"/projects/demo/test.rs";
 let other=b"/projects/demo/log";
 session.authorize(7,1,main)?;
 agent_02::create(&mut fs,main,b"fn old() {\n}\n",0)?;
 agent_02::create(&mut fs,test,b"old();\n",1)?;
 agent_02::create(&mut fs,other,b"start\n",2)?;
 let mut tabs=agent_08::Tabs::<4>::new(7);
 let index=tabs.open(7,&fs,main)?;
 if tabs.stale(7,&fs,index)?{return Err(Error::Conflict);}
 let rev=fs.metadata(main).map_err(|_|Error::Missing)?.0;
 agent_06::patch(&mut fs,main,rev,3,6,b"new")?;
 if !tabs.stale(7,&fs,index)?{return Err(Error::Conflict);}
 let revision=fs.version();
 if agent_07::refactor::<12,32768>(&mut fs,root,revision,b"old",b"new")?!=1{
  return Err(Error::Conflict);
 }
 let mut hits=[0usize;4];
 if agent_10::search(&fs,test,b"new",&mut hits)?!=1||hits[0]!=0{
  return Err(Error::Conflict);
 }
 let mut listed=[[0u8;96];12];
 if agent_09::list(&fs,root,&mut listed)?!=3{return Err(Error::Conflict);}
 let source_rev=fs.metadata(main).map_err(|_|Error::Missing)?.0;
 agent_13::validate(&fs,main,source_rev,1,4)?;
 let who=Identity{pid:7,generation:1};
 let ticket=rights::grant(true,who,&fs,root,rights::READ|rights::WRITE,100)
  .map_err(|_|Error::Denied)?;
 let mut output=[0u8;64];
 let size=agent_11::execute(&fs,who,ticket,1,&[b"cat /projects/demo/test.rs"],&mut output)?;
 if &output[..size]!=b"new();\n"{return Err(Error::Conflict);}
 let log_rev=fs.metadata(other).map_err(|_|Error::Missing)?.0;
 agent_12::append(&mut fs,who,ticket,1,other,log_rev,b"done\n")?;
 if !agent_14::build::<3>(7,&[0,1,2],&[true,true,true])?{
  return Err(Error::Conflict);
 }
 let digest=crate::workstation_r032::agent_02::digest(b"artifact");
 agent_15::verify(&fs,main,source_rev,b"artifact",digest)?;
 let mut suite=agent_16::Suite::<3>::new(7);
 suite.report(7,true,1,true)?;suite.report(7,true,2,true)?;
 if !suite.all_passed(7)?{return Err(Error::Conflict);}
 if !agent_17::hit(&fs,main,source_rev,1,4096,1,4096,true)?{
  return Err(Error::Conflict);
 }
 let mut trace=agent_18::Trace::<4>::new(7);
 trace.record(7,true,agent_18::Frame{sequence:1,pc:4096})?;
 if trace.latest().ok_or(Error::Missing)?.pc!=4096{return Err(Error::Conflict);}
 let panels=agent_19::layout(1200,800)?;
 if panels.iter().map(|p|p.w as u32*p.h as u32).sum::<u32>()!=960000{
  return Err(Error::Conflict);
 }
 let copied=b"/projects/demo/copy.rs";
 agent_03::copy(&mut fs,main,copied,source_rev)?;
 let copy_rev=fs.metadata(copied).map_err(|_|Error::Missing)?.0;
 agent_04::rename(&mut fs,copied,b"/projects/demo/renamed.rs",copy_rev)?;
 agent_05::save(&mut fs,main,source_rev,b"fn updated(){}\n")?;
 Ok(())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn complete_live_guest_workstation_integration(){assert_eq!(smoke(),Ok(()));}
 #[test]fn foreign_owner_cannot_change_project_source(){
 let s=agent_01::Session::new(7,1,b"/projects/demo").unwrap();
 assert_eq!(s.authorize(8,1,b"/projects/demo/main.rs"),Err(Error::Denied));
 }
}

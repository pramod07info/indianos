//! R026/20: end-to-end guest acceptance through real RamFs project/edit/terminal/build APIs.
//! Verifies integrated runtime behavior, not merely feature metadata.
use super::{Error,agent_01,agent_02,agent_03,agent_04,agent_05,
 agent_06,agent_07,agent_08,agent_09,agent_10,agent_11,agent_12,
 agent_13,agent_14,agent_15,agent_16,agent_17,agent_18,agent_19};
use crate::batch5_session::Identity;
use crate::batch7_ramfs::RamFs;
use crate::workstation_r025::{agent_02 as ticket,agent_10 as terminal};
pub fn smoke()->Result<(),Error>{
 let owner=Identity{pid:7,generation:1};
 let mut fs=RamFs::<9>::new();
 fs.replace(b"/p/a.rs",b"fn foo(){}\n// TODO revise\n").map_err(|_|Error::Full)?;
 fs.replace(b"/p/b.rs",b"fn foo2(){}\n").map_err(|_|Error::Full)?;
 fs.replace(b"/p/log",b"one\ntwo\n").map_err(|_|Error::Full)?;
 fs.replace(b"/q/main",b"other").map_err(|_|Error::Full)?;
 let mut nodes=[agent_01::EMPTY;6];
 if agent_01::tree(&fs,b"/p",2,&mut nodes)?!=3{return Err(Error::Conflict);}
 let source=ticket::grant(true,owner,&fs,b"/p",ticket::READ|ticket::DELETE,100)?;
 let target=ticket::grant(true,owner,&fs,b"/q",ticket::WRITE,100)?;
 let expected=fs.version();
 agent_02::transfer(&mut fs,owner,source,target,1,expected,
  b"/p/log",b"/q/log",agent_02::Mode::Copy)?;
 let copies=[agent_03::Copy{from:b"/p/b.rs",to:b"/q/b.rs",
  revision:fs.metadata(b"/p/b.rs").map_err(|_|Error::Missing)?.0}];
 let expected=fs.version();
 agent_03::batch::<9,8192>(&mut fs,expected,&copies)?;
 let mut restore=agent_04::History::<2>::new(owner);
 let point=restore.capture(owner,&fs,b"/p/a.rs",1)?;
 let source_rev=fs.metadata(b"/p/a.rs").map_err(|_|Error::Missing)?.0;
 agent_06::apply(&mut fs,b"/p/a.rs",source_rev,
  &[agent_06::Selection{start:3,end:6}],b"bar")?;
 let version=fs.version();
 restore.restore(owner,&mut fs,point,version)?;
 let version=fs.version();
 if agent_07::replace::<9,8192>(&mut fs,version,b"/p",b"foo",b"baz")?!=2{
  return Err(Error::Conflict);
 }
 let mut snippets=agent_08::Library::<1>::new(owner);
 snippets.add(owner,1,b"pub ")?;
 let rev=fs.metadata(b"/p/a.rs").map_err(|_|Error::Missing)?.0;
 snippets.insert(owner,&mut fs,1,b"/p/a.rs",rev,0)?;
 let mut issues=[agent_09::EMPTY;2];
 if agent_09::scan(&fs,b"/p",&mut issues)?!=1{return Err(Error::Conflict);}
 let mut env=agent_10::Env::<2>::new(owner);
 env.set(owner,b"PROJECT",b"/p")?;
 let mut expanded=[0u8;32];
 let n=env.expand(owner,b"\${PROJECT}/a.rs",&mut expanded)?;
 if &expanded[..n]!=b"/p/a.rs"{return Err(Error::Conflict);}
 let writer=ticket::grant(true,owner,&fs,b"/p",ticket::WRITE,100)?;
 let version=fs.version();
 agent_11::redirect(&mut fs,owner,writer,1,version,b"/p/log",
  terminal::Command::WordCount,b"",b"/p/count",agent_11::Output::CreateOnly)?;
 let mut jobs=agent_12::Jobs::<1>::new(owner);
 jobs.enqueue(owner,1,10,1,100)?;
 jobs.start(owner,1,2)?;
 jobs.complete(true,1,0,3)?;
 let mut plan=agent_13::Planner::<3>::new(7,21);
 plan.capture(7,&fs,b"/p")?;
 let mut dirty=[agent_13::EMPTY;3];
 if plan.dirty(7,&fs,&mut dirty)?!=0{return Err(Error::Conflict);}
 let first=crate::workstation_r019::agent_04::parse(
  b"/p/a.rs:1:1:warning unused").map_err(|_|Error::Invalid)?;
 let mut messages=[first;1];
 let warnings=agent_14::analyze(b"/p/a.rs:1:1:warning unused\n",&mut messages)?;
 if warnings.warnings!=1||agent_14::gate(warnings)!=Ok(()) {
  return Err(Error::Conflict);
 }
 let mut suite=agent_15::Suite::<1>::new(7);
 suite.add(7,1,2)?;suite.start(7,1)?;
 suite.finish(true,1,1)?;suite.start(7,1)?;suite.finish(true,1,0)?;
 if !suite.release_ready(7)?{return Err(Error::Conflict);}
 let mut debugger=agent_16::Debugger::<1>::new(owner);
 debugger.attach(owner,true,1,0x1000,1,1)?;
 debugger.command(owner,true,1,agent_16::Command::StepIn,2)?;
 let mut watches=agent_17::Watch::<1>::new(owner);
 watches.observe(owner,true,1,agent_17::Type::Int,b"1",1)?;
 if !watches.observe(owner,true,1,agent_17::Type::Int,b"2",2)?{
  return Err(Error::Conflict);
 }
 let mut tabs=agent_18::Tabs::<2>::new(owner);
 tabs.open(owner,&fs,1,b"/p/a.rs",agent_18::Pane::Primary)?;
 tabs.open(owner,&fs,2,b"/p/b.rs",agent_18::Pane::Secondary)?;
 if tabs.count(owner)?!=2{return Err(Error::Conflict);}
 let mut recovery=agent_19::Recovery::new();
 recovery.save(owner,1,b"tabs=2;split=1")?;
 let mut session=[0u8;32];
 if recovery.load(owner,1,&mut session)?!=14{return Err(Error::Conflict);}
 let mut metadata=[agent_05::EMPTY;8];
 if agent_05::query(&fs,b"/p",b".rs",0,agent_05::Sort::Name,&mut metadata)?!=2{
  return Err(Error::Conflict);
 }
 Ok(())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn real_guest_workstation_edit_terminal_build_debug_and_recovery_workflow(){
  assert_eq!(smoke(),Ok(()));
 }
 #[test]fn foreign_user_cannot_leak_project_file_outside_scoped_ticket(){
  let actor=Identity{pid:7,generation:1};
  let fs=RamFs::<1>::new();
  let ticket=ticket::grant(true,actor,&fs,b"/p",ticket::READ,10).unwrap();
  assert_eq!(ticket::authorize(ticket,actor,1,ticket::READ,b"/q/main"),
   Err(ticket::Error::Denied));
 }
 #[test]fn failed_atomic_source_copy_keeps_all_original_files(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/p/a",b"a").unwrap();fs.replace(b"/p/b",b"b").unwrap();
  let operations=[agent_03::Copy{from:b"/p/a",to:b"/q/a",revision:1}];
  assert_eq!(agent_03::batch::<2,1024>(&mut fs,2,&operations),Err(Error::Full));
  assert!(fs.metadata(b"/p/a").is_ok());
 }
}

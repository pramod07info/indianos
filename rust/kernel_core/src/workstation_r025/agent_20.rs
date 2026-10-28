//! R025/20: real guest-backed Developer Workstation cross-module integration acceptance.
//! Verifies actual RamFs source operations; external executable launch and GPU UI are distinct.
use super::{Error,agent_01,agent_02,agent_03,agent_04,agent_05,
 agent_06,agent_07,agent_08,agent_09,agent_10,agent_11,agent_12,
 agent_13,agent_14,agent_15,agent_16,agent_17,agent_18,agent_19};
use crate::batch5_session::Identity;
use crate::batch7_ramfs::RamFs;
pub fn smoke()->Result<(),Error>{
 let owner=Identity{pid:7,generation:1};
 let mut fs=RamFs::<5>::new();
 fs.replace(b"/p/main.rs",b"fn hello(){}\n#[test]\nfn smoke(){}\n")
  .map_err(|_|Error::Full)?;
 fs.replace(b"/p/lib.rs",b"pub fn use_it(){hello();}\n")
  .map_err(|_|Error::Full)?;
 fs.replace(b"/q/main.rs",b"fn other(){}\n").map_err(|_|Error::Full)?;
 let mut projects=agent_01::Switcher::<2>::new(owner);
 projects.open(owner,&fs,1,b"/p")?;
 projects.open(owner,&fs,2,b"/q")?;
 projects.switch(owner,1)?;
 if projects.active(owner)?.id!=1{return Err(Error::Conflict);}
 let ticket=agent_02::grant(true,owner,&fs,b"/p",
  agent_02::READ|agent_02::WRITE,100)?;
 let mut recent=agent_03::Recents::<2>::new(owner);
 recent.touch(owner,&fs,b"/p/main.rs")?;
 let mut lines=agent_04::Index::<8>::new();
 if lines.build(&fs,b"/p/main.rs")?!=3{return Err(Error::Conflict);}
 if lines.position(&fs,b"/p/main.rs",2,1)!=Ok(13){
  return Err(Error::Conflict);
 }
 let mut search=agent_05::Search::<4>::new(owner);
 if search.refresh(owner,&fs,b"/p/main.rs",b"hello")?!=1{
  return Err(Error::Conflict);
 }
 let mut refs=[agent_08::EMPTY;4];
 if agent_08::references(&fs,b"/p",b"hello",&mut refs)?!=2{
  return Err(Error::Conflict);
 }
 let mut watcher=agent_09::Watch::<4>::new(owner);
 watcher.observe(owner,&fs,b"/p/lib.rs",0)?;
 let version=fs.metadata(b"/p/main.rs").map_err(|_|Error::Missing)?.0;
 agent_07::apply(&mut fs,b"/p/main.rs",version,1,agent_07::Action::Indent)?;
 watcher.observe(owner,&fs,b"/p/main.rs",version)?;
 let mut queued=agent_06::Saves::<2>::new(owner);
 queued.stage(owner,&fs,b"/p/main.rs",b"fn hello(){}\n#[test]\nfn smoke(){}\n")?;
 queued.stage(owner,&fs,b"/p/lib.rs",b"pub fn use_it(){hello();}\n")?;
 let revision=fs.version();
 queued.save::<5,8192>(owner,&mut fs,revision)?;
 let mut terminal=[0u8;128];
 let n=agent_10::run(&fs,b"/p/main.rs",agent_10::Command::Find,
  b"smoke",&mut terminal)?;
 if &terminal[..n]!=b"fn smoke(){}\n"{return Err(Error::Conflict);}
 let steps=[agent_11::Step{command:agent_10::Command::Stat,
   path:b"/p/main.rs",param:b""},
  agent_11::Step{command:agent_10::Command::WordCount,
   path:b"/p/main.rs",param:b""}];
 let mut stdout=[0u8;64];let mut index=[agent_11::EMPTY;2];
 if agent_11::execute(&fs,&steps,&mut stdout,&mut index)?==0{
  return Err(Error::Conflict);
 }
 let mut audit=agent_12::Audit::<2>::new(owner);
 audit.record(owner,true,1,1,agent_12::Kind::Build,0)?;
 let mut manifest=agent_14::Manifest::<3>::new(7,21);
 manifest.capture(7,&fs,b"/p")?;
 let mut testcases=[agent_15::EMPTY;3];
 let discovered=agent_15::discover(&fs,b"/p",2,&mut testcases)?;
 if discovered!=1{return Err(Error::Conflict);}
 let mut jobs=agent_13::Graph::<2>::new(7);
 jobs.add(7,1,0,agent_13::Stage::Compile,21)?;
 jobs.add(7,2,1,agent_13::Stage::Test,21)?;
 jobs.start(7,1,21)?;jobs.finish(true,1,0)?;
 jobs.start(7,2,21)?;jobs.finish(true,2,0)?;
 let mut bp=agent_16::Breakpoints::<1>::new(owner);
 bp.add(owner,&fs,1,b"/p/main.rs",1)?;
 if bp.hit(owner,true,&fs,1)!=Ok(1){return Err(Error::Conflict);}
 let mut watches=agent_17::Watches::<1>::new(owner);
 watches.observe(owner,true,1,agent_17::Type::Integer,b"42",2)?;
 let mut router=agent_18::Router::<1>::new(owner);
 router.submit(owner,1,agent_18::Action::OpenFile,fs.version())?;
 let mut read=[0u8;128];
 let n=router.execute(owner,1,ticket,&fs,b"/p/main.rs",b"",&mut read)?;
 if n==0||!read[..n].starts_with(b"fn hello"){return Err(Error::Conflict);}
 let mut gate=agent_19::Gate::new();
 gate.draft(7,1,&fs)?;
 gate.validate(true,7,&fs,&manifest,&jobs,discovered as u32)?;
 gate.review(7,8)?;gate.publish(true,7,&fs)?;
 if gate.state()!=Some(agent_19::State::Published){
  return Err(Error::Conflict);
 }
 let mut other=[0u8;32];
 let size=fs.read(b"/q/main.rs",0,&mut other).map_err(|_|Error::Missing)?;
 if &other[..size]!=b"fn other(){}\n"{return Err(Error::Conflict);}
 Ok(())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn real_project_switcher_source_edit_build_test_debug_and_release_smoke(){
  assert_eq!(smoke(),Ok(()));
 }
 #[test]fn cross_project_access_requires_actual_scope_permission(){
  let owner=Identity{pid:7,generation:1};
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/p/a",b"source").unwrap();
  let t=agent_02::grant(true,owner,&fs,b"/p",agent_02::READ,10).unwrap();
  assert_eq!(agent_02::authorize(t,owner,1,agent_02::READ,b"/q/a"),
   Err(Error::Denied));
 }
 #[test]fn stale_builder_manifest_does_not_publish_after_user_edit(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"a").unwrap();
  let mut m=agent_14::Manifest::<1>::new(7,21);
  m.capture(7,&fs,b"/p").unwrap();
  fs.replace(b"/p/a",b"changed").unwrap();
  assert_eq!(m.verify(7,&fs),Err(Error::Stale));
 }
}

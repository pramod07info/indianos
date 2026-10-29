//! R032/20: integrated real-guest workstation acceptance with project bytes,
//! source edits, terminal, trusted build/test evidence, debugger and pane geometry.
//! This exercises kernel-side services; it is not a full graphical IDE renderer.
use super::{Error,agent_01,agent_02,agent_03,agent_04,agent_05,agent_06,
 agent_07,agent_08,agent_09,agent_10,agent_11,agent_12,agent_13,
 agent_14,agent_15,agent_16,agent_17,agent_18,agent_19};
use crate::batch7_ramfs::RamFs;
use crate::batch5_session::Identity;
use crate::workstation_r025::agent_02 as rights;
use crate::workstation_r029::agent_18::Rect;
use crate::workstation_r031::agent_18::Frame;
pub fn smoke()->Result<(),Error>{
 let mut fs=RamFs::<16>::new();
 agent_01::create::<16,16384>(&mut fs,b"/projects/app",0,b"app")?;
 fs.replace(b"/projects/app/log",b"one\nerror\n").map_err(|_|Error::Full)?;
 fs.replace(b"/projects/app/tests.rs",b"#[test] fn demo() {}\n")
  .map_err(|_|Error::Full)?;
 let mut baseline=[agent_02::EMPTY;16];
 if agent_02::inspect(&fs,b"/projects/app",&mut baseline)?!=5{
  return Err(Error::Conflict);
 }
 let generation=fs.version();
 let copied=crate::workstation_r031::agent_01::clone_tree::<16,16384>(
  &mut fs,generation,b"/projects/app",b"/backup");
 if copied!=Ok(5){return Err(Error::Conflict);}
 let mut diffs=[agent_03::EMPTY;8];
 if agent_03::diff(&fs,b"/projects/app",b"/backup",&mut diffs)?!=0{
  return Err(Error::Conflict);
 }
 let version=fs.version();
 if agent_05::replace_project::<16,16384>(
  &mut fs,b"/projects/app",version,b"fn main",b"fn program")? != (1,1){
  return Err(Error::Conflict);
 }
 let mut dirty=[0usize;16];
 let audit=agent_04::verify(&fs,b"/projects/app",&baseline[..5],&mut dirty)?;
 if audit.stale!=1||audit.added!=0{return Err(Error::Conflict);}
 if agent_03::diff(&fs,b"/projects/app",b"/backup",&mut diffs)?!=1{
  return Err(Error::Conflict);
 }
 let mut hunks=[agent_06::EMPTY;4];
 if agent_06::compare(&fs,b"/backup/src/main.rs",b"/projects/app/src/main.rs",
  &mut hunks)?!=1{return Err(Error::Conflict);}
 let mut editor=agent_08::Session::<3>::new(7);
 let main=b"/projects/app/src/main.rs";
 let tab=editor.open(7,&fs,main)?;
 editor.mark_dirty(7,tab)?;
 if editor.close(7,tab,false)!=Err(Error::Conflict){return Err(Error::Conflict);}
 let sel=agent_09::Selection::select(7,&fs,main,3,10)?;
 let mut copy=[0u8;16];let n=sel.copy(7,&fs,&mut copy)?;
 if &copy[..n]!=b"program"{return Err(Error::Conflict);}
 let who=Identity{pid:7,generation:1};
 let t=rights::grant(true,who,&fs,b"/projects/app",rights::READ|rights::WRITE,100)
  .map_err(|_|Error::Denied)?;
 let rev=fs.metadata(main).map_err(|_|Error::Missing)?.0;
 agent_13::append(&mut fs,who,t,1,main,rev,b"// saved\n")?;
 editor.save_verified(7,tab,&fs)?;editor.close(7,tab,false)?;
 let mut source=[0u8;256];
 let size=fs.read(main,0,&mut source).map_err(|_|Error::Missing)?;
 let mut folds=[agent_10::NONE;8];
 if agent_10::find::<8,16>(&source[..size],&mut folds)?!=1{
  return Err(Error::Conflict);
 }
 let mut diagnostics=agent_07::Diagnostics::<2>::new(7);
 diagnostics.record(7,true,&fs,main,1,4,901,agent_07::Severity::Warning)?;
 if !diagnostics.release_ready(7,&fs)?{return Err(Error::Conflict);}
 let mut names=[agent_11::EMPTY;16];
 if agent_11::list(&fs,who,t,1,b"/projects/app",&mut names)?!=5{
  return Err(Error::Conflict);
 }
 let mut grep=[agent_12::EMPTY;16];
 if agent_12::grep(&fs,who,t,1,b"/projects/app",b"fn",&mut grep)?<2{
  return Err(Error::Conflict);
 }
 let log=b"/projects/app/log";
 let rev=fs.metadata(log).map_err(|_|Error::Missing)?.0;
 agent_13::append(&mut fs,who,t,1,log,rev,b"done\n")?;
 let mut steps=agent_14::Plan::<3>::new(7);
 steps.add(7,0,0)?;steps.add(7,1,1)?;steps.add(7,2,2)?;
 let mut ordered=[0u8;3];
 if steps.order(7,&mut ordered)?!=3||ordered!=[0,1,2]{
  return Err(Error::Conflict);
 }
 for id in 0..3{
  if steps.next(7)?!=Some(id){return Err(Error::Conflict);}
  steps.finish(7,true,id,true)?;
 }
 if !steps.green(){return Err(Error::Conflict);}
 let mut artifact=agent_15::Registry::<1>::new(7);
 artifact.register(7,true,&fs,main,1,1,b"test-ELF")?;
 if !artifact.verify(7,&fs,main,1,1,b"test-ELF")?{
  return Err(Error::Conflict);
 }
 let mut results=agent_16::Results::<1>::new(7);
 results.report(7,true,&fs,b"/projects/app/tests.rs",1,1,true)?;
 if !results.ready(7,&fs)?{return Err(Error::Conflict);}
 let mut breakpoint=agent_17::Breakpoints::<1>::new(7);
 breakpoint.install(7,&fs,main,1,4096)?;
 if !breakpoint.check(7,true,&fs,main,
  Frame{sequence:1,pc:4096,line:1,value:0})?{
  return Err(Error::Conflict);
 }
 let mut ws=agent_18::Workspace::new(7,Rect{x:0,y:0,w:1200,h:800})?;
 if !ws.complete(){return Err(Error::Conflict);}
 ws.focus(7,3)?;
 if ws.active(7)?.kind!=agent_18::Kind::Terminal{return Err(Error::Conflict);}
 let mut actions=[agent_19::Action::Open;8];
 if agent_19::search(b"build",true,&mut actions)?!=1||
  actions[0]!=agent_19::Action::Build{
  return Err(Error::Conflict);
 }
 if agent_19::resolve(agent_19::Action::Build,false)!=Err(Error::Denied){
  return Err(Error::Conflict);
 }
 Ok(())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn all_twenty_workstreams_integrate_on_real_guest_file_bytes(){
  assert_eq!(smoke(),Ok(()));
 }
 #[test]fn integration_never_allows_foreign_session_to_write_source(){
  let who=Identity{pid:7,generation:1};let other=Identity{pid:8,generation:1};
  let mut fs=RamFs::<1>::new();fs.replace(b"/projects/app/a",b"old").unwrap();
  let t=rights::grant(true,who,&fs,b"/projects/app",rights::WRITE,100).unwrap();
  assert_eq!(agent_13::append(&mut fs,other,t,1,b"/projects/app/a",1,b"new"),
   Err(Error::Denied));assert_eq!(fs.version(),1);
 }
}
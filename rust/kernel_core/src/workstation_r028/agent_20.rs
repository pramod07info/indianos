//! R028/20: integrated guest workstation acceptance against actual RamFs bytes.
//! Exercises project explorer, editor, safe terminal, build/test and UI preferences.
use super::{Error,agent_01,agent_02,agent_03,agent_04,
 agent_05,agent_06,agent_07,agent_08,agent_09,agent_10,
 agent_11,agent_12,agent_13,agent_14,agent_15,agent_16,
 agent_17,agent_18,agent_19};
use crate::batch5_session::Identity;
use crate::batch7_ramfs::RamFs;
pub fn smoke()->Result<(),Error>{
 let owner=Identity{pid:7,generation:1};
 let mut fs=RamFs::<6>::new();
 fs.replace(b"/app/main.rs",b"use crate::util;\nfn hello(){\n  do_work();\n}\n")
  .map_err(|_|Error::Full)?;
 fs.replace(b"/app/lib.rs",b"fn hello(){\n}\n").map_err(|_|Error::Full)?;
 fs.replace(b"/app/readme",b"hello  \r\n").map_err(|_|Error::Full)?;
 fs.replace(b"/else/other.rs",b"fn outside(){}").map_err(|_|Error::Full)?;
 let mut history=agent_02::History::<1>::new(owner);
 let point=history.capture(owner,&fs,b"/app/readme",1)?;
 let original=agent_03::inspect(&fs,b"/app/readme")?;
 if original.eol!=agent_03::Eol::CrLf{return Err(Error::Conflict);}
 let mut imports=[agent_04::EMPTY;2];
 if agent_04::extract(&fs,b"/app",&mut imports)?!=1{return Err(Error::Conflict);}
 let diff=agent_05::files(&fs,b"/app/main.rs",b"/app/lib.rs")?;
 if agent_05::changed(diff)==0{return Err(Error::Conflict);}
 let a=fs.metadata(b"/app/main.rs").map_err(|_|Error::Missing)?.0;
 let b=fs.metadata(b"/app/lib.rs").map_err(|_|Error::Missing)?.0;
 let edits=[
  agent_06::Edit{path:b"/app/main.rs",revision:a,start:20,end:25,
   old:b"hello",new:b"start"},
  agent_06::Edit{path:b"/app/lib.rs",revision:b,start:3,end:8,
   old:b"hello",new:b"start"},
 ];
 // Verify exact preimage offsets before committing atomic two-document edit.
 let mut text=[0u8;128];
 fs.read(b"/app/main.rs",0,&mut text).map_err(|_|Error::Missing)?;
 if &text[20..25]!=b"hello"{return Err(Error::Conflict);}
 let version=fs.version();agent_06::commit::<6,8192>(&mut fs,version,&edits)?;
 if agent_07::locate(&fs,b"/app/lib.rs",
  fs.metadata(b"/app/lib.rs").map_err(|_|Error::Missing)?.0,1,4)?!=3{
  return Err(Error::Conflict);
 }
 let mut suggestions=[agent_08::EMPTY;3];
 let n=agent_08::index(&fs,b"/app/main.rs",b"sta",&mut suggestions)?;
 if n==0{return Err(Error::Conflict);}
 let rev=fs.metadata(b"/app/readme").map_err(|_|Error::Missing)?.0;
 let cleanup=agent_09::apply(&mut fs,b"/app/readme",rev)?;
 if cleanup.crlf!=1{return Err(Error::Conflict);}
 let mut restored=[0u8;32];
 if history.preview(owner,point,&mut restored)?==0{return Err(Error::Conflict);}
 let cmd=agent_10::parse(b"grep /app/readme hello")?;
 if cmd.command!=agent_10::Command::Grep{return Err(Error::Conflict);}
 let mut hits=[agent_11::EMPTY;2];
 if agent_11::search(b"compile ok\ncompile ok",b"ok",0,&mut hits)?.0!=2{
  return Err(Error::Conflict);
 }
 let mut queue=agent_12::Queue::<1>::new(owner);
 queue.submit(owner,1,cmd.command,1,10)?;
 queue.start(owner,1,2)?;queue.finish(true,1,0)?;
 let mut source_index=[agent_13::EMPTY;3];
 let len=agent_13::capture(&fs,b"/app",&mut source_index)?;
 if len!=2||agent_13::dirty(&fs,&source_index[..len])?!=0{
  return Err(Error::Conflict);
 }
 let mut build=agent_14::Scheduler::<2>::new(1);
 build.add(1,0)?;build.add(2,1)?;
 if build.dispatch()!=Ok(Some(1)){return Err(Error::Conflict);}
 build.finish(true,1,true)?;
 if build.dispatch()!=Ok(Some(2)){return Err(Error::Conflict);}
 build.finish(true,2,true)?;
 if !build.all_passed(){return Err(Error::Conflict);}
 let mut tests=[agent_15::EMPTY;3];
 if agent_15::project(&fs,b"/app",&mut tests)?!=2{
  return Err(Error::Conflict);
 }
 let mut conditional=agent_16::Conditions::<1>::new(owner);
 conditional.add(owner,1,agent_16::Comparator::Ge,40,1)?;
 if !conditional.check(owner,true,1,42)?{return Err(Error::Conflict);}
 let mut trace=agent_17::Trace::<2>::new(owner);
 trace.record(owner,true,1,0x1000,1,1)?;
 trace.record(owner,true,1,0x1004,2,2)?;
 let mut frames=[agent_17::EMPTY;2];
 if trace.since(owner,0,&mut frames)?!=2{return Err(Error::Conflict);}
 let mut shortcuts=agent_18::Bindings::<1>::new(owner);
 shortcuts.bind(owner,83,1,agent_18::Action::Save)?;
 if shortcuts.resolve(owner,83,1)?!=Some(agent_18::Action::Save){
  return Err(Error::Conflict);
 }
 let mut profiles=agent_19::Profiles::<1>::new(owner);
 profiles.save(owner,agent_19::Profile{id:1,
  theme:agent_19::Theme::Crystal,font_scale:100,
  split_ratio:50,animations:true})?;
 if profiles.active(owner)?.theme!=agent_19::Theme::Crystal{
  return Err(Error::Conflict);
 }
 let before=fs.version();
 if agent_01::move_tree::<6,8192>(&mut fs,before,b"/app",b"/project")?!=3{
  return Err(Error::Conflict);
 }
 if fs.metadata(b"/project/main.rs").is_err()||
  fs.metadata(b"/app/main.rs").is_ok(){return Err(Error::Conflict);}
 Ok(())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn real_guest_files_editor_terminal_build_debug_and_theme_integration(){
  assert_eq!(smoke(),Ok(()));
 }
 #[test]fn project_tree_move_refuses_unsafe_nested_target(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/app/a",b"a").unwrap();
  assert_eq!(agent_01::move_tree::<1,1024>(&mut fs,1,b"/app",b"/app/nested"),
   Err(Error::Invalid));
 }
}
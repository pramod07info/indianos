//! R029/20: real guest workstation end-to-end tests from project source to saved layout.
//! No fake compiler/debugger engine: externally trusted inputs remain explicit.
use super::{Error,agent_01,agent_02,agent_03,agent_04,agent_05,agent_06,
 agent_07,agent_08,agent_09,agent_10,agent_11,agent_12,agent_13,
 agent_14,agent_15,agent_16,agent_17,agent_18,agent_19};
use crate::batch5_session::Identity;
use crate::batch7_ramfs::RamFs;
use crate::workstation_r025::agent_02 as rights;
pub fn smoke()->Result<(),Error>{
 let who=Identity{pid:7,generation:1};
 let mut fs=RamFs::<6>::new();
 fs.replace(b"/p/main.rs",b"use crate::util;\nfn hello() {\n work();\n}\n")
  .map_err(|_|Error::Full)?;
 fs.replace(b"/p/lib.rs",b"// library\n").map_err(|_|Error::Full)?;
 fs.replace(b"/p/log",b"ok\nerror\n").map_err(|_|Error::Full)?;
 let mut deltas=[agent_03::EMPTY;64];
 if agent_03::files(&fs,b"/p/main.rs",b"/p/lib.rs",&mut deltas)?==0{
  return Err(Error::Conflict);
 }
 let mut dependents=[agent_04::EMPTY;3];
 if agent_04::dependent_files(&fs,b"/p",b"crate::util",&mut dependents)?!=1{
  return Err(Error::Conflict);
 }
 let newest=agent_05::newest(&fs,b"/p")?.ok_or(Error::Missing)?;
 if newest.revision!=3{return Err(Error::Conflict);}
 let mut undo=agent_06::Undo::new(who);
 undo.edit(who,&mut fs,b"/p/lib.rs",2,b"// changed\n")?;
 undo.undo(who,&mut fs)?;
 undo.redo(who,&mut fs)?;
 let mut clip=agent_07::Clipboard::new(who);
 clip.copy(who,b"hello",0,5)?;
 let rev=fs.metadata(b"/p/lib.rs").map_err(|_|Error::Missing)?.0;
 clip.paste(who,&mut fs,b"/p/lib.rs",rev,0)?;
 let rev=fs.metadata(b"/p/main.rs").map_err(|_|Error::Missing)?.0;
 agent_08::toggle(&mut fs,b"/p/main.rs",rev,2)?;
 let rev=fs.metadata(b"/p/main.rs").map_err(|_|Error::Missing)?.0;
 agent_08::toggle(&mut fs,b"/p/main.rs",rev,2)?;
 let mut found=[agent_09::EMPTY;4];
 if agent_09::search(&fs,b"/p",b"hello",&mut found)?!=2{
  return Err(Error::Conflict);
 }
 let ticket=rights::grant(true,who,&fs,b"/p",rights::READ,200)
  .map_err(|_|Error::Denied)?;
 let mut stdout=[0u8;64];
 let n=agent_10::run(&fs,who,ticket,1,b"grep /p/log error",&mut stdout)?;
 if &stdout[..n]!=b"error\n"{return Err(Error::Conflict);}
 let mut history=agent_11::History::<2>::new(who);
 history.push(who,b"grep /p/log error",1)?;
 if history.find(who,b"grep")?.is_none(){return Err(Error::Conflict);}
 let p=agent_12::page(b"one\ntwo\nthree\n",0,2,&mut stdout)?;
 if !p.has_more||p.len!=8{return Err(Error::Conflict);}
 let mut cache=agent_13::Cache::<2>::new(7,21);
 cache.pin(7,21,&fs,b"/p/main.rs")?;
 if cache.dirty(7,21,&fs)?!=0{return Err(Error::Conflict);}
 let mut diagnostics=[agent_14::EMPTY;2];
 let m=agent_14::parse(b"/p/main.rs:2:warning:unused\n",&mut diagnostics)?;
 if agent_14::release(&diagnostics[..m])!=Ok(()){return Err(Error::Conflict);}
 let mut failures=agent_15::Reports::<1>::new(7);
 failures.record(7,true,1,42,1,2,b"test initially failed")?;
 if failures.release_ready(7)?{return Err(Error::Conflict);}
 failures.clear_verified(7,true,1)?;
 if !failures.release_ready(7)?{return Err(Error::Conflict);}
 let mut stops=agent_16::Registry::<1>::new(who);
 stops.add(who,&fs,b"/p/main.rs",2)?;
 if !stops.hit(who,&fs,true,b"/p/main.rs",2)?{return Err(Error::Conflict);}
 let mut watch=agent_17::Watches::<1>::new(who);
 watch.observe(who,true,1,41,1)?;
 if !watch.observe(who,true,1,42,2)?{return Err(Error::Conflict);}
 let mut windows=[agent_18::EMPTY;3];
 let screen=agent_18::Rect{x:0,y:0,w:1200,h:800};
 if agent_18::layout(screen,agent_18::Preset::TripleColumn,&mut windows)?!=3||
  !agent_18::covers(screen,&windows){return Err(Error::Conflict);}
 let expected=fs.version();
 agent_19::save(&mut fs,who,expected,b"editor=2;terminal=1;theme=crystal")?;
 let mut restored=[0u8;64];
 let n=agent_19::load(&fs,who,&mut restored)?;
 if &restored[..n]!=b"editor=2;terminal=1;theme=crystal"{
  return Err(Error::Conflict);
 }
 let version=fs.version();
 agent_01::trash::<6,8192>(&mut fs,version,b"/p/lib.rs",b"/trash/lib.rs")?;
 if agent_01::entries(&fs)!=1{return Err(Error::Conflict);}
 let version=fs.version();
 agent_01::restore::<6,8192>(&mut fs,version,b"/trash/lib.rs",b"/p/lib.rs")?;
 let rev=fs.metadata(b"/p/main.rs").map_err(|_|Error::Missing)?.0;
 if agent_02::merge(&mut fs,b"/p/main.rs",rev,b"different",b"incoming")
  !=Err(Error::Conflict){return Err(Error::Conflict);}
 if fs.metadata(b"/p/lib.rs").is_err(){return Err(Error::Missing);}
 Ok(())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn authentic_guest_editor_terminal_build_debug_window_and_trash_smoke(){
  assert_eq!(smoke(),Ok(()));
 }
 #[test]fn project_scoped_terminal_rejects_foreign_file(){
  let who=Identity{pid:7,generation:1};
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"safe").unwrap();
  let t=rights::grant(true,who,&fs,b"/p",rights::READ,10).unwrap();
  assert_eq!(agent_10::run(&fs,who,t,1,b"cat /other/private",&mut[0;10]),
   Err(Error::Denied));
 }
}

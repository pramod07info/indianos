//! R036/20 end-to-end functional acceptance using real guest file data.
use super::{Error,agent_01,agent_02,agent_03,agent_04,agent_05,agent_06,agent_07,
 agent_08,agent_09,agent_10,agent_11,agent_12,agent_13,agent_14,agent_15,
 agent_16,agent_17,agent_18,agent_19};
use crate::batch7_ramfs::RamFs;
use crate::batch5_session::Identity;
use crate::workstation_r025::agent_02 as rights;
pub fn smoke()->Result<(),Error>{
 let mut fs=RamFs::<12>::new();
 agent_02::create::<12,32768>(&mut fs,0,&[
  (b"/p/main.rs",b"fn old() {\n}\n"),
  (b"/p/test.rs",b"old();\n"),
  (b"/p/log",b"begin\n")])?;
 let mut inventory=[agent_01::EMPTY;12];
 if agent_01::inventory(&fs,b"/p",&mut inventory)?!=3{return Err(Error::Conflict);}
 let mut tabs=agent_06::Tabs::<4>::new(7);
 let main=b"/p/main.rs";
 let rev=fs.metadata(main).map_err(|_|Error::Missing)?.0;
 let tab=tabs.open(7,1,rev)?;
 agent_05::patch(&mut fs,main,rev,3,6,b"new")?;
 if tabs.check(7,tab,fs.metadata(main).map_err(|_|Error::Missing)?.0)!=Err(Error::Stale){
  return Err(Error::Conflict);
 }
 let mut found=[agent_08::EMPTY;8];
 if agent_08::search(&fs,b"/p",b"old",&mut found)?!=1{return Err(Error::Conflict);}
 let version=fs.version();
 if agent_09::replace::<12,32768>(&mut fs,b"/p",version,b"old",b"new")?!=1{
  return Err(Error::Conflict);
 }
 if agent_07::locate(&fs,main,1,4)?!=3{return Err(Error::Conflict);}
 let checkpoint=agent_10::capture(&fs,main)?;
 if !agent_10::unchanged(&fs,main,checkpoint)?{return Err(Error::Conflict);}
 let who=Identity{pid:7,generation:1};
 let ticket=rights::grant(true,who,&fs,b"/p",rights::READ|rights::WRITE,100)
 .map_err(|_|Error::Denied)?;
 let mut output=[0u8;64];
 let n=agent_11::execute(&fs,who,ticket,1,&[b"cat /p/test.rs"],&mut output)?;
 if &output[..n]!=b"new();\n"{return Err(Error::Conflict);}
 let mut matches=[crate::workstation_r032::agent_12::EMPTY;8];
 if agent_12::grep(&fs,who,ticket,1,b"/p",b"new",&mut matches)?<2{
  return Err(Error::Conflict);
 }
 let log_rev=fs.metadata(b"/p/log").map_err(|_|Error::Missing)?.0;
 agent_13::append(&mut fs,who,ticket,1,b"/p/log",log_rev,b"done\n")?;
 if !agent_14::execute::<3>(7,&[0,1,2],&[true,true,true])?{
  return Err(Error::Conflict);
 }
 let revision=fs.metadata(main).map_err(|_|Error::Missing)?.0;
 let digest=crate::workstation_r032::agent_02::digest(b"artifact");
 agent_15::verify(&fs,main,revision,b"artifact",digest)?;
 if !agent_16::results(&fs,main,true,4,0,revision)?{
  return Err(Error::Conflict);
 }
 if !agent_17::breakpoint(&fs,main,revision,1,4096,1,4096,true)?{
  return Err(Error::Conflict);
 }
 let mut trace=agent_18::Trace::<4>::new(7);
 trace.push(7,true,agent_18::Frame{sequence:1,pc:4096})?;
 if trace.latest().ok_or(Error::Missing)?.pc!=4096{return Err(Error::Conflict);}
 let mut layout=agent_19::Layout::new(7,1200,800)?;
 if layout.coverage()!=960000{return Err(Error::Conflict);}
 layout.focus(7,2)?;
 let rev=fs.metadata(main).map_err(|_|Error::Missing)?.0;
 agent_03::rename(&mut fs,b"/p",main,b"/p/entry.rs",rev)?;
 let generation=fs.version();
 agent_04::delete::<12,32768>(&mut fs,b"/p",generation,&[b"/p/log"])?;
 if fs.metadata(b"/p/log").is_ok(){return Err(Error::Conflict);}
 Ok(())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn all_twenty_services_exercised_on_real_guest_files(){assert_eq!(smoke(),Ok(()));}
 #[test]fn stale_project_generation_blocks_batch_create(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/p/existing",b"old").unwrap();
  assert_eq!(agent_02::create::<2,4096>(&mut fs,0,&[(b"/p/new",b"new")]),
  Err(Error::Stale));
 }
}

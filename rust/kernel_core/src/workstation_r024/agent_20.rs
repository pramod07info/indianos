//! R024/20: guest-safe real file-manager, editor and terminal workbench acceptance.
//! No synthetic desktop screenshots or claims of external process execution.
use super::{Error,agent_01,agent_02,agent_03,agent_04,agent_05,
 agent_06,agent_07,agent_08,agent_09,agent_10,
 agent_11,agent_12,agent_13,agent_14,agent_15,
 agent_16,agent_17,agent_18,agent_19};
use crate::batch5_session::Identity;
use crate::batch7_ramfs::RamFs;
pub fn smoke()->Result<(),Error>{
 let owner=Identity{pid:7,generation:1};
 let mut fs=RamFs::<5>::new();
 agent_01::execute(&mut fs,0,agent_01::Operation::Create,
  b"/p/main.rs",b"fn main() {}\n").map_err(|_|Error::Full)?;
 agent_01::execute(&mut fs,1,agent_01::Operation::Copy,
  b"/p/main.rs",b"/p/copy.rs")?;
 let mut usage=agent_04::stats(&fs,b"/p")?;
 if usage.files!=2{return Err(Error::Conflict);}
 let mut bin=agent_02::Bin::<2>::new(owner);
 let expected=fs.version();
 let slot=bin.trash(owner,&mut fs,b"/p/copy.rs",expected)?;
 let expected=fs.version();
 bin.restore(owner,&mut fs,slot,expected)?;
 let mut hit=[agent_07::EMPTY;4];
 if agent_07::scan(&fs,b"/p",b"main",&mut hit)!=Ok(2){
  return Err(Error::Conflict);
 }
 let range=agent_08::select(&fs,b"/p/main.rs",3,7)?;
 agent_08::replace(&mut fs,b"/p/main.rs",range,b"entry")?;
 let mut diff=agent_06::files(&fs,b"/p/main.rs",b"/p/copy.rs")?;
 if agent_06::identical(diff){return Err(Error::Conflict);}
 let (version,_)=fs.metadata(b"/p/main.rs").map_err(|_|Error::Missing)?;
 let mut diagnostics=[agent_09::Finding{path:[0;96],len:0,
  byte:0,line:0,column:0,version:0,
  severity:crate::workstation_r019::agent_04::Severity::Note};1];
 let d=crate::workstation_r019::agent_04::parse(
  b"/p/main.rs:1:4:warning source changed").map_err(|_|Error::Invalid)?;
 diagnostics[0]=agent_09::locate(&fs,d)?;
 if agent_09::navigate(&fs,diagnostics[0])!=Ok(3){
  return Err(Error::Conflict);
 }
 let mut edits=agent_10::Saves::<1>::new(owner);
 edits.enqueue(owner,&fs,b"/p/main.rs",b"fn main() {}\n")?;
 edits.flush::<5,8192>(owner,&mut fs)?;
 let rev=fs.version();
 agent_11::run(&mut fs,rev,agent_11::Command::Write,
  b"/p/config",b"theme=dark\n")?;
 let cfgrev=fs.metadata(b"/p/config").map_err(|_|Error::Missing)?.0;
 agent_15::set(&mut fs,b"/p/config",cfgrev,b"target",b"x86_64")?;
 let path=agent_12::normalize(b"/p",b"./config")?;
 if !agent_12::exists(&fs,path){return Err(Error::Missing);}
 let mut log=agent_13::History::<2>::new(owner);
 log.record(owner,1,b"cat /p/config")?;
 let mut output=[0u8;128];
 let n=agent_14::cat_grep(&fs,b"/p/config",b"theme",&mut output)?.0;
 if &output[..n]!=b"theme=dark\n"{return Err(Error::Conflict);}
 let mut locks=agent_16::Locks::<2>::new();
 let token=locks.acquire(owner,&fs,b"/p/config",1,100)?;
 locks.check(owner,&fs,b"/p/config",token,2)?;
 let mut launcher=agent_17::Launcher::<1>::new(owner);
 launcher.pin(owner,&fs,1,agent_17::App::Editor,b"/p/main.rs",1)?;
 launcher.launch(owner,&fs,1,1)?;
 let mut cache=agent_18::Cache::<1>::new();
 cache.insert(true,7,1,&fs,b"/p/main.rs",21,[3;32])?;
 cache.lookup(7,1,&fs,b"/p/main.rs",21)?;
 let mut coverage=agent_19::Coverage::<1>::new();
 coverage.register(7,&fs,b"/p/main.rs")?;
 coverage.hit(true,7,&fs,b"/p/main.rs",1)?;
 if !coverage.passed(7,b"/p/main.rs",100)?{return Err(Error::Conflict);}
 usage=agent_04::stats(&fs,b"/p")?;
 if usage.files!=3{return Err(Error::Conflict);}
 diff=agent_06::files(&fs,b"/p/main.rs",b"/p/copy.rs")?;
 if !agent_06::identical(diff){return Err(Error::Conflict);}
 let _=version;Ok(())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn real_file_manager_editor_terminal_and_cache_end_to_end(){
  assert_eq!(smoke(),Ok(()));
 }
 #[test]fn stale_user_source_write_never_overwrites_current_file(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"old").unwrap();
  fs.replace(b"/a",b"new").unwrap();
  assert_eq!(agent_11::run(&mut fs,1,agent_11::Command::Write,
   b"/a",b"stale"),Err(Error::Stale));
 }
 #[test]fn owner_generation_isolation_prevents_developer_trash_theft(){
  let one=Identity{pid:7,generation:1};
  let another=Identity{pid:7,generation:2};
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"secret").unwrap();
  let mut bin=agent_02::Bin::<1>::new(one);
  let slot=bin.trash(one,&mut fs,b"/a",1).unwrap();
  let rev=fs.version();
  assert_eq!(bin.restore(another,&mut fs,slot,rev),Err(Error::Denied));
 }
}

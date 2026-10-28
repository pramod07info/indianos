//! R023/20: end-to-end RAM-backed Developer Workstation acceptance.
//! Actual scaffold, edits, format, terminal, test/results, build and release.
use super::{Error,agent_01,agent_03,agent_04,agent_05,agent_08,
 agent_10,agent_11,agent_12,agent_15,agent_16,agent_18,agent_19};
use crate::workstation_r022::agent_17;
use crate::batch5_session::Identity;
use crate::batch7_ramfs::RamFs;
pub fn smoke()->Result<(),Error>{
 let owner=Identity{pid:7,generation:1};
 let mut fs=RamFs::<4>::new();
 agent_01::create::<4,8192>(&mut fs,0,b"/p",agent_01::Template::RustCli)?;
 if agent_01::count_existing(&fs,b"/p")!=3{return Err(Error::Conflict);}
 let name=b"/p/src/main.rs";
 let (_,size)=fs.metadata(name).map_err(|_|Error::Missing)?;
 let mut source=[0u8;2048];fs.read(name,0,&mut source[..size])
  .map_err(|_|Error::Missing)?;
 if !agent_04::balanced(&source[..size]){return Err(Error::Conflict);}
 let expected=fs.version();
 let rename=agent_03::replace_all::<4,8192>(&mut fs,expected,
  b"/p",b"TODO",b"DONE")?;
 if rename.occurrences!=1{return Err(Error::Conflict);}
 let (rev,_)=fs.metadata(name).map_err(|_|Error::Missing)?;
 agent_05::format_file(&mut fs,name,rev,4)?;
 let mut cmd=[agent_11::EMPTY;3];
 if agent_11::lex(b"cat /p/src/main.rs",&mut cmd)!=Ok(2){
  return Err(Error::Conflict);
 }
 let mut rendered=[0u8;2048];
 let n=agent_12::run(&fs,b"cat /p/src/main.rs",&mut rendered)?;
 if !rendered[..n].windows(4).any(|w|w==b"DONE"){
  return Err(Error::Conflict);
 }
 // A diagnostic arrives from a trusted compiler and applies an exact fix.
 fs.replace(b"/p/issue.rs",b"retrun 1;").map_err(|_|Error::Full)?;
 let diagnostic=crate::workstation_r019::agent_04::parse(
  b"/p/issue.rs:1:1:error unknown return").map_err(|_|Error::Invalid)?;
 let fix=agent_16::propose(&fs,&diagnostic,b"retrun",b"return")?;
 agent_16::apply(&mut fs,fix)?;
 let mut jobs=agent_10::Scheduler::<1>::new(7);
 jobs.add(7,1,0,2,100)?;jobs.start(7,1,1)?;
 jobs.finish(true,1,0,2)?;
 let mut tests=agent_15::Results::<2>::new(7,2);
 tests.ingest(true,7,b"test source::smoke ... ok")?;
 if tests.finish(true,7)?!=(1,0,0){return Err(Error::Conflict);}
 let mut release=agent_19::Releases::<1>::new();
 release.prepare(7,1,&fs,[4;32],1,2)?;
 release.validate(true,7,1,&fs,&jobs,&tests)?;
 release.review(7,1,8)?;
 release.publish(true,7,1,&fs)?;
 if release.state(7,1)!=Ok(agent_19::Stage::Released){
  return Err(Error::Conflict);
 }
 let mut authority=agent_17::Authority::<1>::new();
 authority.issue(true,agent_17::Lease{id:1,owner,rights:1,
  expires:100,uses_remaining:2,revoked:false})
  .map_err(|_|Error::Denied)?;
 let mut ui=agent_18::Dispatcher::<1>::new(owner);
 ui.submit(owner,1,agent_18::Action::ReadFile,1,fs.version())?;
 ui.approve(owner,1,&mut authority,1,1)?;
 let mut out=[0u8;128];
 let count=ui.execute_readonly(owner,1,&fs,b"/p/issue.rs",
  fs.version(),&mut out)?;
 if &out[..count]!=b"return 1;"{return Err(Error::Conflict);}
 Ok(())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn native_project_scaffold_edit_test_release_and_ui_file_open(){
  assert_eq!(smoke(),Ok(()));
 }
 #[test]fn stale_source_cannot_publish_release_or_overwrite_user_file(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"a").unwrap();
  let mut r=agent_19::Releases::<1>::new();
  r.prepare(7,1,&fs,[1;32],1,2).unwrap();
  fs.replace(b"/a",b"changed").unwrap();
  assert_eq!(r.publish(true,7,1,&fs),Err(Error::Stale));
 }
 #[test]fn terminal_does_not_exec_unapproved_native_commands(){
  let fs=RamFs::<1>::new();let mut out=[0u8;64];
  assert_eq!(agent_12::run(&fs,b"bash -c \"danger\"",&mut out),
   Err(Error::Denied));
 }
}

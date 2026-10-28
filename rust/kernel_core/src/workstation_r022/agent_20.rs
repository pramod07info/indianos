//! R022/20: full guest-safe workstation lifecycle acceptance.
//! Exercises actual RamFs bytes and version-aware source workflow rather than
//! pretending this module boots a full graphical IDE or runs a native compiler.
use super::{Error,agent_01,agent_02,agent_03,agent_04,agent_05,
 agent_06,agent_08,agent_09,agent_10,agent_11,
 agent_12,agent_15,agent_17,agent_18,agent_19};
use crate::batch5_session::Identity;
use crate::batch7_ramfs::RamFs;
pub fn smoke()->Result<(),Error>{
 let owner=Identity{pid:7,generation:1};
 let mut fs=RamFs::<3>::new();
 fs.replace(b"/src/main.rs",b"fn main").map_err(|_|Error::Full)?;
 fs.replace(b"/src/tests.rs",b"#[test]\nfn example(){}\n")
  .map_err(|_|Error::Full)?;
 let mut index=[agent_01::EMPTY;3];
 if agent_01::enumerate(&fs,b"/src",&mut index)!=Ok(2){
  return Err(Error::Conflict);
 }
 let mut history=agent_05::History::<1>::new();
 history.capture(&fs,b"/src/main.rs")?;
 let mut editor=agent_03::Session::<128>::new(7);
 editor.open(7,&fs,b"/src/main.rs")?;
 editor.append(7,b"(){}")?;editor.save(7,&mut fs,b"/src/main.rs")?;
 let mut hits=[agent_04::EMPTY;4];
 if agent_04::search(&fs,b"/src",b"main",&mut hits)!=Ok(1){
  return Err(Error::Conflict);
 }
 let mut tests=[agent_12::EMPTY;2];
 if agent_12::discover_file(&fs,b"/src/tests.rs",&mut tests)!=Ok(1){
  return Err(Error::Conflict);
 }
 let mut integrity=[agent_06::EMPTY;3];
 if agent_06::scan(&fs,&mut integrity)!=Ok(2){return Err(Error::Conflict);}
 let mut changes=[agent_01::EMPTY;3];
 if agent_01::enumerate(&fs,b"/src",&mut changes)!=Ok(2){
  return Err(Error::Conflict);
 }
 let mut manifest=agent_09::Export::<3>::new();manifest.capture(&fs)?;
 let mut gate=agent_15::Gateway::<2>::new();
 gate.request(agent_15::Request{id:1,owner:7,rights:3,
  command:agent_15::Command::Compile,artifact:11,expires:100,
  status:agent_15::Status::Running})?;
 gate.authorize(1,7,3,1)?;gate.execute_start(true,1,2)?;
 gate.complete(true,1,true)?;
 let mut build=agent_11::Matrix::<3>::new(7);
 build.add(7,agent_11::Target{id:1,parent:0,arch:agent_11::Arch::X86_64,
  edition:agent_11::Edition::Developer,toolchain:21,abi:1,
  state:agent_11::State::Queued,result:0})?;
 build.start(7,1,1,21)?;build.finish(7,1,true,0)?;
 let mut release=agent_19::Publisher::new();
 release.draft(7,1,fs.version(),21,1,[2;32])?;
 release.validate(true,&fs,&manifest,&build,7,10)?;
 release.approve(8,7)?;
 if !release.publishable(&fs){return Err(Error::Conflict);}
 let mut leases=agent_17::Authority::<1>::new();
 leases.issue(true,agent_17::Lease{id:1,owner,rights:3,
  expires:100,uses_remaining:2,revoked:false})?;
 leases.use_rights(owner,1,1,1)?;
 let mut budget=agent_18::Governor::<2>::new(128,2,16);
 budget.admit(agent_18::Tenant{owner:7,app:1,memory:64,cpu:1,
  priority:agent_18::Priority::Interactive,active:false})?;
 let mut snapshot=agent_10::Restore::<2048>::new(owner);
 snapshot.prepare(owner,&fs)?;
 if snapshot.dry_run::<3>(owner)!=Ok((2,3)){return Err(Error::Conflict);}
 if agent_02::affected(&fs,b"/src")!=2{return Err(Error::Conflict);}
 let mut result=RamFs::<3>::new();
 snapshot.commit(owner,&mut result,0)?;
 if result.count()!=2||result.version()!=fs.version(){
  return Err(Error::Conflict);
 }
 Ok(())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn real_source_edit_search_test_build_release_restore_workflow(){
  assert_eq!(smoke(),Ok(()));
 }
 #[test]fn stale_file_version_prevents_wrong_release_manifest(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"a").unwrap();
  let mut m=agent_09::Export::<1>::new();m.capture(&fs).unwrap();
  fs.replace(b"/a",b"b").unwrap();
  assert_eq!(m.verify(&fs),Err(Error::Stale));
 }
 #[test]fn unauthorized_rights_cannot_submit_compiler_execution(){
  let mut g=agent_15::Gateway::<1>::new();
  g.request(agent_15::Request{id:1,owner:7,rights:3,
   command:agent_15::Command::Compile,artifact:3,
   expires:10,status:agent_15::Status::Requested}).unwrap();
  assert_eq!(g.authorize(1,7,1,1),Err(Error::Denied));
 }
}

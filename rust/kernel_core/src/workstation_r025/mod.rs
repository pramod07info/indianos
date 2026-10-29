//! IndianOS R025: 20 workstreams consolidated into one kernel build.
//! Features execute on actual RamFs bytes, authenticated session identities,
//! bounded source index and trusted build/test evidence. Policy models are
//! NOT a native compiler, compositor implementation, remote Git transport,
//! hardware debugger, installed desktop IDE or durable disk storage.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{Invalid,Denied,Full,Missing,Stale,Conflict,Overflow,NotReady}
pub mod agent_01;
pub mod agent_02;
pub mod agent_03;
pub mod agent_04;
pub mod agent_05;
pub mod agent_06;
pub mod agent_07;
pub mod agent_08;
pub mod agent_09;
pub mod agent_10;
pub mod agent_11;
pub mod agent_12;
pub mod agent_13;
pub mod agent_14;
pub mod agent_15;
pub mod agent_16;
pub mod agent_17;
pub mod agent_18;
pub mod agent_19;
pub mod agent_20;
#[cfg(test)]mod integration_tests{
 use super::*;
 #[test]fn guest_real_source_workspace_build_test_debug_release_smoke(){
  assert_eq!(agent_20::smoke(),Ok(()));
 }
 #[test]fn revoked_project_ticket_and_stale_builder_are_rejected(){
  use crate::batch5_session::Identity;
  use crate::batch7_ramfs::RamFs;
  let actor=Identity{pid:7,generation:1};
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/main",b"safe").unwrap();
  let mut ticket=agent_02::grant(true,actor,&fs,b"/p",agent_02::READ,20).unwrap();
  agent_02::revoke(&mut ticket,true).unwrap();
  assert_eq!(agent_02::read(ticket,actor,1,&fs,b"/p/main",&mut[0;4]),
   Err(Error::Denied));
  let mut manifest=agent_14::Manifest::<1>::new(7,21);
  manifest.capture(7,&fs,b"/p").unwrap();
  fs.replace(b"/p/main",b"modified").unwrap();
  assert_eq!(manifest.verify(7,&fs),Err(Error::Stale));
 }
}

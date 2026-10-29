//! R026 developer workstation: 20 Rust workstreams integrated in one kernel build.
//! Real RamFs source editing is not disk persistence. Build/debug state requires trusted worker.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{Invalid,Denied,Full,Missing,Stale,Conflict,Overflow,NotReady}
impl From<crate::workstation_r025::Error> for Error{
 fn from(e:crate::workstation_r025::Error)->Self{
  use crate::workstation_r025::Error as Old;
  match e{
   Old::Invalid=>Self::Invalid,
   Old::Denied=>Self::Denied,
   Old::Full=>Self::Full,
   Old::Missing=>Self::Missing,
   Old::Stale=>Self::Stale,
   Old::Conflict=>Self::Conflict,
   Old::Overflow=>Self::Overflow,
   Old::NotReady=>Self::NotReady,
  }
 }
}

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
 #[test]fn full_guest_workstation_smoke(){assert_eq!(agent_20::smoke(),Ok(()));}
 #[test]fn stale_editor_restore_is_denied(){
  use crate::batch5_session::Identity;
  use crate::batch7_ramfs::RamFs;
  let owner=Identity{pid:7,generation:1};
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"old").unwrap();
  let mut h=agent_04::History::<1>::new(owner);
  let slot=h.capture(owner,&fs,b"/a",1).unwrap();
  fs.replace(b"/a",b"new").unwrap();
  assert_eq!(h.restore(owner,&mut fs,slot,1),Err(Error::Stale));
 }
}

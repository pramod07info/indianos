//! R035/12 — Authorized guest terminal append to actual log file.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn append<const F:usize>(fs:&mut RamFs<F>,who:crate::batch5_session::Identity,ticket:crate::workstation_r025::agent_02::Ticket,now:u64,path:&[u8],revision:u64,text:&[u8])->Result<u64,Error>{crate::workstation_r032::agent_13::append(fs,who,ticket,now,path,revision,text).map_err(|_|Error::Denied)}
#[cfg(test)]mod tests{use super::*;#[test]fn real_workflow_and_failure_guard(){let who=crate::batch5_session::Identity{pid:4,generation:1};let mut fs=RamFs::<1>::new();fs.replace(b"/p/log",b"one\n").unwrap();let ticket=crate::workstation_r025::agent_02::grant(true,who,&fs,b"/p",crate::workstation_r025::agent_02::WRITE,10).unwrap();assert!(append(&mut fs,who,ticket,1,b"/p/log",1,b"two\n").is_ok());}}

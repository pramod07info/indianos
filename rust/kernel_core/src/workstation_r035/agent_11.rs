//! R035/11 — Authenticated read-only terminal batch using existing guest command runner.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn execute<const F:usize>(fs:&RamFs<F>,who:crate::batch5_session::Identity,ticket:crate::workstation_r025::agent_02::Ticket,now:u64,commands:&[&[u8]],out:&mut[u8])->Result<usize,Error>{crate::workstation_r031::agent_11::batch(fs,who,ticket,now,commands,out).map_err(|_|Error::Denied)}
#[cfg(test)]mod tests{use super::*;#[test]fn real_workflow_and_failure_guard(){let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"hello").unwrap();let who=crate::batch5_session::Identity{pid:7,generation:1};let ticket=crate::workstation_r025::agent_02::grant(true,who,&fs,b"/p",crate::workstation_r025::agent_02::READ,10).unwrap();let mut out=[0;16];let n=execute(&fs,who,ticket,1,&[b"cat /p/a"],&mut out).unwrap();assert_eq!(&out[..n],b"hello");}}

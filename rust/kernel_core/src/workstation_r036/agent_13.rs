//! R036/13: Write-permitted terminal append with source version.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX,PATH_MAX};
pub fn append<const F:usize>(fs:&mut RamFs<F>,who:crate::batch5_session::Identity,ticket:crate::workstation_r025::agent_02::Ticket,now:u64,path:&[u8],rev:u64,text:&[u8])->Result<u64,Error>{crate::workstation_r032::agent_13::append(fs,who,ticket,now,path,rev,text).map_err(|_|Error::Denied)}
#[cfg(test)]mod tests{use super::*;#[test]fn real_functional_regression(){let mut fs=RamFs::<1>::new();fs.replace(b"/p/log",b"first\n").unwrap();let who=crate::batch5_session::Identity{pid:7,generation:1};let t=crate::workstation_r025::agent_02::grant(true,who,&fs,b"/p",crate::workstation_r025::agent_02::WRITE,100).unwrap();assert!(append(&mut fs,who,t,1,b"/p/log",1,b"next\n").is_ok());}}

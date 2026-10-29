//! R036/12: Terminal source-aware grep with scoped ticket.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX,PATH_MAX};
pub fn grep<const F:usize,const N:usize>(fs:&RamFs<F>,who:crate::batch5_session::Identity,ticket:crate::workstation_r025::agent_02::Ticket,now:u64,root:&[u8],pattern:&[u8],out:&mut[crate::workstation_r032::agent_12::Hit;N])->Result<usize,Error>{crate::workstation_r032::agent_12::grep(fs,who,ticket,now,root,pattern,out).map_err(|_|Error::Denied)}
#[cfg(test)]mod tests{use super::*;#[test]fn real_functional_regression(){let mut fs=RamFs::<1>::new();fs.replace(b"/p/a.rs",b"error\nok").unwrap();let who=crate::batch5_session::Identity{pid:7,generation:1};let t=crate::workstation_r025::agent_02::grant(true,who,&fs,b"/p",crate::workstation_r025::agent_02::READ,100).unwrap();let mut out=[crate::workstation_r032::agent_12::EMPTY;2];assert_eq!(grep(&fs,who,t,1,b"/p",b"error",&mut out),Ok(1));}}

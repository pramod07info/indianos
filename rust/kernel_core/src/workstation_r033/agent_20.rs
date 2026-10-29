//! R033/20: integrated guest-side developer workstation regression across 19 real services.
use super::{Error,agent_01,agent_02,agent_03,agent_04,agent_05,agent_06,
 agent_07,agent_08,agent_09,agent_10,agent_11,agent_12,agent_13,
 agent_14,agent_15,agent_16,agent_17,agent_18,agent_19};
use crate::batch7_ramfs::RamFs;
pub fn smoke()->Result<(),Error>{
 let mut fs=RamFs::<3>::new();
 fs.replace(b"/p/main.rs",b"fn main() {\r\n}\r\n").map_err(|_|Error::Full)?;
 let original=agent_02::checksum(&fs,b"/p/main.rs")?;
 if agent_03::lines(&fs,b"/p/main.rs")?!=2{return Err(Error::Conflict);}
 agent_04::admit(&fs,b"/p/main.rs",3,64)?;
 if agent_05::normalize(&mut fs,b"/p/main.rs",1)?!=2{return Err(Error::Conflict);}
 if agent_02::checksum(&fs,b"/p/main.rs")?==original{return Err(Error::Conflict);}
 if agent_06::page(10,3,4)?!=(3,7){return Err(Error::Conflict);}
 if agent_07::cursor("αb".as_bytes(),2)?!=1{return Err(Error::Conflict);}
 let mut history=agent_08::History::<2>::new();history.push(1)?;history.push(2)?;
 if history.undo()?!=2{return Err(Error::Conflict);}
 if agent_09::count(b"ab-ab",b"ab")?!=2{return Err(Error::Conflict);}
 agent_10::guard(b"/p",b"/p/main.rs")?;
 if agent_11::number(b"12")?!=12{return Err(Error::Conflict);}
 if agent_12::tail(b"old\nnew\n",4)?!=b"new\n"{return Err(Error::Conflict);}
 if agent_13::duration(2,10,20)?!=8{return Err(Error::Conflict);}
 if !agent_14::ready(3,3,0)?{return Err(Error::Conflict);}
 if agent_15::advance(1,2)?!=2{return Err(Error::Conflict);}
 if !agent_16::passed(&[(1,true),(2,true)])?{return Err(Error::Conflict);}
 let mut watch=agent_17::Watch{pc:10,generation:1};
 if !agent_17::update(&mut watch,20,2)?{return Err(Error::Conflict);}
 agent_18::unique(&[10,20,30])?;
 if agent_19::pane(800,600,200,200)?!=480000{return Err(Error::Conflict);}
 fs.replace(b"/p/trash.rs",b"delete me").map_err(|_|Error::Full)?;
 let rev=fs.metadata(b"/p/trash.rs").map_err(|_|Error::Missing)?.0;
 agent_01::delete(&mut fs,b"/p/trash.rs",rev)?;
 if fs.metadata(b"/p/trash.rs").is_ok(){return Err(Error::Conflict);}
 Ok(())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn full_twenty_workstream_guest_integration(){assert_eq!(smoke(),Ok(()));}
 #[test]fn source_delete_refuses_stale_revision(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"a").unwrap();
  assert_eq!(agent_01::delete(&mut fs,b"/p/a",0),Err(Error::Stale));
 }
}

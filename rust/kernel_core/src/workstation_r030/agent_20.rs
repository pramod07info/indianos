//! R030/20: guest-resident acceptance joining real files, editor, shell,
//! build policy, diagnostics, test evidence, debug watches and spatial focus.
use super::{Error,agent_01,agent_02,agent_03,agent_04,agent_05,agent_06,
 agent_07,agent_08,agent_09,agent_10,agent_11,agent_12,agent_13,
 agent_14,agent_15,agent_16,agent_17,agent_18,agent_19};
use crate::batch7_ramfs::{RamFs,PATH_MAX};
use crate::workstation_r029::agent_18::Rect;
pub fn smoke()->Result<(),Error>{
 let mut fs=RamFs::<5>::new();
 fs.replace(b"/p/a.rs",b"fn foo(){ let x=1; }\n").map_err(|_|Error::Full)?;
 fs.replace(b"/p/b.rs",b"fn bar(){}\n").map_err(|_|Error::Full)?;
 fs.replace(b"/other/private",b"hidden").map_err(|_|Error::Full)?;
 let mut entries=[agent_01::EMPTY;5];
 if agent_01::inventory(&fs,b"/p",&mut entries)?!=2{return Err(Error::Conflict);}
 if agent_02::usage(&fs,b"/p",3,100)?.files!=2{return Err(Error::Conflict);}
 agent_03::copy(&mut fs,b"/p",b"/p/a.rs",b"/p/c.rs",1)?;
 if agent_04::locate(b"a\nb",2)?.line!=2{return Err(Error::Conflict);}
 let (changed,_)=agent_05::replace_all(&mut fs,b"/p/a.rs",1,b"let x",b"let y")?;
 if changed!=1{return Err(Error::Conflict);}
 let rev=fs.metadata(b"/p/a.rs").map_err(|_|Error::Missing)?.0;
 if agent_06::rename(&mut fs,b"/p/a.rs",rev,b"foo",b"entry")?!=1{
  return Err(Error::Conflict);
 }
 let mut excerpt=[0u8;64];
 if agent_07::lines(&fs,b"/p/a.rs",1,1,&mut excerpt)?.1!=1{
  return Err(Error::Conflict);
 }
 agent_08::balanced(b"fn a(){[1,2]}")?;
 let mut outline=[agent_09::EMPTY;2];
 if agent_09::outline(b"fn first(){}", &mut outline)?!=1{
  return Err(Error::Conflict);
 }
 if !agent_10::matches(b"/p/*.rs",b"/p/a.rs")?{return Err(Error::Conflict);}
 let mut resolved=[0;PATH_MAX];
 let n=agent_11::resolve(b"/p",b"/p",b"src/lib.rs",&mut resolved)?;
 if &resolved[..n]!=b"/p/src/lib.rs"{return Err(Error::Conflict);}
 let n=agent_12::tail(&fs,b"/p/a.rs",1,&mut excerpt)?;
 if n==0{return Err(Error::Conflict);}
 let mut env=agent_13::Env::<2>::new(7);env.set(7,b"WORKSPACE",b"/p")?;
 if env.read(7,b"WORKSPACE",&mut excerpt)?!=2{return Err(Error::Conflict);}
 if agent_14::fingerprint(&fs,b"/p",b"rustc")?.1!=3{
  return Err(Error::Conflict);
 }
 let jobs=[agent_15::Job{id:1,dependencies:1},agent_15::Job{id:0,dependencies:0}];
 let mut order=[0u8;2];
 agent_15::order(&jobs,&mut order)?;
 if order!=[0,1]{return Err(Error::Conflict);}
 let report=agent_16::parse(true,b"warning: unused\n")?;
 if !agent_16::release_ready(report){return Err(Error::Conflict);}
 let mut ledger=agent_17::Ledger::<1>::new(7);
 ledger.record(7,true,agent_17::Test{suite:1,sequence:1,passed:true})?;
 if !ledger.green(7)?{return Err(Error::Conflict);}
 let mut watch=agent_18::Watch::new(7);
 watch.observe(7,true,agent_18::Sample{address:4096,sequence:1,value:7})?;
 if !watch.observe(7,true,agent_18::Sample{address:4096,sequence:2,value:8})?{
  return Err(Error::Conflict);
 }
 let panes=[agent_19::Window{id:1,rect:Rect{x:0,y:0,w:300,h:400}},
  agent_19::Window{id:2,rect:Rect{x:300,y:0,w:300,h:400}}];
 if agent_19::next(&panes,1,agent_19::Direction::Right)?!=2{
  return Err(Error::Conflict);
 }
 Ok(())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn integrated_source_to_build_debug_ui_guest_acceptance(){
  assert_eq!(smoke(),Ok(()));
 }
 #[test]fn integration_does_not_bypass_real_project_filesystem_permissions(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/p/a",b"ok").unwrap();
  assert_eq!(agent_03::copy(&mut fs,b"/p",b"/p/a",b"/private/a",1),
   Err(Error::Denied));
 }
}
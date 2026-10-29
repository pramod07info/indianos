//! R031/20: end-to-end guest integration of project files, editor, terminal,
//! verified release, debugger trace and pane geometry. Actual RamFs bytes.
use super::{Error,agent_01,agent_02,agent_03,agent_04,agent_05,agent_06,
 agent_07,agent_08,agent_09,agent_10,agent_11,agent_12,agent_13,
 agent_14,agent_15,agent_16,agent_17,agent_18,agent_19};
use crate::batch7_ramfs::RamFs;
use crate::batch5_session::Identity;
use crate::workstation_r025::agent_02 as rights;
use crate::workstation_r030::{agent_16 as diagnostics,agent_17 as tests};
use crate::workstation_r029::agent_18::Rect;
pub fn smoke()->Result<(),Error>{
 let mut fs=RamFs::<8>::new();
 fs.replace(b"/projects/p/src/main.rs",b"// TODO\nfn old(){}\n")
  .map_err(|_|Error::Full)?;
 fs.replace(b"/projects/p/src/lib.rs",b"fn old(){}\n")
  .map_err(|_|Error::Full)?;
 fs.replace(b"/projects/p/notes",b"note\n").map_err(|_|Error::Full)?;
 fs.replace(b"/private/file",b"secret").map_err(|_|Error::Full)?;
 if agent_01::clone_tree::<8,16384>(&mut fs,4,b"/projects/p",b"/backup")?!=3{
  return Err(Error::Conflict);
 }
 if agent_02::trash::<8,16384>(&mut fs,7,b"/projects/p",b"change1")?!=3{
  return Err(Error::Conflict);
 }
 if agent_02::restore::<8,16384>(&mut fs,10,b"change1",b"/projects/p")?!=3{
  return Err(Error::Conflict);
 }
 let ar=fs.metadata(b"/projects/p/src/main.rs").map_err(|_|Error::Missing)?.0;
 let br=fs.metadata(b"/projects/p/src/lib.rs").map_err(|_|Error::Missing)?.0;
 let targets=[
  agent_03::Target{path:b"/projects/p/src/main.rs",revision:ar},
  agent_03::Target{path:b"/projects/p/src/lib.rs",revision:br},
 ];
 let generation=fs.version();
 if agent_03::rename_project::<8,16384>(&mut fs,generation,&targets,b"old",b"entry")?!=2{
  return Err(Error::Conflict);
 }
 let snapshot=agent_04::Snapshot::<16384>::capture(&fs)?;
 snapshot.preview(&fs)?;
 let mut baseline=[agent_05::EMPTY;8];
 let n=agent_05::capture(&fs,b"/projects/p",&mut baseline)?;
 let mut spans=[agent_06::EMPTY;8];
 if agent_06::tokenize(b"let x=\"ok\"; // help",&mut spans)?<2{
  return Err(Error::Conflict);
 }
 let rev=fs.metadata(b"/projects/p/src/lib.rs").map_err(|_|Error::Missing)?.0;
 let edits=[agent_07::Edit{start:3,end:8,expected:b"entry",replacement:b"compile"}];
 agent_07::apply(&mut fs,b"/projects/p/src/lib.rs",rev,&edits)?;
 let mut dirty=[0usize;8];
 if agent_05::changed(&fs,&baseline[..n],&mut dirty)?!=1{
  return Err(Error::Conflict);
 }
 let note=fs.metadata(b"/projects/p/notes").map_err(|_|Error::Missing)?.0;
 if agent_08::clean(&mut fs,b"/projects/p/notes",note)?.0!=0{
  return Err(Error::Conflict);
 }
 let mut tasks=[agent_09::EMPTY;8];
 if agent_09::scan(&fs,b"/projects/p",&mut tasks)?<1{return Err(Error::Conflict);}
 let mut marks=agent_10::Bookmarks::<2>::new(7);
 let pinned=marks.pin(7,&fs,b"/projects/p/src/main.rs",0)?;
 if marks.navigate(7,&fs,pinned)?!=0{return Err(Error::Conflict);}
 let who=Identity{pid:7,generation:1};
 let ticket=rights::grant(true,who,&fs,b"/projects/p",rights::READ,100)
  .map_err(|_|Error::Denied)?;
 let mut output=[0u8;80];
 if agent_11::batch(&fs,who,ticket,1,&[b"cat /projects/p/notes"],&mut output)?!=5{
  return Err(Error::Conflict);
 }
 let mut history=agent_12::Scrollback::<64>::new(7);
 history.append(7,b"command ok\n")?;
 if history.read(7,&mut output)?==0{return Err(Error::Conflict);}
 let mut cwd=agent_13::Cwd::new(7,b"/projects/p")?;
 cwd.cd(7,&fs,b"src")?;
 if cwd.path(7)?!=b"/projects/p/src"{return Err(Error::Conflict);}
 let mut jobs=agent_14::Queue::<1>::new(7);
 jobs.enqueue(7,1,100)?;jobs.start(7,1,1)?;jobs.finish(7,true,1,0)?;
 let mut env=crate::workstation_r030::agent_13::Env::<1>::new(7);
 env.set(7,b"HOME",b"/projects/p")?;
 let n=agent_15::expand(&env,7,b"workspace=${HOME}",&mut output)?;
 if &output[..n]!=b"workspace=/projects/p"{return Err(Error::Conflict);}
 let mut cache=agent_16::Cache::new(7);
 cache.certify(7,true,true,&fs,b"/projects/p",b"rustc")?;
 let mut ledger=tests::Ledger::<1>::new(7);
 ledger.record(7,true,tests::Test{suite:1,sequence:1,passed:true})
  .map_err(|_|Error::Denied)?;
 let report=diagnostics::parse(true,b"warning: unused").map_err(|_|Error::Invalid)?;
 let permit=agent_17::authorize(7,true,&fs,b"/projects/p",b"rustc",
  &cache,&ledger,report,1)?;
 if !agent_17::current(permit,&fs){return Err(Error::Conflict);}
 let mut trace=agent_18::Trace::<2>::new(7);
 trace.observe(7,true,agent_18::Frame{sequence:1,pc:4096,line:2,value:10})?;
 let mut frames=[agent_18::EMPTY;2];
 if trace.after(7,0,&mut frames)?!=1{return Err(Error::Conflict);}
 let viewport=Rect{x:0,y:0,w:1200,h:800};
 let split=agent_19::split(viewport,agent_19::Axis::Horizontal,65,200)?;
 if !agent_19::valid(split,viewport){return Err(Error::Conflict);}
 if snapshot.preview(&fs)!=Err(Error::Stale){return Err(Error::Conflict);}
 Ok(())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn all_workstation_features_work_together_on_guest_file_bytes(){
  assert_eq!(smoke(),Ok(()));
 }
 #[test]fn unauthorized_project_copy_rejects_overlapping_destination(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/projects/p/src/a",b"a").unwrap();
  assert_eq!(agent_01::clone_tree::<2,2048>(&mut fs,1,b"/projects/p",b"/projects/p/copy"),
   Err(Error::Invalid));
 }
}
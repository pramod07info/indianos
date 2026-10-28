//! R021/20: full bounded workstation integration on real guest RamFs.
//! Demonstrates edit/autosave -> source tools -> lint -> build/test -> release gates.
//! Does not claim physical disk durability, a native compiler or graphical IDE.
use super::{Error,agent_01,agent_02,agent_03,agent_04,agent_05,
 agent_06,agent_07,agent_08,agent_09,agent_14};
use crate::batch5_session::Identity;
use crate::batch7_ramfs::RamFs;
pub fn smoke()->Result<(),Error>{
 let owner=Identity{pid:7,generation:1};
 let mut fs=RamFs::<3>::new();
 let mut autosave=agent_02::AutoSave::<2>::new(owner,0);
 autosave.stage(owner,b"/src/main.rs",b"pub fn main() {}\n")?;
 autosave.stage(owner,b"/src/lib.rs",b"pub struct Core {}\n")?;
 autosave.commit::<3,2048>(owner,&mut fs)?;
 let mut symbols=[agent_04::EMPTY;4];
 if agent_04::from_file(&fs,b"/src/main.rs",&mut symbols)!=Ok(1){
  return Err(Error::Conflict);
 }
 let mut candidates=[agent_05::EMPTY;4];
 let n=agent_05::in_file(&fs,b"/src/main.rs",b"ma",&mut candidates)?;
 if n!=1{return Err(Error::Conflict);}
 let mut findings=[agent_08::EMPTY;4];
 if agent_08::lint_file(&fs,b"/src/main.rs",80,&mut findings)!=Ok(0){
  return Err(Error::Conflict);
 }
 let mut watch=agent_03::Watch::<2>::new(owner,b"/src")?;
 watch.publish(true,b"/src/main.rs",1,agent_03::Kind::Create)?;
 let mut events=[agent_03::EMPTY;2];
 if watch.since(owner,0,&mut events)!=Ok(1){return Err(Error::Conflict);}
 let mut scheduler=agent_06::Pool::<2>::new(128,2);
 scheduler.submit(1,7,64,1,100)?;scheduler.submit(2,7,32,1,100)?;
 scheduler.start(7,1,1)?;scheduler.start(7,2,1)?;
 let mut logs=agent_07::Stream::<2>::new();
 logs.append(true,7,1,agent_07::Level::Info,b"compile succeeded")?;
 scheduler.finish(7,1,true)?;scheduler.finish(7,2,true)?;
 let mut tests=agent_09::Suite::<2>::new();
 tests.register(7,1,101,2)?;tests.record(true,7,1,101,true,0,5)?;
 if tests.totals(7,1)!=(0,1,0,0){return Err(Error::Conflict);}
 let mut manifest=agent_01::Manifest::<2>::new(7);
 manifest.add(7,0,agent_01::Package{id:1,version:1,
  digest:[1;32],depends:[0;4],owner:7})?;
 let mut release=agent_14::Gate::new(7,100,101,0b111)?;
 release.approve(8,101)?;release.report_ci(true,101,0b111,true)?;
 release.ready(100)?;release.merge_record(true,101,100)?;
 if fs.count()!=2||release.state!=agent_14::State::Merged{
  return Err(Error::Conflict);
 }
 Ok(())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn actual_workspace_autosave_build_test_review_pipeline(){
  assert_eq!(smoke(),Ok(()));
 }
 #[test]fn autosave_rejects_partial_source_update_before_build(){
  let owner=Identity{pid:7,generation:1};
  let mut fs=RamFs::<1>::new();
  let mut save=agent_02::AutoSave::<2>::new(owner,0);
  save.stage(owner,b"/a",b"one").unwrap();
  save.stage(owner,b"/b",b"two").unwrap();
  assert_eq!(save.commit::<1,1024>(owner,&mut fs),Err(Error::Full));
  assert_eq!(fs.count(),0);
 }
 #[test]fn unverified_release_cannot_be_marked_merged(){
  let mut gate=agent_14::Gate::new(7,100,101,0b11).unwrap();
  gate.approve(8,101).unwrap();
  gate.report_ci(false,101,0b11,true).unwrap_err();
  assert_eq!(gate.ready(100),Err(Error::NotReady));
 }
}

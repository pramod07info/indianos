//! R027/20: real guest developer-workstation end-to-end regression.
//! Uses live RamFs source operations, trusted metadata and bounded UI state.
use super::{Error,agent_01,agent_02,agent_03,agent_04,
 agent_05,agent_06,agent_07,agent_08,agent_09,agent_10,
 agent_11,agent_12,agent_13,agent_14,agent_15,agent_16,
 agent_17,agent_18,agent_19};
use crate::batch5_session::Identity;
use crate::batch7_ramfs::RamFs;
use crate::workstation_r025::agent_02 as rights;
pub fn smoke()->Result<(),Error>{
 let owner=Identity{pid:7,generation:1};
 let mut fs=RamFs::<8>::new();
 fs.replace(b"/p/main.rs",b"fn hello(){\n work();\n}\n").map_err(|_|Error::Full)?;
 fs.replace(b"/p/README.md",b"doc").map_err(|_|Error::Full)?;
 fs.replace(b"/q/other.rs",b"fn other(){}\n").map_err(|_|Error::Full)?;
 let mut inventory=[agent_01::EMPTY;5];
 if agent_01::list(&fs,b"/p",2,&mut inventory)?!=2{
  return Err(Error::Conflict);
 }
 let changes=[agent_02::Rename{from:b"/p/README.md",
  to:b"/p/ABOUT.md",revision:2}];
 let version=fs.version();
 agent_02::apply::<8,8192>(&mut fs,version,&changes)?;
 let used=agent_03::measure(&fs,b"/p",3,1024)?;
 if used.files!=2{return Err(Error::Conflict);}
 let old=agent_04::capture(&fs,b"/p/main.rs")?;
 agent_04::verify(&fs,old)?;
 let file_rev=fs.metadata(b"/p/main.rs").map_err(|_|Error::Missing)?.0;
 let patch=agent_05::Patch{start:3,end:8,expected:b"hello",replacement:b"start"};
 agent_05::apply(&mut fs,b"/p/main.rs",file_rev,patch)?;
 let mut bookmark=agent_06::Bookmarks::<1>::new(owner);
 bookmark.add(owner,&fs,1,b"/p/main.rs",2)?;
 if bookmark.jump(owner,&fs,1)?==0{return Err(Error::Conflict);}
 let mut folds=[agent_07::EMPTY;2];
 if agent_07::scan(&fs,b"/p/main.rs",&mut folds)?!=1{
  return Err(Error::Conflict);
 }
 let mut hits=[agent_08::EMPTY;2];
 if agent_08::file_preview(&fs,b"/p/main.rs",b"start",&mut hits)?!=1{
  return Err(Error::Conflict);
 }
 let expected=fs.version();
 agent_09::generate(&mut fs,expected,b"/p/lib.rs",agent_09::Template::RustLibrary)?;
 let mut alias=agent_10::Aliases::<1>::new(owner);
 alias.set(owner,b"ll",b"cat")?;
 let mut name=[0u8;8];
 if alias.resolve(owner,b"ll",&mut name)?!=3{return Err(Error::Conflict);}
 let mut log=agent_11::Scrollback::<2>::new(owner);
 log.append(owner,1,b"file created")?;
 if log.search(owner,b"created")?.is_none(){return Err(Error::Conflict);}
 let ticket=rights::grant(true,owner,&fs,b"/p",
  rights::READ|rights::WRITE,100).map_err(|_|Error::Denied)?;
 let mut source=[0u8;64];
 let read=agent_12::read(ticket,owner,1,&fs,b"/p/main.rs",&mut source)?;
 if !source[..read].starts_with(b"fn start"){return Err(Error::Conflict);}
 let mut plan=agent_13::Plan::<2>::new(7);
 plan.add(7,1,0,21)?;plan.add(7,2,1,21)?;
 plan.start(7,1,21)?;plan.complete(true,1,0)?;
 plan.start(7,2,21)?;plan.complete(true,2,0)?;
 if !plan.ready(7)?{return Err(Error::Conflict);}
 let mut manifest=agent_14::Manifest::<1>::new(7,21);
 manifest.add(7,&fs,b"/p/main.rs")?;
 manifest.verify(7,&fs,21)?;
 let mut tests=agent_15::Results::<1>::new(7);
 tests.register(7,1)?;tests.run(7,1)?;tests.report(true,1,0,12)?;
 if !tests.release_ready(7)?{return Err(Error::Conflict);}
 let mut callstack=agent_16::Callstack::<1>::new(owner);
 callstack.update(owner,true,1,&[agent_16::Frame{
  ip:0x1000,sp:0x2000,line:2,file_id:1}])?;
 if callstack.depth(owner)?!=1{return Err(Error::Conflict);}
 if agent_17::evaluate_text(b"40",b"2",agent_17::Operator::Add)?.number!=42{
  return Err(Error::Conflict);
 }
 let mut layout=agent_18::Layout::<2>::new(owner,1280,720);
 layout.open(owner,1,agent_18::Rect{x:0,y:0,w:640,h:720})?;
 layout.open(owner,2,agent_18::Rect{x:640,y:0,w:640,h:720})?;
 layout.focus(owner,1)?;
 if !layout.get(owner,1)?.focused{return Err(Error::Conflict);}
 let mut session=agent_19::Store::new();
 session.save(owner,1,b"tabs=2;focused=1")?;
 let mut state=[0u8;32];
 if session.export(owner,&mut state)?.1==0{return Err(Error::Conflict);}
 Ok(())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn real_file_editor_terminal_build_debug_and_layout_guest_acceptance(){
  assert_eq!(smoke(),Ok(()));
 }
 #[test]fn project_slash_boundary_blocks_cross_project_file_read(){
  let owner=Identity{pid:7,generation:1};
  let fs=RamFs::<1>::new();
  let ticket=rights::grant(true,owner,&fs,b"/p",rights::READ,10)
   .map_err(|_|Error::Denied).unwrap();
  assert_eq!(agent_12::check(ticket,owner,1,b"/px/secret",
   agent_12::Operation::Read),Err(Error::Denied));
 }
 #[test]fn invalid_source_patch_cannot_update_real_document(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"old").unwrap();
  let patch=agent_05::Patch{start:0,end:3,
   expected:b"wrong",replacement:b"new"};
  assert_eq!(agent_05::apply(&mut fs,b"/a",1,patch),Err(Error::Conflict));
  assert_eq!(fs.version(),1);
 }
}

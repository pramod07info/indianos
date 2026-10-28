//! R023/01: atomic project scaffolding into actual kernel RAM filesystem.
//! Creates real source/manifest bytes; does not run a compiler or installer.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Template{RustCli,RustLibrary,WebStarter}
const SUFFIXES:[&[u8];3]=[b"/project.toml",b"/src/main.rs",b"/src/lib.rs"];
fn paths(root:&[u8],index:usize)->Result<([u8;PATH_MAX],usize),Error>{
 if root.len()<2||root.len()>PATH_MAX-14||root[0]!=b'/'||
 root.last()==Some(&b'/')||root.windows(2).any(|w|w==b".."){
  return Err(Error::Invalid);
 }
 let suffix=SUFFIXES[index];let n=root.len()+suffix.len();
 if n>PATH_MAX{return Err(Error::Full);}
 let mut out=[0u8;PATH_MAX];
 out[..root.len()].copy_from_slice(root);
 out[root.len()..n].copy_from_slice(suffix);Ok((out,n))
}
fn sources(t:Template)->(&'static[u8],&'static[u8],&'static[u8]){
 match t{
  Template::RustCli=>(b"name = \"new-cli\"\ntype = \"rust-cli\"\n",
   b"fn main() {\n    // TODO: implement\n}\n",b"pub fn version() -> u32 { 1 }\n"),
  Template::RustLibrary=>(b"name = \"new-library\"\ntype = \"rust-library\"\n",
   b"fn main() {}\n",b"pub fn hello() -> &'static str { \"hello\" }\n"),
  Template::WebStarter=>(b"name = \"web-starter\"\ntype = \"web\"\n",
   b"fn main() { /* serve web assets */ }\n",b"pub fn routes() -> usize { 1 }\n"),
 }
}
pub fn create<const F:usize,const IMAGE:usize>(fs:&mut RamFs<F>,
 expected:u64,root:&[u8],template:Template)->Result<u64,Error>{
 if fs.version()!=expected{return Err(Error::Stale);}
 let mut base=[0u8;IMAGE];
 let n=fs.export_snapshot(&mut base).map_err(|_|Error::Full)?;
 let mut staged=RamFs::<F>::new();
 staged.import_snapshot(&base[..n]).map_err(|_|Error::Invalid)?;
 let (manifest,main,library)=sources(template);
 for (i,data) in [manifest,main,library].iter().enumerate(){
  let (path,len)=paths(root,i)?;
  if staged.metadata(&path[..len]).is_ok(){return Err(Error::Conflict);}
  staged.replace(&path[..len],data).map_err(|_|Error::Full)?;
 }
 let mut final_image=[0u8;IMAGE];
 let used=staged.export_snapshot(&mut final_image).map_err(|_|Error::Full)?;
 fs.import_snapshot(&final_image[..used]).map_err(|_|Error::Invalid)?;
 Ok(fs.version())
}
pub fn preview(root:&[u8],out:&mut[[u8;PATH_MAX];3])
 ->Result<[usize;3],Error>{
 let mut staged=[[0u8;PATH_MAX];3];let mut lengths=[0;3];
 for i in 0..3{let (p,len)=paths(root,i)?;staged[i]=p;lengths[i]=len;}
 *out=staged;Ok(lengths)
}
pub fn count_existing<const F:usize>(fs:&RamFs<F>,root:&[u8])->usize{
 (0..3).filter(|&i|paths(root,i).ok().is_some_and(|(p,n)|
   fs.metadata(&p[..n]).is_ok())).count()
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn new_workspace_scaffolds_three_real_files_in_one_change(){
  let mut fs=RamFs::<4>::new();
  assert_eq!(create::<4,4096>(&mut fs,0,b"/projects/app",Template::RustCli),Ok(3));
  assert_eq!(count_existing(&fs,b"/projects/app"),3);
  let mut data=[0;64];
  let n=fs.read(b"/projects/app/src/main.rs",0,&mut data).unwrap();
  assert!(data[..n].windows(7).any(|w|w==b"fn main"));
 }
 #[test]fn destination_collision_leaves_other_template_files_uncreated(){
  let mut fs=RamFs::<4>::new();
  fs.replace(b"/p/src/main.rs",b"original").unwrap();
  assert_eq!(create::<4,4096>(&mut fs,1,b"/p",Template::RustCli),Err(Error::Conflict));
  assert!(fs.metadata(b"/p/src/lib.rs").is_err());
  assert_eq!(fs.version(),1);
 }
 #[test]fn failed_capacity_and_stale_revision_preserve_real_workspace(){
  let mut fs=RamFs::<2>::new();
  assert_eq!(create::<2,4096>(&mut fs,0,b"/p",Template::RustLibrary),Err(Error::Full));
  assert_eq!(fs.count(),0);
  assert_eq!(create::<2,4096>(&mut fs,1,b"/p",Template::WebStarter),Err(Error::Stale));
 }
}

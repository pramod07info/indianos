//! R025/15: real Rust source #[test] discovery, bounded test selection and shards.
//! Lexical plan only; execution is performed by trusted native test worker.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Test{pub path:[u8;PATH_MAX],pub path_len:usize,pub name:[u8;64],
 pub len:usize,pub line:usize,pub shard:u8}
pub const EMPTY:Test=Test{path:[0;PATH_MAX],path_len:0,
 name:[0;64],len:0,line:0,shard:0};
pub fn discover<const F:usize,const O:usize>(fs:&RamFs<F>,prefix:&[u8],
 shards:u8,out:&mut[Test;O])->Result<usize,Error>{
 if prefix.first()!=Some(&b'/')||shards==0{return Err(Error::Invalid);}
 let mut stage=[EMPTY;O];let mut count=0usize;
 for i in 0..fs.count(){
  let mut path=[0u8;PATH_MAX];
  let (len,size,_)=fs.entry(i,&mut path).map_err(|_|Error::Invalid)?
   .ok_or(Error::Missing)?;
  if !path[..len].starts_with(prefix)||!path[..len].ends_with(b".rs"){continue;}
  let mut bytes=[0u8;CONTENT_MAX];
  fs.read(&path[..len],0,&mut bytes[..size]).map_err(|_|Error::Missing)?;
  let mut pending=false;
  for (idx,line) in bytes[..size].split(|&b|b==b'\n').enumerate(){
   let line=&line[line.iter().position(|b|*b!=b' ').unwrap_or(line.len())..];
   if line.starts_with(b"#[test]"){pending=true;continue;}
   if pending{
    let declaration=if line.starts_with(b"fn "){&line[3..]}
     else if line.starts_with(b"pub fn "){&line[7..]}
     else{pending=false;continue;};
    let n=declaration.iter().take_while(|b|b.is_ascii_alphanumeric()||**b==b'_').count();
    if n==0||n>64||declaration.get(n)!=Some(&b'('){return Err(Error::Invalid);}
    if count==O{return Err(Error::Full);}
    let mut test=EMPTY;test.path=path;test.path_len=len;
    test.name[..n].copy_from_slice(&declaration[..n]);test.len=n;
    test.line=idx+1;test.shard=(count%(shards as usize)) as u8;
    stage[count]=test;count+=1;pending=false;
   }
  }
 }
 *out=stage;Ok(count)
}
pub fn shard_count(tests:&[Test],shard:u8)->usize{
 tests.iter().filter(|t|t.shard==shard).count()
}
pub fn matching<'a>(tests:&'a[Test],name:&[u8])->Option<&'a Test>{
 tests.iter().find(|t|&t.name[..t.len]==name)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn discover_real_rust_tests_and_assign_distinct_shards(){
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/p/tests.rs",b"#[test]\nfn first(){}\n#[test]\nfn second(){}\n").unwrap();
  let mut tests=[EMPTY;2];assert_eq!(discover(&fs,b"/p",2,&mut tests),Ok(2));
  assert_eq!(tests[0].shard,0);assert_eq!(tests[1].shard,1);
 }
 #[test]fn syntax_error_in_test_declaration_refuses_partial_plan(){
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/p/a.rs",b"#[test]\nfn invalid name(){}\n").unwrap();
  let mut out=[EMPTY;1];
  assert_eq!(discover(&fs,b"/p",2,&mut out),Err(Error::Invalid));
  assert_eq!(out,[EMPTY]);
 }
 #[test]fn test_capacity_exceeded_preserves_previous_workbench_test_list(){
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/p/a.rs",b"#[test]\nfn a(){}\n#[test]\nfn b(){}\n").unwrap();
  let mut out=[EMPTY;1];assert_eq!(discover(&fs,b"/p",2,&mut out),Err(Error::Full));
  assert_eq!(out,[EMPTY]);
 }
}

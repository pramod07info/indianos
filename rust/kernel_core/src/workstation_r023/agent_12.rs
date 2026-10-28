//! R023/12: real-filesystem terminal builtins: pwd, ls, cat, inspect.
//! No external executables, shell pipes, network calls or file system writes.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
use super::agent_11::{lex,equals,Token,EMPTY};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Builtin{Pwd,Files,Cat,Stat,Unknown}
pub fn builtin(token:&Token)->Builtin{
 if equals(token,b"pwd"){Builtin::Pwd}
 else if equals(token,b"ls"){Builtin::Files}
 else if equals(token,b"cat"){Builtin::Cat}
 else if equals(token,b"stat"){Builtin::Stat}
 else{Builtin::Unknown}
}
fn path(token:&Token)->Result<&[u8],Error>{
 if token.len==0||token.bytes[0]!=b'/'||
  token.bytes[..token.len].windows(2).any(|x|x==b".."){
  return Err(Error::Invalid);
 }Ok(&token.bytes[..token.len])
}
fn write<const B:usize>(src:&[u8],out:&mut[u8;B])->Result<usize,Error>{
 if src.len()>B{return Err(Error::Full);}
 let mut stage=[0u8;B];stage[..src.len()].copy_from_slice(src);
 *out=stage;Ok(src.len())
}
pub fn run<const F:usize,const B:usize>(
 fs:&RamFs<F>,cmd:&[u8],out:&mut[u8;B])->Result<usize,Error>{
 let mut argv=[EMPTY;3];let n=lex(cmd,&mut argv)?;
 if n==0{return Err(Error::Invalid);}
 match builtin(&argv[0]){
  Builtin::Pwd=>{
   if n!=1{return Err(Error::Invalid);}
   write(b"/",out)
  }
  Builtin::Cat=>{
   if n!=2{return Err(Error::Invalid);}
   let filename=path(&argv[1])?;
   let (_,size)=fs.metadata(filename).map_err(|_|Error::Missing)?;
   if size>B{return Err(Error::Full);}
   let mut stage=[0u8;B];
   let n=fs.read(filename,0,&mut stage[..size]).map_err(|_|Error::Missing)?;
   *out=stage;Ok(n)
  }
  Builtin::Stat=>{
   if n!=2{return Err(Error::Invalid);}
   let filename=path(&argv[1])?;
   let (version,size)=fs.metadata(filename).map_err(|_|Error::Missing)?;
   let mut stage=[0u8;B];
   let mut cursor=0;
   for value in [size as u64,version]{
    if cursor>0{
     if cursor==B{return Err(Error::Full);}stage[cursor]=b' ';cursor+=1;
    }
    let mut digits=[0u8;20];let mut len=0usize;let mut x=value;
    loop{digits[len]=b'0'+(x%10) as u8;len+=1;x/=10;if x==0{break;}}
    if cursor+len>B{return Err(Error::Full);}
    for i in 0..len{stage[cursor+i]=digits[len-i-1];}cursor+=len;
   }
   *out=stage;Ok(cursor)
  }
  Builtin::Files=>{
   if n>2{return Err(Error::Invalid);}
   let prefix=if n==1{b"/".as_slice()}else{path(&argv[1])?};
   let mut stage=[0u8;B];let mut cursor=0;
   for i in 0..fs.count(){
    let mut name=[0u8;PATH_MAX];
    let (len,_,_)=fs.entry(i,&mut name).map_err(|_|Error::Invalid)?
     .ok_or(Error::Missing)?;
    if !name[..len].starts_with(prefix){continue;}
    if cursor+len+1>B{return Err(Error::Full);}
    stage[cursor..cursor+len].copy_from_slice(&name[..len]);
    cursor+=len;stage[cursor]=b'\n';cursor+=1;
   }
   *out=stage;Ok(cursor)
  }
  Builtin::Unknown=>Err(Error::Denied)
 }
}
pub fn available(cmd:&[u8])->bool{
 let mut args=[EMPTY;3];
 lex(cmd,&mut args).ok().is_some_and(|n|n>0&&builtin(&args[0])!=Builtin::Unknown)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn live_files_shell_lists_reads_and_stats_source_files(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/src/a",b"hello").unwrap();
  fs.replace(b"/src/b",b"world").unwrap();
  let mut out=[0u8;128];
  assert_eq!(run(&fs,b"cat /src/a",&mut out),Ok(5));
  assert_eq!(&out[..5],b"hello");
  let n=run(&fs,b"ls /src",&mut out).unwrap();
  assert_eq!(&out[..n],b"/src/a\n/src/b\n");
  let n=run(&fs,b"stat /src/a",&mut out).unwrap();
  assert_eq!(&out[..n],b"5 1");
 }
 #[test]fn missing_file_and_unapproved_shell_executable_not_run(){
  let fs=RamFs::<1>::new();let mut out=[0u8;64];
  assert_eq!(run(&fs,b"cat /missing",&mut out),Err(Error::Missing));
  assert_eq!(run(&fs,b"curl example.com",&mut out),Err(Error::Denied));
  assert!(!available(b"curl example.com"));
 }
 #[test]fn insufficient_cat_output_does_not_publish_partial_contents(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"secret").unwrap();
  let mut out=[44u8;3];
  assert_eq!(run(&fs,b"cat /a",&mut out),Err(Error::Full));
  assert_eq!(out,[44;3]);
 }
}

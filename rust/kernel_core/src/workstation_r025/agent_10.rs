//! R025/10: typed read-only terminal cat, stat, wc, grep and head.
//! Operates on live RAM filesystem bytes; cannot invoke external executables.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Command{Cat,Stat,WordCount,Find,Head}
fn number(mut value:usize,out:&mut[u8])->Result<usize,Error>{
 let mut reverse=[0u8;20];let mut len=0;
 loop{reverse[len]=b'0'+(value%10) as u8;len+=1;value/=10;if value==0{break;}}
 if len>out.len(){return Err(Error::Full);}
 for i in 0..len{out[i]=reverse[len-i-1];}Ok(len)
}
pub fn run<const F:usize>(fs:&RamFs<F>,path:&[u8],
 command:Command,param:&[u8],out:&mut[u8])->Result<usize,Error>{
 let (_,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
 let mut input=[0u8;CONTENT_MAX];
 fs.read(path,0,&mut input[..size]).map_err(|_|Error::Missing)?;
 let mut stage=[0u8;CONTENT_MAX];let mut count=0;
 match command{
  Command::Cat=>{stage[..size].copy_from_slice(&input[..size]);count=size;}
  Command::Stat=>{count=number(size,&mut stage)?;}
  Command::WordCount=>{
   let lines=input[..size].iter().filter(|&&b|b==b'\n').count()+
    usize::from(size>0&&input[size-1]!=b'\n');
   count=number(lines,&mut stage)?;
  }
  Command::Find=>{
   if param.is_empty()||param.len()>64{return Err(Error::Invalid);}
   for line in input[..size].split_inclusive(|b|*b==b'\n'){
    if line.windows(param.len()).any(|w|w==param){
     stage[count..count+line.len()].copy_from_slice(line);count+=line.len();
    }
   }
  }
  Command::Head=>{
   if param.len()!=1||!param[0].is_ascii_digit(){return Err(Error::Invalid);}
   let limit=(param[0]-b'0') as usize;let mut lines=0;
   for line in input[..size].split_inclusive(|b|*b==b'\n'){
    if lines>=limit{break;}
    stage[count..count+line.len()].copy_from_slice(line);
    count+=line.len();lines+=1;
   }
  }
 }
 if count>out.len(){return Err(Error::Full);}
 out[..count].copy_from_slice(&stage[..count]);Ok(count)
}
pub fn size<const F:usize>(fs:&RamFs<F>,path:&[u8])->Result<usize,Error>{
 fs.metadata(path).map(|(_,n)|n).map_err(|_|Error::Missing)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn terminal_filters_actual_source_file_and_counts_lines(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/log",b"ok\nerror\nok\n").unwrap();
  let mut out=[0u8;40];
  let n=run(&fs,b"/log",Command::Find,b"error",&mut out).unwrap();
  assert_eq!(&out[..n],b"error\n");
  let n=run(&fs,b"/log",Command::WordCount,b"",&mut out).unwrap();
  assert_eq!(&out[..n],b"3");
 }
 #[test]fn nonexistent_project_file_fails_closed(){
  let fs=RamFs::<1>::new();
  assert_eq!(run(&fs,b"/missing",Command::Cat,b"",&mut [0;3]),Err(Error::Missing));
 }
 #[test]fn short_terminal_output_never_leaks_partial_contents(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"long").unwrap();
  let mut out=[77;1];
  assert_eq!(run(&fs,b"/a",Command::Cat,b"",&mut out),Err(Error::Full));
  assert_eq!(out,[77]);
 }
}

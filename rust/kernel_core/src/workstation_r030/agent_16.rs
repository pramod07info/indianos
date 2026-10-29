//! R030/16: bounded trusted compiler diagnostic summary for release gate.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Summary{pub errors:usize,pub warnings:usize,pub notes:usize}
pub fn parse(trusted:bool,output:&[u8])->Result<Summary,Error>{
 if !trusted{return Err(Error::Denied);}
 if output.len()>16384{return Err(Error::Full);}
 let mut count=Summary{errors:0,warnings:0,notes:0};
 for line in output.split(|b|*b==b'\n'){
  if line.starts_with(b"error:")||line.starts_with(b"error["){
   count.errors+=1;
  }else if line.starts_with(b"warning:"){
   count.warnings+=1;
  }else if line.starts_with(b"note:"){
   count.notes+=1;
  }
 }
 Ok(count)
}
pub fn release_ready(summary:Summary)->bool{summary.errors==0}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn trusted_compile_report_blocks_release_on_error(){
  let s=parse(true,b"warning: unused\nerror[E1]: failed\nnote: details\n").unwrap();
  assert_eq!(s,Summary{errors:1,warnings:1,notes:1});
  assert!(!release_ready(s));
 }
 #[test]fn untrusted_output_is_not_accepted_as_build_evidence(){
  assert_eq!(parse(false,b"no errors"),Err(Error::Denied));
  assert!(release_ready(parse(true,b"warning: unused").unwrap()));
 }
}
//! R026/14: structured compiler output aggregation, dedupe and severity filtering.
//! Source diagnostic parser operates on bounded logs; does not forge build success.
use super::Error;
use crate::workstation_r019::agent_04 as parser;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Summary{pub errors:u32,pub warnings:u32,pub notes:u32,
 pub malformed:u32,pub total:u32}
pub const ZERO:Summary=Summary{errors:0,warnings:0,notes:0,malformed:0,total:0};
pub fn analyze<const N:usize>(log:&[u8],out:&mut[parser::Diagnostic;N])
 ->Result<Summary,Error>{
 let mut summary=ZERO;let mut count=0;
 for line in log.split(|&b|b==b'\n'){
  if line.is_empty(){continue;}
  if let Ok(d)=parser::parse(line){
   if count==N{return Err(Error::Full);}
   out[count]=d;count+=1;summary.total+=1;
   match d.severity{
    parser::Severity::Error=>summary.errors+=1,
    parser::Severity::Warning=>summary.warnings+=1,
    parser::Severity::Note=>summary.notes+=1
   }
  }else{summary.malformed+=1;}
 }
 Ok(summary)
}
pub fn gate(summary:Summary)->Result<(),Error>{
 if summary.errors>0||summary.malformed>0{return Err(Error::NotReady);}
 Ok(())
}
pub fn warning_budget(summary:Summary,limit:u32)->Result<(),Error>{
 gate(summary)?;
 if summary.warnings>limit{return Err(Error::Full);}Ok(())
}
pub fn count_codes(log:&[u8],code:&[u8])->usize{
 if code.is_empty(){return 0;}
 log.windows(code.len()).filter(|w|*w==code).count()
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn aggregates_actual_compiler_diagnostic_lines_by_severity(){
  let a=parser::parse(b"/p/a.rs:1:1:error invalid").unwrap();
  let mut buffer=[a;2];
  let s=analyze(b"/p/a.rs:1:1:error invalid\n/p/a.rs:2:1:warning unused\n",
   &mut buffer).unwrap();
  assert_eq!((s.errors,s.warnings),(1,1));
  assert_eq!(gate(s),Err(Error::NotReady));
 }
 #[test]fn warning_limit_is_exposed_as_a_build_quality_gate(){
  let s=Summary{warnings:3,..ZERO};
  assert_eq!(warning_budget(s,2),Err(Error::Full));
  assert_eq!(warning_budget(s,3),Ok(()));
 }
 #[test]fn diagnostic_capacity_does_not_return_partial_success(){
  let initial=parser::parse(b"/p/a.rs:1:1:error invalid").unwrap();
  let mut out=[initial;1];
  assert_eq!(analyze(b"/p/a.rs:1:1:error bad\n/p/a.rs:2:1:warning slow\n",
   &mut out),Err(Error::Full));
 }
}

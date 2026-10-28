//! R022/13: compiler diagnostic triage by source path/severity/job.
//! Acts on validated output of R019 parser; never opens arbitrary file paths.
use super::Error;
use crate::workstation_r019::agent_04::{Diagnostic,Severity};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Summary{pub owner:u32,pub job:u32,pub file_count:u16,
 pub errors:u16,pub warnings:u16,pub notes:u16}
pub struct Inbox<const N:usize>{owner:u32,job:u32,events:[Option<Diagnostic>;N]}
impl<const N:usize> Inbox<N>{
 pub const fn new(owner:u32,job:u32)->Self{
  Self{owner,job,events:[None;N]}
 }
 fn auth(&self,caller:u32)->Result<(),Error>{
  if caller==0||caller!=self.owner||self.job==0{return Err(Error::Denied);}
  Ok(())
 }
 pub fn insert(&mut self,trusted:bool,caller:u32,
  diagnostic:Diagnostic)->Result<(),Error>{
  self.auth(caller)?;
  if !trusted||diagnostic.path_len==0||diagnostic.line==0{
   return Err(Error::Denied);
  }
  if self.events.iter().flatten().any(|x|*x==diagnostic){
   return Err(Error::Conflict);
  }
  let at=self.events.iter().position(Option::is_none).ok_or(Error::Full)?;
  self.events[at]=Some(diagnostic);Ok(())
 }
 pub fn parse_and_insert(&mut self,trusted:bool,caller:u32,
  text:&[u8])->Result<(),Error>{
  self.auth(caller)?;
  if !trusted{return Err(Error::Denied);}
  let diagnostic=crate::workstation_r019::agent_04::parse(text)
   .map_err(|_|Error::Invalid)?;
  self.insert(true,caller,diagnostic)
 }
 pub fn summary(&self,caller:u32)->Result<Summary,Error>{
  self.auth(caller)?;
  let mut result=Summary{owner:caller,job:self.job,file_count:0,
   errors:0,warnings:0,notes:0};
  for (i,d) in self.events.iter().flatten().enumerate(){
   let mut unique=true;
   for older in self.events.iter().flatten().take(i){
    if older.path_len==d.path_len&&older.path[..d.path_len]==d.path[..d.path_len]{
     unique=false;break;
    }
   }
   if unique{result.file_count=result.file_count.checked_add(1)
    .ok_or(Error::Overflow)?;}
   let count=match d.severity{
    Severity::Error=>&mut result.errors,
    Severity::Warning=>&mut result.warnings,
    Severity::Note=>&mut result.notes
   };
   *count=count.checked_add(1).ok_or(Error::Overflow)?;
  }
  Ok(result)
 }
 pub fn clear(&mut self,caller:u32)->Result<(),Error>{
  self.auth(caller)?;self.events=[None;N];Ok(())
 }
 pub fn count(&self,caller:u32)->Result<usize,Error>{
  self.auth(caller)?;Ok(self.events.iter().flatten().count())
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn source_error_warning_summary_groups_real_file_locations(){
  let mut i=Inbox::<3>::new(7,1);
  i.parse_and_insert(true,7,b"/src/a:1:1:error bad").unwrap();
  i.parse_and_insert(true,7,b"/src/a:2:1:warning unused").unwrap();
  i.parse_and_insert(true,7,b"/src/b:1:1:note info").unwrap();
  let s=i.summary(7).unwrap();
  assert_eq!((s.file_count,s.errors,s.warnings,s.notes),(2,1,1,1));
 }
 #[test]fn forged_compiler_event_and_duplicate_failure_cannot_be_inserted(){
  let mut i=Inbox::<1>::new(7,1);
  assert_eq!(i.parse_and_insert(false,7,b"/a:1:1:error bad"),
   Err(Error::Denied));
  i.parse_and_insert(true,7,b"/a:1:1:error bad").unwrap();
  assert_eq!(i.parse_and_insert(true,7,b"/a:1:1:error bad"),
   Err(Error::Conflict));
 }
 #[test]fn user_b_cannot_read_user_a_diagnostic_source_paths(){
  let mut i=Inbox::<1>::new(7,1);
  i.parse_and_insert(true,7,b"/a:1:1:error bad").unwrap();
  assert_eq!(i.summary(8),Err(Error::Denied));assert_eq!(i.count(8),Err(Error::Denied));
 }
}

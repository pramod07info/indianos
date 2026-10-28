//! R019/12 bounded command palette with authenticated action filtering.
//! Commands are registered by trusted UI services, not arbitrary text eval.
use super::Error;
pub const LABEL:usize=48;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Action{pub id:u32,pub owner:u32,pub allowed:u32,
 pub label:[u8;LABEL],pub length:usize}
pub struct Palette<const N:usize>{entries:[Option<Action>;N]}
impl<const N:usize> Palette<N>{
 pub const fn new()->Self{Self{entries:[None;N]}}
 pub fn register(&mut self,id:u32,owner:u32,right:u32,label:&[u8])
  ->Result<(),Error>{
  if id==0||owner==0||right==0||label.is_empty()||label.len()>LABEL||
   label.iter().any(|c|!c.is_ascii_alphanumeric()&&
     !matches!(*c,b' '|b'-'|b'_'|b'.')){
   return Err(Error::Invalid);
  }
  if self.entries.iter().flatten().any(|a|a.id==id){return Err(Error::Conflict);}
  let slot=self.entries.iter_mut().find(|s|s.is_none()).ok_or(Error::Full)?;
  let mut name=[0;LABEL];name[..label.len()].copy_from_slice(label);
  *slot=Some(Action{id,owner,allowed:right,label:name,length:label.len()});Ok(())
 }
 pub fn search(&self,owner:u32,rights:u32,query:&[u8],
  output:&mut[Action])->Result<usize,Error>{
  if owner==0||query.is_empty()||query.len()>LABEL||!query.is_ascii(){
   return Err(Error::Invalid);
  }
  let mut count=0;
  for a in self.entries.iter().flatten(){
   if a.owner==owner&&a.allowed&rights==a.allowed&&
     a.label[..a.length].windows(query.len())
     .any(|w|w.eq_ignore_ascii_case(query)){count+=1;}
  }
  if count>output.len(){return Err(Error::Full);}
  let mut at=0;
  for a in self.entries.iter().flatten(){
   if a.owner==owner&&a.allowed&rights==a.allowed&&
    a.label[..a.length].windows(query.len())
    .any(|w|w.eq_ignore_ascii_case(query)){output[at]=*a;at+=1;}
  }Ok(at)
 }
 pub fn execute_allowed(&self,owner:u32,rights:u32,id:u32)->Result<u32,Error>{
  let action=self.entries.iter().flatten().find(|a|a.id==id)
   .ok_or(Error::Missing)?;
  if owner==0||action.owner!=owner||rights&action.allowed!=action.allowed{
   return Err(Error::Denied);
  }Ok(action.id)
 }
 pub fn revoke_owner(&mut self,owner:u32)->usize{
  let mut n=0;for e in &mut self.entries{
   if e.is_some_and(|a|a.owner==owner){*e=None;n+=1;}
  }n
 }
}
#[cfg(test)]mod tests{
 use super::*;
 const E:Action=Action{id:0,owner:0,allowed:0,label:[0;LABEL],length:0};
 #[test]fn only_available_editor_command_is_found(){
  let mut p=Palette::<3>::new();
  p.register(1,7,1,b"Open project").unwrap();
  p.register(2,7,2,b"Build project").unwrap();
  p.register(3,8,1,b"Open project").unwrap();
  let mut out=[E;2];
  assert_eq!(p.search(7,1,b"project",&mut out),Ok(1));
  assert_eq!(out[0].id,1);assert_eq!(p.execute_allowed(7,1,2),Err(Error::Denied));
 }
 #[test]fn short_output_cannot_publish_partial_command_list(){
  let mut p=Palette::<2>::new();
  p.register(1,7,1,b"Open").unwrap();p.register(2,7,1,b"Open tab").unwrap();
  let mut out=[E;1];assert_eq!(p.search(7,1,b"open",&mut out),Err(Error::Full));
  assert_eq!(out,[E]);
 }
 #[test]fn invalid_command_name_and_cross_owner_cannot_register_or_run(){
  let mut p=Palette::<1>::new();
  assert_eq!(p.register(1,7,1,b"run;rm"),Err(Error::Invalid));
  p.register(1,7,1,b"Open").unwrap();
  assert_eq!(p.execute_allowed(8,1,1),Err(Error::Denied));
 }
}

//! R019/07 process-owned source breakpoints and conditional hit counters.
//! These are debugger-side descriptors; CPU trap installation is separate.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Breakpoint{pub owner:u32,pub file:u32,pub line:u32,
 pub enabled:bool,pub hits:u32,pub stop_every:u32}
pub struct Breakpoints<const N:usize>{slots:[Option<Breakpoint>;N]}
impl<const N:usize> Breakpoints<N>{
 pub const fn new()->Self{Self{slots:[None;N]}}
 fn find(&self,owner:u32,file:u32,line:u32)->Option<usize>{
  self.slots.iter().position(|b|b.is_some_and(
    |b|b.owner==owner&&b.file==file&&b.line==line))
 }
 pub fn add(&mut self,owner:u32,file:u32,line:u32,
  every:u32)->Result<(),Error>{
  if owner==0||file==0||line==0||every==0{return Err(Error::Invalid);}
  if self.find(owner,file,line).is_some(){return Err(Error::Conflict);}
  let slot=self.slots.iter_mut().find(|s|s.is_none()).ok_or(Error::Full)?;
  *slot=Some(Breakpoint{owner,file,line,enabled:true,hits:0,stop_every:every});
  Ok(())
 }
 pub fn toggle(&mut self,owner:u32,file:u32,line:u32,on:bool)
  ->Result<(),Error>{
  let i=self.find(owner,file,line).ok_or(Error::Missing)?;
  let mut b=self.slots[i].ok_or(Error::Missing)?;
  b.enabled=on;self.slots[i]=Some(b);Ok(())
 }
 pub fn hit(&mut self,owner:u32,file:u32,line:u32)
  ->Result<bool,Error>{
  let i=self.find(owner,file,line).ok_or(Error::Missing)?;
  let mut b=self.slots[i].ok_or(Error::Missing)?;
  if !b.enabled{return Ok(false);}
  b.hits=b.hits.checked_add(1).ok_or(Error::Overflow)?;
  let stop=b.hits%b.stop_every==0;
  self.slots[i]=Some(b);Ok(stop)
 }
 pub fn clear_file(&mut self,owner:u32,file:u32)->usize{
  if owner==0||file==0{return 0;}
  let mut n=0;for b in &mut self.slots{
   if b.is_some_and(|b|b.owner==owner&&b.file==file){*b=None;n+=1;}
  }n
 }
 pub fn snapshot(&self,owner:u32,out:&mut[Breakpoint])->Result<usize,Error>{
  if owner==0{return Err(Error::Denied);}
  let count=self.slots.iter().flatten().filter(|b|b.owner==owner).count();
  if count>out.len(){return Err(Error::Full);}
  let mut i=0;for b in self.slots.iter().flatten(){
   if b.owner==owner{out[i]=*b;i+=1;}
  }Ok(i)
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn break_every_third_source_line_hit(){
  let mut bp=Breakpoints::<2>::new();bp.add(1,10,20,3).unwrap();
  assert_eq!(bp.hit(1,10,20),Ok(false));
  assert_eq!(bp.hit(1,10,20),Ok(false));
  assert_eq!(bp.hit(1,10,20),Ok(true));
  bp.toggle(1,10,20,false).unwrap();assert_eq!(bp.hit(1,10,20),Ok(false));
 }
 #[test]fn scoped_clear_preserves_other_developer(){
  let mut bp=Breakpoints::<2>::new();
  bp.add(1,10,20,1).unwrap();bp.add(2,10,20,1).unwrap();
  assert_eq!(bp.clear_file(1,10),1);
  assert_eq!(bp.hit(2,10,20),Ok(true));
 }
 #[test]fn short_snapshot_does_not_publish_partial_entries(){
  let mut bp=Breakpoints::<2>::new();bp.add(1,10,20,1).unwrap();bp.add(1,10,21,1).unwrap();
  let sentinel=Breakpoint{owner:7,file:7,line:7,enabled:false,hits:0,stop_every:1};
  let mut buf=[sentinel;1];
  assert_eq!(bp.snapshot(1,&mut buf),Err(Error::Full));assert_eq!(buf,[sentinel]);
 }
}

//! R019/14 authenticated multi-document tab layout and dirty flags.
//! Layout is metadata; actual compositor/ring-3 presentation is separate.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Tab{pub owner:u32,pub document:u32,pub split:u8,
 pub dirty:bool,pub pinned:bool,pub focused:bool}
pub struct Tabs<const N:usize>{slots:[Option<Tab>;N]}
impl<const N:usize> Tabs<N>{
 pub const fn new()->Self{Self{slots:[None;N]}}
 fn locate(&self,owner:u32,document:u32)->Option<usize>{
  self.slots.iter().position(|s|s.is_some_and(
    |t|t.owner==owner&&t.document==document))
 }
 pub fn open(&mut self,owner:u32,doc:u32,split:u8)->Result<(),Error>{
  if owner==0||doc==0||split>3{return Err(Error::Invalid);}
  if self.locate(owner,doc).is_some(){return Err(Error::Conflict);}
  let slot=self.slots.iter_mut().find(|x|x.is_none()).ok_or(Error::Full)?;
  *slot=Some(Tab{owner,document:doc,split,dirty:false,pinned:false,focused:false});
  Ok(())
 }
 pub fn focus(&mut self,owner:u32,doc:u32)->Result<(),Error>{
  if self.locate(owner,doc).is_none(){return Err(Error::Missing);}
  for item in &mut self.slots{
   if let Some(mut tab)=*item{
    if tab.owner==owner{tab.focused=tab.document==doc;*item=Some(tab);}
   }
  }Ok(())
 }
 pub fn mark_dirty(&mut self,owner:u32,doc:u32,dirty:bool)
  ->Result<(),Error>{
  let i=self.locate(owner,doc).ok_or(Error::Missing)?;
  let mut t=self.slots[i].ok_or(Error::Missing)?;
  t.dirty=dirty;self.slots[i]=Some(t);Ok(())
 }
 pub fn move_to_split(&mut self,owner:u32,doc:u32,split:u8)
  ->Result<(),Error>{
  if split>3{return Err(Error::Invalid);}
  let i=self.locate(owner,doc).ok_or(Error::Missing)?;
  let mut t=self.slots[i].ok_or(Error::Missing)?;
  t.split=split;self.slots[i]=Some(t);Ok(())
 }
 pub fn close(&mut self,owner:u32,doc:u32,force:bool)->Result<(),Error>{
  let i=self.locate(owner,doc).ok_or(Error::Missing)?;
  let tab=self.slots[i].ok_or(Error::Missing)?;
  if (tab.dirty||tab.pinned)&&!force{return Err(Error::Conflict);}
  self.slots[i]=None;Ok(())
 }
 pub fn snapshot(&self,owner:u32,out:&mut[Tab])->Result<usize,Error>{
  if owner==0{return Err(Error::Denied);}
  let n=self.slots.iter().flatten().filter(|x|x.owner==owner).count();
  if n>out.len(){return Err(Error::Full);}
  let mut i=0;for t in self.slots.iter().flatten(){
   if t.owner==owner{out[i]=*t;i+=1;}
  }Ok(i)
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn tab_focus_split_and_dirty_document_prevent_accidental_close(){
  let mut t=Tabs::<2>::new();t.open(1,10,0).unwrap();t.open(1,20,1).unwrap();
  t.focus(1,20).unwrap();t.mark_dirty(1,20,true).unwrap();
  assert_eq!(t.close(1,20,false),Err(Error::Conflict));
  t.move_to_split(1,20,2).unwrap();
  let mut out=[Tab{owner:0,document:0,split:0,dirty:false,pinned:false,focused:false};2];
  assert_eq!(t.snapshot(1,&mut out),Ok(2));assert_eq!(out[1].split,2);
  assert!(out[1].focused);t.close(1,20,true).unwrap();
 }
 #[test]fn other_owner_cannot_focus_or_close_document(){
  let mut t=Tabs::<1>::new();t.open(1,10,0).unwrap();
  assert_eq!(t.focus(2,10),Err(Error::Missing));
  assert_eq!(t.close(2,10,true),Err(Error::Missing));
 }
 #[test]fn capacity_and_invalid_split_are_atomic(){
  let mut t=Tabs::<1>::new();t.open(1,10,0).unwrap();
  assert_eq!(t.open(1,20,1),Err(Error::Full));
  assert_eq!(t.move_to_split(1,10,4),Err(Error::Invalid));
  let mut out=[Tab{owner:0,document:0,split:0,dirty:false,pinned:false,focused:false};1];
  t.snapshot(1,&mut out).unwrap();assert_eq!(out[0].split,0);
 }
}

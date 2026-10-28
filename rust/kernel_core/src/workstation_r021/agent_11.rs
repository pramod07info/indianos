//! R021/11: bounded debugger variable snapshots from an authenticated supervisor.
//! Values are typed byte descriptions, never arbitrary memory reads or expression eval.
use super::Error;
use crate::batch5_session::Identity;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum ValueKind{Signed,Unsigned,Bool,Text}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Value{pub session:u32,pub name:[u8;40],pub name_len:usize,
 pub owner:Identity,pub kind:ValueKind,pub data:[u8;80],pub len:usize,
 pub snapshot:u64}
pub struct Inspector<const N:usize>{owner:Identity,session:u32,values:[Option<Value>;N],
 epoch:u64}
impl<const N:usize> Inspector<N>{
 pub const fn new(owner:Identity,session:u32)->Self{
  Self{owner,session,values:[None;N],epoch:0}
 }
 pub fn begin(&mut self,supervisor:bool,epoch:u64)->Result<(),Error>{
  if !supervisor||self.owner.pid==0||self.session==0||epoch<=self.epoch{
   return Err(Error::Denied);
  }
  self.values=[None;N];self.epoch=epoch;Ok(())
 }
 pub fn record(&mut self,supervisor:bool,epoch:u64,
  name:&[u8],kind:ValueKind,data:&[u8])->Result<(),Error>{
  if !supervisor||epoch==0||epoch!=self.epoch{return Err(Error::Denied);}
  if name.is_empty()||name.len()>40||data.len()>80||
    !name.iter().all(|b|b.is_ascii_alphanumeric()||*b==b'_'){
   return Err(Error::Invalid);
  }
  if kind==ValueKind::Bool&&data!=b"true"&&data!=b"false"{
   return Err(Error::Invalid);
  }
  if kind==ValueKind::Text&&core::str::from_utf8(data).is_err(){
   return Err(Error::Invalid);
  }
  if self.values.iter().flatten().any(|v|&v.name[..v.name_len]==name){
   return Err(Error::Conflict);
  }
  let at=self.values.iter().position(Option::is_none).ok_or(Error::Full)?;
  let mut v=Value{session:self.session,name:[0;40],name_len:name.len(),
   owner:self.owner,kind,data:[0;80],len:data.len(),snapshot:epoch};
  v.name[..name.len()].copy_from_slice(name);
  v.data[..data.len()].copy_from_slice(data);
  self.values[at]=Some(v);Ok(())
 }
 pub fn read(&self,owner:Identity,name:&[u8])->Result<Value,Error>{
  if owner.pid==0||owner!=self.owner{return Err(Error::Denied);}
  self.values.iter().flatten().find(|v|&v.name[..v.name_len]==name)
   .copied().ok_or(Error::Missing)
 }
 pub fn snapshot(&self,owner:Identity,out:&mut[Value])->Result<usize,Error>{
  if owner.pid==0||owner!=self.owner{return Err(Error::Denied);}
  let n=self.values.iter().flatten().count();
  if n>out.len(){return Err(Error::Full);}
  let mut i=0;for v in self.values.iter().flatten(){out[i]=*v;i+=1;}
  Ok(i)
 }
 pub fn clear(&mut self,owner:Identity)->Result<(),Error>{
  if owner.pid==0||owner!=self.owner{return Err(Error::Denied);}
  self.values=[None;N];Ok(())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn variable_inspector_records_typed_snapshot_data(){
  let mut i=Inspector::<2>::new(id(1),10);
  i.begin(true,1).unwrap();
  i.record(true,1,b"answer",ValueKind::Signed,b"42").unwrap();
  let v=i.read(id(1),b"answer").unwrap();
  assert_eq!(&v.data[..v.len],b"42");
 }
 #[test]fn forged_debugger_and_recycled_pid_cannot_read_or_record(){
  let mut i=Inspector::<1>::new(id(1),10);
  assert_eq!(i.begin(false,1),Err(Error::Denied));i.begin(true,1).unwrap();
  assert_eq!(i.record(false,1,b"secret",ValueKind::Text,b"pw"),
    Err(Error::Denied));
  assert_eq!(i.read(id(2),b"secret"),Err(Error::Denied));
 }
 #[test]fn malformed_utf8_and_old_snapshot_are_rejected(){
  let mut i=Inspector::<1>::new(id(1),10);i.begin(true,1).unwrap();
  assert_eq!(i.record(true,1,b"a",ValueKind::Text,&[0xff]),
   Err(Error::Invalid));
  assert_eq!(i.record(true,0,b"a",ValueKind::Signed,b"1"),
   Err(Error::Denied));
 }
}

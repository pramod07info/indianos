//! R019/17 developer HTTP request/response metadata inspector.
//! All secrets and body content are deliberately excluded from captured data.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Method{Get,Post,Put,Patch,Delete}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Sample{pub owner:u32,pub request:u64,pub method:Method,
 pub status:u16,pub duration_ms:u32,pub request_bytes:u32,
 pub response_bytes:u32,pub tls_verified:bool}
pub struct Inspector<const N:usize>{data:[Option<Sample>;N],cursor:usize}
impl<const N:usize> Inspector<N>{
 pub const fn new()->Self{Self{data:[None;N],cursor:0}}
 pub fn record(&mut self,from_trusted_transport:bool,entry:Sample)
  ->Result<(),Error>{
  if !from_trusted_transport||entry.owner==0||entry.request==0||
   entry.status<100||entry.status>599{return Err(Error::Denied);}
  if N==0{return Err(Error::Full);}
  if self.data.iter().flatten().any(|x|x.owner==entry.owner&&
    x.request==entry.request){return Err(Error::Conflict);}
  self.data[self.cursor]=Some(entry);self.cursor=(self.cursor+1)%N;Ok(())
 }
 pub fn latest(&self,owner:u32)->Result<Sample,Error>{
  if owner==0{return Err(Error::Denied);}
  self.data.iter().flatten().filter(|s|s.owner==owner)
   .max_by_key(|s|s.request).copied().ok_or(Error::Missing)
 }
 pub fn snapshot(&self,owner:u32,out:&mut[Sample])->Result<usize,Error>{
  if owner==0{return Err(Error::Denied);}
  let count=self.data.iter().flatten().filter(|s|s.owner==owner).count();
  if count>out.len(){return Err(Error::Full);}
  let mut at=0;
  for s in self.data.iter().flatten(){
   if s.owner==owner{out[at]=*s;at+=1;}
  }Ok(at)
 }
 pub fn failures(&self,owner:u32)->usize{
  self.data.iter().flatten().filter(|s|s.owner==owner&&s.status>=400).count()
 }
 pub fn clear(&mut self,owner:u32)->usize{
  if owner==0{return 0;}
  let mut n=0;for item in &mut self.data{
   if item.is_some_and(|x|x.owner==owner){*item=None;n+=1;}
  }n
 }
}
#[cfg(test)]mod tests{
 use super::*;
 fn s(owner:u32,id:u64,status:u16)->Sample{
  Sample{owner,request:id,method:Method::Get,status,duration_ms:50,
   request_bytes:4,response_bytes:8,tls_verified:true}}
 #[test]fn error_rate_is_scoped_to_request_owner(){
  let mut i=Inspector::<3>::new();
  i.record(true,s(1,10,200)).unwrap();
  i.record(true,s(1,11,503)).unwrap();
  i.record(true,s(2,12,404)).unwrap();
  assert_eq!(i.failures(1),1);assert_eq!(i.latest(1),Ok(s(1,11,503)));
 }
 #[test]fn untrusted_transport_cannot_insert_a_forged_response(){
  let mut i=Inspector::<1>::new();
  assert_eq!(i.record(false,s(1,1,200)),Err(Error::Denied));
  assert_eq!(i.latest(1),Err(Error::Missing));
 }
 #[test]fn short_view_is_atomic_and_clear_is_owner_scoped(){
  let mut i=Inspector::<3>::new();
  i.record(true,s(1,1,200)).unwrap();i.record(true,s(1,2,404)).unwrap();
  i.record(true,s(2,3,200)).unwrap();
  let mut o=[s(9,9,200);1];
  assert_eq!(i.snapshot(1,&mut o),Err(Error::Full));
  assert_eq!(o,[s(9,9,200)]);
  assert_eq!(i.clear(1),2);assert_eq!(i.failures(2),0);
 }
}

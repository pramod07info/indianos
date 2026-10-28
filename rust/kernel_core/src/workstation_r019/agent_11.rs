//! R019/11 bounded owner-scoped command history and terminal scrollback.
//! No arbitrary command execution, secrets persistence or shell injection.
use super::Error;
pub const COMMAND:usize=160;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Record{pub owner:u32,pub seq:u64,pub status:u16,pub len:usize,
 pub text:[u8;COMMAND]}
pub struct History<const N:usize>{entries:[Option<Record>;N],next:usize,seq:u64}
impl<const N:usize> History<N>{
 pub const fn new()->Self{Self{entries:[None;N],next:0,seq:0}}
 pub fn push(&mut self,owner:u32,line:&[u8],status:u16)
  ->Result<u64,Error>{
  if owner==0||N==0||line.is_empty()||line.len()>COMMAND||
    line.iter().any(|b|!b.is_ascii_graphic()&&*b!=b' '){
   return Err(Error::Invalid);
  }
  let seq=self.seq.checked_add(1).ok_or(Error::Overflow)?;
  let mut text=[0;COMMAND];text[..line.len()].copy_from_slice(line);
  self.entries[self.next]=Some(Record{owner,seq,status,len:line.len(),text});
  self.next=(self.next+1)%N;self.seq=seq;Ok(seq)
 }
 pub fn recent(&self,owner:u32,output:&mut[Record])
  ->Result<usize,Error>{
  if owner==0{return Err(Error::Denied);}
  let size=self.entries.iter().flatten().filter(|r|r.owner==owner).count();
  if size>output.len(){return Err(Error::Full);}
  let start=if self.entries.iter().all(Option::is_some){self.next}else{0};
  let mut n=0;
  for k in 0..N{
   if let Some(r)=self.entries[(start+k)%N]{
    if r.owner==owner{output[n]=r;n+=1;}
   }
  }Ok(n)
 }
 pub fn latest(&self,owner:u32)->Result<Record,Error>{
  if owner==0{return Err(Error::Denied);}
  self.entries.iter().flatten().filter(|r|r.owner==owner)
   .max_by_key(|r|r.seq).copied().ok_or(Error::Missing)
 }
 pub fn clear_owner(&mut self,owner:u32)->usize{
  if owner==0{return 0;}
  let mut n=0;for e in &mut self.entries{
   if e.is_some_and(|r|r.owner==owner){*e=None;n+=1;}
  }n
 }
 pub fn count_failures(&self,owner:u32)->usize{
  self.entries.iter().flatten().filter(|r|r.owner==owner&&r.status!=0).count()
 }
}
#[cfg(test)]mod tests{
 use super::*;
 const EMPTY:Record=Record{owner:0,seq:0,status:0,len:0,text:[0;COMMAND]};
 #[test]fn command_history_tracks_order_and_exit_code(){
  let mut h=History::<3>::new();
  h.push(1,b"ls",0).unwrap();h.push(1,b"cat /src",1).unwrap();
  assert_eq!(h.count_failures(1),1);
  let mut out=[EMPTY;3];assert_eq!(h.recent(1,&mut out),Ok(2));
  assert_eq!(&out[1].text[..out[1].len],b"cat /src");
 }
 #[test]fn ring_overwrite_and_owner_isolation_are_bounded(){
  let mut h=History::<2>::new();
  h.push(1,b"old",0).unwrap();h.push(2,b"private",0).unwrap();
  h.push(1,b"new",0).unwrap();assert_eq!(h.latest(1).unwrap().seq,3);
  assert_eq!(h.clear_owner(2),1);
  assert_eq!(h.latest(2),Err(Error::Missing));
 }
 #[test]fn malformed_command_cannot_erase_history(){
  let mut h=History::<1>::new();h.push(1,b"safe",0).unwrap();
  assert_eq!(h.push(1,b"bad\n",1),Err(Error::Invalid));
  assert_eq!(&h.latest(1).unwrap().text[..4],b"safe");
 }
}

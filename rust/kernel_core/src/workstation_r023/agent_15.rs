//! R023/15: parses trusted Rust test output lines into run-level pass/fail registry.
//! Receives captured text from a trusted runner; no test executable invoked here.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Verdict{Pass,Fail,Ignore}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Test{pub name:[u8;96],pub len:usize,pub verdict:Verdict}
pub const EMPTY:Test=Test{name:[0;96],len:0,verdict:Verdict::Ignore};
pub struct Results<const N:usize>{owner:u32,run:u32,
 data:[Option<Test>;N],count:usize,finished:bool}
impl<const N:usize> Results<N>{
 pub const fn new(owner:u32,run:u32)->Self{
  Self{owner,run,data:[None;N],count:0,finished:false}
 }
 fn auth(&self,owner:u32)->Result<(),Error>{
  if owner==0||owner!=self.owner||self.run==0{return Err(Error::Denied);}Ok(())
 }
 pub fn ingest(&mut self,trusted:bool,owner:u32,line:&[u8])
  ->Result<Option<Verdict>,Error>{
  self.auth(owner)?;
  if !trusted||self.finished{return Err(Error::Denied);}
  if !line.starts_with(b"test "){return Ok(None);}
  let body=&line[5..];
  let Some(sep)=body.windows(5).position(|w|w==b" ... ")else{
   return Err(Error::Invalid);
  };
  let name=&body[..sep];if name.is_empty()||name.len()>96{return Err(Error::Invalid);}
  let result=&body[sep+5..];
  let v=match result{
   b"ok"=>Verdict::Pass,b"FAILED"=>Verdict::Fail,b"ignored"=>Verdict::Ignore,
   _=>return Err(Error::Invalid)
  };
  if self.data.iter().flatten().any(|t|&t.name[..t.len]==name){
   return Err(Error::Conflict);
  }
  if self.count==N{return Err(Error::Full);}
  let mut item=EMPTY;item.name[..name.len()].copy_from_slice(name);
  item.len=name.len();item.verdict=v;
  self.data[self.count]=Some(item);self.count+=1;Ok(Some(v))
 }
 pub fn finish(&mut self,trusted:bool,owner:u32)
  ->Result<(usize,usize,usize),Error>{
  self.auth(owner)?;if !trusted{return Err(Error::Denied);}
  if self.finished{return Err(Error::Conflict);}
  self.finished=true;Ok(self.totals(owner)?)
 }
 pub fn totals(&self,owner:u32)->Result<(usize,usize,usize),Error>{
  self.auth(owner)?;
  let mut out=(0,0,0);
  for t in self.data.iter().flatten(){
   match t.verdict{Verdict::Pass=>out.0+=1,Verdict::Fail=>out.1+=1,
    Verdict::Ignore=>out.2+=1}
  }Ok(out)
 }
 pub fn all_passed(&self,owner:u32)->Result<bool,Error>{
  self.auth(owner)?;
  Ok(self.finished&&self.count>0&&self.data.iter().flatten()
   .all(|t|t.verdict!=Verdict::Fail))
 }
 pub fn failures(&self,owner:u32,out:&mut[Test])->Result<usize,Error>{
  self.auth(owner)?;
  let n=self.data.iter().flatten().filter(|t|t.verdict==Verdict::Fail).count();
  if n>out.len(){return Err(Error::Full);}
  let mut i=0;for t in self.data.iter().flatten(){
   if t.verdict==Verdict::Fail{out[i]=*t;i+=1;}
  }Ok(i)
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn real_rust_test_output_reports_pass_and_fail_totals(){
  let mut r=Results::<3>::new(7,1);
  r.ingest(true,7,b"test app::works ... ok").unwrap();
  r.ingest(true,7,b"test app::broken ... FAILED").unwrap();
  assert_eq!(r.finish(true,7),Ok((1,1,0)));
  assert_eq!(r.all_passed(7),Ok(false));
 }
 #[test]fn forged_results_and_duplicate_names_cannot_trick_release(){
  let mut r=Results::<2>::new(7,1);
  assert_eq!(r.ingest(false,7,b"test a ... ok"),Err(Error::Denied));
  r.ingest(true,7,b"test a ... ok").unwrap();
  assert_eq!(r.ingest(true,7,b"test a ... ok"),Err(Error::Conflict));
 }
 #[test]fn short_failure_view_does_not_partially_publish_tests(){
  let mut r=Results::<2>::new(7,1);
  r.ingest(true,7,b"test a ... FAILED").unwrap();
  r.ingest(true,7,b"test b ... FAILED").unwrap();
  let mut out=[EMPTY;1];
  assert_eq!(r.failures(7,&mut out),Err(Error::Full));
  assert_eq!(out,[EMPTY]);
 }
}

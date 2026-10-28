//! R037/28: bounded crash-log buffer isolated by kernel-authenticated PID.
//! Records hold fault metadata only; never store user pointers or secrets.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Crash{pub sequence:u64,pub pid:u32,pub generation:u64,pub vector:u16,
    pub code:u32,pub uptime:u64}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum CrashError{Invalid,Exhausted}
pub struct CrashLog<const N:usize>{
    entries:[Option<Crash>;N],next:usize,count:usize,sequence:u64,
}
impl<const N:usize> CrashLog<N>{
    pub const fn new()->Self{Self{
        entries:[None;N],next:0,count:0,sequence:0,
    }}
    /// Caller is a trusted synchronous exception handler, not userspace.
    pub fn record(&mut self,pid:u32,generation:u64,vector:u16,code:u32,uptime:u64)
        ->Result<Crash,CrashError>{
        if pid==0||generation==0||N==0||vector>255{
            return Err(CrashError::Invalid);
        }
        let sequence=self.sequence.checked_add(1).ok_or(CrashError::Exhausted)?;
        let entry=Crash{sequence,pid,generation,vector,code,uptime};
        self.entries[self.next]=Some(entry);
        self.next=(self.next+1)%N;
        self.count=self.count.saturating_add(1).min(N);
        self.sequence=sequence;
        Ok(entry)
    }
    pub fn count(&self)->usize{self.count}
    /// Oldest-first, stable copy; no untrusted pointers retained.
    pub fn get(&self,index:usize)->Option<Crash>{
        if index>=self.count{return None;}
        let start=if self.count==N{self.next}else{0};
        self.entries[(start+index)%N]
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn ring_overwrites_oldest_but_preserves_order(){
        let mut log=CrashLog::<2>::new();
        log.record(1,1,6,1,100).unwrap();
        log.record(2,1,14,2,200).unwrap();
        log.record(3,1,13,3,300).unwrap();
        assert_eq!(log.count(),2);
        assert_eq!(log.get(0).unwrap().pid,2);
        assert_eq!(log.get(1).unwrap().pid,3);
        assert_eq!(log.get(2),None);
    }
    #[test]fn empty_log_and_invalid_fault_rejected(){
        let mut log=CrashLog::<0>::new();
        assert_eq!(log.record(1,1,1,1,1),Err(CrashError::Invalid));
        let mut valid=CrashLog::<2>::new();
        assert_eq!(valid.record(0,1,1,1,1),Err(CrashError::Invalid));
        assert_eq!(valid.count(),0);
    }
    #[test]fn exhaustion_never_reuses_sequence(){
        let mut log=CrashLog::<1>::new();log.sequence=u64::MAX;
        assert_eq!(log.record(1,1,1,1,1),Err(CrashError::Exhausted));
    }
}


/// R017 developer diagnostics: bounded crash views for exact PID generations.
/// The caller is a trusted supervisor; no application receives another
/// process's fault metadata without a separate authorization gate.
impl<const N:usize> CrashLog<N>{
 pub fn count_owner(&self,pid:u32,generation:u64)->usize{
  if pid==0||generation==0{return 0;}
  (0..self.count).filter(|&i|self.get(i).is_some_and(
    |c|c.pid==pid&&c.generation==generation)).count()
 }
 pub fn inspect_owner(&self,pid:u32,generation:u64,
  out:&mut[Crash])->Result<usize,CrashError>{
  if pid==0||generation==0{return Err(CrashError::Invalid);}
  let count=self.count_owner(pid,generation);
  if count>out.len(){return Err(CrashError::Exhausted);}
  let mut next=0;
  for i in 0..self.count{
   if let Some(item)=self.get(i){
    if item.pid==pid&&item.generation==generation{
     out[next]=item;next+=1;
    }
   }
  }
  Ok(next)
 }
 pub fn latest_owner(&self,pid:u32,generation:u64)->Option<Crash>{
  (0..self.count).rev().filter_map(|i|self.get(i))
   .find(|c|c.pid==pid&&c.generation==generation)
 }
}
#[cfg(test)]mod r017_crash_diagnostics_tests{
 use super::*;
 #[test]fn two_faults_belonging_to_one_process_are_ordered(){
  let mut log=CrashLog::<4>::new();
  log.record(1,1,6,0,10).unwrap();
  log.record(2,1,14,0,11).unwrap();
  log.record(1,1,13,0,12).unwrap();
  let empty=Crash{sequence:0,pid:0,generation:0,vector:0,code:0,uptime:0};
  let mut out=[empty;2];
  assert_eq!(log.inspect_owner(1,1,&mut out),Ok(2));
  assert_eq!(out[0].uptime,10);assert_eq!(out[1].uptime,12);
 }
 #[test]fn stale_generation_cannot_observe_current_faults(){
  let mut log=CrashLog::<2>::new();log.record(1,2,6,0,4).unwrap();
  assert_eq!(log.count_owner(1,1),0);
  assert_eq!(log.latest_owner(1,1),None);
 }
 #[test]fn too_short_output_is_never_partially_populated(){
  let mut log=CrashLog::<2>::new();
  log.record(1,1,6,0,1).unwrap();log.record(1,1,6,0,2).unwrap();
  let empty=Crash{sequence:0,pid:0,generation:0,vector:0,code:0,uptime:0};
  let mut out=[empty;1];
  assert_eq!(log.inspect_owner(1,1,&mut out),Err(CrashError::Exhausted));
  assert_eq!(out,[empty]);
 }
}

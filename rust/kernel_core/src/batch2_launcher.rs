//! R036/12: authenticated process-launch intent queue, not an ELF loader.
//! Only trusted kernel code can insert a verified image descriptor.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Image { pub image_id:u32, pub entry:u64, pub mapped_end:u64, pub verified:bool }
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Launch { pub pid:u32, pub generation:u64, pub image:Image }
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum LaunchError { Invalid, Unverified, Duplicate, Full, Unauthorized, Empty }
pub struct LaunchQueue<const N:usize> { entries:[Option<Launch>;N] }
impl<const N:usize> LaunchQueue<N> {
    pub const fn new()->Self {Self{entries:[None;N]}}
    pub fn stage(&mut self, intent:Launch)->Result<(),LaunchError>{
        if intent.pid==0 || intent.generation==0 || intent.image.image_id==0 ||
            intent.image.entry==0 || intent.image.entry>=intent.image.mapped_end {
            return Err(LaunchError::Invalid);
        }
        if !intent.image.verified{return Err(LaunchError::Unverified);}
        if self.entries.iter().flatten().any(|existing|existing.pid==intent.pid){
            return Err(LaunchError::Duplicate);
        }
        let slot=self.entries.iter_mut().find(|x|x.is_none()).ok_or(LaunchError::Full)?;
        *slot=Some(intent);
        Ok(())
    }
    /// Must be called with authenticated scheduler PID+generation.
    /// Orbit L1: atomic multi-app startup preflight: if any image is
    /// unverified, stale or capacity-exceeding, no launch request publishes.
    pub fn stage_batch(&mut self,requests:&[Launch])->Result<(),LaunchError>{
        if requests.is_empty()||requests.len()>N{return Err(LaunchError::Full);}
        let mut staged=Self{entries:self.entries};
        for intent in requests{staged.stage(*intent)?;}
        self.entries=staged.entries;Ok(())
    }
    /// Orbit L2: inspect only the exact authenticated process generation.
    /// This is an *intention*, not proof a binary was already executed.
    pub fn inspect(&self,pid:u32,generation:u64)->Result<Launch,LaunchError>{
        let request=self.entries.iter().flatten().find(|p|p.pid==pid&&pid!=0)
            .ok_or(LaunchError::Empty)?;
        if generation==0||request.generation!=generation{
            return Err(LaunchError::Unauthorized);
        }
        Ok(*request)
    }
    /// Orbit L3: revoke a specific incarnation without touching a recycled
    /// PID or another process's queued launch.
    pub fn cancel(&mut self,pid:u32,generation:u64)->Result<(),LaunchError>{
        if self.retire(pid,generation){Ok(())}else{Err(LaunchError::Unauthorized)}
    }
    /// Orbit L4: bounded observable status for privileged launcher only.
    pub fn pending(&self)->usize{self.entries.iter().filter(|p|p.is_some()).count()}
    pub fn available(&self)->usize{N-self.pending()}
    /// Consume an entire authenticated group only after every requested
    /// process and caller's destination have passed validation.
    pub fn take_batch(&mut self,requests:&[(u32,u64)],out:&mut[Launch])
        ->Result<usize,LaunchError>{
        if requests.is_empty(){return Err(LaunchError::Invalid);}
        if out.len()<requests.len(){return Err(LaunchError::Full);}
        if requests.len()>N{return Err(LaunchError::Full);}
        let mut staged=Self{entries:self.entries};
        let mut found=[None;N];
        for (i,&(pid,generation)) in requests.iter().enumerate(){
            if requests[..i].contains(&(pid,generation)){
                return Err(LaunchError::Duplicate);
            }
            found[i]=Some(staged.take(pid,generation)?);
        }
        for (i,slot) in found[..requests.len()].iter().enumerate(){
            out[i]=slot.ok_or(LaunchError::Empty)?;
        }
        self.entries=staged.entries;
        Ok(requests.len())
    }
    /// Cancel a cohesive launch group, never partially. Stale identities
    /// cannot abort another process incarnation.
    pub fn cancel_batch(&mut self,requests:&[(u32,u64)])
        ->Result<usize,LaunchError>{
        if requests.is_empty()||requests.len()>N{
            return Err(LaunchError::Invalid);
        }
        let mut staged=Self{entries:self.entries};
        for (i,&(pid,generation)) in requests.iter().enumerate(){
            if requests[..i].contains(&(pid,generation)){
                return Err(LaunchError::Duplicate);
            }
            staged.cancel(pid,generation)?;
        }
        self.entries=staged.entries;Ok(requests.len())
    }
    /// Preview trusted images scheduled to start; output buffer must be
    /// large enough for every item or no byte will be written.
    pub fn list(&self,out:&mut[Launch])->Result<usize,LaunchError>{
        let count=self.pending();
        if out.len()<count{return Err(LaunchError::Full);}
        let mut i=0;
        for launch in self.entries.iter().flatten(){
            out[i]=*launch;i+=1;
        }
        Ok(count)
    }
    /// Quarantine a compromised image ID from the queue, not from disks.
    pub fn cancel_image(&mut self,image_id:u32)->usize{
        if image_id==0{return 0;}
        let mut removed=0;
        for entry in &mut self.entries{
            if entry.is_some_and(|x|x.image.image_id==image_id){
                *entry=None;removed+=1;
            }
        }
        removed
    }
    pub fn pending_image(&self,image_id:u32)->usize{
        if image_id==0{return 0;}
        self.entries.iter().flatten().filter(|x|x.image.image_id==image_id)
            .count()
    }
    pub fn take(&mut self,pid:u32,generation:u64)->Result<Launch,LaunchError>{
        let slot=self.entries.iter_mut().find(|x|
            x.is_some_and(|request|request.pid==pid)).ok_or(LaunchError::Empty)?;
        let launch=slot.ok_or(LaunchError::Empty)?;
        if launch.generation!=generation{return Err(LaunchError::Unauthorized);}
        *slot=None;
        Ok(launch)
    }
    pub fn retire(&mut self,pid:u32,generation:u64)->bool{
        let slot=self.entries.iter_mut().find(|x|
            x.is_some_and(|request|request.pid==pid&&request.generation==generation));
        if let Some(item)=slot{*item=None;true}else{false}
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    fn intent()->Launch{Launch{pid:4,generation:2,image:Image{
        image_id:5,entry:0x1000,mapped_end:0x2000,verified:true}}}
    #[test]fn orbit_launcher_batch_consumes_all_or_nothing(){
        let mut q=LaunchQueue::<3>::new();
        let first=intent();let mut second=intent();second.pid=6;
        q.stage_batch(&[first,second]).unwrap();
        let mut out=[first;2];
        assert_eq!(q.take_batch(&[(4,2),(6,3)],&mut out),
                   Err(LaunchError::Unauthorized));
        assert_eq!(q.pending(),2);
        assert_eq!(q.take_batch(&[(4,2),(6,2)],&mut out),Ok(2));
        assert_eq!(out,[first,second]);assert_eq!(q.pending(),0);
    }
    #[test]fn orbit_launcher_cancel_batch_and_inventory_are_atomic(){
        let mut q=LaunchQueue::<3>::new();
        let first=intent();let mut second=intent();second.pid=6;
        q.stage_batch(&[first,second]).unwrap();
        let mut short=[first;1];
        assert_eq!(q.list(&mut short),Err(LaunchError::Full));
        assert_eq!(short,[first]);
        assert_eq!(q.cancel_batch(&[(4,2),(6,1)]),
                   Err(LaunchError::Unauthorized));
        assert_eq!(q.pending(),2);
        assert_eq!(q.cancel_batch(&[(4,2),(6,2)]),Ok(2));
        assert_eq!(q.pending(),0);
    }
    #[test]fn orbit_launcher_image_quarantine_is_exact(){
        let mut q=LaunchQueue::<3>::new();let first=intent();
        let mut second=intent();second.pid=6;second.image.image_id=7;
        q.stage_batch(&[first,second]).unwrap();
        assert_eq!(q.pending_image(first.image.image_id),1);
        assert_eq!(q.cancel_image(7),1);
        assert_eq!(q.inspect(4,2),Ok(first));
        assert_eq!(q.cancel_image(0),0);
    }
    #[test]fn orbit_batch_launch_is_transactional_and_enforces_identity(){
        let mut q=LaunchQueue::<3>::new();
        let first=intent();
        let mut second=intent();second.pid=5;
        let mut third=intent();third.pid=6;third.image.verified=false;
        assert_eq!(q.stage_batch(&[first,second,third]),Err(LaunchError::Unverified));
        assert_eq!(q.pending(),0);assert_eq!(q.available(),3);
        q.stage_batch(&[first,second]).unwrap();
        assert_eq!(q.pending(),2);
        assert_eq!(q.inspect(4,3),Err(LaunchError::Unauthorized));
        assert_eq!(q.inspect(4,2),Ok(first));
        assert_eq!(q.cancel(4,3),Err(LaunchError::Unauthorized));
        assert_eq!(q.pending(),2);
        q.cancel(4,2).unwrap();
        assert_eq!(q.pending(),1);
        assert_eq!(q.inspect(5,2),Ok(second));
    }
    #[test]fn orbit_batch_duplicates_and_over_capacity_do_not_publish(){
        let mut q=LaunchQueue::<2>::new();let item=intent();
        assert_eq!(q.stage_batch(&[item,item]),Err(LaunchError::Duplicate));
        assert_eq!(q.pending(),0);
        let mut second=item;second.pid=5;
        let mut third=item;third.pid=6;
        assert_eq!(q.stage_batch(&[item,second,third]),Err(LaunchError::Full));
        assert_eq!(q.pending(),0);
    }
    #[test] fn staged_verified_launch_has_one_consumer(){
        let mut q=LaunchQueue::<1>::new();
        q.stage(intent()).unwrap();
        assert_eq!(q.stage(intent()),Err(LaunchError::Duplicate));
        assert_eq!(q.take(4,1),Err(LaunchError::Unauthorized));
        assert_eq!(q.take(4,2),Ok(intent()));
        assert_eq!(q.take(4,2),Err(LaunchError::Empty));
    }
    #[test] fn unsigned_and_invalid_entries_never_run(){
        let mut q=LaunchQueue::<1>::new();
        let mut a=intent();a.image.verified=false;
        assert_eq!(q.stage(a),Err(LaunchError::Unverified));
        a=intent();a.image.entry=a.image.mapped_end;
        assert_eq!(q.stage(a),Err(LaunchError::Invalid));
    }
    #[test] fn retire_rejects_old_generation(){
        let mut q=LaunchQueue::<1>::new();q.stage(intent()).unwrap();
        assert!(!q.retire(4,1));
        assert!(q.retire(4,2));
        assert_eq!(q.take(4,2),Err(LaunchError::Empty));
    }
}

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

//! R037/23: NVMe namespace I/O plan and command-id lifecycle.
//! This is host-testable queue accounting only, not PCIe NVMe initialization.
pub const MAX_QUEUE_DEPTH:usize=64;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Request{pub command_id:u16,pub namespace_id:u32,pub lba:u64,
    pub blocks:u16,pub dma_address:u64}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum NvmeError{Invalid,OutOfBounds,Duplicate,Full,NotFound,Stale}
pub struct Queue<const N:usize>{active:[Option<Request>;N],epoch:u64}
impl<const N:usize> Queue<N>{
    pub const fn new(epoch:u64)->Self{Self{active:[None;N],epoch}}
    pub fn submit(&mut self,epoch:u64,req:Request,namespace_blocks:u64)
        ->Result<(),NvmeError>{
        if epoch==0||epoch!=self.epoch{return Err(NvmeError::Stale);}
        if N>MAX_QUEUE_DEPTH||req.namespace_id==0||req.blocks==0||
            req.dma_address==0||req.dma_address%4096!=0{return Err(NvmeError::Invalid);}
        if req.lba>=namespace_blocks||
            u64::from(req.blocks)>namespace_blocks-req.lba{
            return Err(NvmeError::OutOfBounds);
        }
        if self.active.iter().flatten().any(|r|r.command_id==req.command_id){
            return Err(NvmeError::Duplicate);
        }
        let free=self.active.iter_mut().find(|s|s.is_none()).ok_or(NvmeError::Full)?;
        *free=Some(req);Ok(())
    }
    pub fn complete(&mut self,epoch:u64,id:u16)->Result<Request,NvmeError>{
        if epoch==0||epoch!=self.epoch{return Err(NvmeError::Stale);}
        let slot=self.active.iter_mut().find(|e|
            e.is_some_and(|request|request.command_id==id))
            .ok_or(NvmeError::NotFound)?;
        slot.take().ok_or(NvmeError::NotFound)
    }
    /// Changing a controller epoch invalidates every old command ID.
    pub fn reset(&mut self,new_epoch:u64)->Result<(),NvmeError>{
        if new_epoch==0||new_epoch<=self.epoch{return Err(NvmeError::Stale);}
        self.active=[None;N];self.epoch=new_epoch;Ok(())
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    fn req()->Request{Request{command_id:4,namespace_id:1,lba:10,blocks:8,
        dma_address:0x2000}}
    #[test]fn submit_complete_and_duplicate_guards(){
        let mut q=Queue::<2>::new(1);
        q.submit(1,req(),100).unwrap();
        assert_eq!(q.submit(1,req(),100),Err(NvmeError::Duplicate));
        assert_eq!(q.complete(1,4),Ok(req()));
        assert_eq!(q.complete(1,4),Err(NvmeError::NotFound));
    }
    #[test]fn controller_reset_invalidates_old_completions(){
        let mut q=Queue::<1>::new(5);
        q.submit(5,req(),100).unwrap();
        q.reset(6).unwrap();
        assert_eq!(q.complete(5,4),Err(NvmeError::Stale));
        assert_eq!(q.complete(6,4),Err(NvmeError::NotFound));
    }
    #[test]fn invalid_namespace_and_lba_fail_closed(){
        let mut q=Queue::<1>::new(2);
        assert_eq!(q.submit(2,req(),15),Err(NvmeError::OutOfBounds));
        let mut r=req();r.namespace_id=0;
        assert_eq!(q.submit(2,r,100),Err(NvmeError::Invalid));
    }
}

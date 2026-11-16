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
            req.dma_address==0||req.dma_address%4096!=0||
            req.dma_address.checked_add(u64::from(req.blocks)*512).is_none(){
            return Err(NvmeError::Invalid);
        }
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
    /// Reject a completion carrying stale namespace/LBA/DMA identity without
    /// consuming an unrelated live request with the same numeric command ID.
    pub fn complete_exact(&mut self,epoch:u64,id:u16,namespace_id:u32,
        lba:u64,dma_address:u64)->Result<Request,NvmeError>{
        if epoch==0||epoch!=self.epoch{return Err(NvmeError::Stale);}
        let slot=self.active.iter_mut().find(|entry|
            entry.is_some_and(|req|req.command_id==id))
            .ok_or(NvmeError::NotFound)?;
        if slot.as_ref().is_none_or(|req|
            req.namespace_id!=namespace_id||req.lba!=lba||
            req.dma_address!=dma_address){return Err(NvmeError::Stale);}
        slot.take().ok_or(NvmeError::NotFound)
    }
    /// Count only live commands for an authenticated controller epoch.
    pub fn outstanding(&self,epoch:u64)->Result<usize,NvmeError>{
        if epoch==0||epoch!=self.epoch{return Err(NvmeError::Stale);}
        Ok(self.active.iter().filter(|slot|slot.is_some()).count())
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

#[cfg(test)]
mod m1_ws02_nvme_dma_tests {
 use super::*;
 #[test] fn dma_span_does_not_wrap_and_epoch_count_is_authorized() {
  let mut q=Queue::<2>::new(7);
  let request=Request{command_id:1,namespace_id:1,lba:0,blocks:16,
   dma_address:u64::MAX & !4095};
  assert_eq!(q.submit(7,request,1024),Err(NvmeError::Invalid));
  assert_eq!(q.outstanding(7),Ok(0));
  let valid=Request{dma_address:0x4000,..request};
  assert_eq!(q.submit(7,valid,1024),Ok(()));
  assert_eq!(q.outstanding(7),Ok(1));
  assert_eq!(q.outstanding(6),Err(NvmeError::Stale));
  assert_eq!(q.complete(7,1),Ok(valid));
  assert_eq!(q.outstanding(7),Ok(0));
 }
}

#[cfg(test)]
mod ws23_nvme_exact_completion_tests{
 use super::*;
 #[test]fn mismatched_namespace_or_dma_cannot_pop_live_command(){
  let mut q=Queue::<1>::new(12);
  let request=Request{command_id:7,namespace_id:2,lba:10,blocks:8,dma_address:0x4000};
  q.submit(12,request,100).unwrap();
  assert_eq!(q.complete_exact(12,7,1,10,0x4000),Err(NvmeError::Stale));
  assert_eq!(q.complete_exact(12,7,2,10,0x8000),Err(NvmeError::Stale));
  assert_eq!(q.complete_exact(11,7,2,10,0x4000),Err(NvmeError::Stale));
  assert_eq!(q.outstanding(12),Ok(1));
  assert_eq!(q.complete_exact(12,7,2,10,0x4000),Ok(request));
  assert_eq!(q.outstanding(12),Ok(0));
 }
}


// PR #782 WS35-38: strict controller-epoch and DMA ownership boundary.
// These methods do not touch a real NVMe controller or execute IOMMU mapping.
impl<const N:usize> Queue<N>{
 /// WS35: Verify all DMA pages before mutating the command-id table.
 pub fn submit_owned<F>(&mut self,epoch:u64,req:Request,namespace_blocks:u64,
  mut owns_page:F)->Result<(),NvmeError> where F:FnMut(u64,u64)->bool{
  if epoch==0||epoch!=self.epoch{return Err(NvmeError::Stale);}
  if N>MAX_QUEUE_DEPTH||req.namespace_id==0||req.blocks==0||
    req.dma_address==0||req.dma_address%4096!=0 {
   return Err(NvmeError::Invalid);
  }
  if req.lba>=namespace_blocks||
   u64::from(req.blocks)>namespace_blocks-req.lba{
   return Err(NvmeError::OutOfBounds);
  }
  let bytes=u64::from(req.blocks)*512;
  req.dma_address.checked_add(bytes).ok_or(NvmeError::Invalid)?;
  let mut off=0u64;
  while off<bytes{
   let addr=req.dma_address.checked_add(off).ok_or(NvmeError::Invalid)?;
   if !owns_page(addr,4096){return Err(NvmeError::Invalid);}
   off=off.checked_add(4096).ok_or(NvmeError::Invalid)?;
  }
  self.submit(epoch,req,namespace_blocks)
 }
 /// WS36: Exact request identity includes block count (previous exact helper
 /// checked only controller epoch, CID, namespace, LBA and DMA address).
 pub fn complete_request(&mut self,epoch:u64,expected:Request)
  ->Result<Request,NvmeError>{
  if epoch==0||epoch!=self.epoch{return Err(NvmeError::Stale);}
  let slot=self.active.iter_mut().find(|slot|
    slot.is_some_and(|req|req.command_id==expected.command_id))
    .ok_or(NvmeError::NotFound)?;
  if slot.as_ref()!=Some(&expected){return Err(NvmeError::Stale);}
  slot.take().ok_or(NvmeError::NotFound)
 }
 /// WS37: Cancellation only consumes the exact in-flight request and never
 /// a recycled CID after reset.
 pub fn abort_request(&mut self,epoch:u64,expected:Request)
  ->Result<Request,NvmeError>{
  self.complete_request(epoch,expected)
 }
 /// WS38: Controller reset never drops pending requests without independent,
 /// trusted hardware quiescence. A boolean is not evidence of DMA isolation.
 pub fn reset_quiesced(&mut self,new_epoch:u64,hardware_quiesced:bool)
  ->Result<(),NvmeError>{
  if !hardware_quiesced{return Err(NvmeError::Stale);}
  self.reset(new_epoch)
 }
}
#[cfg(test)]
mod pr782_ws35_38_tests{
 use super::*;
 fn request()->Request{Request{command_id:9,namespace_id:2,
  lba:20,blocks:16,dma_address:0x4000}}
 #[test]fn ws35_unmapped_second_page_never_enqueues(){
  let mut q=Queue::<2>::new(3);
  let mut pages=0;
  assert_eq!(q.submit_owned(2,request(),100,|_,_|{
   pages+=1;true
  }),Err(NvmeError::Stale));
  assert_eq!(pages,0);
  assert_eq!(q.submit_owned(3,request(),100,|a,_|{
   pages+=1;a==0x4000
  }),Err(NvmeError::Invalid));
  assert_eq!(q.outstanding(3),Ok(0));
  assert_eq!(q.submit_owned(3,request(),100,|_,_|true),Ok(()));
 }
 #[test]fn ws36_full_identity_including_blocks_is_required(){
  let mut q=Queue::<1>::new(3);
  q.submit(3,request(),100).unwrap();
  let bad=Request{blocks:8,..request()};
  assert_eq!(q.complete_request(3,bad),Err(NvmeError::Stale));
  assert_eq!(q.outstanding(3),Ok(1));
  assert_eq!(q.complete_request(3,request()),Ok(request()));
 }
 #[test]fn ws37_abort_does_not_consume_wrong_or_stale_cid(){
  let mut q=Queue::<1>::new(3);q.submit(3,request(),100).unwrap();
  assert_eq!(q.abort_request(3,Request{dma_address:0x5000,..request()}),
   Err(NvmeError::Stale));
  assert_eq!(q.abort_request(2,request()),Err(NvmeError::Stale));
  assert_eq!(q.outstanding(3),Ok(1));
  assert_eq!(q.abort_request(3,request()),Ok(request()));
 }
 #[test]fn ws38_reset_requires_hardware_quiescence(){
  let mut q=Queue::<1>::new(3);q.submit(3,request(),100).unwrap();
  assert_eq!(q.reset_quiesced(4,false),Err(NvmeError::Stale));
  assert_eq!(q.outstanding(3),Ok(1));
  assert_eq!(q.reset_quiesced(3,true),Err(NvmeError::Stale));
  q.reset_quiesced(4,true).unwrap();
  assert_eq!(q.outstanding(4),Ok(0));
 }
}


// PR782 WS96-100: transactional queue operations on the trusted controller
// command table. Still does not issue PCIe commands or pin DMA buffers.
impl<const N:usize> Queue<N>{
 /// WS96: Batch-submit after a full dry-run on a private fixed-size staging
 /// copy. Duplicate command IDs, out-of-range DMA or capacity deny all.
 pub fn submit_batch(&mut self,epoch:u64,requests:&[(Request,u64)])
  ->Result<usize,NvmeError>{
  if epoch==0||epoch!=self.epoch{return Err(NvmeError::Stale);}
  if requests.is_empty(){return Err(NvmeError::Invalid);}
  let mut stage=Queue::<N>{active:self.active,epoch:self.epoch};
  for &(request,blocks) in requests{
   stage.submit(epoch,request,blocks)?;
  }
  self.active=stage.active;Ok(requests.len())
 }
 /// WS97: A batch of exact acknowledgements must match every complete
 /// command before any CIDs are freed. All failures preserve queue state.
 pub fn complete_batch(&mut self,epoch:u64,expected:&[Request])
  ->Result<usize,NvmeError>{
  if epoch==0||epoch!=self.epoch{return Err(NvmeError::Stale);}
  if expected.is_empty(){return Err(NvmeError::Invalid);}
  let mut stage=Queue::<N>{active:self.active,epoch:self.epoch};
  for &request in expected{stage.complete_request(epoch,request)?;}
  self.active=stage.active;Ok(expected.len())
 }
 /// WS98: Count one exact namespace's in-flight work for admission limits.
 pub fn namespace_outstanding(&self,epoch:u64,namespace:u32)
  ->Result<usize,NvmeError>{
  if epoch==0||epoch!=self.epoch{return Err(NvmeError::Stale);}
  if namespace==0{return Err(NvmeError::Invalid);}
  Ok(self.active.iter().flatten().filter(|r|r.namespace_id==namespace).count())
 }
 /// WS99: Copy the complete in-flight request table in stable slot order,
 /// preserving the output array on insufficient caller capacity.
 pub fn snapshot_requests(&self,epoch:u64,out:&mut[Request])
  ->Result<usize,NvmeError>{
  if epoch==0||epoch!=self.epoch{return Err(NvmeError::Stale);}
  let count=self.outstanding(epoch)?;
  if count>out.len(){return Err(NvmeError::Full);}
  let mut i=0;
  for request in self.active.iter().flatten(){out[i]=*request;i+=1;}
  Ok(i)
 }
 /// WS100: Do not reset until no software request is in flight and trusted
 /// hardware shutdown is proved; this avoids silently dropping live IO.
 pub fn reset_drained(&mut self,new_epoch:u64,hardware_stopped:bool)
  ->Result<(),NvmeError>{
  if !hardware_stopped{return Err(NvmeError::Stale);}
  if self.outstanding(self.epoch)?!=0{return Err(NvmeError::Full);}
  self.reset_quiesced(new_epoch,true)
 }
}
#[cfg(test)]
mod pr782_ws96_100_tests{
 use super::*;
 fn r(id:u16)->Request{Request{command_id:id,namespace_id:3,
  lba:10,blocks:8,dma_address:0x4000}}
 #[test]fn ws96_submit_batch_rolls_back_every_partial_request(){
  let mut q=Queue::<2>::new(4);
  assert_eq!(q.submit_batch(4,&[(r(1),100),(r(1),100)]),Err(NvmeError::Duplicate));
  assert_eq!(q.outstanding(4),Ok(0));
  assert_eq!(q.submit_batch(4,&[(r(1),100),(r(2),100)]),Ok(2));
  assert_eq!(q.outstanding(4),Ok(2));
  assert_eq!(q.submit_batch(3,&[(r(3),100)]),Err(NvmeError::Stale));
 }
 #[test]fn ws97_complete_batch_requires_every_matching_full_request(){
  let mut q=Queue::<2>::new(4);q.submit_batch(4,&[(r(1),100),(r(2),100)]).unwrap();
  assert_eq!(q.complete_batch(4,&[r(1),Request{blocks:7,..r(2)}]),
   Err(NvmeError::Stale));assert_eq!(q.outstanding(4),Ok(2));
  assert_eq!(q.complete_batch(4,&[r(1),r(2)]),Ok(2));
  assert_eq!(q.outstanding(4),Ok(0));
 }
 #[test]fn ws98_exact_namespace_count_excludes_others(){
  let mut q=Queue::<2>::new(4);let foreign=Request{namespace_id:5,..r(2)};
  q.submit_batch(4,&[(r(1),100),(foreign,100)]).unwrap();
  assert_eq!(q.namespace_outstanding(4,3),Ok(1));
  assert_eq!(q.namespace_outstanding(4,5),Ok(1));
  assert_eq!(q.namespace_outstanding(4,0),Err(NvmeError::Invalid));
 }
 #[test]fn ws99_short_snapshot_is_atomic(){
  let mut q=Queue::<2>::new(4);
  q.submit_batch(4,&[(r(1),100),(r(2),100)]).unwrap();
  let mut short=[r(9)];
  assert_eq!(q.snapshot_requests(4,&mut short),Err(NvmeError::Full));
  assert_eq!(short,[r(9)]);
  let mut out=[r(9);2];
  assert_eq!(q.snapshot_requests(4,&mut out),Ok(2));
  assert_eq!(out,[r(1),r(2)]);
 }
 #[test]fn ws100_reset_requires_both_hardware_and_software_quiescence(){
  let mut q=Queue::<1>::new(4);q.submit(4,r(1),100).unwrap();
  assert_eq!(q.reset_drained(5,true),Err(NvmeError::Full));
  q.complete_request(4,r(1)).unwrap();
  assert_eq!(q.reset_drained(5,false),Err(NvmeError::Stale));
  assert_eq!(q.reset_drained(5,true),Ok(()));
  assert_eq!(q.outstanding(5),Ok(0));
 }
}

//! R037/24: validate VirtIO split-ring descriptor chains before trusting them.
//! No MMIO transport negotiation, DMA ownership or actual device operations.
pub const NEXT:u16=1;
pub const WRITE:u16=2;
pub const INDIRECT:u16=4;
pub const MAX_CHAIN:usize=32;
pub const MAX_BYTES:u64=1024*1024;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Descriptor{pub address:u64,pub length:u32,pub flags:u16,pub next:u16}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum VirtError{Invalid,TooLong,Cycle,OutOfBounds,Unsupported}
pub fn validate_chain(descriptors:&[Descriptor],head:usize,device_writes:bool)
    ->Result<u64,VirtError>{
    if descriptors.is_empty()||descriptors.len()>256||head>=descriptors.len(){
        return Err(VirtError::OutOfBounds);
    }
    let mut seen=[false;256];
    let mut current=head;let mut count=0usize;let mut total=0u64;
    loop{
        if current>=descriptors.len(){return Err(VirtError::OutOfBounds);}
        if seen[current]{return Err(VirtError::Cycle);}
        seen[current]=true;count+=1;
        if count>MAX_CHAIN{return Err(VirtError::TooLong);}
        let d=descriptors[current];
        if d.flags & !(NEXT|WRITE|INDIRECT)!=0 ||
            d.flags&INDIRECT!=0{return Err(VirtError::Unsupported);}
        if d.address==0||d.length==0||
            d.address.checked_add(u64::from(d.length)).is_none(){
            return Err(VirtError::Invalid);
        }
        if (d.flags&WRITE!=0)!=device_writes{
            return Err(VirtError::Unsupported);
        }
        total=total.checked_add(u64::from(d.length)).ok_or(VirtError::TooLong)?;
        if total>MAX_BYTES{return Err(VirtError::TooLong);}
        if d.flags&NEXT==0{return Ok(total);}
        current=d.next as usize;
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    fn descriptor(next:u16,flags:u16)->Descriptor{
        Descriptor{address:0x1000,length:512,flags,next}
    }
    #[test]fn accepts_bounded_write_chain(){
        let chain=[descriptor(1,NEXT|WRITE),descriptor(0,WRITE)];
        assert_eq!(validate_chain(&chain,0,true),Ok(1024));
    }
    #[test]fn cycle_outside_ring_and_untrusted_flags_rejected(){
        assert_eq!(validate_chain(&[descriptor(0,NEXT)],0,false),Err(VirtError::Cycle));
        assert_eq!(validate_chain(&[descriptor(2,NEXT)],0,false),
            Err(VirtError::OutOfBounds));
        assert_eq!(validate_chain(&[descriptor(0,INDIRECT)],0,false),
            Err(VirtError::Unsupported));
    }
    #[test]fn refuses_direction_mismatch(){
        assert_eq!(validate_chain(&[descriptor(0,WRITE)],0,false),
            Err(VirtError::Unsupported));
    }
}

/// Enforce negotiated VirtIO split-queue capacity before mapping the ring.
/// This validates metadata only; the DMA owner must separately pin the memory.
pub fn validate_ring_size(entries:u16)->Result<usize,VirtError>{
 let size=usize::from(entries);
 if size==0||size>256||!size.is_power_of_two(){
  return Err(VirtError::OutOfBounds);
 }
 Ok(size)
}
/// Strict ring-level entry point; requires a legitimate negotiated queue size.
pub fn validate_ring(descriptors:&[Descriptor],head:usize,device_writes:bool)
 ->Result<u64,VirtError>{
 let count=u16::try_from(descriptors.len()).map_err(|_|VirtError::OutOfBounds)?;
 validate_ring_size(count)?;
 validate_chain(descriptors,head,device_writes)
}
#[cfg(test)]
mod m1_ws03_virtio_ring_tests {
 use super::*;
 #[test] fn ring_sizes_must_be_nonzero_bounded_powers_of_two() {
  for n in [1,2,4,8,16,32,64,128,256]{assert_eq!(validate_ring_size(n),Ok(n as usize));}
  for n in [0,3,5,63,255,257,u16::MAX]{
   assert_eq!(validate_ring_size(n),Err(VirtError::OutOfBounds));
  }
  let good=[Descriptor{address:0x1000,length:512,flags:0,next:0}];
  assert_eq!(validate_ring(&good,0,false),Ok(512));
  assert_eq!(validate_ring(&good,1,false),Err(VirtError::OutOfBounds));
  assert_eq!(validate_ring(&[good[0];3],0,false),Err(VirtError::OutOfBounds));
 }
}

/// Reject overlapping DMA buffers inside an otherwise valid split chain.
/// This preflight is intentionally separate from the historical validator:
/// real DMA pinning, IOMMU and ownership checks remain mandatory.
pub fn validate_disjoint_chain(descriptors:&[Descriptor],head:usize,
 device_writes:bool)->Result<u64,VirtError>{
 let bytes=validate_ring(descriptors,head,device_writes)?;
 let mut visited=[(0u64,0u64);MAX_CHAIN];
 let mut count=0usize;let mut current=head;
 loop{
  let item=descriptors[current];
  let end=item.address.checked_add(u64::from(item.length))
      .ok_or(VirtError::Invalid)?;
  for &(start,stop) in &visited[..count]{
   if item.address<stop&&start<end{return Err(VirtError::Invalid);}
  }
  visited[count]=(item.address,end);count+=1;
  if item.flags&NEXT==0{break;}
  current=item.next as usize;
 }
 Ok(bytes)
}
#[cfg(test)]
mod ws24_virtio_alias_tests{
 use super::*;
 #[test]fn overlapping_split_descriptors_do_not_pass_strict_preflight(){
  let first=Descriptor{address:0x1000,length:512,flags:NEXT,next:1};
  let overlap=Descriptor{address:0x1100,length:512,flags:0,next:0};
  assert_eq!(validate_disjoint_chain(&[first,overlap],0,false),Err(VirtError::Invalid));
  let separated=Descriptor{address:0x1200,..overlap};
  assert_eq!(validate_disjoint_chain(&[first,separated],0,false),Ok(1024));
  assert_eq!(validate_disjoint_chain(&[first,separated],0,true),Err(VirtError::Unsupported));
 }
}


// PR #782 WS39-41: strict split-queue ownership and completion cursor gates.
pub fn validate_owned_disjoint_chain<F>(descriptors:&[Descriptor],head:usize,
 device_writes:bool,mut owns_page:F)->Result<u64,VirtError>
 where F:FnMut(u64,u64)->bool{
 // WS39: reject cycles/aliasing first, before consulting an IOMMU owner.
 let total=validate_disjoint_chain(descriptors,head,device_writes)?;
 let mut current=head;
 loop{
  let desc=descriptors[current];
  let end=desc.address.checked_add(u64::from(desc.length))
   .ok_or(VirtError::Invalid)?;
  let mut page=desc.address&!4095;
  while page<end{
   if !owns_page(page,4096){return Err(VirtError::Invalid);}
   page=page.checked_add(4096).ok_or(VirtError::Invalid)?;
  }
  if desc.flags&NEXT==0{break;}
  current=usize::from(desc.next);
 }
 Ok(total)
}
/// WS40: Force every ring segment within the device-negotiated per-buffer
/// budget. A descriptor cannot evade the limit by chain total alone.
pub fn validate_bounded_segments(descriptors:&[Descriptor],head:usize,
 device_writes:bool,max_segment:u32)->Result<u64,VirtError>{
 let total=validate_disjoint_chain(descriptors,head,device_writes)?;
 if max_segment==0{return Err(VirtError::Invalid);}
 let mut current=head;
 loop{
  let d=descriptors[current];
  if d.length>max_segment{return Err(VirtError::TooLong);}
  if d.flags&NEXT==0{break;}
  current=usize::from(d.next);
 }
 Ok(total)
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct UsedRingCursor{epoch:u64,used:u16}
impl UsedRingCursor{
 pub const fn new(epoch:u64,used:u16)->Self{Self{epoch,used}}
 /// WS41: A completion batch cannot exceed negotiated ring capacity, repeat
 /// an old index, or cross a retired queue generation. Handles u16 wrap.
 pub fn advance(&mut self,epoch:u64,next:u16,ring_size:u16)
  ->Result<u16,VirtError>{
  let limit=validate_ring_size(ring_size)? as u16;
  if epoch==0||epoch!=self.epoch{return Err(VirtError::Invalid);}
  let delta=next.wrapping_sub(self.used);
  if delta==0||delta>limit{return Err(VirtError::OutOfBounds);}
  self.used=next;Ok(delta)
 }
 pub fn used(&self)->u16{self.used}
}
#[cfg(test)]
mod pr782_ws39_41_tests{
 use super::*;
 fn ring()->[Descriptor;2]{[
  Descriptor{address:0x1000,length:4096,flags:NEXT,next:1},
  Descriptor{address:0x2000,length:512,flags:0,next:0},
 ]}
 #[test]fn ws39_deny_unowned_dma_without_mutating_queue(){
  let mut pages=0;
  assert_eq!(validate_owned_disjoint_chain(&ring(),0,false,|a,n|{
   pages+=1;n==4096&&a==0x1000
  }),Err(VirtError::Invalid));
  assert_eq!(pages,2);
  assert_eq!(validate_owned_disjoint_chain(&ring(),0,false,|_,_|true),Ok(4608));
 }
 #[test]fn ws40_per_descriptor_budget_is_enforced(){
  assert_eq!(validate_bounded_segments(&ring(),0,false,2048),Err(VirtError::TooLong));
  assert_eq!(validate_bounded_segments(&ring(),0,false,4096),Ok(4608));
  assert_eq!(validate_bounded_segments(&ring(),0,false,0),Err(VirtError::Invalid));
 }
 #[test]fn ws41_rejects_stale_and_oversized_used_index_delta(){
  let mut cursor=UsedRingCursor::new(5,u16::MAX);
  assert_eq!(cursor.advance(4,0,2),Err(VirtError::Invalid));
  assert_eq!(cursor.advance(5,2,2),Err(VirtError::OutOfBounds));
  assert_eq!(cursor.used(),u16::MAX);
  assert_eq!(cursor.advance(5,0,2),Ok(1));
  assert_eq!(cursor.advance(5,0,2),Err(VirtError::OutOfBounds));
  assert_eq!(cursor.advance(5,1,3),Err(VirtError::OutOfBounds));
 }
}


// PR782 WS101-105: split-ring layout, available/used ownership and
// checked mixed-direction buffers. These perform no DMA or transport writes.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct RingLayout{pub descriptors:u64,pub available:u64,pub used:u64,
 pub entries:u16}
/// WS101: Verify all three physical ring apertures are aligned, bounded,
/// disjoint and non-wrapping before the trusted mapper pins any pages.
pub fn validate_ring_layout(layout:RingLayout)->Result<(),VirtError>{
 let n=validate_ring_size(layout.entries)? as u64;
 if layout.descriptors==0||layout.available==0||layout.used==0||
  layout.descriptors%16!=0||layout.available%2!=0||layout.used%4!=0{
  return Err(VirtError::Invalid);
 }
 let start=[layout.descriptors,layout.available,layout.used];
 let lengths=[n*16,6+n*2,6+n*8];
 let mut ends=[0u64;3];
 for i in 0..3{
  ends[i]=start[i].checked_add(lengths[i]).ok_or(VirtError::Invalid)?;
  for j in 0..i{
   if start[i]<ends[j]&&start[j]<ends[i]{return Err(VirtError::Invalid);}
  }
 }
 Ok(())
}
/// WS102: Before consuming an available batch, deny duplicate/reused heads,
/// invalid descriptor indices or over-capacity device notifications.
pub fn validate_available_heads(entries:u16,heads:&[u16])
 ->Result<usize,VirtError>{
 let n=validate_ring_size(entries)?;
 if heads.is_empty()||heads.len()>n{return Err(VirtError::OutOfBounds);}
 for (i,&head) in heads.iter().enumerate(){
  if usize::from(head)>=n{return Err(VirtError::OutOfBounds);}
  if heads[..i].contains(&head){return Err(VirtError::Cycle);}
 }
 Ok(heads.len())
}
/// WS103: Device-used length must never exceed an exact known, structurally
/// valid descriptor chain, and a completion ID is never an arbitrary pointer.
pub fn validate_used_element(descriptors:&[Descriptor],head:u16,
 device_writes:bool,written:u32)->Result<(),VirtError>{
 let total=validate_disjoint_chain(descriptors,usize::from(head),device_writes)?;
 if u64::from(written)>total{return Err(VirtError::OutOfBounds);}
 if !device_writes&&written!=0{return Err(VirtError::Unsupported);}
 Ok(())
}
impl UsedRingCursor{
 /// WS104: A caller budget bounds accepted used entries even if a malicious
 /// guest supplies a geometrically valid but unexpected ring-sized delta.
 pub fn advance_budgeted(&mut self,epoch:u64,next:u16,ring_size:u16,
  budget:u16)->Result<u16,VirtError>{
  let n=validate_ring_size(ring_size)? as u16;
  let delta=next.wrapping_sub(self.used);
  if budget==0||budget>n||delta==0||delta>budget{
   return Err(VirtError::OutOfBounds);
  }
  self.advance(epoch,next,ring_size)
 }
}
/// WS105: A mixed-direction VirtIO request has READ-only headers first,
/// then WRITE-only response buffers. No read descriptor may follow a write.
pub fn validate_mixed_chain(descriptors:&[Descriptor],head:usize)
 ->Result<(u64,u64),VirtError>{
 let entries=u16::try_from(descriptors.len()).map_err(|_|VirtError::OutOfBounds)?;
 validate_ring_size(entries)?;
 if head>=descriptors.len(){return Err(VirtError::OutOfBounds);}
 let mut seen=[false;256];let mut regions=[(0u64,0u64);MAX_CHAIN];
 let(mut current,mut count,mut reads,mut writes)=(head,0usize,0u64,0u64);
 let mut writing=false;
 loop{
  if current>=descriptors.len(){return Err(VirtError::OutOfBounds);}
  if seen[current]{return Err(VirtError::Cycle);}
  seen[current]=true;
  if count>=MAX_CHAIN{return Err(VirtError::TooLong);}
  let d=descriptors[current];
  if d.flags&!(NEXT|WRITE)!=0||d.address==0||d.length==0{
   return Err(VirtError::Invalid);
  }
  let end=d.address.checked_add(u64::from(d.length)).ok_or(VirtError::Invalid)?;
  for &(a,b) in &regions[..count]{
   if d.address<b&&a<end{return Err(VirtError::Invalid);}
  }
  regions[count]=(d.address,end);count+=1;
  if d.flags&WRITE!=0{
   writing=true;
   writes=writes.checked_add(u64::from(d.length)).ok_or(VirtError::TooLong)?;
  }else{
   if writing{return Err(VirtError::Unsupported);}
   reads=reads.checked_add(u64::from(d.length)).ok_or(VirtError::TooLong)?;
  }
  if reads.checked_add(writes).is_none_or(|n|n>MAX_BYTES){
   return Err(VirtError::TooLong);
  }
  if d.flags&NEXT==0{break;}
  current=usize::from(d.next);
 }
 if reads==0||writes==0{return Err(VirtError::Unsupported);}
 Ok((reads,writes))
}
#[cfg(test)]
mod pr782_ws101_105_tests{
 use super::*;
 fn d(addr:u64,flags:u16,next:u16)->Descriptor{
  Descriptor{address:addr,length:512,flags,next}
 }
 #[test]fn ws101_ring_aperture_geometry_and_overlap(){
  let layout=RingLayout{descriptors:0x1000,available:0x2000,used:0x3000,entries:8};
  assert_eq!(validate_ring_layout(layout),Ok(()));
  assert_eq!(validate_ring_layout(RingLayout{used:0x1010,..layout}),
   Err(VirtError::Invalid));
  assert_eq!(validate_ring_layout(RingLayout{descriptors:u64::MAX-15,..layout}),
   Err(VirtError::Invalid));
 }
 #[test]fn ws102_duplicated_or_outside_available_heads_rejected(){
  assert_eq!(validate_available_heads(4,&[0,3]),Ok(2));
  assert_eq!(validate_available_heads(4,&[0,0]),Err(VirtError::Cycle));
  assert_eq!(validate_available_heads(4,&[4]),Err(VirtError::OutOfBounds));
  assert_eq!(validate_available_heads(3,&[0]),Err(VirtError::OutOfBounds));
 }
 #[test]fn ws103_malformed_used_element_rejected(){
  let ds=[d(0x1000,WRITE,0)];
  assert_eq!(validate_used_element(&ds,0,true,512),Ok(()));
  assert_eq!(validate_used_element(&ds,0,true,513),Err(VirtError::OutOfBounds));
  assert_eq!(validate_used_element(&ds,0,false,0),Err(VirtError::Unsupported));
 }
 #[test]fn ws104_cursor_budget_blocks_oversized_completion(){
  let mut c=UsedRingCursor::new(3,10);
  assert_eq!(c.advance_budgeted(3,13,8,2),Err(VirtError::OutOfBounds));
  assert_eq!(c.used(),10);
  assert_eq!(c.advance_budgeted(3,12,8,2),Ok(2));
  assert_eq!(c.advance_budgeted(4,13,8,2),Err(VirtError::Invalid));
 }
 #[test]fn ws105_mixed_chain_allows_header_then_response_only(){
  let ok=[d(0x1000,NEXT,1),d(0x2000,WRITE,0)];
  assert_eq!(validate_mixed_chain(&ok,0),Ok((512,512)));
  let inverted=[d(0x1000,NEXT|WRITE,1),d(0x2000,0,0)];
  assert_eq!(validate_mixed_chain(&inverted,0),Err(VirtError::Unsupported));
  let aliased=[d(0x1000,NEXT,1),d(0x1000,WRITE,0)];
  assert_eq!(validate_mixed_chain(&aliased,0),Err(VirtError::Invalid));
 }
}

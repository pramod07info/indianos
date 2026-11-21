//! R028 QEMU-only legacy virtio-net PCI driver, single CPU and IRQs masked.
//!
//! The transport uses two bounded contiguous DMA vrings, one receive and one
//! transmit packet buffer. No interrupt/shared IRQs, multiqueue, mergeable
//! buffers, checksum offload, or modern virtio PCI capability negotiation.
//! The device MUST be an emulated 0x1af4:0x1000 virtio-net legacy PCI card.
//! Never enable this experimental DMA driver in ordinary machine boot.
use core::{arch::asm,ptr,sync::atomic::{compiler_fence,Ordering}};
use crate::network::{EthernetDevice,Error};

const PAGE:usize=4096;
const MAX_QUEUE_SIZE:u16=1024;
const FRAME_CAP:usize=2048;
const VENDOR:u16=0x1af4;
const DEVICE:u16=0x1000;
const QUEUE_RX:u16=0;
const QUEUE_TX:u16=1;
const DESC_WRITE:u16=2;
const VIRTIO_MAC_FEATURE:u32=1<<5;
const DEVICE_ACK:u8=1;
const DRIVER:u8=2;
const DRIVER_OK:u8=4;
const TIMEOUT:u32=6_000_000;
unsafe extern "C"{
    fn pmm_allocate_frame(out:*mut u64)->bool;
    fn pmm_allocate_contiguous_frames(count:u32,out:*mut u64)->bool;
    fn pmm_release_frame(physical:u64)->bool;
    fn boot_hhdm_offset(out:*mut u64)->bool;
}
#[inline] unsafe fn outb(p:u16,v:u8){
    unsafe{asm!("out dx, al",in("dx")p,in("al")v,options(nostack,nomem,preserves_flags));}
}
#[inline] unsafe fn outw(p:u16,v:u16){
    unsafe{asm!("out dx, ax",in("dx")p,in("ax")v,options(nostack,nomem,preserves_flags));}
}
#[inline] unsafe fn outl(p:u16,v:u32){
    unsafe{asm!("out dx, eax",in("dx")p,in("eax")v,options(nostack,nomem,preserves_flags));}
}
#[inline] unsafe fn inb(p:u16)->u8{
    let v:u8;unsafe{asm!("in al, dx",in("dx")p,out("al")v,options(nostack,nomem,preserves_flags));}v
}
#[inline] unsafe fn inw(p:u16)->u16{
    let v:u16;unsafe{asm!("in ax, dx",in("dx")p,out("ax")v,options(nostack,nomem,preserves_flags));}v
}
#[inline] unsafe fn inl(p:u16)->u32{
    let v:u32;unsafe{asm!("in eax, dx",in("dx")p,out("eax")v,options(nostack,nomem,preserves_flags));}v
}
fn pciread(bus:u8,device:u8,fun:u8,reg:u8)->u32{
    let addr=0x8000_0000u32|((bus as u32)<<16)|
        ((device as u32)<<11)|((fun as u32)<<8)|(reg as u32 & 0xfc);
    unsafe{outl(0xcf8,addr);inl(0xcfc)}
}
fn pciwrite(bus:u8,device:u8,fun:u8,reg:u8,value:u32){
    let addr=0x8000_0000u32|((bus as u32)<<16)|
        ((device as u32)<<11)|((fun as u32)<<8)|(reg as u32 & 0xfc);
    unsafe{outl(0xcf8,addr);outl(0xcfc,value);}
}
fn discover()->Option<(u8,u8,u8,u16)>{
    for bus in 0u8..=1{
        for slot in 0u8..32{
            let id=pciread(bus,slot,0,0);
            if id as u16!=VENDOR || (id>>16) as u16!=DEVICE{continue;}
            for function in 0u8..8{
                let current=pciread(bus,slot,function,0);
                if current as u16!=VENDOR || (current>>16) as u16!=DEVICE{
                    continue;
                }
                let bar=pciread(bus,slot,function,0x10);
                if bar&1!=1{return None;} // only legacy I/O BAR, no memory BAR.
                let port=(bar&!3) as u16;
                if port<0x1000 || (port as u32)+0x20>0x10000{return None;}
                let command=pciread(bus,slot,function,0x04);
                pciwrite(bus,slot,function,0x04,command|0x0005);
                if pciread(bus,slot,function,0x04)&0x0005!=0x0005{return None;}
                return Some((bus,slot,function,port));
            }
        }
    }
    None
}
fn direct_map(hhdm:u64,phys:u64)->Option<*mut u8>{
    if phys==0 || phys>=0x1_0000_0000 || phys&0xfff!=0{return None;}
    let virtual_addr=hhdm.checked_add(phys)?;
    if virtual_addr>>48!=0xffff || (virtual_addr & (1u64<<47))==0{return None;}
    Some(virtual_addr as *mut u8)
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct QueueLayout{size:u16,avail:usize,used:usize,frames:u32}
impl QueueLayout{
    fn new(size:u16)->Option<Self>{
        if size==0||!size.is_power_of_two()||size>MAX_QUEUE_SIZE{return None;}
        let avail=(size as usize).checked_mul(16)?;
        let avail_end=avail.checked_add(4)?.checked_add(size as usize*2)?;
        let used=avail_end.checked_add(PAGE-1)?&!(PAGE-1);
        let end=used.checked_add(4)?.checked_add(size as usize*8)?;
        let frames=end.checked_add(PAGE-1)?/PAGE;
        if frames==0||frames>16{return None;}
        Some(Self{size,avail,used,frames:frames as u32})
    }
}
fn queue_layout(port:u16,queue:u16)->Option<QueueLayout>{
    unsafe{outw(port+0x0e,queue);}
    QueueLayout::new(unsafe{inw(port+0x0c)})
}
struct Queue{
    ring_phys:u64,
    ring:*mut u8,
    buffer_phys:u64,
    buffer:*mut u8,
    queued:u16,
    used:u16,
    layout:QueueLayout,
}
impl Queue {
    fn allocate(hhdm:u64,layout:QueueLayout)->Result<Self,Error>{
        let mut ring_phys=0u64;
        if !unsafe{pmm_allocate_contiguous_frames(layout.frames,&mut ring_phys)}{
            return Err(Error::Device);
        }
        let mut bufphys=0u64;
        if !unsafe{pmm_allocate_frame(&mut bufphys)}{
            for n in 0..layout.frames{
                let _=unsafe{pmm_release_frame(ring_phys+n as u64*PAGE as u64)};
            }
            return Err(Error::Device);
        }
        let last=ring_phys.checked_add((layout.frames as u64-1)*PAGE as u64);
        let ring_end_valid=last.is_some_and(|last|direct_map(hhdm,last).is_some());
        let (Some(ring),Some(buffer),true)=(direct_map(hhdm,ring_phys),
            direct_map(hhdm,bufphys),ring_end_valid)
        else{
            let _=unsafe{pmm_release_frame(bufphys)};
            for n in 0..layout.frames{
                let _=unsafe{pmm_release_frame(ring_phys+n as u64*PAGE as u64)};
            }
            return Err(Error::Device);
        };
        unsafe{
            ptr::write_bytes(ring,0,layout.frames as usize*PAGE);
            ptr::write_bytes(buffer,0,PAGE);
        }
        Ok(Self{ring_phys,ring,buffer_phys:bufphys,buffer,queued:0,used:0,layout})
    }
    fn descriptor(&mut self,size:u32,device_writes:bool){
        // Index zero remains the only submitted descriptor. Device ring
        // capacity changes the layout, not the number of owned buffers.
        unsafe{
            ptr::write_volatile(self.ring.cast::<u64>(),self.buffer_phys);
            ptr::write_volatile(self.ring.add(8).cast::<u32>(),size);
            ptr::write_volatile(self.ring.add(12).cast::<u16>(),
                if device_writes{DESC_WRITE}else{0});
            ptr::write_volatile(self.ring.add(14).cast::<u16>(),0);
        }
    }
    fn offer(&mut self){
        let slot=(self.queued as usize%self.layout.size as usize)*2;
        unsafe{
            ptr::write_volatile(self.ring.add(self.layout.avail+4+slot).cast::<u16>(),0);
        }
        compiler_fence(Ordering::Release);
        self.queued=self.queued.wrapping_add(1);
        unsafe{ptr::write_volatile(self.ring.add(self.layout.avail+2).cast::<u16>(),self.queued);}
    }
    fn complete(&self)->bool{
        let idx=unsafe{ptr::read_volatile(self.ring.add(self.layout.used+2).cast::<u16>())};
        compiler_fence(Ordering::Acquire);
        idx!=self.used
    }
    fn consume(&mut self)->Result<Option<usize>,Error>{
        let idx=unsafe{ptr::read_volatile(self.ring.add(self.layout.used+2).cast::<u16>())};
        // Observe the device's publication before reading its used element
        // and DMA bytes. x86 coherent DMA supplies the hardware ordering.
        compiler_fence(Ordering::Acquire);
        if idx==self.used{return Ok(None);}
        // Only descriptor zero is ever outstanding. A jump, rewind, replay
        // or unsolicited completion cannot transfer buffer ownership back.
        if self.queued.wrapping_sub(self.used)!=1 ||
           idx.wrapping_sub(self.used)!=1{return Err(Error::Device);}
        let pos=(self.used as usize%self.layout.size as usize)*8;
        let id=unsafe{ptr::read_volatile(self.ring.add(self.layout.used+4+pos).cast::<u32>())};
        let size=unsafe{ptr::read_volatile(self.ring.add(self.layout.used+8+pos).cast::<u32>())};
        if id!=0 || size as usize>FRAME_CAP{return Err(Error::Device);}
        self.used=self.used.wrapping_add(1);
        Ok(Some(size as usize))
    }
    fn release(self){
        let _=unsafe{pmm_release_frame(self.buffer_phys)};
        for n in 0..self.layout.frames{
            let _=unsafe{pmm_release_frame(self.ring_phys+n as u64*PAGE as u64)};
        }
    }
}
fn configure_queue(port:u16,queue:u16,q:&Queue)->bool{
    unsafe{outw(port+0x0e,queue);}
    if unsafe{inw(port+0x0c)}!=q.layout.size ||
       q.ring_phys/PAGE as u64>u32::MAX as u64{
        return false;
    }
    unsafe{outl(port+0x08,(q.ring_phys/PAGE as u64) as u32);}
    true
}
pub struct VirtioNet{
    port:u16,
    bus:u8,
    slot:u8,
    function:u8,
    mac:[u8;6],
    rx:Queue,
    tx:Queue,
    // A broken completion or TX timeout makes DMA ownership uncertain.
    faulted:bool,
}
impl VirtioNet{
    /// # Safety
    /// QEMU legacy virtio-net, BSP with IRQs disabled and PMM initialized.
    /// No other I/O master may own the PCI device, DMA frames or virtqueues.
    pub unsafe fn probe()->Result<Self,Error>{
        let Some((bus,slot,function,port))=discover()else{return Err(Error::Device);};
        unsafe{outb(port+0x12,0);outb(port+0x12,DEVICE_ACK|DRIVER);}
        let offered=unsafe{inl(port)};
        if offered&VIRTIO_MAC_FEATURE==0{
            unsafe{outb(port+0x12,0);}
            return Err(Error::Device);
        }
        // Refuse checksum offloads, mergeable buffers, TSO and any feature
        // that changes the legacy 10-byte virtio_net_hdr layout.
        unsafe{outl(port+4,VIRTIO_MAC_FEATURE);}
        let mut mac=[0u8;6];
        for (i,byte) in mac.iter_mut().enumerate(){
            *byte=unsafe{inb(port+0x14+i as u16)};
        }
        if mac==[0;6] || mac==[0xff;6] || mac[0]&1!=0{
            unsafe{outb(port+0x12,0);}
            return Err(Error::Device);
        }
        let mut hhdm=0u64;
        if !unsafe{boot_hhdm_offset(&mut hhdm)}{return Err(Error::Device);}
        // Legacy devices expose a fixed size rather than accepting our
        // preferred size. QEMU revisions use both 256 and 1024 RX entries.
        let (Some(rx_layout),Some(tx_layout))=(queue_layout(port,QUEUE_RX),
            queue_layout(port,QUEUE_TX))else{
            unsafe{outb(port+0x12,0);}return Err(Error::Device);
        };
        let mut rx=Queue::allocate(hhdm,rx_layout)?;
        let mut tx=match Queue::allocate(hhdm,tx_layout){
            Ok(v)=>v,
            Err(e)=>{rx.release();return Err(e);}
        };
        let rings_ready=
            configure_queue(port,QUEUE_RX,&rx) &&
            configure_queue(port,QUEUE_TX,&tx);
        if !rings_ready {
            unsafe{outb(port+0x12,0);}
            rx.release();tx.release();
            return Err(Error::Device);
        }
        rx.descriptor(FRAME_CAP as u32,true);
        rx.offer();
        unsafe{outb(port+0x12,DEVICE_ACK|DRIVER|DRIVER_OK);}
        compiler_fence(Ordering::SeqCst);
        unsafe{outw(port+0x10,QUEUE_RX);}
        Ok(Self{port,bus,slot,function,mac,rx,tx,faulted:false})
    }
    /// Quiesce the legacy NIC and verify that the device has actually
    /// acknowledged reset before releasing DMA pages to the PMM. A removed
    /// device may return 0xff, so an uncertain reset must retain all DMA
    /// frames rather than risk a use-after-free. Requires BSP+CLI and a
    /// stable PCI I/O-port mapping; no IOMMU or real hardware qualification.
    #[must_use]
    pub fn close(self)->bool{
        unsafe{outb(self.port+0x12,0);}
        compiler_fence(Ordering::SeqCst);
        // A removed/replaced device may return all-ones PCI identity,
        // and a device may change after we disable bus mastering.
        let reset=unsafe{inb(self.port+0x12)};
        let identity_before=pciread(self.bus,self.slot,self.function,0);
        if reset!=0 || identity_before!=legacy_nic_identity(){return false;}
        let command=pciread(self.bus,self.slot,self.function,0x04);
        // Preserve status W1C bits by writing only COMMAND[15:0].
        let disabled=(command & 0xffff) & !0x0004;
        pciwrite(self.bus,self.slot,self.function,0x04,disabled);
        compiler_fence(Ordering::SeqCst);
        let observed=pciread(self.bus,self.slot,self.function,0x04);
        let identity_after=pciread(self.bus,self.slot,self.function,0);
        if !nic_dma_retired(reset,identity_before,identity_after,observed){
            return false;
        }
        self.rx.release();self.tx.release();
        true
    }
}
/// Exact expected PCI device ID, not an arbitrary substituted controller.
fn legacy_nic_identity()->u32{((DEVICE as u32)<<16)|VENDOR as u32}
/// Only an unchanged, reset, IO-decodable and bus-master-disabled device
/// permits releasing ring memory. Physical IOMMU qualification is separate.
fn nic_dma_retired(reset:u8,before:u32,after:u32,command:u32)->bool{
    reset==0 && before==legacy_nic_identity() &&
       after==before && (command&0x0004)==0 && (command&0x0001)!=0
}
/// Validate used length without untrusted DMA length subtraction/overflow.
fn receive_payload_len(used_len:usize,capacity:usize)->Result<usize,Error>{
    // A complete Ethernet frame needs a 14-byte destination/source/type
    // header after the ten-byte legacy virtio transport header.
    if used_len<10+14 || used_len>1514+10 {return Err(Error::Invalid);}
    let payload=used_len-10;
    if payload>capacity {return Err(Error::Invalid);}
    Ok(payload)
}
/// Without negotiated checksum/GSO features a legacy device must not
/// inject offload metadata that would bypass the kernel's checksum checks.
fn rx_offload_header_valid(header:&[u8])->bool {
    header.len()==10 && header.iter().all(|&byte|byte==0)
}
/// The R028 stack accepts untagged Ethernet-II ingress only. An invalid
/// source address or 802.3 length field must not reach higher-level parsers.
fn unsupported_vlan_tag(ethertype:u16)->bool {
    matches!(ethertype,0x8100|0x88a8|0x9100)
}
fn ethernet_ingress_valid(frame:&[u8])->bool{
    if !(14..=1514).contains(&frame.len()){return false;}
    let source=&frame[6..12];
    if source[0]&1!=0 || source.iter().all(|&x|x==0){return false;}
    let ethertype=u16::from_be_bytes([frame[12],frame[13]]);
    ethertype>=0x0600 && !unsupported_vlan_tag(ethertype)
}
/// Filter foreign unicast before returning a frame to the guest stack.
/// Broadcast and multicast remain eligible for ARP/DHCP discovery; the
/// upper network stack still validates protocol and destination IP.
fn ethernet_destination_allowed(frame:&[u8],local:[u8;6])->bool{
    if !(14..=1514).contains(&frame.len()) ||
       local[0]&1!=0 || local.iter().all(|&x|x==0){
        return false;
    }
    let dest=&frame[..6];
    dest==local.as_slice() || dest==[0xff;6].as_slice() || dest[0]&1!=0
}
/// The device owns the used-ring length. It cannot report bytes beyond
/// the actual submitted header and Ethernet frame.
/// In R028 the sole supported TX caller is the kernel's Ethernet stack.
fn ethernet_egress_valid(frame:&[u8],local_mac:[u8;6])->bool{
    if !(14..=1514).contains(&frame.len()) ||
       local_mac[0]&1!=0 || local_mac.iter().all(|&x|x==0){
        return false;
    }
    let src=&frame[6..12];
    let dst=&frame[..6];
    let protocol=u16::from_be_bytes([frame[12],frame[13]]);
    src==local_mac.as_slice() && !dst.iter().all(|&x|x==0) &&
    protocol>=0x0600 && !unsupported_vlan_tag(protocol)
}
fn tx_completion_length_valid(reported:usize,frame_length:usize)->bool {
    (14..=1514).contains(&frame_length) && reported<=frame_length+10
}
impl EthernetDevice for VirtioNet{
    fn mac(&self)->[u8;6]{self.mac}
    fn send(&mut self,frame:&[u8])->Result<(),Error>{
        if self.faulted{return Err(Error::Device);}
        if !ethernet_egress_valid(frame,self.mac){return Err(Error::Invalid);}
        // A legacy virtio-net frame has a ten-byte header with all
        // segmentation/checksum offload fields clear.
        unsafe{
            ptr::write_bytes(self.tx.buffer,0,10);
            ptr::copy_nonoverlapping(frame.as_ptr(),self.tx.buffer.add(10),frame.len());
        }
        self.tx.descriptor((10+frame.len()) as u32,false);
        self.tx.offer();
        compiler_fence(Ordering::SeqCst);
        unsafe{outw(self.port+0x10,QUEUE_TX);}
        for _ in 0..TIMEOUT{
            if self.tx.complete(){
                if !matches!(self.tx.consume(),Ok(Some(n))
                    if tx_completion_length_valid(n,frame.len())){
                    self.faulted=true;
                    return Err(Error::Device);
                }
                return Ok(());
            }
            core::hint::spin_loop();
        }
        // Do not reuse the descriptor: the device could still read its DMA buffer.
        self.faulted=true;
        Err(Error::Timeout)
    }
    fn poll(&mut self,out:&mut [u8])->Result<Option<usize>,Error>{
        if self.faulted{return Err(Error::Device);}
        if !self.rx.complete(){return Ok(None);}
        let length=match self.rx.consume(){
            Ok(Some(length))=>length,
            Ok(None)=>return Ok(None),
            Err(error)=>{self.faulted=true;return Err(error);}
        };
        let n=match receive_payload_len(length,out.len()){
            Ok(n)=>n,
            Err(error)=>{
                // The descriptor ID was valid. Discard bad input, rearm RX.
                // Caller receives an error and no bytes are copied.
                self.rx.offer();
                compiler_fence(Ordering::Release);
                unsafe{outw(self.port+0x10,QUEUE_RX);}
                return Err(error);
            }
        };
        compiler_fence(Ordering::Acquire);
        // Offload bits may not be trusted unless features were negotiated.
        // Drop rather than publish a frame if the NIC claims unsupported GSO
        // or checksum work. Return no caller bytes on rejection.
        let header=unsafe{core::slice::from_raw_parts(self.rx.buffer,10)};
        if !rx_offload_header_valid(header){
            self.rx.offer();
            compiler_fence(Ordering::Release);
            unsafe{outw(self.port+0x10,QUEUE_RX);}
            return Err(Error::Invalid);
        }
        let frame=unsafe{core::slice::from_raw_parts(self.rx.buffer.add(10),n)};
        if !ethernet_ingress_valid(frame) ||
           !ethernet_destination_allowed(frame,self.mac){
            self.rx.offer();
            compiler_fence(Ordering::Release);
            unsafe{outw(self.port+0x10,QUEUE_RX);}
            return Err(Error::Invalid);
        }
        unsafe{ptr::copy_nonoverlapping(frame.as_ptr(),out.as_mut_ptr(),n);}
        self.rx.offer();
        compiler_fence(Ordering::Release);
        unsafe{outw(self.port+0x10,QUEUE_RX);}
        Ok(Some(n))
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    fn mock_queue(ring:&mut [u64;1536],buffer:&mut [u64;512])->Queue{
        Queue{ring_phys:0x1000,ring:ring.as_mut_ptr().cast(),
            buffer_phys:0x4000,buffer:buffer.as_mut_ptr().cast(),queued:0,used:0,
            layout:QueueLayout::new(256).unwrap()}
    }
    fn completion(q:&Queue,index:u16,id:u32,len:u32){
        let pos=q.used as usize%q.layout.size as usize*8;
        unsafe{
            ptr::write_volatile(q.ring.add(q.layout.used+4+pos).cast::<u32>(),id);
            ptr::write_volatile(q.ring.add(q.layout.used+8+pos).cast::<u32>(),len);
            ptr::write_volatile(q.ring.add(q.layout.used+2).cast::<u16>(),index);
        }
    }
    #[test]fn completions_require_exact_outstanding_descriptor_and_progress(){
        for (queued,used,index,id,len) in [
            (1,0,2,0,74),(1,0,u16::MAX,0,74), // jump / rewind
            (0,0,1,0,74),(2,0,1,0,74), // no offer / overcommit
            (1,0,1,1,74),(1,0,1,0,FRAME_CAP as u32+1),
        ]{
            let mut ring=[0u64;1536];let mut buf=[0u64;512];
            let mut q=mock_queue(&mut ring,&mut buf);
            q.queued=queued;q.used=used;completion(&q,index,id,len);
            assert_eq!(q.consume(),Err(Error::Device));
            assert_eq!((q.used,q.queued),(used,queued));
        }
    }
    #[test]fn completion_wraparound_transfers_ownership_exactly_once(){
        let mut ring=[0u64;1536];let mut buf=[0u64;512];
        let mut q=mock_queue(&mut ring,&mut buf);
        q.used=u16::MAX;q.queued=u16::MAX;
        completion(&q,u16::MAX,0,0);
        assert_eq!(q.consume(),Ok(None));
        q.offer();assert_eq!(q.queued,0);
        completion(&q,0,0,74);
        assert_eq!(q.consume(),Ok(Some(74)));assert_eq!(q.used,0);
        assert_eq!(q.consume(),Ok(None));
        // Replaying the old index must not return the same DMA bytes.
        completion(&q,u16::MAX,0,74);
        assert_eq!(q.consume(),Err(Error::Device));assert_eq!(q.used,0);
    }
    #[test]fn malformed_rx_faults_before_copy_or_descriptor_rearm(){
        let mut rr=[0u64;1536];let mut rb=[0u64;512];
        let mut tr=[0u64;1536];let mut tb=[0u64;512];
        let mut rx=mock_queue(&mut rr,&mut rb);rx.offer();
        completion(&rx,2,0,74);
        let mut dev=VirtioNet{port:0,bus:0,slot:0,function:0,mac:[2;6],rx,
            tx:mock_queue(&mut tr,&mut tb),faulted:false};
        let mut out=[0xaa;100];
        assert_eq!(dev.poll(&mut out),Err(Error::Device));
        assert!(dev.faulted);assert_eq!(out,[0xaa;100]);
        assert_eq!(dev.rx.queued,1);assert_eq!(dev.rx.used,0);
        assert_eq!(dev.send(&[0;60]),Err(Error::Device));
        assert_eq!(dev.poll(&mut out),Err(Error::Device));
    }
    #[test]fn dma_allocation_counts_and_ring_layout(){
        let layout=QueueLayout::new(256).unwrap();
        assert_eq!(layout.frames as usize*PAGE,12288);
        assert_eq!(layout.avail,4096);assert_eq!(layout.used,8192);
        assert_eq!(layout.avail+4+2*(layout.size as usize-1)+2,4612);
        assert!(layout.avail+4+2*layout.size as usize<=layout.used);
        assert!(layout.used+4+8*layout.size as usize<=layout.frames as usize*PAGE);
    }
    #[test]fn legacy_queue_layouts_are_bounded_aligned_and_nonoverlapping(){
        for size in [0,3,255,1025,2048,u16::MAX]{assert_eq!(QueueLayout::new(size),None);}
        for power in 0..=10{
            let size=1<<power;let layout=QueueLayout::new(size).unwrap();
            assert_eq!(layout.avail,16*size as usize);
            assert_eq!(layout.used%PAGE,0);
            assert!(layout.avail+4+2*size as usize<=layout.used);
            assert!(layout.used+4+8*size as usize<=layout.frames as usize*PAGE);
            assert!(layout.frames<=8);
        }
        assert_eq!(QueueLayout::new(1024),Some(QueueLayout{
            size:1024,avail:16384,used:20480,frames:8}));
    }
    #[test]fn large_legacy_ring_last_slot_and_index_wrap_stay_inside_dma_allocation(){
        let layout=QueueLayout::new(1024).unwrap();
        let mut backing=[0xdddd_dddd_dddd_ddddu64;4097];
        backing[..4096].fill(0);let mut data=[0u64;512];
        let mut q=Queue{ring_phys:0x1000,ring:backing.as_mut_ptr().cast(),
            buffer_phys:0x9000,buffer:data.as_mut_ptr().cast(),queued:u16::MAX,
            used:u16::MAX,layout};
        completion(&q,u16::MAX,0,0);
        q.offer();assert_eq!(q.queued,0);
        completion(&q,0,0,74);
        assert_eq!(q.consume(),Ok(Some(74)));assert_eq!(q.used,0);
        assert_eq!(q.consume(),Ok(None));
        assert_eq!(backing[4096],0xdddd_dddd_dddd_dddd);
    }
    #[test]fn refuse_phys_overflow_and_invalid_hhdm(){
        assert!(direct_map(u64::MAX,0x1000).is_none());
        assert!(direct_map(0xffff800000000000,0).is_none());
        assert!(direct_map(0xffff800000000000,0x1_0000_0000).is_none());
        assert!(direct_map(0xffff800000000000,0x1001).is_none());
        assert!(direct_map(0xffff800000000000,0x1000).is_some());
    }
    #[test]
    fn receive_lengths_reject_short_oversized_and_small_caller_buffers(){
        assert_eq!(receive_payload_len(74,64),Ok(64));
        assert_eq!(receive_payload_len(1524,1514),Ok(1514));
        assert_eq!(receive_payload_len(9,1600),Err(Error::Invalid));
        assert_eq!(receive_payload_len(1525,1600),Err(Error::Invalid));
        assert_eq!(receive_payload_len(110,99),Err(Error::Invalid));
        assert_eq!(receive_payload_len(usize::MAX,usize::MAX),Err(Error::Invalid));
    }

}


// P0-469: DMA used-ring lengths must not exceed submitted transmit bytes.
#[cfg(test)]
mod kernel_p0_batch19_w5_tests {
    use super::*;
    #[test]
    fn kernel_p0_batch19_w5_rejects_forged_tx_completion_lengths(){
        assert!(tx_completion_length_valid(0,14));
        assert!(tx_completion_length_valid(24,14));
        assert!(tx_completion_length_valid(1524,1514));
        assert!(!tx_completion_length_valid(25,14));
        assert!(!tx_completion_length_valid(1525,1514));
        assert!(!tx_completion_length_valid(0,13));
        assert!(!tx_completion_length_valid(0,1515));
        assert!(!tx_completion_length_valid(usize::MAX,1514));
    }
}


// P0-509–514: deny DMA page recycling after a changing PCI identity.
#[cfg(test)]
mod kernel_p0_batch20_w5_tests{
    use super::*;
    #[test]
    fn kernel_p0_batch20_w5_quiesced_nic_requires_stable_device_id(){
        let id=legacy_nic_identity();
        assert!(nic_dma_retired(0,id,id,0x0001));
        assert!(!nic_dma_retired(0xff,id,id,0x0001));
        assert!(!nic_dma_retired(0,id,u32::MAX,0x0001));
        assert!(!nic_dma_retired(0,u32::MAX,id,0x0001));
        assert!(!nic_dma_retired(0,id,id,0x0005));
        assert!(!nic_dma_retired(0,id,id,0));
    }
}


// P0-549: malformed RX packets/offload flags cannot enter the actual
// R028 virtio Ethernet receive path while checksum offload is disabled.
#[cfg(test)]
mod kernel_p0_batch21_w5_tests {
    use super::*;
    #[test]fn kernel_p0_batch21_w5_short_frames_and_offload_flags_denied(){
        assert_eq!(receive_payload_len(23,2048),Err(Error::Invalid));
        assert_eq!(receive_payload_len(24,2048),Ok(14));
        assert_eq!(receive_payload_len(1524,1514),Ok(1514));
        assert!(rx_offload_header_valid(&[0;10]));
        for i in 0..10{
            let mut forged=[0u8;10];forged[i]=1;
            assert!(!rx_offload_header_valid(&forged));
        }
        assert!(!rx_offload_header_valid(&[0;9]));
    }
}


// Batch22 W5 P0-575..577: NIC DMA bytes are filtered by the actual
// VirtioNet::poll ingress path, before user-visible Ethernet buffers.
#[cfg(test)]
mod kernel_p0_batch22_w5_tests{
    use super::*;
    fn ipv4()->[u8;60]{
        let mut f=[0u8;60];
        f[..6].copy_from_slice(&[0x52,0x54,0,0x01,0x02,0x03]);
        f[6..12].copy_from_slice(&[0x52,0x54,0,0xab,0xcd,0xef]);
        f[12..14].copy_from_slice(&[0x08,0x00]);
        f
    }
    #[test]fn kernel_p0_batch22_w5_575_multicast_source_rejected(){
        let mut f=ipv4();assert!(ethernet_ingress_valid(&f));
        f[6]=0x53;assert!(!ethernet_ingress_valid(&f));
        f[6]=0xff;assert!(!ethernet_ingress_valid(&f));
    }
    #[test]fn kernel_p0_batch22_w5_576_zero_source_rejected(){
        let mut f=ipv4();f[6..12].fill(0);
        assert!(!ethernet_ingress_valid(&f));
        assert!(!ethernet_ingress_valid(&f[..13]));
    }
    #[test]fn kernel_p0_batch22_w5_577_length_coded_frames_rejected(){
        let mut f=ipv4();
        f[12..14].copy_from_slice(&[0x05,0xdc]);
        assert!(!ethernet_ingress_valid(&f));
        f[12..14].copy_from_slice(&[0x08,0x06]);
        assert!(ethernet_ingress_valid(&f));
        f[12..14].copy_from_slice(&[0x08,0x00]);
        assert!(ethernet_ingress_valid(&f));
    }
}


// Batch23 W5 P0-598..601: reject forged TX source addresses and unsupported
// frame formats before handing any bytes to the live virtio DMA queue.
#[cfg(test)]
mod kernel_p0_batch23_w5_tests{
 use super::*;
 fn packet()->([u8;60],[u8;6]){
    let mac=[0x52,0x54,0,0xaa,0xbb,0xcc];
    let mut packet=[0u8;60];packet[..6].fill(0xff);
    packet[6..12].copy_from_slice(&mac);
    packet[12..14].copy_from_slice(&[0x08,0x06]);
    (packet,mac)
 }
 #[test]fn kernel_p0_batch23_w5_598_valid_arp_ethernet_passes(){
    let (packet,mac)=packet();
    assert!(ethernet_egress_valid(&packet,mac));
 }
 #[test]fn kernel_p0_batch23_w5_599_spoofed_source_denied(){
    let (mut packet,mac)=packet();packet[11]^=1;
    assert!(!ethernet_egress_valid(&packet,mac));
    assert!(!ethernet_egress_valid(&packet,[0;6]));
 }
 #[test]fn kernel_p0_batch23_w5_600_invalid_destination_denied(){
    let (mut packet,mac)=packet();packet[..6].fill(0);
    assert!(!ethernet_egress_valid(&packet,mac));
    packet[..6].copy_from_slice(&[0x52,0x54,0,1,2,3]);
    assert!(ethernet_egress_valid(&packet,mac));
 }
 #[test]fn kernel_p0_batch23_w5_601_length_coded_short_frame_denied(){
    let (mut packet,mac)=packet();packet[12..14].copy_from_slice(&[5,0xdc]);
    assert!(!ethernet_egress_valid(&packet,mac));
    packet[12..14].copy_from_slice(&[0x08,0x00]);
    assert!(ethernet_egress_valid(&packet,mac));
    assert!(!ethernet_egress_valid(&packet[..13],mac));
 }
}


// Batch24 W5 P0-620..622: actual R028 virtio receive ownership checks
// reject foreign unicast before caller-visible bytes are copied.
#[cfg(test)]
mod kernel_p0_batch24_w5_tests{
    use super::*;
    const LOCAL:[u8;6]=[0x52,0x54,0,0x11,0x22,0x33];
    fn frame()->[u8;60] {
        let mut f=[0u8;60];
        f[..6].copy_from_slice(&LOCAL);
        f[6..12].copy_from_slice(&[0x52,0x54,0,0x44,0x55,0x66]);
        f[12..14].copy_from_slice(&[0x08,0x06]);
        f
    }
    #[test]fn kernel_p0_batch24_w5_620_local_unicast_retained(){
        let f=frame();
        assert!(ethernet_ingress_valid(&f));
        assert!(ethernet_destination_allowed(&f,LOCAL));
    }
    #[test]fn kernel_p0_batch24_w5_621_foreign_unicast_rejected(){
        let mut f=frame();
        f[..6].copy_from_slice(&[0x52,0x54,0,0xaa,0xbb,0xcc]);
        assert!(ethernet_ingress_valid(&f));
        assert!(!ethernet_destination_allowed(&f,LOCAL));
        assert!(!ethernet_destination_allowed(&f,[0;6]));
    }
    #[test]fn kernel_p0_batch24_w5_622_discovery_broadcast_and_multicast_retained(){
        let mut f=frame();f[..6].fill(0xff);
        assert!(ethernet_destination_allowed(&f,LOCAL));
        f[..6].copy_from_slice(&[0x01,0x00,0x5e,0,0,1]);
        assert!(ethernet_destination_allowed(&f,LOCAL));
        assert!(!ethernet_destination_allowed(&f[..13],LOCAL));
    }
}

// P0-646..650: bounded raw virtio input before guest-visible buffers.
#[cfg(test)]
mod kernel_p0_batch25_w5_tests {
    use super::*;
    const LOCAL:[u8;6]=[0x52,0x54,0,1,2,3];
    fn frame()->[u8;60]{
        let mut f=[0u8;60];
        f[..6].copy_from_slice(&LOCAL);
        f[6..12].copy_from_slice(&[0x52,0x54,0,4,5,6]);
        f[12..14].copy_from_slice(&[8,6]);f
    }
    #[test]fn kernel_p0_batch25_w5_646_exact_virtio_header(){
        assert!(rx_offload_header_valid(&[0;10]));
        assert!(!rx_offload_header_valid(&[0;11]));
        assert!(!rx_offload_header_valid(&[0;9]));
    }
    #[test]fn kernel_p0_batch25_w5_647_oversized_frame_denied(){
        let mut packet=[0u8;1515];packet[..14].copy_from_slice(&frame()[..14]);
        assert!(!ethernet_ingress_valid(&packet));
        assert!(!ethernet_destination_allowed(&packet,LOCAL));
    }
    #[test]fn kernel_p0_batch25_w5_648_minimum_ethernet_frame(){
        let sample=frame();
        let short=&sample[..14];
        assert!(ethernet_ingress_valid(short));
        assert!(ethernet_destination_allowed(short,LOCAL));
    }
    #[test]fn kernel_p0_batch25_w5_649_multicast_source_denied(){
        let mut f=frame();f[6]=0x01;
        assert!(!ethernet_ingress_valid(&f));
    }
    #[test]fn kernel_p0_batch25_w5_650_tx_source_identity_enforced(){
        let mut f=frame();
        // TX must carry this NIC's source, not the foreign RX fixture source.
        f[6..12].copy_from_slice(&LOCAL);
        assert!(ethernet_egress_valid(&f,LOCAL));
        f[6]=0x54;
        assert!(!ethernet_egress_valid(&f,LOCAL));
    }
}

// P0-670..672: no VLAN tag is admitted by the untagged-only R028 ABI.
#[cfg(test)]
mod kernel_p0_batch26_w5_tests {
    use super::*;
    const MAC:[u8;6]=[0x52,0x54,0,1,2,3];
    fn frame(ethertype:u16)->[u8;60] {
        let mut f=[0u8;60];f[..6].copy_from_slice(&MAC);
        f[6..12].copy_from_slice(&MAC);
        f[12..14].copy_from_slice(&ethertype.to_be_bytes());f
    }
    #[test]fn kernel_p0_batch26_w5_670_untagged_ipv4_arp_retained(){
        for kind in [0x0800,0x0806] {
            let f=frame(kind);
            assert!(ethernet_ingress_valid(&f));
            assert!(ethernet_egress_valid(&f,MAC));
        }
    }
    #[test]fn kernel_p0_batch26_w5_671_vlan_encapsulation_denied_on_rx(){
        for tag in [0x8100,0x88a8,0x9100] {
            assert!(!ethernet_ingress_valid(&frame(tag)));
        }
    }
    #[test]fn kernel_p0_batch26_w5_672_vlan_encapsulation_denied_on_tx(){
        for tag in [0x8100,0x88a8,0x9100] {
            assert!(!ethernet_egress_valid(&frame(tag),MAC));
        }
    }
}

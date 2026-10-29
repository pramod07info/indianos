//! R028 QEMU-only legacy virtio-net PCI driver, single CPU and IRQs masked.
//!
//! The transport uses 2 contiguous 3-page DMA vrings, one receive and one
//! transmit packet buffer. No interrupt/shared IRQs, multiqueue, mergeable
//! buffers, checksum offload, or modern virtio PCI capability negotiation.
//! The device MUST be an emulated 0x1af4:0x1000 virtio-net legacy PCI card.
//! Never enable this experimental DMA driver in ordinary machine boot.
use core::{arch::asm,ptr,sync::atomic::{compiler_fence,Ordering}};
use crate::network::{EthernetDevice,Error};

const PAGE:usize=4096;
const QSIZE:u16=256;
const RING_FRAMES:u32=3;
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
struct Queue{
    ring_phys:u64,
    ring:*mut u8,
    buffer_phys:u64,
    buffer:*mut u8,
    queued:u16,
    used:u16,
}
impl Queue {
    fn allocate(hhdm:u64)->Result<Self,Error>{
        let mut ring_phys=0u64;
        if !unsafe{pmm_allocate_contiguous_frames(RING_FRAMES,&mut ring_phys)}{
            return Err(Error::Device);
        }
        let mut bufphys=0u64;
        if !unsafe{pmm_allocate_frame(&mut bufphys)}{
            for n in 0..RING_FRAMES{
                let _=unsafe{pmm_release_frame(ring_phys+n as u64*PAGE as u64)};
            }
            return Err(Error::Device);
        }
        let (Some(ring),Some(buffer))=(direct_map(hhdm,ring_phys),direct_map(hhdm,bufphys))
        else{
            let _=unsafe{pmm_release_frame(bufphys)};
            for n in 0..RING_FRAMES{
                let _=unsafe{pmm_release_frame(ring_phys+n as u64*PAGE as u64)};
            }
            return Err(Error::Device);
        };
        unsafe{
            ptr::write_bytes(ring,0,RING_FRAMES as usize*PAGE);
            ptr::write_bytes(buffer,0,PAGE);
        }
        Ok(Self{ring_phys,ring,buffer_phys:bufphys,buffer,queued:0,used:0})
    }
    fn descriptor(&mut self,size:u32,device_writes:bool){
        // Descriptor table occupies ring page 0; index 0 is the only
        // submitted descriptor, all other 255 are intentionally unused.
        unsafe{
            ptr::write_volatile(self.ring.cast::<u64>(),self.buffer_phys);
            ptr::write_volatile(self.ring.add(8).cast::<u32>(),size);
            ptr::write_volatile(self.ring.add(12).cast::<u16>(),
                if device_writes{DESC_WRITE}else{0});
            ptr::write_volatile(self.ring.add(14).cast::<u16>(),0);
        }
    }
    fn offer(&mut self){
        let slot=(self.queued as usize%QSIZE as usize)*2;
        unsafe{
            ptr::write_volatile(self.ring.add(PAGE+4+slot).cast::<u16>(),0);
        }
        compiler_fence(Ordering::Release);
        self.queued=self.queued.wrapping_add(1);
        unsafe{ptr::write_volatile(self.ring.add(PAGE+2).cast::<u16>(),self.queued);}
    }
    fn complete(&self)->bool{
        let idx=unsafe{ptr::read_volatile(self.ring.add(PAGE*2+2).cast::<u16>())};
        compiler_fence(Ordering::Acquire);
        idx!=self.used
    }
    fn consume(&mut self)->Result<Option<usize>,Error>{
        let idx=unsafe{ptr::read_volatile(self.ring.add(PAGE*2+2).cast::<u16>())};
        // Observe the device's publication before reading its used element
        // and DMA bytes. x86 coherent DMA supplies the hardware ordering.
        compiler_fence(Ordering::Acquire);
        if idx==self.used{return Ok(None);}
        // Only descriptor zero is ever outstanding. A jump, rewind, replay
        // or unsolicited completion cannot transfer buffer ownership back.
        if self.queued.wrapping_sub(self.used)!=1 ||
           idx.wrapping_sub(self.used)!=1{return Err(Error::Device);}
        let pos=(self.used as usize%QSIZE as usize)*8;
        let id=unsafe{ptr::read_volatile(self.ring.add(PAGE*2+4+pos).cast::<u32>())};
        let size=unsafe{ptr::read_volatile(self.ring.add(PAGE*2+8+pos).cast::<u32>())};
        if id!=0 || size as usize>FRAME_CAP{return Err(Error::Device);}
        self.used=self.used.wrapping_add(1);
        Ok(Some(size as usize))
    }
    fn release(self){
        let _=unsafe{pmm_release_frame(self.buffer_phys)};
        for n in 0..RING_FRAMES{
            let _=unsafe{pmm_release_frame(self.ring_phys+n as u64*PAGE as u64)};
        }
    }
}
fn configure_queue(port:u16,queue:u16,q:&Queue)->bool{
    unsafe{outw(port+0x0e,queue);}
    if unsafe{inw(port+0x0c)}!=QSIZE ||
       q.ring_phys/PAGE as u64>u32::MAX as u64{
        return false;
    }
    unsafe{outl(port+0x08,(q.ring_phys/PAGE as u64) as u32);}
    true
}
pub struct VirtioNet{
    port:u16,
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
        let Some((_,_,_,port))=discover()else{return Err(Error::Device);};
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
        let mut rx=Queue::allocate(hhdm)?;
        let mut tx=match Queue::allocate(hhdm){
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
        Ok(Self{port,mac,rx,tx,faulted:false})
    }
    pub fn close(self){
        // Quiesce all DMA before releasing any physical frames to PMM.
        unsafe{outb(self.port+0x12,0);}
        compiler_fence(Ordering::SeqCst);
        self.rx.release();self.tx.release();
    }
}
/// Validate used length without untrusted DMA length subtraction/overflow.
fn receive_payload_len(used_len:usize,capacity:usize)->Result<usize,Error>{
    if used_len<10 || used_len>1514+10 {return Err(Error::Invalid);}
    let payload=used_len-10;
    if payload>capacity {return Err(Error::Invalid);}
    Ok(payload)
}
impl EthernetDevice for VirtioNet{
    fn mac(&self)->[u8;6]{self.mac}
    fn send(&mut self,frame:&[u8])->Result<(),Error>{
        if self.faulted{return Err(Error::Device);}
        if frame.len()<14 || frame.len()>1514{return Err(Error::Invalid);}
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
                if !matches!(self.tx.consume(),Ok(Some(_))){
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
        unsafe{ptr::copy_nonoverlapping(self.rx.buffer.add(10),out.as_mut_ptr(),n);}
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
            buffer_phys:0x4000,buffer:buffer.as_mut_ptr().cast(),queued:0,used:0}
    }
    fn completion(q:&Queue,index:u16,id:u32,len:u32){
        let pos=q.used as usize%QSIZE as usize*8;
        unsafe{
            ptr::write_volatile(q.ring.add(PAGE*2+4+pos).cast::<u32>(),id);
            ptr::write_volatile(q.ring.add(PAGE*2+8+pos).cast::<u32>(),len);
            ptr::write_volatile(q.ring.add(PAGE*2+2).cast::<u16>(),index);
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
        let mut dev=VirtioNet{port:0,mac:[2;6],rx,
            tx:mock_queue(&mut tr,&mut tb),faulted:false};
        let mut out=[0xaa;100];
        assert_eq!(dev.poll(&mut out),Err(Error::Device));
        assert!(dev.faulted);assert_eq!(out,[0xaa;100]);
        assert_eq!(dev.rx.queued,1);assert_eq!(dev.rx.used,0);
        assert_eq!(dev.send(&[0;60]),Err(Error::Device));
        assert_eq!(dev.poll(&mut out),Err(Error::Device));
    }
    #[test]fn dma_allocation_counts_and_ring_layout(){
        assert_eq!(QSIZE,256);
        assert_eq!(RING_FRAMES as usize*PAGE,12288);
        assert_eq!(PAGE+4+2*(QSIZE as usize-1)+2,4612);
        assert!(PAGE+4+2*(QSIZE as usize-1)<PAGE*2);
        assert!(PAGE*2+4+8*(QSIZE as usize-1)+8<3*PAGE);
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

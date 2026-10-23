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
        compiler_fence(Ordering::Acquire);
        let idx=unsafe{ptr::read_volatile(self.ring.add(PAGE*2+2).cast::<u16>())};
        idx!=self.used
    }
    fn consume(&mut self)->Option<usize>{
        if !self.complete(){return None;}
        let pos=(self.used as usize%QSIZE as usize)*8;
        let id=unsafe{ptr::read_volatile(self.ring.add(PAGE*2+4+pos).cast::<u32>())};
        let size=unsafe{ptr::read_volatile(self.ring.add(PAGE*2+8+pos).cast::<u32>())};
        self.used=self.used.wrapping_add(1);
        if id!=0 || size as usize>FRAME_CAP{return None;}
        Some(size as usize)
    }
    fn release(self){
        let _=unsafe{pmm_release_frame(self.buffer_phys)};
        for n in 0..RING_FRAMES{
            let _=unsafe{pmm_release_frame(self.ring_phys+n as u64*PAGE as u64)};
        }
    }
}
pub struct VirtioNet{
    port:u16,
    mac:[u8;6],
    rx:Queue,
    tx:Queue,
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
        for (index,q) in [(QUEUE_RX,&mut rx),(QUEUE_TX,&mut tx)]{
            unsafe{outw(port+0x0e,index);}
            if unsafe{inw(port+0x0c)}!=QSIZE {
                unsafe{outb(port+0x12,0);}
                rx.release();tx.release();return Err(Error::Device);
            }
            if q.ring_phys/PAGE as u64>u32::MAX as u64{
                unsafe{outb(port+0x12,0);}
                rx.release();tx.release();return Err(Error::Device);
            }
            unsafe{outl(port+0x08,(q.ring_phys/PAGE as u64) as u32);}
        }
        rx.descriptor(FRAME_CAP as u32,true);
        rx.offer();
        unsafe{outb(port+0x12,DEVICE_ACK|DRIVER|DRIVER_OK);}
        compiler_fence(Ordering::SeqCst);
        unsafe{outw(port+0x10,QUEUE_RX);}
        Ok(Self{port,mac,rx,tx})
    }
    pub fn close(self){
        // Quiesce all DMA before releasing any physical frames to PMM.
        unsafe{outb(self.port+0x12,0);}
        compiler_fence(Ordering::SeqCst);
        self.rx.release();self.tx.release();
    }
}
impl EthernetDevice for VirtioNet{
    fn mac(&self)->[u8;6]{self.mac}
    fn send(&mut self,frame:&[u8])->Result<(),Error>{
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
                if self.tx.consume().is_none(){return Err(Error::Device);}
                return Ok(());
            }
            core::hint::spin_loop();
        }
        Err(Error::Timeout)
    }
    fn poll(&mut self,out:&mut [u8])->Result<Option<usize>,Error>{
        if !self.rx.complete(){return Ok(None);}
        let Some(length)=self.rx.consume()else{return Err(Error::Device);};
        if length<10 || length>1514+10 || out.len()<length-10{
            return Err(Error::Invalid);
        }
        let n=length-10;
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
    #[test]fn dma_allocation_counts_and_ring_layout(){
        assert_eq!(QSIZE,256);
        assert_eq!(RING_FRAMES as usize*PAGE,12288);
        assert_eq!(PAGE+4+2*(QSIZE as usize-1)+2,4614);
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
}

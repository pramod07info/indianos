//! R029 test-only polling PS/2 auxiliary mouse for one BSP, IRQs masked.
//!
//! We intentionally do NOT enable an IRQ12 handler or share the keyboard's
//! interrupt-driven driver. No normal boot touches the PS/2 auxiliary port.
use core::arch::asm;
const CTRL:u16=0x64;
const DATA:u16=0x60;
const MAX_WAIT:usize=150_000;
#[inline]unsafe fn input(port:u16)->u8{
    let val:u8;unsafe{asm!("in al,dx",in("dx")port,out("al")val,
        options(nomem,nostack,preserves_flags));}val
}
#[inline]unsafe fn output(port:u16,value:u8){
    unsafe{asm!("out dx,al",in("dx")port,in("al")value,
        options(nomem,nostack,preserves_flags));}
}
fn writable()->bool{
    for _ in 0..MAX_WAIT{if unsafe{input(CTRL)}&2==0{return true;}}
    false
}
fn command(byte:u8)->bool{
    if !writable(){return false;}
    unsafe{output(CTRL,byte);}true
}
fn payload(byte:u8)->bool{
    if !writable(){return false;}
    unsafe{output(DATA,byte);}true
}
/// Controller configuration replies use the primary data channel.
fn controller_reply_ready(status:u8)->bool{
    status&0x01!=0&&status&0xe0==0
}
fn read_reply()->Option<u8>{
    for _ in 0..MAX_WAIT{
        let status=unsafe{input(CTRL)};
        if status&1!=0{
            // Never consume an auxiliary byte as controller config, and do
            // not consume a corrupted transfer.
            if !controller_reply_ready(status){return None;}
            return Some(unsafe{input(DATA)});
        }
    }
    None
}
/// Mouse ACK must arrive from the AUX channel, never a keyboard byte
/// whose value happens to be 0xFA. A pending primary transfer belongs to
/// the keyboard driver and is left untouched.
fn mouse_ack()->bool{
    for _ in 0..MAX_WAIT{
        let status=unsafe{input(CTRL)};
        if status&1!=0{
            if !valid_mouse_status(status){return false;}
            return unsafe{input(DATA)}==0xfa;
        }
    }
    false
}
fn mouse_command(byte:u8)->bool{
    if !command(0xd4) || !payload(byte){return false;}
    mouse_ack()
}
/// PS/2 OBF must belong to the auxiliary mouse and have no controller
/// parity/timeout flag. The keyboard broker owns primary-port bytes.
fn valid_mouse_status(status:u8)->bool{
    status&0x21==0x21&&status&0xc0==0
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(crate) struct Packet{pub dx:i16,pub dy:i16,pub left:bool}
pub(crate) struct Mouse {
    packet:[u8;3],received:usize,last_left:bool
}
impl Mouse{
    pub(crate) fn init()->Option<Self>{
        // Drain firmware leftovers. Mouse IRQ remains disabled; no conflict
        // with the PIC or the APIC IRQ route used by other test configurations.
        for _ in 0..64{
            if unsafe{input(CTRL)}&1==0{break;}
            let _=unsafe{input(DATA)};
        }
        if !command(0xa8) || !command(0x20){return None;}
        let config=read_reply()?;
        if !command(0x60) || !payload((config&!0x22)|0x00){return None;}
        if !mouse_command(0xf6)||!mouse_command(0xf4){return None;}
        Some(Self{packet:[0;3],received:0,last_left:false})
    }
    pub(crate) fn poll(&mut self)->Option<Packet>{
        for _ in 0..3{
            let status=unsafe{input(CTRL)};
            // Never steal primary keyboard bytes; they must reach the
            // independent Set-1 driver even while a partial mouse packet
            // is queued. Discard and resync only broken AUX transfers.
            if status&1==0||status&0x20==0{return None;}
            let byte=unsafe{input(DATA)};
            if !valid_mouse_status(status){
                self.packet=[0;3];
                self.received=0;
                return None;
            }
            if let Some(packet)=self.feed(byte){return Some(packet);}
        }
        None
    }
    fn feed(&mut self,byte:u8)->Option<Packet>{
        if self.received==0 && byte&8==0{return None;}
        self.packet[self.received]=byte;
        self.received+=1;
        if self.received<3{return None;}
        self.received=0;
        let [flags,x,y]=self.packet;
        if flags&0xc0!=0{return None;} // overflow: ignore entire packet.
        let dx=if flags&0x10!=0{(x as i16)-256}else{x as i16};
        let dy=if flags&0x20!=0{(y as i16)-256}else{y as i16};
        let left=flags&1!=0;
        self.last_left=left;
        Some(Packet{dx,dy,left})
    }
    pub(crate) fn disable(&self){let _=mouse_command(0xf5);}
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn m1_r047_primary_keyboard_status_never_claimed_by_mouse(){
        for status in [0x00,0x01,0x03,0x11,0x80,0x81,0xff]{
            assert!(!valid_mouse_status(status));
        }
        for status in [0x21,0x23,0x31]{
            assert!(valid_mouse_status(status));
        }
    }
    #[test]fn m1_r047_corrupt_aux_status_is_rejected_before_packet_decode(){
        for status in [0x61,0xa1,0xe1]{
            assert!(!valid_mouse_status(status));
        }
    }
    #[test]fn m1_r047_mouse_out_of_sync_bytes_cannot_forge_a_click(){
        let mut mouse=Mouse{packet:[0;3],received:0,last_left:false};
        assert_eq!(mouse.feed(0x01),None);
        assert_eq!(mouse.feed(0x09),None);
        assert_eq!(mouse.feed(0),None);
        assert_eq!(mouse.feed(0),Some(Packet{dx:0,dy:0,left:true}));
        assert_eq!(mouse.feed(0xc9),None); // invalid overflow header
        assert_eq!(mouse.feed(0),None);
        assert_eq!(mouse.feed(0),None);
        assert_eq!(mouse.feed(0x08),None);
        assert_eq!(mouse.feed(1),None);
        assert_eq!(mouse.feed(2),Some(Packet{dx:1,dy:2,left:false}));
    }
    #[test]fn m1_r053_mouse_ack_never_consumes_primary_keyboard_status(){
        for status in [0x00,0x01,0x03,0x11,0x61,0x81,0xa1,0xe1]{
            assert!(!valid_mouse_status(status));
        }
        for status in [0x21,0x23,0x31]{
            assert!(valid_mouse_status(status));
        }
        assert!(controller_reply_ready(0x01));
        assert!(!controller_reply_ready(0x21));
        assert!(!controller_reply_ready(0x81));
    }
    #[test]fn m1_r053_controller_and_mouse_ack_channels_are_disjoint(){
        for status in 0u8..=u8::MAX{
            assert!(!(controller_reply_ready(status)&&valid_mouse_status(status)),
                "overlapping PS2 ownership at {status:#x}");
        }
        assert!(controller_reply_ready(0x11));
        assert!(valid_mouse_status(0x31));
    }
    #[test]fn packet_sync_buttons_and_signed_axes(){
        let mut m=Mouse{packet:[0;3],received:0,last_left:false};
        assert_eq!(m.feed(0),None);
        assert_eq!(m.feed(0x09),None);
        assert_eq!(m.feed(5),None);
        assert_eq!(m.feed(3),Some(Packet{dx:5,dy:3,left:true}));
        assert_eq!(m.feed(0x38),None);
        assert_eq!(m.feed(0xfb),None);
        assert_eq!(m.feed(0xf6),Some(Packet{dx:-5,dy:-10,left:false}));
        assert_eq!(m.feed(0x48),None);
        assert_eq!(m.feed(1),None);
        assert_eq!(m.feed(1),None);
    }
}

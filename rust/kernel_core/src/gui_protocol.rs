//! R029: strict wire ABI for the experimental ring-3 graphics client.
//!
//! Kernel owns the framebuffer; userspace may submit only a bounded drawing
//! command, never a pixel pointer or a physical MMIO address.
pub const WIRE_BYTES:usize=64;
pub const COMMAND_LIMIT:usize=8;
pub const FILL:u32=1;
pub const TEXT:u32=2;
pub const MAX_TEXT:usize=24;
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum Error{Version,Operation,Window,Bounds,Color,Text,Reserved}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct Command{
    pub window:u32,pub operation:u32,pub x:u16,pub y:u16,
    pub width:u16,pub height:u16,pub rgb:u32,
    pub text:[u8;32],pub text_len:u8,
}
impl Command{
    pub const EMPTY:Self=Self{window:0,operation:0,x:0,y:0,
        width:0,height:0,rgb:0,text:[0;32],text_len:0};
    /// All bytes are copied from a validated userspace mapping first.
    pub fn parse(data:&[u8;WIRE_BYTES])->Result<Self,Error>{
        if &data[..4]!=b"R29U" || u32::from_le_bytes(data[4..8].try_into().unwrap())!=1{
            return Err(Error::Version);
        }
        let get32=|off:usize|u32::from_le_bytes(data[off..off+4].try_into().unwrap());
        let get16=|off:usize|u16::from_le_bytes(data[off..off+2].try_into().unwrap());
        let window=get32(8);let operation=get32(12);
        if window!=1 && window!=2{return Err(Error::Window);}
        if operation!=FILL && operation!=TEXT{return Err(Error::Operation);}
        if data[28..32]!=[0;4]{return Err(Error::Reserved);}
        let x=get16(16);let y=get16(18);
        let width=get16(20);let height=get16(22);
        let rgb=get32(24);
        if rgb>0x00ff_ffff{return Err(Error::Color);}
        if x>1200||y>1000||width==0||height==0||
           (x as u32+width as u32)>1200||
           (y as u32+height as u32)>1000{return Err(Error::Bounds);}
        let mut text=[0u8;32];text.copy_from_slice(&data[32..64]);
        let text_len=if operation==FILL{
            if text.iter().any(|&b|b!=0){return Err(Error::Text);}
            0
        }else{
            let Some(len)=text.iter().position(|&b|b==0)else{return Err(Error::Text);};
            if len==0||len>MAX_TEXT||
               text[len..].iter().any(|&b|b!=0)||
               text[..len].iter().any(|&b|!(0x20..=0x7e).contains(&b)){
                return Err(Error::Text);
            }
            if (width as usize)<(len*12)||height<14{return Err(Error::Bounds);}
            len as u8
        };
        Ok(Self{window,operation,x,y,width,height,rgb,text,text_len})
    }
    pub fn within_client(self,client_width:u32,client_height:u32)->bool{
        self.x as u32+ self.width as u32<=client_width &&
        self.y as u32+ self.height as u32<=client_height
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    fn command(window:u32,operation:u32)->[u8;WIRE_BYTES]{
        let mut a=[0u8;WIRE_BYTES];
        a[..4].copy_from_slice(b"R29U");a[4..8].copy_from_slice(&1u32.to_le_bytes());
        a[8..12].copy_from_slice(&window.to_le_bytes());
        a[12..16].copy_from_slice(&operation.to_le_bytes());
        a[16..18].copy_from_slice(&9u16.to_le_bytes());
        a[18..20].copy_from_slice(&10u16.to_le_bytes());
        a[20..22].copy_from_slice(&100u16.to_le_bytes());
        a[22..24].copy_from_slice(&24u16.to_le_bytes());
        a[24..28].copy_from_slice(&0x00abcc11u32.to_le_bytes());a
    }
    #[test]fn trusted_client_rectangles_and_text(){
        let f=Command::parse(&command(1,FILL)).unwrap();
        assert!(f.within_client(300,250));
        assert!(!f.within_client(100,250));
        let mut t=command(2,TEXT);
        t[32..37].copy_from_slice(b"HELLO");
        let p=Command::parse(&t).unwrap();
        assert_eq!(&p.text[..p.text_len as usize],b"HELLO");
    }
    #[test]fn reject_malformed_unbounded_and_fake_pointers(){
        let mut a=command(1,FILL);
        for bad in [0,3,u32::MAX]{
            a[8..12].copy_from_slice(&bad.to_le_bytes());
            assert_eq!(Command::parse(&a),Err(Error::Window));
        }
        let mut a=command(1,FILL);
        a[28]=1;assert_eq!(Command::parse(&a),Err(Error::Reserved));
        a[28]=0;a[24..28].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(Command::parse(&a),Err(Error::Color));
        let mut a=command(1,FILL);
        a[16..18].copy_from_slice(&u16::MAX.to_le_bytes());
        assert_eq!(Command::parse(&a),Err(Error::Bounds));
        let mut a=command(2,TEXT);
        a[32..64].fill(b'A');
        assert_eq!(Command::parse(&a),Err(Error::Text));
        let mut a=command(1,FILL);
        a[63]=1;assert_eq!(Command::parse(&a),Err(Error::Text));
    }
    #[test]fn untrusted_guest_cannot_reach_window_titlebars(){
        let good=Command::parse(&command(1,FILL)).unwrap();
        assert!(good.within_client(320,230));
        assert!(!good.within_client(50,230));
        let mut a=command(2,FILL);
        a[18..20].copy_from_slice(&300u16.to_le_bytes());
        let invalid=Command::parse(&a).unwrap();
        assert!(!invalid.within_client(320,230));
    }
}

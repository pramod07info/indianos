#[path="../../../rust/kernel_core/src/gui_protocol.rs"]
mod gui_protocol;
#[path="../../../rust/kernel_core/src/pixel_protocol.rs"]
mod pixel_protocol;
#[path="../src/desktop_pixels.rs"]
mod raster;
use raster::State;
use gui_protocol::{FILL,TEXT,REPLACE_TEXT};
fn command(window:u32,operation:u32,x:u16,y:u16,w:u16,h:u16,color:u32,text:&[u8])->[u8;64]{
 let mut b=[0u8;64];b[..4].copy_from_slice(b"R29U");b[4..8].copy_from_slice(&1u32.to_le_bytes());
 b[8..12].copy_from_slice(&window.to_le_bytes());b[12..16].copy_from_slice(&operation.to_le_bytes());
 for (at,value) in [(16,x),(18,y),(20,w),(22,h)]{b[at..at+2].copy_from_slice(&value.to_le_bytes());}
 b[24..28].copy_from_slice(&color.to_le_bytes());b[32..32+text.len()].copy_from_slice(text);b
}
#[test]fn app_rows_retain_fill_and_replace_short_text_without_stale_glyphs(){
 let mut state=State::new(1);
 let fill=command(1,FILL,0,0,60,20,0x123456,b"");
 assert!(state.prepare(&fill).is_some());
 let text=command(1,TEXT,0,0,48,14,0xffffff,b"LONG");
 assert!(state.prepare(&text).is_some());
 let mut old=[0;60];assert!(state.row(0,0,&mut old));
 assert!(old[..48].iter().any(|&p|p==0xffff_ffff));
 let replace=command(1,REPLACE_TEXT,0,0,48,14,0xffffff,b"A");
 assert!(state.prepare(&replace).is_some());
 let mut row=[0;60];assert!(state.row(0,0,&mut row));
 assert!(row[12..].iter().all(|&p|p==0xff12_3456));
 assert!(row[..12].iter().any(|&p|p==0xffff_ffff));
}
#[test]fn app_models_are_private_and_invalid_batches_preserve_old_pixels(){
 let mut first=State::new(1);let mut second=State::new(2);
 assert!(first.prepare(&command(1,FILL,2,2,3,3,0x112233,b"")).is_some());
 assert!(second.prepare(&command(2,FILL,2,2,3,3,0x445566,b"")).is_some());
 let mut a=[0;8];let mut b=[0;8];
 assert!(first.row(0,2,&mut a));assert!(second.row(0,2,&mut b));
 assert_eq!(a[0],0);assert_eq!(a[2],0xff11_2233);assert_eq!(b[2],0xff44_5566);
 for bad in [command(2,FILL,0,0,1,1,0,b""),command(1,FILL,767,0,2,1,0,b""),
             command(1,REPLACE_TEXT,0,0,12,14,0,b"X")]{assert!(first.prepare(&bad).is_none());}
 let mut after=[0;8];assert!(first.row(0,2,&mut after));assert_eq!(after,a);
 assert!(!first.row(768,0,&mut [0]));assert!(!first.row(0,768,&mut [0]));
 assert!(!first.row(0,0,&mut [0;257]));assert!(!first.row(0,0,&mut []));
}

//! Private retained app raster planes. Window placement stays in PID4;
//! this bounded kernel backend never gives a client framebuffer authority.
use crate::gui_protocol::{Command,COMMAND_LIMIT,FILL,TEXT};
const PAGE:usize=4096;
pub(super) const MAX_WIDTH:usize=768;
pub(super) const MAX_HEIGHT:usize=768;
const PIXELS:usize=MAX_WIDTH*MAX_HEIGHT;
const MASK_WORDS:usize=PIXELS/64;

// These allocations are pinned for the boot and never copied or assigned
// after guards are removed. The payload size is exactly 594 complete pages.
#[cfg(not(test))]
#[repr(C,align(4096))]
struct GuardedPixels{
    lower:[u8;PAGE],pixels:[u32;PIXELS],coverage:[u64;MASK_WORDS],upper:[u8;PAGE],
}
#[cfg(not(test))]
impl GuardedPixels{
    const fn new()->Self{Self{lower:[0;PAGE],pixels:[0;PIXELS],
        coverage:[0;MASK_WORDS],upper:[0;PAGE]}}
}
#[cfg(not(test))]
static mut PLANES:[GuardedPixels;2]=[GuardedPixels::new(),GuardedPixels::new()];
#[derive(Clone,Copy)]
struct RasterIdentity{
    valid:bool,width:usize,height:usize,count:usize,format:usize,
    commands:[Command;COMMAND_LIMIT],
}
impl RasterIdentity{
    const fn new()->Self{Self{valid:false,width:0,height:0,count:0,format:0,
        commands:[Command::EMPTY;COMMAND_LIMIT]}}
}
#[cfg(not(test))]
static mut IDENTITIES:[RasterIdentity;2]=[RasterIdentity::new();2];
#[cfg(test)]
struct TestPlane{pixels:std::vec::Vec<u32>,coverage:std::vec::Vec<u64>,identity:RasterIdentity}
#[cfg(test)]
impl TestPlane{
    const fn new()->Self{Self{pixels:std::vec::Vec::new(),coverage:std::vec::Vec::new(),
        identity:RasterIdentity::new()}}
}
/// Production is a handle to the two boot-pinned stores. Host fixtures own
/// independent heap buffers, avoiding test races and megabyte stack copies.
pub(super) struct PixelCache{
    #[cfg(test)] planes:core::cell::RefCell<[TestPlane;2]>,
}
impl PixelCache{
    pub(super) const fn new()->Self{Self{
        #[cfg(test)] planes:core::cell::RefCell::new([TestPlane::new(),TestPlane::new()]),
    }}
    pub(super) fn clear_slot(&self,slot:usize){
        if slot>=2{return;}
        #[cfg(not(test))]
        unsafe{
            let plane=&mut (*core::ptr::addr_of_mut!(PLANES))[slot];
            plane.pixels.fill(0);plane.coverage.fill(0);
            (*core::ptr::addr_of_mut!(IDENTITIES))[slot]=RasterIdentity::new();
        }
        #[cfg(test)]
        {
            let mut planes=self.planes.borrow_mut();let plane=&mut planes[slot];
            plane.pixels.fill(0);plane.coverage.fill(0);plane.identity=RasterIdentity::new();
        }
    }
    pub(super) fn clear(&self){for slot in 0..2{self.clear_slot(slot);}}
    pub(super) fn blit(&self,slot:usize,width:usize,height:usize,commands:&[Command],
        rgb:fn(u8,u8,u8)->Option<u32>,pixel:impl FnMut(usize,usize,u32))->bool{
        if slot>=2||width==0||height==0||width>MAX_WIDTH||height>MAX_HEIGHT||
           commands.len()>COMMAND_LIMIT{return false;}
        #[cfg(not(test))]
        unsafe{
            let plane=&mut (*core::ptr::addr_of_mut!(PLANES))[slot];
            let identity=&mut (*core::ptr::addr_of_mut!(IDENTITIES))[slot];
            raster_and_blit(&mut plane.pixels,&mut plane.coverage,identity,
                width,height,commands,rgb,pixel)
        }
        #[cfg(test)]
        {
            let mut planes=self.planes.borrow_mut();let plane=&mut planes[slot];
            let count=width*height;
            if plane.pixels.len()!=count{
                plane.pixels.resize(count,0);plane.coverage.resize(count.div_ceil(64),0);
                plane.identity.valid=false;
            }
            raster_and_blit(&mut plane.pixels,&mut plane.coverage,&mut plane.identity,
                width,height,commands,rgb,pixel)
        }
    }
}

#[cfg(not(test))]
pub(super) fn guard_layout()->[(u64,u64,u64,u64);2]{
    core::array::from_fn(|slot|unsafe{
        let plane=&(*core::ptr::addr_of!(PLANES))[slot];
        (core::ptr::addr_of!(plane.lower) as u64,core::ptr::addr_of!(plane.pixels) as u64,
         core::ptr::addr_of!(plane.upper) as u64,core::ptr::addr_of!(plane.upper) as u64)
    })
}
// Host mapping tests use the runtime's geometry validator rather than a
// hardware-backed singleton. No host address is presented as a real guard.
#[cfg(test)]
pub(super) fn guard_layout()->[(u64,u64,u64,u64);2]{[(0,0,0,0);2]}

fn raster_and_blit(pixels:&mut[u32],coverage:&mut[u64],identity:&mut RasterIdentity,
    width:usize,height:usize,commands:&[Command],rgb:fn(u8,u8,u8)->Option<u32>,
    mut pixel:impl FnMut(usize,usize,u32))->bool{
    let count=width*height;
    if pixels.len()<count||coverage.len()<count.div_ceil(64){return false;}
    let cached=identity.valid&&identity.width==width&&identity.height==height&&
        identity.format==rgb as usize&&identity.count==commands.len()&&
        identity.commands[..commands.len()]==*commands;
    if !cached{
        // Preflight the whole batch before changing a previously good plane.
        let mut colors=[0;COMMAND_LIMIT];
        for (index,command) in commands.iter().enumerate(){
            if !command.within_client(width as u32,height as u32)||command.rgb>0xffffff||
               !matches!(command.operation,FILL|TEXT)||command.text_len as usize>command.text.len(){return false;}
            if command.operation==TEXT&&(command.text_len==0||
                command.text_len as u32*12>command.width as u32||command.height<14){return false;}
            let Some(color)=rgb((command.rgb>>16) as u8,(command.rgb>>8) as u8,command.rgb as u8)
                else{return false;};
            colors[index]=color;
        }
        pixels.fill(0);coverage.fill(0);
        for (command,color) in commands.iter().zip(colors){
            let x=command.x as usize;let y=command.y as usize;
            if command.operation==FILL{
                for cy in y..y+command.height as usize{
                    for cx in x..x+command.width as usize{put(pixels,coverage,width,cx,cy,color);}
                }
            }else{
                for (index,&ch) in command.text[..command.text_len as usize].iter().enumerate(){
                    for (row,bits) in crate::console::glyph(ch).into_iter().enumerate(){
                        for bit in 0..5{
                            if bits&(1<<(4-bit))!=0{
                                for dy in 0..2{for dx in 0..2{
                                    put(pixels,coverage,width,x+index*12+bit*2+dx,y+row*2+dy,color);
                                }}
                            }
                        }
                    }
                }
            }
        }
        identity.valid=true;identity.width=width;identity.height=height;
        identity.count=commands.len();identity.format=rgb as usize;
        identity.commands.fill(Command::EMPTY);
        identity.commands[..commands.len()].copy_from_slice(commands);
    }
    for (index,&color) in pixels[..count].iter().enumerate(){
        if coverage[index/64]&(1u64<<(index%64))!=0{pixel(index%width,index/width,color);}
    }
    true
}
fn put(pixels:&mut[u32],coverage:&mut[u64],width:usize,x:usize,y:usize,color:u32){
    let index=y*width+x;pixels[index]=color;coverage[index/64]|=1u64<<(index%64);
}

#[cfg(test)]
mod tests{
    use super::*;
    fn rgb(r:u8,g:u8,b:u8)->Option<u32>{Some((r as u32)<<16|(g as u32)<<8|b as u32)}
    fn fill(color:u32)->Command{Command{window:1,operation:FILL,x:1,y:1,width:2,height:2,
        rgb:color,text:[0;32],text_len:0}}
    #[test]fn real_planes_isolate_pixels_and_preserve_transparent_chrome(){
        let cache=PixelCache::new();let mut first=[9;16];let mut second=[9;16];
        assert!(cache.blit(0,4,4,&[fill(0x123456)],rgb,|x,y,p|first[y*4+x]=p));
        assert!(cache.blit(1,4,4,&[fill(0x654321)],rgb,|x,y,p|second[y*4+x]=p));
        assert_eq!(first[0],9);assert_eq!(first[5],0x123456);assert_eq!(second[5],0x654321);
        cache.clear_slot(0);
        let planes=cache.planes.borrow();
        assert!(planes[0].pixels.iter().all(|&p|p==0));
        assert!(planes[0].coverage.iter().all(|&p|p==0));
        assert_eq!(planes[1].pixels[5],0x654321);
    }
    #[test]fn failed_batches_preserve_pixels_and_replacement_removes_stale_glyphs(){
        let cache=PixelCache::new();let good=fill(7);
        assert!(cache.blit(0,4,4,&[good],rgb,|_,_,_|{}));
        let mut invalid=good;invalid.x=4;
        assert!(!cache.blit(0,4,4,&[invalid],rgb,|_,_,_|panic!("invalid batch blitted")));
        assert_eq!(cache.planes.borrow()[0].pixels[5],7);
        assert!(!cache.blit(2,4,4,&[],rgb,|_,_,_|{}));
        assert!(!cache.blit(0,MAX_WIDTH+1,4,&[],rgb,|_,_,_|{}));
        assert!(cache.blit(0,4,4,&[],rgb,|_,_,_|panic!("old pixel survived replacement")));
        assert!(cache.planes.borrow()[0].coverage.iter().all(|&p|p==0));
        cache.clear();assert!(!cache.planes.borrow()[1].identity.valid);
    }
}

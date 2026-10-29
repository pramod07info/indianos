//! R014.2: single-core Limine framebuffer text console, bounded 5x7 font.
//! The serial console remains the primary diagnostic channel. We never write
//! to the framebuffer from an IRQ handler: input stays in its IRQ-safe FIFO.
use core::ptr;
use crate::boot::{console_framebuffer,ConsoleFramebuffer};

const CELL_W:usize=8;
const CELL_H:usize=16;
const MAX_COLS:usize=100;
const MAX_ROWS:usize=40;
struct Screen {
    surface:ConsoleFramebuffer,
    cols:usize,rows:usize,col:usize,row:usize,
    // The cursor stays inside the owned cell rectangle even after a full line.
    pending_wrap:bool,
}
static mut SCREEN:Option<Screen>=None;

// 5-bit rows for the supported US-ASCII shell alphabet. Lowercase renders
// using the corresponding uppercase glyph; unsupported bytes become '?'.
pub(crate) fn glyph(ch:u8)->[u8;7] {
    match ch.to_ascii_uppercase(){
        b'A'=>[14,17,17,31,17,17,17],
        b'B'=>[30,17,17,30,17,17,30],
        b'C'=>[14,17,16,16,16,17,14],
        b'D'=>[30,17,17,17,17,17,30],
        b'E'=>[31,16,16,30,16,16,31],
        b'F'=>[31,16,16,30,16,16,16],
        b'G'=>[14,17,16,23,17,17,14],
        b'H'=>[17,17,17,31,17,17,17],
        b'I'=>[31,4,4,4,4,4,31],
        b'J'=>[7,2,2,2,18,18,12],
        b'K'=>[17,18,20,24,20,18,17],
        b'L'=>[16,16,16,16,16,16,31],
        b'M'=>[17,27,21,21,17,17,17],
        b'N'=>[17,25,21,19,17,17,17],
        b'O'=>[14,17,17,17,17,17,14],
        b'P'=>[30,17,17,30,16,16,16],
        b'Q'=>[14,17,17,17,21,18,13],
        b'R'=>[30,17,17,30,20,18,17],
        b'S'=>[15,16,16,14,1,1,30],
        b'T'=>[31,4,4,4,4,4,4],
        b'U'=>[17,17,17,17,17,17,14],
        b'V'=>[17,17,17,17,17,10,4],
        b'W'=>[17,17,17,21,21,21,10],
        b'X'=>[17,17,10,4,10,17,17],
        b'Y'=>[17,17,10,4,4,4,4],
        b'Z'=>[31,1,2,4,8,16,31],
        b'0'=>[14,17,19,21,25,17,14],
        b'1'=>[4,12,4,4,4,4,14],
        b'2'=>[14,17,1,2,4,8,31],
        b'3'=>[30,1,1,14,1,1,30],
        b'4'=>[2,6,10,18,31,2,2],
        b'5'=>[31,16,30,1,1,17,14],
        b'6'=>[6,8,16,30,17,17,14],
        b'7'=>[31,1,2,4,8,8,8],
        b'8'=>[14,17,17,14,17,17,14],
        b'9'=>[14,17,17,15,1,2,12],
        b' '=>[0;7],
        b'>'=>[16,8,4,2,4,8,16],
        b'/'=>[1,1,2,4,8,16,16],
        b':'=>[0,4,4,0,4,4,0],
        b'.'=>[0,0,0,0,0,12,12],
        b'-'=>[0,0,0,31,0,0,0],
        b'_'=>[0,0,0,0,0,0,31],
        b'!'=>[4,4,4,4,4,0,4],
        b'['=>[14,8,8,8,8,8,14],
        b']'=>[14,2,2,2,2,2,14],
        b'='=>[0,31,0,31,0,0,0],
        b'?'=>[14,17,1,2,4,0,4],
        _=>[14,17,1,2,4,0,4],
    }
}
fn paint(s:&Screen,col:usize,row:usize,ch:u8) {
    let pixels=glyph(ch);
    let x=col*CELL_W;
    let y=row*CELL_H;
    for dy in 0..CELL_H{
        let bits=if (dy/2)<7{pixels[dy/2]}else{0};
        for dx in 0..CELL_W{
            let lit=dx>=1&&dx<6&&bits & (1<<(5-dx))!=0;
            let address=unsafe{s.surface.address.add((y+dy)*s.surface.pitch+(x+dx)*4)};
            unsafe{ptr::write_volatile(address.cast::<u32>(),
                if lit{s.surface.fg}else{s.surface.bg});}
        }
    }
}
fn advance(s:&mut Screen){
    s.pending_wrap=false;
    s.col=0;
    if s.row+1<s.rows{s.row+=1;}else{
        // Small scroll region; never touch unowned framebuffer pixels.
        let stride=s.surface.pitch;
        let limit=s.cols*CELL_W*4;
        for y in 0..((s.rows-1)*CELL_H){
            for x in (0..limit).step_by(4){
                unsafe {
                    let src=s.surface.address.add((y+CELL_H)*stride+x);
                    let dst=s.surface.address.add(y*stride+x);
                    ptr::write_volatile(dst.cast::<u32>(),ptr::read_volatile(src.cast::<u32>()));
                }
            }
        }
        for col in 0..s.cols{paint(s,col,s.rows-1,b' ');}
    }
}
fn put(s:&mut Screen,byte:u8){
    match byte{
        b'\r'=>{s.col=0;s.pending_wrap=false;},
        b'\n'=>advance(s),
        8=>{
            if s.pending_wrap{
                s.pending_wrap=false;
                paint(s,s.col,s.row,b' ');
            }else if s.col>0{
                s.col-=1;paint(s,s.col,s.row,b' ');
            }
        }
        32..=126=>{
            if s.pending_wrap{advance(s);}
            paint(s,s.col,s.row,byte);
            if s.col+1<s.cols{s.col+=1;}else{s.pending_wrap=true;}
        }
        _=>{}
    }
}
pub(crate) fn init()->bool{
    let Some(surface)=console_framebuffer()else{return false;};
    let cols=(surface.width/CELL_W).min(MAX_COLS);
    let rows=(surface.height/CELL_H).min(MAX_ROWS);
    if cols<20||rows<4{return false;}
    let mut s=Screen{surface,cols,rows,col:0,row:0,pending_wrap:false};
    for row in 0..rows{for col in 0..cols{paint(&s,col,row,b' ');}}
    for &b in b"IndianOS R014.2 framebuffer terminal\n"{put(&mut s,b);}
    unsafe{SCREEN=Some(s);}
    true
}
pub(crate) fn write(bytes:&[u8]){
    // Called from synchronous syscall/trap path with IF=0, not from IRQ1.
    if let Some(s)=unsafe{(&mut *ptr::addr_of_mut!(SCREEN)).as_mut()}{
        for &b in bytes{put(s,b);}
        put(s,b'\n');
    }
}
pub(crate) fn echo(byte:u8){
    if let Some(s)=unsafe{(&mut *ptr::addr_of_mut!(SCREEN)).as_mut()}{
        put(s,byte);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    const SENTINEL:u32=0x12345678;
    const FG:u32=0xffffff;
    const BG:u32=0x102030;
    // Extra scanlines, fractional cells, and pitch padding surround the owned area.
    fn fixture(cols:usize,rows:usize)->(std::vec::Vec<u32>,Screen){
        let width=cols*CELL_W+3;
        let height=rows*CELL_H+3;
        let stride=width+5;
        let mut pixels=std::vec![SENTINEL;stride*height+2];
        let surface=ConsoleFramebuffer{address:unsafe{pixels.as_mut_ptr().add(1)}.cast(),
            width,height,pitch:stride*4,fg:FG,bg:BG};
        let screen=Screen{surface,cols,rows,col:0,row:0,pending_wrap:false};
        for row in 0..rows{for col in 0..cols{paint(&screen,col,row,b' ');}}
        (pixels,screen)
    }
    fn cell(pixels:&[u32],s:&Screen,col:usize,row:usize,ch:u8){
        for y in 0..CELL_H{for x in 0..CELL_W{
            let bits=if y/2<7{glyph(ch)[y/2]}else{0};
            let lit=x>=1&&x<6&&bits&(1<<(5-x))!=0;
            assert_eq!(pixels[1+(row*CELL_H+y)*(s.surface.pitch/4)+col*CELL_W+x],
                if lit{FG}else{BG},"cell ({col},{row}) pixel ({x},{y})");
        }}
    }
    fn bounded(pixels:&[u32],s:&Screen){
        assert!(s.col<s.cols&&s.row<s.rows);
        let stride=s.surface.pitch/4;
        for (i,pixel) in pixels.iter().enumerate(){
            if i==0||i==pixels.len()-1{assert_eq!(*pixel,SENTINEL);continue;}
            let offset=i-1;
            if offset/stride>=s.rows*CELL_H||offset%stride>=s.cols*CELL_W{
                assert_eq!(*pixel,SENTINEL,"outside owned rectangle at {offset}");
            }
        }
    }
    #[test] fn last_column_newline_advances_once(){
        let (pixels,mut s)=fixture(2,3);
        for &b in b"AB\nC"{put(&mut s,b);}
        cell(&pixels,&s,0,0,b'A');cell(&pixels,&s,1,0,b'B');
        cell(&pixels,&s,0,1,b'C');cell(&pixels,&s,0,2,b' ');
        assert_eq!((s.col,s.row,s.pending_wrap),(1,1,false));bounded(&pixels,&s);
    }
    #[test] fn last_column_backspace_erases_before_wrap(){
        let (pixels,mut s)=fixture(2,1);
        for &b in b"AB"{put(&mut s,b);}
        cell(&pixels,&s,0,0,b'A');cell(&pixels,&s,1,0,b'B');
        put(&mut s,8);
        cell(&pixels,&s,0,0,b'A');cell(&pixels,&s,1,0,b' ');
        assert_eq!((s.col,s.row,s.pending_wrap),(1,0,false));
        put(&mut s,b'C');cell(&pixels,&s,1,0,b'C');
        bounded(&pixels,&s);
    }
    #[test] fn carriage_return_cancels_pending_wrap(){
        let (pixels,mut s)=fixture(2,2);
        for &b in b"AB\rC"{put(&mut s,b);}
        cell(&pixels,&s,0,0,b'C');cell(&pixels,&s,1,0,b'B');
        cell(&pixels,&s,0,1,b' ');
        assert_eq!((s.col,s.row,s.pending_wrap),(1,0,false));bounded(&pixels,&s);
    }
    #[test] fn bottom_right_waits_for_printable_then_scrolls_owned_pixels(){
        let (pixels,mut s)=fixture(2,2);
        for &b in b"ABCD"{put(&mut s,b);}
        cell(&pixels,&s,0,0,b'A');cell(&pixels,&s,1,0,b'B');
        cell(&pixels,&s,0,1,b'C');cell(&pixels,&s,1,1,b'D');
        assert_eq!((s.col,s.row,s.pending_wrap),(1,1,true));
        put(&mut s,0);assert!(s.pending_wrap);
        put(&mut s,b'E');
        cell(&pixels,&s,0,0,b'C');cell(&pixels,&s,1,0,b'D');
        cell(&pixels,&s,0,1,b'E');cell(&pixels,&s,1,1,b' ');bounded(&pixels,&s);
    }
    #[test] fn bottom_right_newline_scrolls_exactly_once(){
        let (pixels,mut s)=fixture(2,2);
        for &b in b"ABCD\n"{put(&mut s,b);}
        cell(&pixels,&s,0,0,b'C');cell(&pixels,&s,1,0,b'D');
        cell(&pixels,&s,0,1,b' ');cell(&pixels,&s,1,1,b' ');
        assert_eq!((s.col,s.row,s.pending_wrap),(0,1,false));bounded(&pixels,&s);
    }
    #[test] fn single_column_wraps_and_backspaces_without_crossing_rows(){
        let (pixels,mut s)=fixture(1,2);
        for &b in b"AB\x08\x08"{put(&mut s,b);}
        cell(&pixels,&s,0,0,b'A');cell(&pixels,&s,0,1,b' ');
        assert_eq!((s.col,s.row,s.pending_wrap),(0,1,false));bounded(&pixels,&s);
    }
    #[test] fn single_cell_controls_and_wrap_stay_bounded(){
        let (pixels,mut s)=fixture(1,1);
        for &b in b"A\x08B\rC"{put(&mut s,b);bounded(&pixels,&s);}
        cell(&pixels,&s,0,0,b'C');assert!(s.pending_wrap);
        put(&mut s,b'\n');cell(&pixels,&s,0,0,b' ');assert!(!s.pending_wrap);
        put(&mut s,8);cell(&pixels,&s,0,0,b' ');
        for &b in b"DE"{put(&mut s,b);bounded(&pixels,&s);}
        cell(&pixels,&s,0,0,b'E');
    }
    #[test] fn glyphs_are_bounded(){
        assert_eq!(glyph(b' '),[0;7]);
        assert_ne!(glyph(b'A'),glyph(b'?'));
        for b in 0..=127u8{
            assert!(glyph(b).iter().all(|row|*row<=31));
        }
    }
    #[test] fn glyph_is_stable_for_lowercase(){
        assert_eq!(glyph(b'h'),glyph(b'H'));
        assert_eq!(glyph(b'0'),[14,17,19,21,25,17,14]);
    }
}

//! Window policy owned by the isolated ring-3 compositor, never by an app.
#[path="../../../rust/kernel_core/src/gui_protocol.rs"]
mod wire;
use wire::DesktopFrame;
#[derive(Clone)]
pub struct Manager{frame:DesktopFrame,drag:Option<(usize,i32,i32)>,down:bool,recovery_release:bool}
impl Manager{
 pub fn new(bytes:&[u8;64])->Option<Self>{Some(Self{frame:DesktopFrame::parse(bytes)?,drag:None,down:false,recovery_release:false})}
 pub fn dimensions(&self)->(u16,u16){(self.frame.width,self.frame.height)}
 /// Replacement policy cannot inherit a drag or reinterpret a held button.
 pub fn quarantine_pointer(&mut self){self.drag=None;self.down=true;self.recovery_release=true;}
 pub fn wire(&self)->[u8;64]{self.frame.wire()}
 pub fn front(&self)->u8{self.frame.front}
 pub fn client_origin(&self, slot: usize) -> Option<(u16, u16)> {
  let window = self.frame.windows.get(slot)?;
  if window.minimized { return None; }
  Some((window.x.checked_add(wire::CLIENT_LEFT)?, window.y.checked_add(wire::CLIENT_TOP)?))
 }

 /// Translate one desktop strip into the covered portion of a live client row.
 /// Surface dimensions come from the generation-pinned snapshot.
 pub fn client_read_span(
  &self, slot: usize, screen: (u16, u16), count: u16, surface: (u16, u16),
 ) -> Option<RowRead> {
  let (window_x, window_y) = self.client_origin(slot)?;
  let (screen_x, screen_y) = screen;
  let y = screen_y.checked_sub(window_y)?;
  if y >= surface.1 || screen_y >= self.frame.height { return None; }
  let start = screen_x.max(window_x);
  let end = (u32::from(screen_x) + u32::from(count))
      .min(u32::from(window_x) + u32::from(surface.0))
      .min(u32::from(self.frame.width));
  if u32::from(start) >= end { return None; }
  Some(RowRead { x: start - window_x, y, count: (end - u32::from(start)) as u16,
                 destination: start - screen_x })
 }
 /// Stage a row in bounded chunks before exposing any of its pixels.
 pub fn compose_row<const Chunk: usize>(
  &self, screen: (u16, u16), row: &mut [u32], scratch: &mut [u32],
  mut compose: impl FnMut((u16, u16), &mut [u32]) -> Option<()>,
 ) -> Option<()> {
  let (x, y) = screen;
  if Chunk == 0 || row.is_empty() || row.len() != scratch.len() ||
     y >= self.frame.height || row.len() > usize::from(self.frame.width.checked_sub(x)?) {
   return None;
  }
  scratch.copy_from_slice(row);
  for (index, chunk) in scratch.chunks_mut(Chunk).enumerate() {
   compose((x + (index * Chunk) as u16, y), chunk)?;
  }
  row.copy_from_slice(scratch);
  Some(())
 }

 /// Composite client samples over an existing background/chrome strip.
 /// The front window body is opaque, including where its client has no pixel.
 pub fn compose_screen_strip<const N: usize>(
  &self, screen: (u16, u16), destinations: [i32; 2],
  layers: &mut [[Option<u32>; N]; 2], strip: &mut [u32],
 ) {
  let front = usize::from(self.frame.front - 1);
  let window = self.frame.windows[front];
  let (screen_x, screen_y) = (i32::from(screen.0), i32::from(screen.1));
  let left = i32::from(window.x);
  let top = i32::from(window.y);
  if !window.minimized && (top..top + i32::from(window.height)).contains(&screen_y) {
   let rear = 1 - front;
   let start = screen_x + destinations[rear];
   for (offset, pixel) in layers[rear].iter_mut().enumerate() {
    if (left..left + i32::from(window.width)).contains(&(start + offset as i32)) {
     *pixel = None;
    }
   }
  }
  compose_client_row(front == 1, [
   ClientSpan { x: destinations[0], pixels: &layers[0] },
   ClientSpan { x: destinations[1], pixels: &layers[1] },
  ], strip);
 }
 pub fn control(&mut self,request:&[u8])->Option<u8>{
  let mut next=self.clone();
  match request{
   [b'P',front @ (1|2)]=>{next.frame.windows[*front as usize-1].minimized=false;next.frame.front=*front;},
   [b'K',key]=>match key{
    b'a'=>next.frame.launcher=!next.frame.launcher,
    b'1'|b'2'=>{let front=key-b'0';next.frame.windows[front as usize-1].minimized=false;next.frame.front=front;},
    b'h'=>{let w=&mut next.frame.windows[0];w.x=(w.x as u32+20).min((next.frame.width-w.width) as u32) as u16;},
    b'm'=>{next.frame.windows[1].minimized=true;next.frame.windows[0].minimized=false;next.frame.front=1;next.drag=None;},
    b'r'=>{next.frame.windows[1].minimized=false;next.frame.front=2;},
    _=>return None,
   },
   [b'M',xl,xh,yl,yh,buttons]=>{
    let x=u16::from_le_bytes([*xl,*xh]);let y=u16::from_le_bytes([*yl,*yh]);
    if x>=next.frame.width||y>=next.frame.height||buttons&!3!=0{return None;}
    let down=buttons&1!=0;next.frame.pointer_x=x;next.frame.pointer_y=y;
    if next.recovery_release{next.drag=None;if !down{next.recovery_release=false;}}
    if down&&!next.down&&!next.recovery_release{
     next.drag=None;
     let front=next.frame.front as usize-1;
     for index in [front,1-front]{
      let w=next.frame.windows[index];
      if !w.minimized&&x>=w.x&&y>=w.y&&(x as u32)<w.x as u32+w.width as u32&&
         (y as u32)<w.y as u32+w.height as u32{
       next.frame.front=index as u8+1;
       if y<w.y+35{next.drag=Some((index,x as i32-w.x as i32,y as i32-w.y as i32));}
       break;
      }
     }
    }
    if down{
     if let Some((index,dx,dy))=next.drag{
      let w=&mut next.frame.windows[index];
      w.x=(x as i32-dx).clamp(0,(next.frame.width-w.width) as i32) as u16;
      w.y=(y as i32-dy).clamp(48,(next.frame.height-42-w.height) as i32) as u16;
     }
    }else{next.drag=None;}
    next.down=down;
   },
   _=>return None,
  }
  // An explicit keyboard/presentation transition invalidates the old drag
  // anchor. Preserve the physical button latch: a held button cannot capture
  // another window until a release and a fresh press arrive.
  if request[0]!=b'M'{next.drag=None;}
  // Preserve a valid full state even when dimensions/flags evolve later.
  DesktopFrame::parse(&next.wire())?;
  let front=next.front();*self=next;Some(front)
 }
}

#[path="../../../rust/kernel_core/src/font5x7.rs"]
mod font;

/// One clipped scanline of the existing IndianOS desktop, assembled in ring3.
/// The callback blends a pinned client row immediately after that window's
/// opaque chrome, so a transparent foreground never reveals a rear window.
impl Manager{
 pub fn render_row(&self,y:u16,pixels:&mut[u32],masks:[u32;3],
   mut client:impl FnMut(usize,(u16,u16),&mut[u32])->Option<()>)->Option<()>{
  let (width,height)=self.dimensions();
  if width>1600||height>1600||y>=height||pixels.len()!=usize::from(width)||
     masks.iter().any(|m|*m==0||(*m>>m.trailing_zeros())!=255)||
     masks[0]&masks[1]!=0||masks[0]&masks[2]!=0||masks[1]&masks[2]!=0{return None;}
  let color=|r:u32,g:u32,b:u32|(masks[0]/255)*r|(masks[1]/255)*g|(masks[2]/255)*b;
  let bg=color(16,28,55);let top=color(12,20,43);let bar=color(22,41,75);
  let title=color(15,110,136);let unfocused=color(75,88,117);
  let light=color(236,245,248);let dark=color(31,50,73);let panel=color(205,223,231);
  let white=color(255,255,255);let accent=color(247,159,60);
  pixels.fill(bg);
  let h=i32::from(height);
  {
   let mut r=SceneRow{y:i32::from(y),pixels};
   r.fill(0,0,i32::from(width),48,top);r.text(20,18,b"INDIANOS DESKTOP",white);
   r.fill(0,h-42,i32::from(width),42,bar);r.fill(20,h-34,90,25,title);
   r.text(30,h-29,b"APPS",white);
  }
  let front=usize::from(self.frame.front-1);
  for slot in [1-front,front]{
   let w=self.frame.windows[slot];if w.minimized{continue;}
   let (x,wy,ww,wh)=(i32::from(w.x),i32::from(w.y),i32::from(w.width),i32::from(w.height));
   {
    let mut r=SceneRow{y:i32::from(y),pixels};
    r.fill(x-2,wy-2,ww+4,wh+4,dark);r.fill(x,wy,ww,wh,light);
    r.fill(x,wy,ww,35,if slot==front{title}else{unfocused});
    r.fill(x+ww-30,wy+8,18,18,accent);
    r.text(x+18,wy+11,if slot==0{b"FILES"}else{b"TERMINAL"},white);
    if slot==0{
     r.fill(x+10,wy+48,ww-20,32,panel);r.text(x+18,wy+57,b"HOME / FILES",dark);
     for (row,label) in [b"DOCUMENTS".as_slice(),b"DOWNLOADS",b"INDIANOS"].iter().enumerate(){
      let sy=wy+94+row as i32*46;r.fill(x+23,sy,26,23,accent);r.text(x+66,sy+5,label,dark);
     }
    }else{
     r.fill(x+9,wy+44,ww-18,wh-54,dark);
     for (dy,text,c) in [(64,b"INDIANOS TERMINAL".as_slice(),accent),(92,b"$ HELP",white),
       (120,b"READY FOR COMMANDS",white),(150,b"$ _",accent)]{r.text(x+25,wy+dy,text,c);}
    }
   }
   client(slot,(0,y),pixels)?;
  }
  let mut r=SceneRow{y:i32::from(y),pixels};
  if self.frame.launcher{
   r.fill(20,h-225,340,183,top);r.fill(26,h-215,328,30,title);
   r.text(36,h-207,b"APPLICATIONS",white);r.text(42,h-168,b"1  FILE MANAGER",white);
   r.text(42,h-132,b"2  TERMINAL",white);r.text(42,h-95,b"FOCUS  MOVE  MINIMIZE",white);
  }
  let (px,py)=(i32::from(self.frame.pointer_x),i32::from(self.frame.pointer_y));
  r.fill(px-1,py-10,3,21,accent);r.fill(px-10,py-1,21,3,accent);r.fill(px-1,py-1,3,3,white);
  Some(())
 }
}
struct SceneRow<'a>{y:i32,pixels:&'a mut[u32]}

/// Duplicate control delivery after a compositor restart must not toggle or
/// move a window twice. Advance only after the complete frame publishes.
pub struct FrameOrder{committed:u64}
#[derive(Debug,PartialEq,Eq)]
pub enum Delivery{Duplicate,Next}
impl FrameOrder{
 pub fn new(committed:u64)->Self{Self{committed}}
 pub fn classify(&self,sequence:u64)->Option<Delivery>{
  if sequence==0{return None;}
  if sequence==self.committed{return Some(Delivery::Duplicate);}
  (self.committed.checked_add(1)==Some(sequence)).then_some(Delivery::Next)
 }
 pub fn publish(&mut self,sequence:u64)->bool{
  if self.classify(sequence)!=Some(Delivery::Next){return false;}
  self.committed=sequence;true
 }
}
impl SceneRow<'_>{
 fn fill(&mut self,x:i32,y:i32,width:i32,height:i32,color:u32){
  if width<=0||height<=0||self.y<y||i64::from(self.y)>=i64::from(y)+i64::from(height){return;}
  let left=i64::from(x).clamp(0,self.pixels.len() as i64) as usize;
  let right=(i64::from(x)+i64::from(width)).clamp(0,self.pixels.len() as i64) as usize;
  if left<right{self.pixels[left..right].fill(color);}
 }
 fn text(&mut self,x:i32,y:i32,bytes:&[u8],color:u32){
  let dy=self.y-y;if !(0..14).contains(&dy){return;}
  for (index,ch) in bytes.iter().take(36).enumerate(){
   let bits=font::glyph(*ch)[dy as usize/2];
   for bit in 0..5{if bits&(1<<(4-bit))!=0{self.fill(x+index as i32*12+bit*2,self.y,2,1,color);}}
  }
 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowRead {
    pub x: u16,
    pub y: u16,
    pub count: u16,
    pub destination: u16,
}

/// A visible span of a client surface in screen coordinates. Transparent
/// pixels preserve the destination, including pixels from a rear window.
pub struct ClientSpan<'a> {
    pub x: i32,
    pub pixels: &'a [Option<u32>],
}

/// Draw client spans in desktop stacking order into a caller-owned row.
/// The caller provides any background or window chrome already in the row.
pub fn compose_client_row(terminal_front: bool, spans: [ClientSpan<'_>; 2], row: &mut [u32]) {
    let front = usize::from(terminal_front);
    for index in [1 - front, front] {
        let span = &spans[index];
        let left = i64::from(span.x);
        let start = left.max(0).min(row.len() as i64);
        let end = (left + span.pixels.len() as i64).min(row.len() as i64);
        if start >= end {
            continue;
        }
        let pixels = &span.pixels[(start - left) as usize..(end - left) as usize];
        for (out, pixel) in row[start as usize..end as usize].iter_mut().zip(pixels) {
            if let Some(color) = pixel {
                *out = *color;
            }
        }
    }
}

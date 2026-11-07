//! Window policy owned by the isolated ring-3 compositor, never by an app.
#[path="../../../rust/kernel_core/src/gui_protocol.rs"]
mod wire;
use wire::DesktopFrame;
#[derive(Clone)]
pub struct Manager{frame:DesktopFrame,drag:Option<(usize,i32,i32)>,down:bool}
impl Manager{
 pub fn new(bytes:&[u8;64])->Option<Self>{Some(Self{frame:DesktopFrame::parse(bytes)?,drag:None,down:false})}
 pub fn wire(&self)->[u8;64]{self.frame.wire()}
 pub fn front(&self)->u8{self.frame.front}
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
    if down&&!next.down{
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
  // Preserve a valid full state even when dimensions/flags evolve later.
  DesktopFrame::parse(&next.wire())?;
  let front=next.front();*self=next;Some(front)
 }
}

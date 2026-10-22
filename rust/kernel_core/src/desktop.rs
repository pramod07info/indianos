//! R018: allocation-free desktop shell model.
//! This is a headless, deterministic UI foundation. It does not drive a GPU,
//! framebuffer, mouse, or window system yet.
pub const MAX_WINDOWS:usize=8;
pub const MIN_WIDTH:u32=160;
pub const MIN_HEIGHT:u32=100;

#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct Rect{pub x:i32,pub y:i32,pub width:u32,pub height:u32}
impl Rect{
    pub const fn new(x:i32,y:i32,width:u32,height:u32)->Self{
        Self{x,y,width,height}
    }
    pub fn contains(&self,x:i32,y:i32)->bool{
        x as i64>=self.x as i64 && y as i64>=self.y as i64 &&
        (x as i64)<self.x as i64+self.width as i64 &&
        (y as i64)<self.y as i64+self.height as i64
    }
}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum App{Launcher,Files,Terminal,Settings,SystemMonitor,AiAssistant}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct Window{
    pub id:u32,pub app:App,pub bounds:Rect,
    pub visible:bool,pub minimized:bool,
}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum DesktopError{InvalidDisplay,InvalidBounds,Full,NotFound,IdExhausted}
pub struct Desktop{
    pub width:u32,pub height:u32,
    windows:[Option<Window>;MAX_WINDOWS],
    count:usize,next_id:u32,focused:Option<u32>,
}
impl Desktop{
    pub fn new(width:u32,height:u32)->Result<Self,DesktopError>{
        if width<MIN_WIDTH||height<MIN_HEIGHT||width>i32::MAX as u32||
           height>i32::MAX as u32{return Err(DesktopError::InvalidDisplay);}
        Ok(Self{width,height,windows:[None;MAX_WINDOWS],count:0,next_id:1,focused:None})
    }
    fn valid(&self,b:Rect)->bool{
        b.width>=MIN_WIDTH&&b.height>=MIN_HEIGHT&&b.x>=0&&b.y>=0&&
        (b.x as u64+b.width as u64)<=self.width as u64&&
        (b.y as u64+b.height as u64)<=self.height as u64
    }
    pub fn open(&mut self,app:App,bounds:Rect)->Result<u32,DesktopError>{
        if !self.valid(bounds){return Err(DesktopError::InvalidBounds);}
        if self.count==MAX_WINDOWS{return Err(DesktopError::Full);}
        let id=self.next_id;
        self.next_id=self.next_id.checked_add(1).ok_or(DesktopError::IdExhausted)?;
        self.windows[self.count]=Some(Window{id,app,bounds,visible:true,minimized:false});
        self.count+=1;self.focused=Some(id);Ok(id)
    }
    pub fn focus(&mut self,id:u32)->Result<(),DesktopError>{
        let idx=self.windows[..self.count].iter().position(|w|
            w.is_some_and(|w|w.id==id)).ok_or(DesktopError::NotFound)?;
        let mut window=self.windows[idx].unwrap();
        window.visible=true;window.minimized=false;
        for j in idx..self.count-1{self.windows[j]=self.windows[j+1];}
        self.windows[self.count-1]=Some(window);self.focused=Some(id);Ok(())
    }
    pub fn minimize(&mut self,id:u32)->Result<(),DesktopError>{
        let window=self.windows[..self.count].iter_mut()
            .find_map(|w|w.as_mut().filter(|w|w.id==id))
            .ok_or(DesktopError::NotFound)?;
        window.minimized=true;window.visible=false;
        if self.focused==Some(id){
            self.focused=self.windows[..self.count].iter().rev()
                .filter_map(|w|*w).find(|w|w.visible).map(|w|w.id);
        }
        Ok(())
    }
    pub fn close(&mut self,id:u32)->Result<(),DesktopError>{
        let idx=self.windows[..self.count].iter().position(|w|
            w.is_some_and(|w|w.id==id)).ok_or(DesktopError::NotFound)?;
        for j in idx..self.count-1{self.windows[j]=self.windows[j+1];}
        self.count-=1;self.windows[self.count]=None;
        if self.focused==Some(id){
            self.focused=self.windows[..self.count].iter().rev()
                .filter_map(|w|*w).find(|w|w.visible).map(|w|w.id);
        }
        Ok(())
    }
    pub fn focused(&self)->Option<u32>{self.focused}
    pub fn windows(&self)->impl Iterator<Item=&Window>{
        self.windows[..self.count].iter().filter_map(Option::as_ref)
    }
    pub fn hit_test(&self,x:i32,y:i32)->Option<u32>{
        self.windows().rev().find(|w|w.visible&&w.bounds.contains(x,y))
            .map(|w|w.id)
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    fn bounds()->Rect{Rect::new(10,10,320,240)}
    #[test]fn open_focus_minimize_close(){
        let mut d=Desktop::new(1024,768).unwrap();
        let a=d.open(App::Files,bounds()).unwrap();
        let b=d.open(App::Terminal,bounds()).unwrap();
        assert_eq!(d.hit_test(20,20),Some(b));
        d.focus(a).unwrap();
        assert_eq!(d.hit_test(20,20),Some(a));
        d.minimize(a).unwrap();
        assert_eq!(d.focused(),Some(b));
        d.close(b).unwrap();
        assert_eq!(d.focused(),None);
        d.focus(a).unwrap();
        assert_eq!(d.focused(),Some(a));
    }
    #[test]fn bounds_and_capacity(){
        assert!(matches!(Desktop::new(100,100),Err(DesktopError::InvalidDisplay)));
        let mut d=Desktop::new(1024,768).unwrap();
        assert_eq!(d.open(App::Files,Rect::new(-1,0,320,240)),Err(DesktopError::InvalidBounds));
        assert_eq!(d.open(App::Files,Rect::new(900,0,320,240)),Err(DesktopError::InvalidBounds));
        for _ in 0..MAX_WINDOWS{d.open(App::Files,bounds()).unwrap();}
        assert_eq!(d.open(App::Files,bounds()),Err(DesktopError::Full));
        assert_eq!(d.close(999),Err(DesktopError::NotFound));
    }
}

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
/// Reversible window-placement metadata, not a durable session snapshot.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct LayoutSnapshot{
    slots:[Option<(u32,Rect)>;MAX_WINDOWS],
    count:usize
}
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
    /// Move an existing window within the display; do not change z order.
    pub fn move_window(&mut self,id:u32,x:i32,y:i32)->Result<(),DesktopError>{
        let index=self.windows[..self.count].iter()
            .position(|slot|slot.is_some_and(|window|window.id==id))
            .ok_or(DesktopError::NotFound)?;
        let mut window=self.windows[index].unwrap();
        let proposed=Rect::new(x,y,window.bounds.width,window.bounds.height);
        if !self.valid(proposed){return Err(DesktopError::InvalidBounds);}
        window.bounds=proposed;
        self.windows[index]=Some(window);
        Ok(())
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
    /// Orbit W1: resize an existing window atomically inside validated
    /// screen bounds; invalid requests never mutate live geometry.
    pub fn resize(&mut self,id:u32,width:u32,height:u32)->Result<(),DesktopError>{
        let original=self.windows().find(|w|w.id==id).copied()
            .ok_or(DesktopError::NotFound)?;
        self.set_bounds(id,Rect::new(original.bounds.x,original.bounds.y,width,height))
    }
    /// Orbit W2: snap a window into the left/right half of a caller-approved
    /// work area, excluding the rail and taskbar. No forged caller authority.
    pub fn snap_half(&mut self,id:u32,area:Rect,right:bool)
        ->Result<(),DesktopError>{
        if !self.valid(area)||area.width<MIN_WIDTH*2{return Err(DesktopError::InvalidBounds);}
        let left=area.width/2;
        let width=if right{area.width-left}else{left};
        let x=if right{area.x+left as i32}else{area.x};
        self.set_bounds(id,Rect::new(x,area.y,width,area.height))
    }
    /// Orbit W3: maximize into an authorized work area. The returned prior
    /// bounds are an explicit restore token; the caller must retain them.
    pub fn maximize_in(&mut self,id:u32,area:Rect)->Result<Rect,DesktopError>{
        if !self.valid(area){return Err(DesktopError::InvalidBounds);}
        let old=self.windows().find(|w|w.id==id).map(|w|w.bounds)
            .ok_or(DesktopError::NotFound)?;
        self.set_bounds(id,area)?;
        Ok(old)
    }
    /// Orbit W4: caller-owned restore token is validated against current
    /// dimensions. Restoring cannot escape the active display boundaries.
    pub fn restore_bounds(&mut self,id:u32,original:Rect)
        ->Result<(),DesktopError>{self.set_bounds(id,original)}
    /// Orbit W5: rotate focus over visible, non-minimized windows. Does not
    /// revive a closed/minimized app or cross a process ownership boundary.
    pub fn focus_next_visible(&mut self)->Result<u32,DesktopError>{
        let active=self.focused;
        let mut candidates=[None;MAX_WINDOWS];
        let mut length=0;
        for w in self.windows(){
            if w.visible&&!w.minimized{
                candidates[length]=Some(w.id);length+=1;
            }
        }
        if length==0{return Err(DesktopError::NotFound);}
        let next=active.and_then(|current|
            candidates[..length].iter().position(|x|*x==Some(current)))
            .map_or(0,|index|(index+1)%length);
        let id=candidates[next].ok_or(DesktopError::NotFound)?;
        self.focus(id)?;
        Ok(id)
    }
    /// Internal all-or-nothing bounds update reused across resize, snap
    /// and maximize. It deliberately does not change stacking order.
    fn set_bounds(&mut self,id:u32,rect:Rect)->Result<(),DesktopError>{
        if !self.valid(rect){return Err(DesktopError::InvalidBounds);}
        let window=self.windows[..self.count].iter_mut().find_map(|item|
            item.as_mut().filter(|w|w.id==id)).ok_or(DesktopError::NotFound)?;
        window.bounds=rect;
        Ok(())
    }
    /// Return a complete, non-mutating list of displayed windows.
    /// Too-small outputs are rejected before copying any identifier.
    pub fn visible_ids(&self,out:&mut[u32])->Result<usize,DesktopError>{
        let count=self.windows().filter(|w|w.visible&&!w.minimized).count();
        if out.len()<count{return Err(DesktopError::Full);}
        let mut i=0;
        for w in self.windows(){
            if w.visible&&!w.minimized{out[i]=w.id;i+=1;}
        }
        Ok(count)
    }
    /// Atomic geometry snapshot. IDs are never reused within a Desktop;
    /// the caller is responsible for authenticating the screen owner.
    pub fn snapshot(&self)->LayoutSnapshot{
        let mut slots=[None;MAX_WINDOWS];
        let mut count=0;
        for w in self.windows(){
            slots[count]=Some((w.id,w.bounds));count+=1;
        }
        LayoutSnapshot{slots,count}
    }
    /// Restore only when the identical set of windows still exists and
    /// every saved rectangle remains valid on the current display.
    pub fn apply_snapshot(&mut self,snapshot:LayoutSnapshot)
        ->Result<(),DesktopError>{
        if snapshot.count!=self.count{return Err(DesktopError::InvalidBounds);}
        let mut staged=self.windows;
        for saved in snapshot.slots[..snapshot.count].iter().copied(){
            let Some((id,rect))=saved else{return Err(DesktopError::InvalidBounds)};
            if !self.valid(rect){return Err(DesktopError::InvalidBounds);}
            let slot=staged[..self.count].iter_mut().find_map(|slot|
                slot.as_mut().filter(|w|w.id==id))
                .ok_or(DesktopError::NotFound)?;
            slot.bounds=rect;
        }
        self.windows=staged;Ok(())
    }
    /// Tile 1..=MAX_WINDOWS visible windows in a bounded grid in one
    /// transaction. If any tile is too small nothing is moved.
    pub fn tile_visible(&mut self,area:Rect,columns:usize)
        ->Result<usize,DesktopError>{
        if !self.valid(area)||columns==0||columns>MAX_WINDOWS{
            return Err(DesktopError::InvalidBounds);
        }
        let count=self.windows().filter(|w|w.visible&&!w.minimized).count();
        if count==0{return Err(DesktopError::NotFound);}
        let cols=columns.min(count);let rows=(count+cols-1)/cols;
        if area.width/(cols as u32)<MIN_WIDTH||
           area.height/(rows as u32)<MIN_HEIGHT{
            return Err(DesktopError::InvalidBounds);
        }
        let mut staged=self.windows;
        let mut n=0usize;
        for slot in staged[..self.count].iter_mut(){
            let Some(w)=slot.as_mut() else{continue};
            if !w.visible||w.minimized{continue;}
            let col=n%cols;let row=n/cols;
            let start_x=area.width as u64*col as u64/cols as u64;
            let end_x=area.width as u64*(col+1) as u64/cols as u64;
            let start_y=area.height as u64*row as u64/rows as u64;
            let end_y=area.height as u64*(row+1) as u64/rows as u64;
            let rect=Rect::new(area.x+start_x as i32,area.y+start_y as i32,
                (end_x-start_x) as u32,(end_y-start_y) as u32);
            if !self.valid(rect){return Err(DesktopError::InvalidBounds);}
            w.bounds=rect;n+=1;
        }
        self.windows=staged;Ok(n)
    }
    /// Find an existing live window for an app without opening duplicates.
    pub fn existing_app(&self,app:App)->Option<u32>{
        self.windows().rev().find(|w|w.app==app&&w.visible&&!w.minimized)
            .map(|w|w.id)
    }
    pub fn focused(&self)->Option<u32>{self.focused}
    pub fn windows(&self)->impl DoubleEndedIterator<Item=&Window> + '_{
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
    #[test]fn orbit_resize_is_atomic_at_display_boundaries(){
        let mut d=Desktop::new(800,600).unwrap();
        let id=d.open(App::Files,Rect::new(40,60,300,240)).unwrap();
        assert_eq!(d.resize(id,400,260),Ok(()));
        assert_eq!(d.windows().next().unwrap().bounds,Rect::new(40,60,400,260));
        assert_eq!(d.resize(id,900,260),Err(DesktopError::InvalidBounds));
        assert_eq!(d.resize(id,20,260),Err(DesktopError::InvalidBounds));
        assert_eq!(d.windows().next().unwrap().bounds,Rect::new(40,60,400,260));
    }
    #[test]fn orbit_snap_halves_never_leave_approved_work_area(){
        let mut d=Desktop::new(1280,800).unwrap();
        let id=d.open(App::Files,Rect::new(80,100,420,300)).unwrap();
        let work=Rect::new(195,62,800,600);
        d.snap_half(id,work,false).unwrap();
        assert_eq!(d.windows().next().unwrap().bounds,Rect::new(195,62,400,600));
        d.snap_half(id,work,true).unwrap();
        assert_eq!(d.windows().next().unwrap().bounds,Rect::new(595,62,400,600));
        assert_eq!(d.snap_half(id,Rect::new(1,1,200,600),false),
                   Err(DesktopError::InvalidBounds));
    }
    #[test]fn orbit_maximize_returns_token_and_restore_rejects_bad_geometry(){
        let mut d=Desktop::new(1280,800).unwrap();
        let old=Rect::new(36,88,500,400);
        let id=d.open(App::Terminal,old).unwrap();
        let work=Rect::new(15,62,1220,655);
        let previous=d.maximize_in(id,work).unwrap();
        assert_eq!(previous,old);
        assert_eq!(d.restore_bounds(id,Rect::new(-1,0,500,400)),
                   Err(DesktopError::InvalidBounds));
        assert_eq!(d.windows().next().unwrap().bounds,work);
        d.restore_bounds(id,previous).unwrap();
        assert_eq!(d.windows().next().unwrap().bounds,old);
    }
    #[test]fn orbit_focus_cycle_skips_minimized_windows(){
        let mut d=Desktop::new(800,600).unwrap();
        let a=d.open(App::Files,bounds()).unwrap();
        let b=d.open(App::Terminal,bounds()).unwrap();
        let c=d.open(App::Settings,bounds()).unwrap();
        d.minimize(b).unwrap();
        assert_eq!(d.focus_next_visible(),Ok(a));
        assert_eq!(d.focus_next_visible(),Ok(c));
        d.minimize(a).unwrap();d.minimize(c).unwrap();
        assert_eq!(d.focus_next_visible(),Err(DesktopError::NotFound));
        assert_eq!(d.focused(),None);
    }
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
    #[test]fn bounded_drag_preserves_window_identity_and_z_order(){
        let mut desktop=Desktop::new(800,600).unwrap();
        let files=desktop.open(App::Files,Rect::new(32,40,320,220)).unwrap();
        let term=desktop.open(App::Terminal,Rect::new(180,180,320,220)).unwrap();
        assert_eq!(desktop.move_window(files,80,90),Ok(()));
        assert_eq!(desktop.hit_test(100,120),Some(files));
        assert_eq!(desktop.hit_test(190,190),Some(term));
        assert_eq!(desktop.move_window(files,-1,10),Err(DesktopError::InvalidBounds));
        assert_eq!(desktop.move_window(files,600,10),Err(DesktopError::InvalidBounds));
        assert_eq!(desktop.move_window(999,1,1),Err(DesktopError::NotFound));
        assert_eq!(desktop.windows().find(|w|w.id==files).unwrap().bounds.x,80);
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
    #[test]fn orbit_grid_tiling_and_atomic_rollback(){
        let mut d=Desktop::new(1280,800).unwrap();
        let a=d.open(App::Files,Rect::new(40,70,350,270)).unwrap();
        let b=d.open(App::Terminal,Rect::new(90,95,350,270)).unwrap();
        let c=d.open(App::Settings,Rect::new(140,125,350,270)).unwrap();
        let old=d.snapshot();
        let area=Rect::new(200,65,960,620);
        assert_eq!(d.tile_visible(area,2),Ok(3));
        assert_eq!(d.windows().count(),3);
        assert_eq!(d.existing_app(App::Terminal),Some(b));
        let tiled=d.snapshot();
        assert_eq!(d.tile_visible(Rect::new(0,0,200,200),2),
                   Err(DesktopError::InvalidBounds));
        assert_eq!(d.snapshot(),tiled);
        assert_eq!(d.apply_snapshot(old),Ok(()));
        assert_eq!(d.snapshot(),old);
        d.close(c).unwrap();
        assert_eq!(d.apply_snapshot(old),Err(DesktopError::InvalidBounds));
        assert_eq!(d.existing_app(App::Files),Some(a));
    }
    #[test]fn orbit_visible_inventory_never_partial_writes(){
        let mut d=Desktop::new(800,600).unwrap();
        let a=d.open(App::Files,Rect::new(10,10,320,240)).unwrap();
        let b=d.open(App::Terminal,Rect::new(30,30,320,240)).unwrap();
        let mut short=[123u32;1];
        assert_eq!(d.visible_ids(&mut short),Err(DesktopError::Full));
        assert_eq!(short,[123]);
        d.minimize(b).unwrap();
        assert_eq!(d.visible_ids(&mut short),Ok(1));
        assert_eq!(short,[a]);
    }

}

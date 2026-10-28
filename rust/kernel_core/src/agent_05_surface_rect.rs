//! Allocation-free rectangle intersection for an authenticated desktop surface.
pub const SCREEN_WIDTH: u32 = 1280;
pub const SCREEN_HEIGHT: u32 = 800;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rectangle { pub x: u32, pub y: u32, pub width: u32, pub height: u32 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RectError { InvalidOwner, Empty, Overflow, OutsideScreen }
fn edges(r: Rectangle) -> Result<(u32, u32), RectError> {
    if r.width == 0 || r.height == 0 { return Err(RectError::Empty); }
    let right = r.x.checked_add(r.width).ok_or(RectError::Overflow)?;
    let bottom = r.y.checked_add(r.height).ok_or(RectError::Overflow)?;
    if right > SCREEN_WIDTH || bottom > SCREEN_HEIGHT { return Err(RectError::OutsideScreen); }
    Ok((right, bottom))
}
/// None means both valid rectangles do not intersect.
pub fn clip_owned(surface: Rectangle, clip: Rectangle, owner: u32, caller: u32)
    -> Result<Option<Rectangle>, RectError>
{
    if owner == 0 || owner != caller { return Err(RectError::InvalidOwner); }
    let (sr, sb) = edges(surface)?;
    let (cr, cb) = edges(clip)?;
    let left = core::cmp::max(surface.x, clip.x);
    let top = core::cmp::max(surface.y, clip.y);
    let right = core::cmp::min(sr, cr);
    let bottom = core::cmp::min(sb, cb);
    if left >= right || top >= bottom { return Ok(None); }
    Ok(Some(Rectangle { x: left, y: top, width: right - left, height: bottom - top }))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn r(x: u32,y: u32,w: u32,h: u32) -> Rectangle {
        Rectangle { x, y, width: w, height: h }
    }
    #[test]
    fn clips_intersection_at_screen_edge() {
        assert_eq!(clip_owned(r(1270,790,10,10),r(1275,795,5,5),1,1),
            Ok(Some(r(1275,795,5,5))));
        assert_eq!(clip_owned(r(0,0,5,5),r(8,8,3,3),1,1), Ok(None));
    }
    #[test]
    fn prevents_cross_owner_access() {
        assert_eq!(clip_owned(r(0,0,5,5),r(0,0,5,5),1,2), Err(RectError::InvalidOwner));
    }
    #[test]
    fn refuses_overflow_and_invalid_dimensions() {
        assert_eq!(clip_owned(r(u32::MAX,0,2,3),r(0,0,1,1),1,1), Err(RectError::Overflow));
        assert_eq!(clip_owned(r(0,0,0,1),r(0,0,1,1),1,1), Err(RectError::Empty));
        assert_eq!(clip_owned(r(1279,0,2,1),r(0,0,1,1),1,1), Err(RectError::OutsideScreen));
    }
}

/// Move an already valid surface within the screen. A caller cannot move a
/// foreign surface or overflow signed/unsigned coordinates.
pub fn move_owned(surface: Rectangle, dx: i32, dy: i32, owner: u32, caller: u32)
    -> Result<Rectangle, RectError>
{
    if owner == 0 || owner != caller { return Err(RectError::InvalidOwner); }
    edges(surface)?;
    let x = i64::from(surface.x) + i64::from(dx);
    let y = i64::from(surface.y) + i64::from(dy);
    if x < 0 || y < 0 || x > i64::from(SCREEN_WIDTH) || y > i64::from(SCREEN_HEIGHT) {
        return Err(RectError::OutsideScreen);
    }
    let moved = Rectangle { x: x as u32, y: y as u32, ..surface };
    edges(moved)?;
    Ok(moved)
}
#[cfg(test)]
mod motion_tests {
    use super::*;
    fn window() -> Rectangle {
        Rectangle { x: 50, y: 25, width: 100, height: 50 }
    }
    #[test]
    fn moves_window_only_within_screen() {
        assert_eq!(move_owned(window(), 10, -5, 1, 1).unwrap().x, 60);
        assert_eq!(move_owned(window(), 10, -5, 1, 1).unwrap().y, 20);
    }
    #[test]
    fn rejects_outside_or_overflow_attempts() {
        assert_eq!(move_owned(window(), -51, 0, 1, 1), Err(RectError::OutsideScreen));
        assert_eq!(move_owned(window(), i32::MAX, 0, 1, 1), Err(RectError::OutsideScreen));
    }
    #[test]
    fn foreign_pid_cannot_move_surface() {
        assert_eq!(move_owned(window(), 1, 1, 4, 5), Err(RectError::InvalidOwner));
    }
}


/// Conservative CPU-desktop damage region covering two authorized
/// rectangles. This is metadata only and never writes framebuffer pixels.
pub fn union_owned(a:Rectangle,b:Rectangle,owner:u32,caller:u32)
    ->Result<Rectangle,RectError>{
    if owner==0||owner!=caller{return Err(RectError::InvalidOwner)}
    let (ar,ab)=edges(a)?;
    let (br,bb)=edges(b)?;
    let left=core::cmp::min(a.x,b.x);
    let top=core::cmp::min(a.y,b.y);
    let right=core::cmp::max(ar,br);
    let bottom=core::cmp::max(ab,bb);
    Ok(Rectangle{x:left,y:top,width:right-left,height:bottom-top})
}
#[cfg(test)]
mod damage_union_tests{
    use super::*;
    fn rect(x:u32,y:u32,w:u32,h:u32)->Rectangle{
        Rectangle{x,y,width:w,height:h}
    }
    #[test]fn merging_adjacent_and_distant_damage_stays_bounded(){
        assert_eq!(union_owned(rect(4,8,20,10),rect(24,8,6,10),5,5),
                   Ok(rect(4,8,26,10)));
        assert_eq!(union_owned(rect(1270,790,10,10),rect(0,0,2,3),5,5),
                   Ok(rect(0,0,1280,800)));
    }
    #[test]fn rejects_foreign_owner_and_zero_owner(){
        assert_eq!(union_owned(rect(0,0,1,1),rect(0,0,1,1),5,6),
                   Err(RectError::InvalidOwner));
        assert_eq!(union_owned(rect(0,0,1,1),rect(0,0,1,1),0,0),
                   Err(RectError::InvalidOwner));
    }
    #[test]fn rejects_corrupted_dimensions_and_screen_escape(){
        assert_eq!(union_owned(rect(0,0,1,1),rect(1279,0,2,1),1,1),
                   Err(RectError::OutsideScreen));
        assert_eq!(union_owned(rect(u32::MAX,0,2,2),rect(0,0,1,1),1,1),
                   Err(RectError::Overflow));
        assert_eq!(union_owned(rect(0,0,1,1),rect(0,0,0,1),1,1),
                   Err(RectError::Empty));
    }
}


/// Source of truth for a desktop surface is the scheduler's PID and process
/// generation. Matching PID alone is insufficient after process retirement.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct SurfacePrincipal { pub pid:u32, pub generation:u64 }
pub fn clip_session_owned(surface:Rectangle,clip:Rectangle,
    owner:SurfacePrincipal,caller:SurfacePrincipal)
    ->Result<Option<Rectangle>,RectError>{
    if owner.pid==0 || owner.generation==0 || owner!=caller {
        return Err(RectError::InvalidOwner);
    }
    clip_owned(surface,clip,owner.pid,caller.pid)
}
#[cfg(test)]
mod generation_surface_tests {
    use super::*;
    fn principal(generation:u64)->SurfacePrincipal{
        SurfacePrincipal{pid:7,generation}
    }
    fn rect()->Rectangle{Rectangle{x:10,y:10,width:20,height:20}}
    #[test]fn active_session_can_clip_its_owned_surface(){
        assert_eq!(clip_session_owned(rect(),rect(),principal(2),principal(2)),
                   Ok(Some(rect())));
    }
    #[test]fn recycled_pid_does_not_inherit_surface_ownership(){
        assert_eq!(clip_session_owned(rect(),rect(),principal(2),principal(3)),
                   Err(RectError::InvalidOwner));
    }
    #[test]fn empty_identity_and_invalid_bounds_fail_closed(){
        assert_eq!(clip_session_owned(rect(),rect(),principal(0),principal(0)),
                   Err(RectError::InvalidOwner));
        let bad=Rectangle{x:1279,y:0,width:2,height:1};
        assert_eq!(clip_session_owned(bad,rect(),principal(2),principal(2)),
                   Err(RectError::OutsideScreen));
    }
}


// R007: desktop event-routing metadata. Window owners are authenticated by
// the caller's scheduler identity; pixel writes happen in another component.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct WindowHit{pub id:u32,pub owner:SurfacePrincipal,pub bounds:Rectangle,pub z:u16}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum WindowError{Invalid,Full,Duplicate,Missing,Geometry(RectError)}
pub struct WindowMap<const N:usize>{entries:[Option<WindowHit>;N]}
impl<const N:usize> WindowMap<N>{
    pub const fn new()->Self{Self{entries:[None;N]}}
    pub fn add(&mut self,w:WindowHit)->Result<(),WindowError>{
        if w.id==0||w.owner.pid==0||w.owner.generation==0{return Err(WindowError::Invalid);}
        edges(w.bounds).map_err(WindowError::Geometry)?;
        if self.entries.iter().flatten().any(|v|v.id==w.id||v.z==w.z){
            return Err(WindowError::Duplicate);
        }
        let slot=self.entries.iter_mut().find(|v|v.is_none()).ok_or(WindowError::Full)?;
        *slot=Some(w);Ok(())
    }
    pub fn hit(&self,x:u32,y:u32)->Result<Option<WindowHit>,WindowError>{
        let mut chosen:Option<WindowHit>=None;
        // Preflight EVERY rectangle: corrupted low windows cannot evade
        // detection by a valid top window.
        for entry in self.entries.iter().flatten(){
            let (right,bottom)=edges(entry.bounds).map_err(WindowError::Geometry)?;
            if x>=entry.bounds.x&&x<right&&y>=entry.bounds.y&&y<bottom{
                if chosen.is_none_or(|prev|entry.z>prev.z){chosen=Some(*entry);}
            }
        }
        Ok(chosen)
    }
    pub fn move_window(&mut self,id:u32,caller:SurfacePrincipal,
        dx:i32,dy:i32)->Result<Rectangle,WindowError>{
        let slot=self.entries.iter_mut().find(|v|v.is_some_and(|w|w.id==id))
            .ok_or(WindowError::Missing)?;
        let mut w=slot.ok_or(WindowError::Missing)?;
        if caller.pid==0||caller.generation==0||w.owner!=caller{
            return Err(WindowError::Invalid);
        }
        let moved=move_owned(w.bounds,dx,dy,caller.pid,caller.pid)
            .map_err(WindowError::Geometry)?;
        w.bounds=moved;*slot=Some(w);Ok(moved)
    }
    pub fn close(&mut self,id:u32,caller:SurfacePrincipal)->Result<(),WindowError>{
        let slot=self.entries.iter_mut().find(|v|v.is_some_and(|w|w.id==id))
            .ok_or(WindowError::Missing)?;
        if slot.ok_or(WindowError::Missing)?.owner!=caller||caller.pid==0{
            return Err(WindowError::Invalid);
        }
        *slot=None;Ok(())
    }
}
#[cfg(test)]mod r007_pointer_tests{
    use super::*;
    fn p(g:u64)->SurfacePrincipal{SurfacePrincipal{pid:8,generation:g}}
    fn r(x:u32)->Rectangle{Rectangle{x,y:0,width:100,height:100}}
    fn w(id:u32,z:u16,owner:SurfacePrincipal,x:u32)->WindowHit{
        WindowHit{id,z,owner,bounds:r(x)}
    }
    #[test]fn topmost_hit_and_identity_checked_move(){
        let mut map=WindowMap::<3>::new();
        map.add(w(1,1,p(1),0)).unwrap();
        map.add(w(2,2,p(2),50)).unwrap();
        assert_eq!(map.hit(60,20).unwrap().unwrap().id,2);
        assert_eq!(map.move_window(2,p(1),10,0),Err(WindowError::Invalid));
        map.move_window(2,p(2),100,0).unwrap();
        assert_eq!(map.hit(60,20).unwrap().unwrap().id,1);
        map.close(2,p(2)).unwrap();
        assert!(map.hit(200,20).unwrap().is_none());
    }
    #[test]fn invalid_geometry_and_duplicate_z_never_publish(){
        let mut map=WindowMap::<1>::new();
        map.add(w(1,1,p(1),0)).unwrap();
        assert_eq!(map.add(w(2,1,p(2),5)),Err(WindowError::Duplicate));
        assert_eq!(map.move_window(1,p(1),i32::MAX,0),
            Err(WindowError::Geometry(RectError::OutsideScreen)));
        assert_eq!(map.hit(5,5).unwrap().unwrap().id,1);
    }
}


// R008 desktop session state; GPU scanout and input-driver hookup remain separate.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct SessionWindow{pub window:WindowHit,pub minimized:bool}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum SessionFault{Invalid,Missing,Duplicate,Full,Geometry(RectError)}
#[derive(Clone,Copy)]
pub struct FocusSession<const N:usize>{
 windows:[Option<SessionWindow>;N],focus:Option<u32>,
}
impl<const N:usize> FocusSession<N>{
 pub const fn new()->Self{Self{windows:[None;N],focus:None}}
 pub fn focused(&self)->Option<u32>{self.focus}
 pub fn visible(&self)->usize{self.windows.iter().flatten().filter(|v|!v.minimized).count()}
 pub fn open(&mut self,w:WindowHit)->Result<(),SessionFault>{
  if w.id==0||w.owner.pid==0||w.owner.generation==0{return Err(SessionFault::Invalid)}
  edges(w.bounds).map_err(SessionFault::Geometry)?;
  if self.windows.iter().flatten().any(|v|v.window.id==w.id||v.window.z==w.z){
   return Err(SessionFault::Duplicate);
  }
  let slot=self.windows.iter_mut().find(|v|v.is_none()).ok_or(SessionFault::Full)?;
  *slot=Some(SessionWindow{window:w,minimized:false});self.focus=Some(w.id);Ok(())
 }
 pub fn hit(&self,x:u32,y:u32)->Result<Option<WindowHit>,SessionFault>{
  let mut top:Option<WindowHit>=None;
  for v in self.windows.iter().flatten(){
   let (right,bottom)=edges(v.window.bounds).map_err(SessionFault::Geometry)?;
   if !v.minimized&&x>=v.window.bounds.x&&x<right&&
    y>=v.window.bounds.y&&y<bottom&&
    top.is_none_or(|old|v.window.z>old.z){top=Some(v.window)}
  }
  Ok(top)
 }
 pub fn focus_at(&mut self,x:u32,y:u32)->Result<Option<u32>,SessionFault>{
  let next=self.hit(x,y)?.map(|w|w.id);self.focus=next;Ok(next)
 }
 pub fn keyboard_owner(&self)->Option<SurfacePrincipal>{
  self.windows.iter().flatten().find(|v|!v.minimized&&
   Some(v.window.id)==self.focus).map(|v|v.window.owner)
 }
 pub fn minimize(&mut self,id:u32,who:SurfacePrincipal,hide:bool)
  ->Result<(),SessionFault>{
  let slot=self.windows.iter_mut().find(|x|x.is_some_and(|v|v.window.id==id))
   .ok_or(SessionFault::Missing)?;
  let v=slot.as_mut().ok_or(SessionFault::Missing)?;
  if who.pid==0||who!=v.window.owner{return Err(SessionFault::Invalid)}
  v.minimized=hide;
  if hide&&self.focus==Some(id){self.focus=None}
  if !hide{self.focus=Some(id)}
  Ok(())
 }
 pub fn close(&mut self,id:u32,who:SurfacePrincipal)->Result<(),SessionFault>{
  let slot=self.windows.iter_mut().find(|x|x.is_some_and(|v|v.window.id==id))
   .ok_or(SessionFault::Missing)?;
  if who.pid==0||slot.as_ref().ok_or(SessionFault::Missing)?.window.owner!=who{
   return Err(SessionFault::Invalid)
  }
  *slot=None;if self.focus==Some(id){self.focus=None}Ok(())
 }
}
#[cfg(test)]mod r008_desktop_focus_tests{
 use super::*;
 fn p(pid:u32,generation:u64)->SurfacePrincipal{SurfacePrincipal{pid,generation}}
 fn w(id:u32,z:u16,owner:SurfacePrincipal,x:u32)->WindowHit{
  WindowHit{id,z,owner,bounds:Rectangle{x,y:0,width:100,height:100}}
 }
 #[test]fn overlap_focus_and_keyboard_isolation(){
  let mut d=FocusSession::<2>::new();
  d.open(w(1,1,p(8,2),0)).unwrap();d.open(w(2,2,p(9,3),20)).unwrap();
  assert_eq!(d.focus_at(30,10),Ok(Some(2)));
  assert_eq!(d.keyboard_owner(),Some(p(9,3)));
  assert_eq!(d.minimize(2,p(9,2),true),Err(SessionFault::Invalid));
  d.minimize(2,p(9,3),true).unwrap();
  assert_eq!(d.focus_at(30,10),Ok(Some(1)));
  assert_eq!(d.keyboard_owner(),Some(p(8,2)));
 }
 #[test]fn minimize_restore_and_owner_scoped_close(){
  let mut d=FocusSession::<1>::new();
  d.open(w(1,1,p(7,1),0)).unwrap();
  d.minimize(1,p(7,1),true).unwrap();
  assert_eq!(d.visible(),0);
  d.minimize(1,p(7,1),false).unwrap();
  assert_eq!(d.focused(),Some(1));
  assert_eq!(d.close(1,p(7,2)),Err(SessionFault::Invalid));
  d.close(1,p(7,1)).unwrap();assert_eq!(d.visible(),0);
 }
 #[test]fn invalid_geometry_is_never_published(){
  let mut d=FocusSession::<1>::new();
  let invalid=WindowHit{id:1,z:1,owner:p(1,1),
   bounds:Rectangle{x:1280,y:0,width:10,height:10}};
  assert_eq!(d.open(invalid),Err(SessionFault::Geometry(RectError::OutsideScreen)));
  assert_eq!(d.focused(),None);
 }
}

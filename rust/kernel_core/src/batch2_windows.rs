//! R036/14: authenticated per-process window/surface ownership and geometry.
//! This is a no_std window-server foundation, not a ring-3 compositor.
pub const WIDTH:u32=1280;
pub const HEIGHT:u32=800;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Rect{pub x:u32,pub y:u32,pub w:u32,pub h:u32}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Window{pub pid:u32,pub generation:u64,pub id:u32,pub rect:Rect}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum WindowError{Invalid,Duplicate,Full,Denied}
fn valid(r:Rect)->bool {
    r.w>0 && r.h>0 && r.x.checked_add(r.w).is_some_and(|v|v<=WIDTH)
        && r.y.checked_add(r.h).is_some_and(|v|v<=HEIGHT)
}
pub struct Windows<const N:usize>{slots:[Option<Window>;N]}
impl<const N:usize> Windows<N>{
    pub const fn new()->Self{Self{slots:[None;N]}}
    pub fn create(&mut self,window:Window)->Result<(),WindowError>{
        if window.pid==0||window.generation==0||window.id==0||!valid(window.rect){
            return Err(WindowError::Invalid);
        }
        if self.slots.iter().flatten().any(|existing|existing.id==window.id){
            return Err(WindowError::Duplicate);
        }
        let free=self.slots.iter_mut().find(|entry|entry.is_none())
            .ok_or(WindowError::Full)?;
        *free=Some(window);Ok(())
    }
    pub fn owns(&self,pid:u32,generation:u64,id:u32)->bool{
        pid!=0&&generation!=0&&id!=0&&self.slots.iter().flatten()
            .any(|window|window.pid==pid&&window.generation==generation&&window.id==id)
    }
    pub fn move_to(&mut self,pid:u32,generation:u64,id:u32,rect:Rect)
        ->Result<(),WindowError>{
        if !valid(rect){return Err(WindowError::Invalid);}
        let slot=self.slots.iter_mut().flatten().find(|item|
            item.pid==pid&&item.generation==generation&&item.id==id&&pid!=0)
            .ok_or(WindowError::Denied)?;
        slot.rect=rect;Ok(())
    }
    pub fn get(&self,id:u32)->Option<Window>{
        self.slots.iter().flatten().copied().find(|s|s.id==id&&id!=0)
    }
    pub fn retire(&mut self,pid:u32,generation:u64){
        if pid==0||generation==0{return;}
        for slot in &mut self.slots{
            if slot.is_some_and(|s|s.pid==pid&&s.generation==generation){*slot=None;}
        }
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    fn w()->Window{Window{pid:10,generation:2,id:5,
        rect:Rect{x:1,y:1,w:50,h:40}}}
    #[test] fn owner_only_can_change_geometry(){
        let mut t=Windows::<2>::new();t.create(w()).unwrap();
        assert!(!t.owns(10,1,5));
        assert_eq!(t.move_to(20,2,5,w().rect),Err(WindowError::Denied));
        let moved=Rect{x:20,y:30,w:100,h:80};
        t.move_to(10,2,5,moved).unwrap();
        assert_eq!(t.get(5).unwrap().rect,moved);
    }
    #[test] fn overflow_and_duplicate_rejected(){
        let mut t=Windows::<1>::new();
        let mut invalid=w();invalid.rect.x=u32::MAX;
        assert_eq!(t.create(invalid),Err(WindowError::Invalid));
        t.create(w()).unwrap();
        assert_eq!(t.create(w()),Err(WindowError::Duplicate));
        assert_eq!(t.move_to(10,2,5,invalid.rect),Err(WindowError::Invalid));
    }
    #[test] fn stale_pid_generation_cannot_inherit_window(){
        let mut t=Windows::<1>::new();t.create(w()).unwrap();t.retire(10,1);
        assert!(t.owns(10,2,5));t.retire(10,2);
        assert!(t.get(5).is_none());
        let mut n=w();n.generation=3;t.create(n).unwrap();
        assert!(!t.owns(10,2,5));assert!(t.owns(10,3,5));
    }
}


/// R017 multi-window developer workspace: all requested geometry changes
/// succeed together or the layout remains completely unchanged.
impl<const N:usize> Windows<N>{
 pub fn move_batch(&mut self,pid:u32,generation:u64,
  updates:&[(u32,Rect)])->Result<usize,WindowError>{
  if pid==0||generation==0||updates.is_empty()||updates.len()>N{
   return Err(WindowError::Invalid);
  }
  let mut shadow=Self{slots:self.slots};
  for (i,&(id,rect)) in updates.iter().enumerate(){
   if updates[..i].iter().any(|(prior,_)|*prior==id){
    return Err(WindowError::Duplicate);
   }
   shadow.move_to(pid,generation,id,rect)?;
  }
  self.slots=shadow.slots;Ok(updates.len())
 }
 pub fn tile_two(&mut self,pid:u32,generation:u64,
  left:u32,right:u32)->Result<(),WindowError>{
  if left==right{return Err(WindowError::Duplicate);}
  let mid=WIDTH/2;
  self.move_batch(pid,generation,&[
   (left,Rect{x:0,y:0,w:mid,h:HEIGHT}),
   (right,Rect{x:mid,y:0,w:WIDTH-mid,h:HEIGHT})
  ])?;Ok(())
 }
}
#[cfg(test)]mod r017_workstation_tiling_tests{
 use super::*;
 fn w(id:u32,p:u32)->Window{Window{pid:p,generation:1,id,
  rect:Rect{x:1,y:1,w:100,h:100}}}
 #[test]fn editor_and_terminal_tile_as_two_windows(){
  let mut a=Windows::<2>::new();a.create(w(1,10)).unwrap();a.create(w(2,10)).unwrap();
  a.tile_two(10,1,1,2).unwrap();
  assert_eq!(a.get(1).unwrap().rect.w,WIDTH/2);
  assert_eq!(a.get(2).unwrap().rect.x,WIDTH/2);
 }
 #[test]fn unauthorized_second_window_cannot_move_first(){
  let mut a=Windows::<2>::new();a.create(w(1,10)).unwrap();a.create(w(2,11)).unwrap();
  assert!(a.tile_two(10,1,1,2).is_err());
  assert_eq!(a.get(1).unwrap().rect.x,1);
 }
 #[test]fn duplicate_ids_and_invalid_rectangle_fail_atomically(){
  let mut a=Windows::<1>::new();a.create(w(1,10)).unwrap();
  assert_eq!(a.move_batch(10,1,&[(1,Rect{x:0,y:0,w:10,h:10}),
   (1,Rect{x:0,y:0,w:12,h:12})]),Err(WindowError::Invalid));
  assert_eq!(a.get(1).unwrap().rect.x,1);
 }
}

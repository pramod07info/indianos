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



/// R085: transactional window registration, optimistic owner geometry and
/// trusted compositor hit-testing. No direct framebuffer access is granted.
impl<const N:usize> Windows<N>{
    pub fn create_batch(&mut self,windows:&[Window])->Result<usize,WindowError>{
        if windows.is_empty(){return Err(WindowError::Invalid);}
        let mut staged=Self{slots:self.slots};
        for &window in windows{staged.create(window)?;}
        self.slots=staged.slots;
        Ok(windows.len())
    }
    /// Closing a window requires the exact owning PID and process epoch.
    pub fn close_exact(&mut self,pid:u32,generation:u64,id:u32)
        ->Result<(),WindowError>{
        if pid==0||generation==0||id==0{return Err(WindowError::Invalid);}
        let slot=self.slots.iter_mut().find(|s|s.is_some_and(|w|
            w.pid==pid&&w.generation==generation&&w.id==id))
            .ok_or(WindowError::Denied)?;
        *slot=None;
        Ok(())
    }
    /// Optimistic geometry update refuses stale UI state.
    pub fn move_if_rect(&mut self,pid:u32,generation:u64,id:u32,
        expected:Rect,replacement:Rect)->Result<(),WindowError>{
        if !valid(replacement){return Err(WindowError::Invalid);}
        let current=self.get(id).ok_or(WindowError::Denied)?;
        if current.pid!=pid||current.generation!=generation||
            pid==0||current.rect!=expected{return Err(WindowError::Denied);}
        self.move_to(pid,generation,id,replacement)
    }
    pub fn owner_window_count(&self,pid:u32,generation:u64)->usize{
        if pid==0||generation==0{return 0;}
        self.slots.iter().flatten().filter(|w|
            w.pid==pid&&w.generation==generation).count()
    }
    /// Trusted compositor only: newest registered window wins a pixel hit.
    /// This is static registration z-order, not dynamic focus stacking.
    pub fn hit_test(&self,x:u32,y:u32)->Option<Window>{
        if x>=WIDTH||y>=HEIGHT{return None;}
        self.slots.iter().rev().flatten().copied().find(|w|
            x>=w.rect.x&&y>=w.rect.y&&
            x<w.rect.x+w.rect.w&&y<w.rect.y+w.rect.h)
    }
}
#[cfg(test)]
mod r085_window_acceptance{
 use super::*;
 fn win(pid:u32,epoch:u64,id:u32,x:u32)->Window{
    Window{pid,generation:epoch,id,rect:Rect{x,y:0,w:100,h:80}}
 }
 #[test]fn batch_create_is_all_or_nothing_even_at_capacity(){
    let mut w=Windows::<2>::new();
    assert_eq!(w.create_batch(&[win(3,1,1,0),win(3,1,1,100)]),
        Err(WindowError::Duplicate));
    assert_eq!(w.owner_window_count(3,1),0);
    assert_eq!(w.create_batch(&[win(3,1,1,0),win(4,2,2,20)]),Ok(2));
    assert_eq!(w.hit_test(30,1).unwrap().id,2);
    assert_eq!(w.create_batch(&[win(3,1,3,100)]),Err(WindowError::Full));
 }
 #[test]fn stale_geometry_and_process_lease_cannot_modify_other_window(){
    let mut w=Windows::<2>::new();
    let first=win(3,1,1,0);
    w.create(first).unwrap();
    let moved=Rect{x:10,y:20,w:100,h:80};
    assert_eq!(w.move_if_rect(3,2,1,first.rect,moved),
        Err(WindowError::Denied));
    assert_eq!(w.move_if_rect(3,1,1,Rect{x:1,..first.rect},moved),
        Err(WindowError::Denied));
    assert_eq!(w.get(1).unwrap().rect,first.rect);
    w.move_if_rect(3,1,1,first.rect,moved).unwrap();
    assert_eq!(w.close_exact(3,2,1),Err(WindowError::Denied));
    w.close_exact(3,1,1).unwrap();
    assert_eq!(w.hit_test(30,30),None);
 }
}


/// R085 extension: strictly bounded window ownership, atomic multiwindow
/// layout, and trusted hit testing. Real ring-3 compositor delivery is not
/// implemented by these pure policy methods.
impl<const N:usize> Windows<N>{
    pub fn window_count(&self)->usize{self.slots.iter().flatten().count()}
    pub fn free_slots(&self)->usize{N-self.window_count()}
    pub fn is_empty(&self)->bool{self.window_count()==0}
    pub fn is_full(&self)->bool{self.window_count()==N}
    pub fn first_window(&self)->Option<Window>{
        self.slots.iter().flatten().copied().next()
    }
    pub fn last_window(&self)->Option<Window>{
        self.slots.iter().rev().flatten().copied().next()
    }
    pub fn first_owner_window(&self,pid:u32,epoch:u64)->Option<Window>{
        if pid==0||epoch==0{return None;}
        self.slots.iter().flatten().copied().find(|w|
            w.pid==pid&&w.generation==epoch)
    }
    pub fn last_owner_window(&self,pid:u32,epoch:u64)->Option<Window>{
        if pid==0||epoch==0{return None;}
        self.slots.iter().rev().flatten().copied().find(|w|
            w.pid==pid&&w.generation==epoch)
    }
    pub fn owner_window_at(&self,pid:u32,epoch:u64,index:usize)->Option<Window>{
        if pid==0||epoch==0{return None;}
        self.slots.iter().flatten().copied().filter(|w|
            w.pid==pid&&w.generation==epoch).nth(index)
    }
    pub fn snapshot_owner(&self,pid:u32,epoch:u64,
        out:&mut[Window])->Result<usize,WindowError>{
        if pid==0||epoch==0{return Err(WindowError::Denied);}
        let count=self.owner_window_count(pid,epoch);
        if out.len()<count{return Err(WindowError::Full);}
        let mut n=0;
        for window in self.slots.iter().flatten(){
            if window.pid==pid&&window.generation==epoch{
                out[n]=*window;n+=1;
            }
        }
        Ok(n)
    }
    /// Close all selected windows or none; no partial close on stale owner.
    pub fn close_batch_exact(&mut self,pid:u32,epoch:u64,ids:&[u32])
        ->Result<usize,WindowError>{
        if pid==0||epoch==0||ids.is_empty()||ids.len()>N{
            return Err(WindowError::Invalid);
        }
        let mut staged=Self{slots:self.slots};
        for (i,&id) in ids.iter().enumerate(){
            if ids[..i].contains(&id){return Err(WindowError::Duplicate);}
            staged.close_exact(pid,epoch,id)?;
        }
        self.slots=staged.slots;
        Ok(ids.len())
    }
    /// Reject an entire layout if any window changed since it was read.
    pub fn move_batch_cas(&mut self,pid:u32,epoch:u64,
        items:&[(u32,Rect,Rect)])->Result<usize,WindowError>{
        if pid==0||epoch==0||items.is_empty()||items.len()>N{
            return Err(WindowError::Invalid);
        }
        let mut staged=Self{slots:self.slots};
        for (i,&(id,old,next)) in items.iter().enumerate(){
            if items[..i].iter().any(|(prior,_,_)|*prior==id){
                return Err(WindowError::Duplicate);
            }
            staged.move_if_rect(pid,epoch,id,old,next)?;
        }
        self.slots=staged.slots;
        Ok(items.len())
    }
    pub fn translate_exact(&mut self,pid:u32,epoch:u64,id:u32,
        dx:i32,dy:i32)->Result<(),WindowError>{
        let item=self.get(id).ok_or(WindowError::Denied)?;
        if !self.owns(pid,epoch,id){return Err(WindowError::Denied);}
        let x=i64::from(item.rect.x)+i64::from(dx);
        let y=i64::from(item.rect.y)+i64::from(dy);
        if x<0||y<0||x>i64::from(u32::MAX)||y>i64::from(u32::MAX){
            return Err(WindowError::Invalid);
        }
        self.move_to(pid,epoch,id,Rect{x:x as u32,y:y as u32,..item.rect})
    }
    pub fn resize_exact(&mut self,pid:u32,epoch:u64,id:u32,
        width:u32,height:u32)->Result<(),WindowError>{
        let current=self.get(id).ok_or(WindowError::Denied)?;
        if !self.owns(pid,epoch,id){return Err(WindowError::Denied);}
        self.move_to(pid,epoch,id,Rect{w:width,h:height,..current.rect})
    }
    pub fn move_centered(&mut self,pid:u32,epoch:u64,id:u32,
        width:u32,height:u32)->Result<(),WindowError>{
        if width==0||height==0||width>WIDTH||height>HEIGHT{
            return Err(WindowError::Invalid);
        }
        self.move_to(pid,epoch,id,Rect{x:(WIDTH-width)/2,
            y:(HEIGHT-height)/2,w:width,h:height})
    }
    pub fn snap_left(&mut self,pid:u32,epoch:u64,id:u32)
        ->Result<(),WindowError>{
        self.move_to(pid,epoch,id,Rect{x:0,y:0,w:WIDTH/2,h:HEIGHT})
    }
    pub fn snap_right(&mut self,pid:u32,epoch:u64,id:u32)
        ->Result<(),WindowError>{
        self.move_to(pid,epoch,id,Rect{x:WIDTH/2,y:0,w:WIDTH-WIDTH/2,h:HEIGHT})
    }
    pub fn overlap_count(&self,id:u32)->Option<usize>{
        let item=self.get(id)?;
        let r=item.rect;
        Some(self.slots.iter().flatten().filter(|w|w.id!=id&&
            r.x<w.rect.x+w.rect.w&&w.rect.x<r.x+r.w&&
            r.y<w.rect.y+w.rect.h&&w.rect.y<r.y+r.h).count())
    }
    /// Kernel compositor only: see only the authenticated owner's window
    /// under the pointer. Never give this method to an untrusted caller.
    pub fn hit_test_owner(&self,pid:u32,epoch:u64,x:u32,y:u32)->Option<Window>{
        if pid==0||epoch==0||x>=WIDTH||y>=HEIGHT{return None;}
        self.slots.iter().rev().flatten().copied().find(|w|
            w.pid==pid&&w.generation==epoch&&
            x>=w.rect.x&&y>=w.rect.y&&
            x<w.rect.x+w.rect.w&&y<w.rect.y+w.rect.h)
    }
    pub fn validate_state(&self)->Result<(),WindowError>{
        for (index,item) in self.slots.iter().enumerate(){
            let Some(window)=item else{continue};
            if window.pid==0||window.generation==0||window.id==0||
                !valid(window.rect){return Err(WindowError::Invalid);}
            if self.slots[..index].iter().flatten().any(|old|
                old.id==window.id){return Err(WindowError::Duplicate);}
        }
        Ok(())
    }
}
#[cfg(test)]
mod r085_extension_windows_tests{
 use super::*;
 fn w(pid:u32,gen:u64,id:u32,x:u32)->Window{
    Window{pid,generation:gen,id,rect:Rect{x,y:10,w:120,h:90}}
 }
 const EMPTY:Window=Window{pid:0,generation:0,id:0,
    rect:Rect{x:0,y:0,w:0,h:0}};
 #[test]fn multiwindow_layout_is_atomic_and_owner_scoped(){
    let mut ws=Windows::<3>::new();
    assert!(ws.is_empty());assert_eq!(ws.free_slots(),3);
    ws.create_batch(&[w(3,1,1,0),w(3,1,2,100),w(4,2,3,200)]).unwrap();
    assert!(ws.is_full());assert_eq!(ws.window_count(),3);
    assert_eq!(ws.first_window().unwrap().id,1);
    assert_eq!(ws.last_window().unwrap().id,3);
    assert_eq!(ws.first_owner_window(3,1).unwrap().id,1);
    assert_eq!(ws.last_owner_window(3,1).unwrap().id,2);
    assert_eq!(ws.owner_window_at(3,1,1).unwrap().id,2);
    assert_eq!(ws.owner_window_at(3,1,2),None);
    assert_eq!(ws.overlap_count(1),Some(1));
    assert_eq!(ws.hit_test_owner(3,1,110,15).unwrap().id,2);
    assert_eq!(ws.hit_test_owner(4,2,110,15),None);
    let mut short=[EMPTY;1];
    assert_eq!(ws.snapshot_owner(3,1,&mut short),Err(WindowError::Full));
    assert_eq!(short,[EMPTY]);
    let mut out=[EMPTY;3];
    assert_eq!(ws.snapshot_owner(3,1,&mut out),Ok(2));
    assert_eq!(out[0].id,1);assert_eq!(out[1].id,2);
    assert_eq!(ws.validate_state(),Ok(()));
    let previous=ws.get(1).unwrap().rect;
    assert_eq!(ws.move_batch_cas(3,1,&[
        (1,previous,Rect{x:10,..previous}),
        (2,Rect{x:90,..previous},Rect{x:30,..previous})
    ]),Err(WindowError::Denied));
    assert_eq!(ws.get(1).unwrap().rect,previous);
    assert_eq!(ws.close_batch_exact(3,1,&[1,3]),Err(WindowError::Denied));
    assert_eq!(ws.window_count(),3);
    assert_eq!(ws.close_batch_exact(3,1,&[1,2]),Ok(2));
    assert_eq!(ws.window_count(),1);
 }
 #[test]fn geometry_rejects_unsafe_transforms_and_supports_tiling(){
    let mut ws=Windows::<2>::new();ws.create(w(3,1,1,10)).unwrap();
    assert_eq!(ws.translate_exact(3,1,1,-100,0),Err(WindowError::Invalid));
    assert_eq!(ws.resize_exact(3,1,1,0,200),Err(WindowError::Invalid));
    assert_eq!(ws.move_centered(3,1,1,WIDTH+1,100),Err(WindowError::Invalid));
    ws.translate_exact(3,1,1,10,10).unwrap();
    assert_eq!(ws.get(1).unwrap().rect.x,20);
    ws.resize_exact(3,1,1,40,30).unwrap();
    assert_eq!(ws.get(1).unwrap().rect.w,40);
    ws.move_centered(3,1,1,300,200).unwrap();
    assert_eq!(ws.get(1).unwrap().rect.x,(WIDTH-300)/2);
    ws.snap_left(3,1,1).unwrap();
    assert_eq!(ws.get(1).unwrap().rect.w,WIDTH/2);
    ws.snap_right(3,1,1).unwrap();
    assert_eq!(ws.get(1).unwrap().rect.x,WIDTH/2);
    assert_eq!(ws.snap_left(3,2,1),Err(WindowError::Denied));
 }
}


/// R085 extension III: transactional window lifetime and trusted compositor
/// layout primitives. Registration slot ordering determines static z-order;
/// process creation, physical display, and ring-3 rendering remain separate.
impl<const N:usize> Windows<N>{
    pub fn area_of(&self,id:u32)->Option<u64>{
        self.get(id).map(|w|u64::from(w.rect.w)*u64::from(w.rect.h))
    }
    pub fn owner_area(&self,pid:u32,epoch:u64)->u64{
        if pid==0||epoch==0{return 0;}
        self.slots.iter().flatten().filter(|w|
            w.pid==pid&&w.generation==epoch).map(|w|
            u64::from(w.rect.w)*u64::from(w.rect.h)).sum()
    }
    pub fn owner_has_window(&self,pid:u32,epoch:u64)->bool{
        self.owner_window_count(pid,epoch)>0
    }
    pub fn window_at(&self,index:usize)->Option<Window>{
        self.slots.iter().flatten().copied().nth(index)
    }
    pub fn snapshot_all(&self,out:&mut[Window])->Result<usize,WindowError>{
        let count=self.window_count();
        if out.len()<count{return Err(WindowError::Full);}
        for (i,w) in self.slots.iter().flatten().enumerate(){out[i]=*w;}
        Ok(count)
    }
    pub fn window_contains(&self,id:u32,x:u32,y:u32)->bool{
        self.get(id).is_some_and(|w|x>=w.rect.x&&y>=w.rect.y&&
            x<w.rect.x+w.rect.w&&y<w.rect.y+w.rect.h)
    }
    pub fn windows_overlap(&self,a:u32,b:u32)->bool{
        if a==b{return false;}
        let (Some(a),Some(b))=(self.get(a),self.get(b))else{return false};
        let (a,b)=(a.rect,b.rect);
        a.x<b.x+b.w&&b.x<a.x+a.w&&a.y<b.y+b.h&&b.y<a.y+a.h
    }
    pub fn intersection(&self,a:u32,b:u32)->Option<Rect>{
        if a==b{return None;}
        let (a,b)=(self.get(a)?.rect,self.get(b)?.rect);
        let (x,y)=(a.x.max(b.x),a.y.max(b.y));
        let right=(a.x+a.w).min(b.x+b.w);
        let bottom=(a.y+a.h).min(b.y+b.h);
        if x>=right||y>=bottom{return None;}
        Some(Rect{x,y,w:right-x,h:bottom-y})
    }
    pub fn count_at(&self,x:u32,y:u32)->usize{
        self.slots.iter().flatten().filter(|w|
            x>=w.rect.x&&y>=w.rect.y&&
            x<w.rect.x+w.rect.w&&y<w.rect.y+w.rect.h).count()
    }
    pub fn owner_count_at(&self,pid:u32,epoch:u64,x:u32,y:u32)->usize{
        if pid==0||epoch==0{return 0;}
        self.slots.iter().flatten().filter(|w|w.pid==pid&&
            w.generation==epoch&&x>=w.rect.x&&y>=w.rect.y&&
            x<w.rect.x+w.rect.w&&y<w.rect.y+w.rect.h).count()
    }
    /// Rebuild only ordering of live windows. The trusted compositor owns
    /// this call; client-provided IDs are checked against exact PID epochs.
    pub fn raise_exact(&mut self,pid:u32,epoch:u64,id:u32)
        ->Result<(),WindowError>{
        if !self.owns(pid,epoch,id){return Err(WindowError::Denied);}
        let mut ordered=[None;N];let mut count=0;let mut selected=None;
        for window in self.slots.iter().flatten(){
            if window.id==id{selected=Some(*window);}
            else{ordered[count]=Some(*window);count+=1;}
        }
        ordered[count]=selected;self.slots=ordered;Ok(())
    }
    pub fn lower_exact(&mut self,pid:u32,epoch:u64,id:u32)
        ->Result<(),WindowError>{
        if !self.owns(pid,epoch,id){return Err(WindowError::Denied);}
        let mut ordered=[None;N];let mut count=0;
        ordered[0]=self.get(id);count+=1;
        for window in self.slots.iter().flatten(){
            if window.id!=id{ordered[count]=Some(*window);count+=1;}
        }
        self.slots=ordered;Ok(())
    }
    pub fn raise_batch_exact(&mut self,pid:u32,epoch:u64,ids:&[u32])
        ->Result<usize,WindowError>{
        if pid==0||epoch==0||ids.is_empty()||ids.len()>N{
            return Err(WindowError::Invalid);
        }
        let mut staged=Self{slots:self.slots};
        for (i,&id) in ids.iter().enumerate(){
            if ids[..i].contains(&id){return Err(WindowError::Duplicate);}
            staged.raise_exact(pid,epoch,id)?;
        }
        self.slots=staged.slots;Ok(ids.len())
    }
    pub fn close_if_rect(&mut self,pid:u32,epoch:u64,id:u32,
        expected:Rect)->Result<(),WindowError>{
        let live=self.get(id).ok_or(WindowError::Denied)?;
        if !self.owns(pid,epoch,id)||live.rect!=expected{
            return Err(WindowError::Denied);
        }
        self.close_exact(pid,epoch,id)
    }
    /// Prevent partial geometry swaps after window resize/focus changes.
    pub fn swap_geometry(&mut self,pid:u32,epoch:u64,
        a:u32,b:u32)->Result<(),WindowError>{
        if a==b{return Err(WindowError::Duplicate);}
        let wa=self.get(a).ok_or(WindowError::Denied)?;
        let wb=self.get(b).ok_or(WindowError::Denied)?;
        if !self.owns(pid,epoch,a)||!self.owns(pid,epoch,b){
            return Err(WindowError::Denied);
        }
        self.move_batch(pid,epoch,&[(a,wb.rect),(b,wa.rect)])?;
        Ok(())
    }
    pub fn tile_top_bottom(&mut self,pid:u32,epoch:u64,
        top:u32,bottom:u32)->Result<(),WindowError>{
        if top==bottom{return Err(WindowError::Duplicate);}
        self.move_batch(pid,epoch,&[
            (top,Rect{x:0,y:0,w:WIDTH,h:HEIGHT/2}),
            (bottom,Rect{x:0,y:HEIGHT/2,w:WIDTH,h:HEIGHT-HEIGHT/2}),
        ])?;
        Ok(())
    }
    pub fn snap_top(&mut self,pid:u32,epoch:u64,id:u32)
        ->Result<(),WindowError>{
        self.move_to(pid,epoch,id,Rect{x:0,y:0,w:WIDTH,h:HEIGHT/2})
    }
    pub fn snap_bottom(&mut self,pid:u32,epoch:u64,id:u32)
        ->Result<(),WindowError>{
        self.move_to(pid,epoch,id,Rect{x:0,y:HEIGHT/2,
            w:WIDTH,h:HEIGHT-HEIGHT/2})
    }
    /// Keep current window size while clamping its origin within display.
    pub fn move_clamped(&mut self,pid:u32,epoch:u64,id:u32,
        x:u32,y:u32)->Result<(),WindowError>{
        let live=self.get(id).ok_or(WindowError::Denied)?;
        if !self.owns(pid,epoch,id){return Err(WindowError::Denied);}
        let rect=Rect{x:x.min(WIDTH-live.rect.w),
            y:y.min(HEIGHT-live.rect.h),..live.rect};
        self.move_to(pid,epoch,id,rect)
    }
    pub fn union_bounds(&self,a:u32,b:u32)->Option<Rect>{
        let (a,b)=(self.get(a)?.rect,self.get(b)?.rect);
        let (x,y)=(a.x.min(b.x),a.y.min(b.y));
        let right=(a.x+a.w).max(b.x+b.w);
        let bottom=(a.y+a.h).max(b.y+b.h);
        Some(Rect{x,y,w:right-x,h:bottom-y})
    }
}
#[cfg(test)]mod r085_extension3_window_tests{
    use super::*;
    const NONE:Window=Window{pid:0,generation:0,id:0,
        rect:Rect{x:0,y:0,w:0,h:0}};
    fn w(id:u32,owner:u32,x:u32)->Window{
        Window{pid:owner,generation:1,id,
            rect:Rect{x,y:0,w:100,h:100}}
    }
    #[test]fn z_order_is_real_and_never_crosses_owners(){
        let mut ws=Windows::<4>::new();
        ws.create_batch(&[w(1,2,0),w(2,2,50),w(3,3,80)]).unwrap();
        assert_eq!(ws.area_of(1),Some(10000));
        assert_eq!(ws.owner_area(2,1),20000);
        assert!(ws.owner_has_window(2,1));
        assert_eq!(ws.window_at(1).unwrap().id,2);
        assert!(ws.window_contains(2,80,10));
        assert!(ws.windows_overlap(1,2));
        assert_eq!(ws.intersection(1,2),Some(Rect{x:50,y:0,w:50,h:100}));
        assert_eq!(ws.union_bounds(1,2),Some(Rect{x:0,y:0,w:150,h:100}));
        assert_eq!(ws.count_at(90,10),3);
        assert_eq!(ws.owner_count_at(2,1,90,10),2);
        let mut short=[NONE;1];
        assert_eq!(ws.snapshot_all(&mut short),Err(WindowError::Full));
        assert_eq!(short,[NONE]);
        let mut out=[NONE;4];
        assert_eq!(ws.snapshot_all(&mut out),Ok(3));
        assert_eq!(ws.hit_test(90,10).unwrap().id,3);
        assert_eq!(ws.raise_exact(2,1,3),Err(WindowError::Denied));
        ws.raise_exact(2,1,1).unwrap();
        assert_eq!(ws.hit_test(90,10).unwrap().id,1);
        ws.lower_exact(2,1,1).unwrap();
        assert_eq!(ws.hit_test(90,10).unwrap().id,3);
        assert_eq!(ws.raise_batch_exact(2,1,&[1,3]),Err(WindowError::Denied));
        assert_eq!(ws.hit_test(90,10).unwrap().id,3);
        ws.close_if_rect(2,1,1,w(1,2,0).rect).unwrap();
        assert_eq!(ws.get(1),None);
    }
    #[test]fn atomic_layout_swap_and_clipping_preserve_owner_geometry(){
        let mut ws=Windows::<3>::new();
        ws.create_batch(&[w(1,2,0),w(2,2,100),w(3,3,1000)]).unwrap();
        let r1=ws.get(1).unwrap().rect;
        assert_eq!(ws.swap_geometry(2,1,1,3),Err(WindowError::Denied));
        assert_eq!(ws.get(1).unwrap().rect,r1);
        ws.swap_geometry(2,1,1,2).unwrap();
        assert_eq!(ws.get(1).unwrap().rect.x,100);
        ws.tile_top_bottom(2,1,1,2).unwrap();
        assert_eq!(ws.get(2).unwrap().rect.y,HEIGHT/2);
        ws.snap_top(2,1,1).unwrap();
        ws.snap_bottom(2,1,1).unwrap();
        assert_eq!(ws.get(1).unwrap().rect.y,HEIGHT/2);
        ws.move_clamped(2,1,1,u32::MAX,u32::MAX).unwrap();
        assert_eq!(ws.get(1).unwrap().rect.x,0);
    }
}

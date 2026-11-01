//! R036/15: process-isolated keyboard routing to a trusted focused window.
//! Hardware PS/2 and terminal injection are deliberately outside this module.
use crate::batch2_windows::Windows;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Focus{pub pid:u32,pub generation:u64,pub window:u32}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct KeyEvent{pub focus:Focus,pub scan_code:u16,pub pressed:bool}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum InputError{NoFocus,Unauthorized,Invalid,Full,Empty}
pub struct Router<const Q:usize>{
    focus:Option<Focus>,queue:[Option<KeyEvent>;Q],len:usize,
}
impl<const Q:usize> Router<Q>{
    pub const fn new()->Self{Self{focus:None,queue:[None;Q],len:0}}
    pub fn set_focus<const W:usize>(&mut self,windows:&Windows<W>,lease:Focus)
        ->Result<(),InputError>{
        if !windows.owns(lease.pid,lease.generation,lease.window){
            return Err(InputError::Unauthorized);
        }
        self.focus=Some(lease);Ok(())
    }
    pub fn physical_key<const W:usize>(&mut self,windows:&Windows<W>,
        scan_code:u16,pressed:bool)->Result<(),InputError>{
        if scan_code==0{return Err(InputError::Invalid);}
        let focus=self.focus.ok_or(InputError::NoFocus)?;
        if !windows.owns(focus.pid,focus.generation,focus.window){
            self.focus=None;return Err(InputError::Unauthorized);
        }
        if self.len==Q{return Err(InputError::Full);}
        self.queue[self.len]=Some(KeyEvent{focus,scan_code,pressed});
        self.len+=1;Ok(())
    }
    pub fn receive(&mut self,pid:u32,generation:u64)
        ->Result<KeyEvent,InputError>{
        let index=self.queue[..self.len].iter().position(|event|event.is_some_and(
            |event|event.focus.pid==pid&&event.focus.generation==generation&&pid!=0))
            .ok_or(InputError::Empty)?;
        let item=self.queue[index].ok_or(InputError::Empty)?;
        for i in index..self.len-1{self.queue[i]=self.queue[i+1];}
        self.len-=1;self.queue[self.len]=None;
        Ok(item)
    }
    pub fn retire(&mut self,pid:u32,generation:u64){
        if self.focus.is_some_and(|f|f.pid==pid&&f.generation==generation){
            self.focus=None;
        }
        let mut idx=0;
        while idx<self.len{
            if self.queue[idx].is_some_and(|e|e.focus.pid==pid&&
                e.focus.generation==generation){
                for i in idx..self.len-1{self.queue[i]=self.queue[i+1];}
                self.len-=1;self.queue[self.len]=None;
            }else{idx+=1;}
        }
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    use crate::batch2_windows::{Window,Rect};
    fn windows()->Windows<2>{
        let mut w=Windows::new();
        w.create(Window{pid:10,generation:2,id:20,
            rect:Rect{x:0,y:0,w:50,h:50}}).unwrap();
        w
    }
    #[test] fn focus_binds_physical_key_to_actual_owner(){
        let w=windows();let mut r=Router::<2>::new();
        let f=Focus{pid:10,generation:2,window:20};
        assert_eq!(r.set_focus(&w,Focus{pid:99,..f}),Err(InputError::Unauthorized));
        r.set_focus(&w,f).unwrap();
        r.physical_key(&w,0x1e,true).unwrap();
        assert_eq!(r.receive(99,2),Err(InputError::Empty));
        assert_eq!(r.receive(10,1),Err(InputError::Empty));
        assert_eq!(r.receive(10,2),Ok(KeyEvent{focus:f,scan_code:0x1e,pressed:true}));
    }
    #[test] fn retired_windows_drop_focus_and_pending_input(){
        let mut w=windows();let mut r=Router::<1>::new();
        r.set_focus(&w,Focus{pid:10,generation:2,window:20}).unwrap();
        r.physical_key(&w,0x1e,true).unwrap();
        w.retire(10,2);
        assert_eq!(r.physical_key(&w,0x1f,true),Err(InputError::Unauthorized));
        r.retire(10,2);
        assert_eq!(r.receive(10,2),Err(InputError::Empty));
        assert_eq!(r.physical_key(&w,0x1e,true),Err(InputError::NoFocus));
    }
    #[test] fn zero_scan_code_and_queue_capacity(){
        let w=windows();let mut r=Router::<1>::new();
        r.set_focus(&w,Focus{pid:10,generation:2,window:20}).unwrap();
        assert_eq!(r.physical_key(&w,0,true),Err(InputError::Invalid));
        r.physical_key(&w,10,true).unwrap();
        assert_eq!(r.physical_key(&w,11,false),Err(InputError::Full));
    }
}



/// R085: live compositor-aware keyboard delivery and atomic physical input.
/// All routing decisions remain in kernel-owned code after PS/2 validation.
impl<const Q:usize> Router<Q>{
    pub fn pending_total(&self)->usize{self.len}
    pub fn pending_for(&self,pid:u32,generation:u64)->usize{
        if pid==0||generation==0{return 0;}
        self.queue[..self.len].iter().flatten().filter(|item|
            item.focus.pid==pid&&item.focus.generation==generation).count()
    }
    pub fn focused(&self)->Option<Focus>{self.focus}
    /// Trusted UI path, clears a focus only when its entire live lease matches.
    pub fn clear_focus_exact(&mut self,focus:Focus)->bool{
        if self.focus==Some(focus){self.focus=None;true}else{false}
    }
    /// Drop key events whose destination window is no longer owned by its
    /// original process incarnation, including recycled-window handles.
    pub fn reap_closed<const W:usize>(&mut self,windows:&Windows<W>)->usize{
        let mut i=0;let mut discarded=0;
        while i<self.len{
            if self.queue[i].is_some_and(|item|!windows.owns(
                item.focus.pid,item.focus.generation,item.focus.window)){
                for j in i+1..self.len{self.queue[j-1]=self.queue[j];}
                self.len-=1;self.queue[self.len]=None;
                discarded+=1;
            }else{i+=1;}
        }
        if self.focus.is_some_and(|f|!windows.owns(
            f.pid,f.generation,f.window)){self.focus=None;}
        discarded
    }
    /// Caller only receives events for its exact PID and generation after
    /// pruning stale window handles; all other owners remain untouched.
    pub fn receive_live<const W:usize>(&mut self,windows:&Windows<W>,
        pid:u32,generation:u64)->Result<KeyEvent,InputError>{
        if pid==0||generation==0{return Err(InputError::Unauthorized);}
        self.reap_closed(windows);
        self.receive(pid,generation)
    }
    /// If one event is invalid or the queue would overflow, no key is sent.
    pub fn physical_batch<const W:usize>(&mut self,windows:&Windows<W>,
        keys:&[(u16,bool)])->Result<usize,InputError>{
        if keys.is_empty(){return Err(InputError::Invalid);}
        let mut staged=Self{focus:self.focus,queue:self.queue,len:self.len};
        for &(code,pressed) in keys{
            staged.physical_key(windows,code,pressed)?;
        }
        *self=staged;
        Ok(keys.len())
    }
}
#[cfg(test)]
mod r085_keyboard_acceptance{
 use super::*;
 use crate::batch2_windows::{Window,Rect};
 fn windows()->Windows<2>{
    let mut w=Windows::new();
    w.create(Window{pid:5,generation:9,id:40,
        rect:Rect{x:1,y:1,w:20,h:20}}).unwrap();
    w
 }
 #[test]fn atomic_physical_input_rejects_invalid_or_full_group(){
    let w=windows();let mut r=Router::<2>::new();
    let focus=Focus{pid:5,generation:9,window:40};
    r.set_focus(&w,focus).unwrap();
    assert_eq!(r.physical_batch(&w,&[(10,true),(0,true)]),
        Err(InputError::Invalid));
    assert_eq!(r.pending_total(),0);
    assert_eq!(r.physical_batch(&w,&[(10,true),(20,false),(30,true)]),
        Err(InputError::Full));
    assert_eq!(r.pending_total(),0);
    assert_eq!(r.physical_batch(&w,&[(10,true),(20,false)]),Ok(2));
    assert_eq!(r.pending_for(5,9),2);
    assert!(!r.clear_focus_exact(Focus{generation:8,..focus}));
    assert!(r.clear_focus_exact(focus));
    assert_eq!(r.pending_total(),2);
 }
 #[test]fn stale_window_input_is_discarded_not_delivered_to_reused_id(){
    let mut w=windows();let mut r=Router::<2>::new();
    let focus=Focus{pid:5,generation:9,window:40};
    r.set_focus(&w,focus).unwrap();
    r.physical_key(&w,10,true).unwrap();
    w.retire(5,9);
    w.create(Window{pid:5,generation:10,id:40,
        rect:Rect{x:1,y:1,w:20,h:20}}).unwrap();
    assert_eq!(r.receive_live(&w,5,9),Err(InputError::Empty));
    assert_eq!(r.pending_total(),0);
    assert_eq!(r.focused(),None);
 }
}


/// R085 extension: per-owner bounded keyboard delivery, diagnostic snapshots
/// and precise stale-event revocation. All events originate in trusted PS/2
/// ingress; user processes cannot inject scan codes using these methods.
impl<const Q:usize> Router<Q>{
    pub fn queue_capacity(&self)->usize{Q}
    pub fn remaining_slots(&self)->usize{Q-self.len}
    pub fn queue_empty(&self)->bool{self.len==0}
    pub fn queue_full(&self)->bool{self.len==Q}
    pub fn is_focused_exact(&self,pid:u32,epoch:u64,window:u32)->bool{
        pid!=0&&epoch!=0&&window!=0&&self.focus==Some(Focus{
            pid,generation:epoch,window})
    }
    pub fn focused_window(&self,pid:u32,epoch:u64)->Option<u32>{
        self.focus.filter(|f|pid!=0&&epoch!=0&&f.pid==pid&&
            f.generation==epoch).map(|f|f.window)
    }
    pub fn pending_window(&self,pid:u32,epoch:u64,window:u32)->usize{
        if pid==0||epoch==0||window==0{return 0;}
        self.queue[..self.len].iter().flatten().filter(|e|
            e.focus.pid==pid&&e.focus.generation==epoch&&
            e.focus.window==window).count()
    }
    pub fn pending_scan(&self,pid:u32,epoch:u64,scan:u16)->usize{
        if pid==0||epoch==0||scan==0{return 0;}
        self.queue[..self.len].iter().flatten().filter(|e|
            e.focus.pid==pid&&e.focus.generation==epoch&&e.scan_code==scan).count()
    }
    pub fn pending_pressed(&self,pid:u32,epoch:u64)->usize{
        if pid==0||epoch==0{return 0;}
        self.queue[..self.len].iter().flatten().filter(|e|
            e.focus.pid==pid&&e.focus.generation==epoch&&e.pressed).count()
    }
    pub fn pending_released(&self,pid:u32,epoch:u64)->usize{
        if pid==0||epoch==0{return 0;}
        self.queue[..self.len].iter().flatten().filter(|e|
            e.focus.pid==pid&&e.focus.generation==epoch&&!e.pressed).count()
    }
    pub fn peek_owner(&self,pid:u32,epoch:u64)->Option<KeyEvent>{
        if pid==0||epoch==0{return None;}
        self.queue[..self.len].iter().flatten().copied().find(|e|
            e.focus.pid==pid&&e.focus.generation==epoch)
    }
    pub fn peek_last_owner(&self,pid:u32,epoch:u64)->Option<KeyEvent>{
        if pid==0||epoch==0{return None;}
        self.queue[..self.len].iter().rev().flatten().copied().find(|e|
            e.focus.pid==pid&&e.focus.generation==epoch)
    }
    pub fn snapshot_owner(&self,pid:u32,epoch:u64,out:&mut[KeyEvent])
        ->Result<usize,InputError>{
        if pid==0||epoch==0{return Err(InputError::Unauthorized);}
        let count=self.pending_for(pid,epoch);
        if out.len()<count{return Err(InputError::Full);}
        let mut n=0;
        for event in self.queue[..self.len].iter().flatten(){
            if event.focus.pid==pid&&event.focus.generation==epoch{
                out[n]=*event;n+=1;
            }
        }
        Ok(n)
    }
    pub fn receive_window(&mut self,pid:u32,epoch:u64,window:u32)
        ->Result<KeyEvent,InputError>{
        if pid==0||epoch==0||window==0{return Err(InputError::Unauthorized);}
        let index=self.queue[..self.len].iter().position(|e|e.is_some_and(|v|
            v.focus.pid==pid&&v.focus.generation==epoch&&
            v.focus.window==window)).ok_or(InputError::Empty)?;
        let event=self.queue[index].ok_or(InputError::Empty)?;
        for j in index+1..self.len{self.queue[j-1]=self.queue[j];}
        self.len-=1;self.queue[self.len]=None;Ok(event)
    }
    pub fn receive_scan(&mut self,pid:u32,epoch:u64,scan:u16)
        ->Result<KeyEvent,InputError>{
        if pid==0||epoch==0||scan==0{return Err(InputError::Unauthorized);}
        let index=self.queue[..self.len].iter().position(|e|e.is_some_and(|v|
            v.focus.pid==pid&&v.focus.generation==epoch&&v.scan_code==scan))
            .ok_or(InputError::Empty)?;
        let event=self.queue[index].ok_or(InputError::Empty)?;
        for j in index+1..self.len{self.queue[j-1]=self.queue[j];}
        self.len-=1;self.queue[self.len]=None;Ok(event)
    }
    pub fn cancel_window(&mut self,pid:u32,epoch:u64,window:u32)
        ->Result<usize,InputError>{
        if pid==0||epoch==0||window==0{return Err(InputError::Unauthorized);}
        let mut n=0;let mut i=0;
        while i<self.len{
            if self.queue[i].is_some_and(|e|e.focus.pid==pid&&
                e.focus.generation==epoch&&e.focus.window==window){
                for j in i+1..self.len{self.queue[j-1]=self.queue[j];}
                self.len-=1;self.queue[self.len]=None;n+=1;
            }else{i+=1;}
        }
        Ok(n)
    }
    pub fn cancel_scan(&mut self,pid:u32,epoch:u64,scan:u16)
        ->Result<usize,InputError>{
        if pid==0||epoch==0||scan==0{return Err(InputError::Unauthorized);}
        let mut n=0;let mut i=0;
        while i<self.len{
            if self.queue[i].is_some_and(|e|e.focus.pid==pid&&
                e.focus.generation==epoch&&e.scan_code==scan){
                for j in i+1..self.len{self.queue[j-1]=self.queue[j];}
                self.len-=1;self.queue[self.len]=None;n+=1;
            }else{i+=1;}
        }
        Ok(n)
    }
    pub fn discard_owner(&mut self,pid:u32,epoch:u64)
        ->Result<usize,InputError>{
        if pid==0||epoch==0{return Err(InputError::Unauthorized);}
        let count=self.pending_for(pid,epoch);
        self.retire(pid,epoch);
        Ok(count)
    }
    /// Supervisor-only global reset after input controller or compositor
    /// failure; avoids delivering stale keys to newly started clients.
    pub fn clear_all_input(&mut self)->usize{
        let removed=self.len;
        self.queue=[None;Q];self.len=0;self.focus=None;
        removed
    }
    pub fn pending_live<const W:usize>(&self,windows:&Windows<W>,
        pid:u32,epoch:u64)->usize{
        if pid==0||epoch==0{return 0;}
        self.queue[..self.len].iter().flatten().filter(|e|
            e.focus.pid==pid&&e.focus.generation==epoch&&windows.owns(
                e.focus.pid,e.focus.generation,e.focus.window)).count()
    }
}
#[cfg(test)]
mod r085_extension_keyboard_tests{
 use super::*;
 use crate::batch2_windows::{Window,Rect};
 fn win()->Windows<2>{
    let mut w=Windows::new();
    w.create(Window{pid:10,generation:2,id:7,
        rect:Rect{x:0,y:0,w:100,h:80}}).unwrap();
    w.create(Window{pid:11,generation:3,id:8,
        rect:Rect{x:0,y:0,w:50,h:30}}).unwrap();
    w
 }
 const EMPTY:KeyEvent=KeyEvent{
    focus:Focus{pid:0,generation:0,window:0},scan_code:0,pressed:false
 };
 #[test]fn live_owner_diagnostics_and_filtered_delivery(){
    let windows=win();let mut r=Router::<4>::new();
    r.set_focus(&windows,Focus{pid:10,generation:2,window:7}).unwrap();
    assert_eq!(r.queue_capacity(),4);
    assert_eq!(r.remaining_slots(),4);
    assert!(r.queue_empty());assert!(!r.queue_full());
    assert!(r.is_focused_exact(10,2,7));
    assert_eq!(r.focused_window(10,2),Some(7));
    assert_eq!(r.focused_window(10,3),None);
    r.physical_batch(&windows,&[(10,true),(10,false),(11,true)]).unwrap();
    assert_eq!(r.pending_window(10,2,7),3);
    assert_eq!(r.pending_scan(10,2,10),2);
    assert_eq!(r.pending_pressed(10,2),2);
    assert_eq!(r.pending_released(10,2),1);
    assert_eq!(r.peek_owner(10,2).unwrap().scan_code,10);
    assert_eq!(r.peek_last_owner(10,2).unwrap().scan_code,11);
    assert_eq!(r.pending_live(&windows,10,2),3);
    let mut tiny=[EMPTY;1];
    assert_eq!(r.snapshot_owner(10,2,&mut tiny),Err(InputError::Full));
    assert_eq!(tiny,[EMPTY]);
    let mut events=[EMPTY;4];
    assert_eq!(r.snapshot_owner(10,2,&mut events),Ok(3));
    assert_eq!(events[1].pressed,false);
    assert_eq!(r.receive_scan(10,2,11).unwrap().scan_code,11);
    assert_eq!(r.receive_window(10,2,7).unwrap().scan_code,10);
    assert_eq!(r.cancel_scan(10,2,10),Ok(1));
    assert_eq!(r.discard_owner(10,2),Ok(0));
    assert!(r.queue_empty());
 }
 #[test]fn owner_cancellation_and_reset_are_generation_bound(){
    let windows=win();let mut r=Router::<4>::new();
    r.set_focus(&windows,Focus{pid:10,generation:2,window:7}).unwrap();
    r.physical_batch(&windows,&[(3,true),(4,false)]).unwrap();
    assert_eq!(r.cancel_window(10,3,7),Ok(0));
    assert_eq!(r.pending_total(),2);
    assert_eq!(r.cancel_window(10,2,7),Ok(2));
    assert_eq!(r.clear_all_input(),0);
    assert_eq!(r.focused(),None);
    assert_eq!(r.receive_scan(0,2,3),Err(InputError::Unauthorized));
 }
}


/// R085 extension III: non-leaking, bounded keyboard queue operations. The
/// device interrupt path remains responsible for validating physical input.
impl<const Q:usize> Router<Q>{
    pub fn can_queue(&self,additional:usize)->bool{
        additional>0&&additional<=self.remaining_slots()
    }
    pub fn owner_has_pending(&self,pid:u32,epoch:u64)->bool{
        self.pending_for(pid,epoch)>0
    }
    pub fn focused_live<const W:usize>(&self,windows:&Windows<W>)->Option<Focus>{
        self.focus.filter(|f|windows.owns(f.pid,f.generation,f.window))
    }
    pub fn stale_count<const W:usize>(&self,windows:&Windows<W>)->usize{
        self.queue[..self.len].iter().flatten().filter(|e|!windows.owns(
            e.focus.pid,e.focus.generation,e.focus.window)).count()
    }
    pub fn first_window_event(&self,pid:u32,epoch:u64,window:u32)
        ->Option<KeyEvent>{
        if pid==0||epoch==0||window==0{return None;}
        self.queue[..self.len].iter().flatten().copied().find(|e|
            e.focus.pid==pid&&e.focus.generation==epoch&&e.focus.window==window)
    }
    pub fn last_window_event(&self,pid:u32,epoch:u64,window:u32)
        ->Option<KeyEvent>{
        if pid==0||epoch==0||window==0{return None;}
        self.queue[..self.len].iter().rev().flatten().copied().find(|e|
            e.focus.pid==pid&&e.focus.generation==epoch&&e.focus.window==window)
    }
    pub fn scan_pressed_count(&self,pid:u32,epoch:u64,scan:u16)->usize{
        if pid==0||epoch==0||scan==0{return 0;}
        self.queue[..self.len].iter().flatten().filter(|e|
            e.focus.pid==pid&&e.focus.generation==epoch&&
            e.scan_code==scan&&e.pressed).count()
    }
    pub fn scan_released_count(&self,pid:u32,epoch:u64,scan:u16)->usize{
        if pid==0||epoch==0||scan==0{return 0;}
        self.queue[..self.len].iter().flatten().filter(|e|
            e.focus.pid==pid&&e.focus.generation==epoch&&
            e.scan_code==scan&&!e.pressed).count()
    }
    pub fn snapshot_window(&self,pid:u32,epoch:u64,window:u32,
        out:&mut[KeyEvent])->Result<usize,InputError>{
        if pid==0||epoch==0||window==0{return Err(InputError::Unauthorized);}
        let count=self.pending_window(pid,epoch,window);
        if out.len()<count{return Err(InputError::Full);}
        let mut n=0;
        for item in self.queue[..self.len].iter().flatten(){
            if item.focus.pid==pid&&item.focus.generation==epoch&&
                item.focus.window==window{out[n]=*item;n+=1;}
        }
        Ok(n)
    }
    pub fn receive_up_to(&mut self,pid:u32,epoch:u64,limit:usize,
        out:&mut[KeyEvent])->Result<usize,InputError>{
        if pid==0||epoch==0{return Err(InputError::Unauthorized);}
        if limit==0{return Err(InputError::Invalid);}
        let count=self.pending_for(pid,epoch).min(limit);
        if count==0{return Err(InputError::Empty);}
        if out.len()<count{return Err(InputError::Full);}
        for target in out.iter_mut().take(count){
            *target=self.receive(pid,epoch)?;
        }
        Ok(count)
    }
    pub fn receive_window_up_to(&mut self,pid:u32,epoch:u64,window:u32,
        limit:usize,out:&mut[KeyEvent])->Result<usize,InputError>{
        if pid==0||epoch==0||window==0{return Err(InputError::Unauthorized);}
        if limit==0{return Err(InputError::Invalid);}
        let count=self.pending_window(pid,epoch,window).min(limit);
        if count==0{return Err(InputError::Empty);}
        if out.len()<count{return Err(InputError::Full);}
        for target in out.iter_mut().take(count){
            *target=self.receive_window(pid,epoch,window)?;
        }
        Ok(count)
    }
    pub fn cancel_pressed(&mut self,pid:u32,epoch:u64)
        ->Result<usize,InputError>{
        if pid==0||epoch==0{return Err(InputError::Unauthorized);}
        let mut n=0;let mut i=0;
        while i<self.len{
            if self.queue[i].is_some_and(|e|e.focus.pid==pid&&
                e.focus.generation==epoch&&e.pressed){
                for j in i+1..self.len{self.queue[j-1]=self.queue[j];}
                self.len-=1;self.queue[self.len]=None;n+=1;
            }else{i+=1;}
        }
        Ok(n)
    }
    pub fn cancel_released(&mut self,pid:u32,epoch:u64)
        ->Result<usize,InputError>{
        if pid==0||epoch==0{return Err(InputError::Unauthorized);}
        let mut n=0;let mut i=0;
        while i<self.len{
            if self.queue[i].is_some_and(|e|e.focus.pid==pid&&
                e.focus.generation==epoch&&!e.pressed){
                for j in i+1..self.len{self.queue[j-1]=self.queue[j];}
                self.len-=1;self.queue[self.len]=None;n+=1;
            }else{i+=1;}
        }
        Ok(n)
    }
    pub fn cancel_scans(&mut self,pid:u32,epoch:u64,scans:&[u16])
        ->Result<usize,InputError>{
        if pid==0||epoch==0||scans.is_empty()||scans.contains(&0){
            return Err(InputError::Unauthorized);
        }
        let mut n=0;let mut i=0;
        while i<self.len{
            if self.queue[i].is_some_and(|e|e.focus.pid==pid&&
                e.focus.generation==epoch&&scans.contains(&e.scan_code)){
                for j in i+1..self.len{self.queue[j-1]=self.queue[j];}
                self.len-=1;self.queue[self.len]=None;n+=1;
            }else{i+=1;}
        }
        Ok(n)
    }
    /// Trusted input switch discards old focused window's already queued keys
    /// to prevent stale typed data from appearing in the new application.
    pub fn switch_focus_drop_pending<const W:usize>(&mut self,
        windows:&Windows<W>,next:Focus)->Result<usize,InputError>{
        if !windows.owns(next.pid,next.generation,next.window){
            return Err(InputError::Unauthorized);
        }
        let old=self.focus;
        let mut staged=Self{focus:self.focus,queue:self.queue,len:self.len};
        staged.set_focus(windows,next)?;
        let removed=if let Some(prev)=old{
            staged.cancel_window(prev.pid,prev.generation,prev.window)?
        }else{0};
        *self=staged;Ok(removed)
    }
    pub fn physical_if_not_queued<const W:usize>(&mut self,
        windows:&Windows<W>,scan:u16,pressed:bool)->Result<bool,InputError>{
        if scan==0{return Err(InputError::Invalid);}
        let focus=self.focus.ok_or(InputError::NoFocus)?;
        if !windows.owns(focus.pid,focus.generation,focus.window){
            self.focus=None;return Err(InputError::Unauthorized);
        }
        if self.queue[..self.len].iter().flatten().any(|e|
            e.focus==focus&&e.scan_code==scan&&e.pressed==pressed){
            return Ok(false);
        }
        self.physical_key(windows,scan,pressed)?;Ok(true)
    }
    pub fn physical_unique_batch<const W:usize>(&mut self,
        windows:&Windows<W>,keys:&[(u16,bool)])->Result<usize,InputError>{
        if keys.is_empty(){return Err(InputError::Invalid);}
        let mut staged=Self{focus:self.focus,queue:self.queue,len:self.len};
        for (index,&(scan,pressed)) in keys.iter().enumerate(){
            if scan==0||keys[..index].contains(&(scan,pressed)){
                return Err(InputError::Invalid);
            }
            if !staged.physical_if_not_queued(windows,scan,pressed)?{
                return Err(InputError::Invalid);
            }
        }
        *self=staged;Ok(keys.len())
    }
    pub fn receive_live_up_to<const W:usize>(&mut self,windows:&Windows<W>,
        pid:u32,epoch:u64,limit:usize,out:&mut[KeyEvent])
        ->Result<usize,InputError>{
        if pid==0||epoch==0{return Err(InputError::Unauthorized);}
        if limit==0{return Err(InputError::Invalid);}
        let mut staged=Self{focus:self.focus,queue:self.queue,len:self.len};
        staged.reap_closed(windows);
        let n=staged.pending_for(pid,epoch).min(limit);
        if n==0{return Err(InputError::Empty);}
        if out.len()<n{return Err(InputError::Full);}
        let result=staged.receive_up_to(pid,epoch,limit,out)?;
        *self=staged;Ok(result)
    }
    pub fn owner_first_scan(&self,pid:u32,epoch:u64)->Option<u16>{
        self.peek_owner(pid,epoch).map(|e|e.scan_code)
    }
    pub fn owner_last_scan(&self,pid:u32,epoch:u64)->Option<u16>{
        self.peek_last_owner(pid,epoch).map(|e|e.scan_code)
    }
}
#[cfg(test)]mod r085_extension3_keyboard_tests{
    use super::*;
    use crate::batch2_windows::{Rect,Window};
    const EMPTY:KeyEvent=KeyEvent{
        focus:Focus{pid:0,generation:0,window:0},scan_code:0,pressed:false
    };
    fn windows()->Windows<3>{
        let mut w=Windows::new();
        w.create(Window{pid:2,generation:4,id:7,
            rect:Rect{x:0,y:0,w:100,h:100}}).unwrap();
        w.create(Window{pid:3,generation:5,id:8,
            rect:Rect{x:10,y:10,w:100,h:100}}).unwrap();
        w
    }
    #[test]fn keyboard_snapshot_dedup_and_batched_delivery_are_isolated(){
        let w=windows();let mut k=Router::<5>::new();
        let f=Focus{pid:2,generation:4,window:7};
        k.set_focus(&w,f).unwrap();
        assert!(k.can_queue(5));assert!(!k.owner_has_pending(2,4));
        assert_eq!(k.focused_live(&w),Some(f));
        assert_eq!(k.physical_if_not_queued(&w,10,true),Ok(true));
        assert_eq!(k.physical_if_not_queued(&w,10,true),Ok(false));
        assert_eq!(k.physical_unique_batch(&w,&[(11,true),(12,false)]),Ok(2));
        assert_eq!(k.scan_pressed_count(2,4,10),1);
        assert_eq!(k.scan_released_count(2,4,12),1);
        assert_eq!(k.first_window_event(2,4,7).unwrap().scan_code,10);
        assert_eq!(k.last_window_event(2,4,7).unwrap().scan_code,12);
        assert_eq!(k.owner_first_scan(2,4),Some(10));
        assert_eq!(k.owner_last_scan(2,4),Some(12));
        assert_eq!(k.stale_count(&w),0);
        let mut tiny=[EMPTY;1];
        assert_eq!(k.snapshot_window(2,4,7,&mut tiny),Err(InputError::Full));
        assert_eq!(tiny,[EMPTY]);
        let mut out=[EMPTY;5];
        assert_eq!(k.snapshot_window(2,4,7,&mut out),Ok(3));
        assert_eq!(k.receive_window_up_to(2,4,7,2,&mut out),Ok(2));
        assert_eq!(k.receive_live_up_to(&w,2,4,1,&mut out),Ok(1));
        assert_eq!(k.pending_total(),0);
    }
    #[test]fn keyboard_focus_transfer_rollback_and_cancel(){
        let w=windows();let mut k=Router::<5>::new();
        let first=Focus{pid:2,generation:4,window:7};
        k.set_focus(&w,first).unwrap();
        k.physical_batch(&w,&[(10,true),(10,false),(11,true)]).unwrap();
        assert_eq!(k.cancel_scans(2,4,&[0]),Err(InputError::Unauthorized));
        assert_eq!(k.pending_total(),3);
        assert_eq!(k.cancel_pressed(2,4),Ok(2));
        assert_eq!(k.cancel_released(2,4),Ok(1));
        k.physical_key(&w,10,true).unwrap();
        assert_eq!(k.switch_focus_drop_pending(&w,
            Focus{pid:2,generation:99,window:7}),Err(InputError::Unauthorized));
        assert_eq!(k.pending_total(),1);
        assert_eq!(k.switch_focus_drop_pending(&w,
            Focus{pid:3,generation:5,window:8}),Ok(1));
        assert_eq!(k.pending_total(),0);
        assert_eq!(k.focused_live(&w).unwrap().pid,3);
        k.physical_key(&w,20,true).unwrap();
        assert_eq!(k.receive_up_to(3,5,1,&mut [EMPTY;1]),Ok(1));
    }
}

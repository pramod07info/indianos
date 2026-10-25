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

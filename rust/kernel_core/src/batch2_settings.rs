//! R036/19: bounded authenticated system-settings snapshots.
//! Persisting settings across reboot and hardware configuration are separate.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Theme{Light,Dark}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Settings{pub theme:Theme,pub pointer_speed:u8,pub key_repeat:u8}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum SettingsError{Invalid,Full,Denied,NotFound}
#[derive(Clone,Copy)]
struct Slot{user:u32,generation:u64,settings:Settings}
pub struct Store<const N:usize>{slots:[Option<Slot>;N]}
impl<const N:usize> Store<N>{
    pub const fn new()->Self{Self{slots:[None;N]}}
    pub fn register(&mut self,user:u32,generation:u64,
        settings:Settings)->Result<(),SettingsError>{
        if user==0||generation==0||!valid(settings){return Err(SettingsError::Invalid);}
        if self.slots.iter().flatten().any(|slot|slot.user==user){
            return Err(SettingsError::Denied);
        }
        let free=self.slots.iter_mut().find(|s|s.is_none()).ok_or(SettingsError::Full)?;
        *free=Some(Slot{user,generation,settings});Ok(())
    }
    pub fn update(&mut self,user:u32,generation:u64,next:Settings)
        ->Result<(),SettingsError>{
        if !valid(next){return Err(SettingsError::Invalid);}
        let slot=self.slots.iter_mut().flatten().find(|s|s.user==user)
            .ok_or(SettingsError::NotFound)?;
        if slot.generation!=generation{return Err(SettingsError::Denied);}
        slot.settings=next;Ok(())
    }
    pub fn read(&self,user:u32,generation:u64)->Result<Settings,SettingsError>{
        let slot=self.slots.iter().flatten().find(|s|s.user==user)
            .ok_or(SettingsError::NotFound)?;
        if slot.generation!=generation{return Err(SettingsError::Denied);}
        Ok(slot.settings)
    }
    pub fn retire(&mut self,user:u32,generation:u64){
        for slot in &mut self.slots{
            if slot.is_some_and(|s|s.user==user&&s.generation==generation){*slot=None;}
        }
    }
}
fn valid(s:Settings)->bool{
    (1..=10).contains(&s.pointer_speed)&&(1..=30).contains(&s.key_repeat)
}
#[cfg(test)]
mod tests{
    use super::*;
    fn light()->Settings{Settings{theme:Theme::Light,pointer_speed:5,key_repeat:10}}
    #[test]fn owner_generations_isolate_settings_updates(){
        let mut store=Store::<2>::new();store.register(1,2,light()).unwrap();
        let dark=Settings{theme:Theme::Dark,..light()};
        assert_eq!(store.update(1,1,dark),Err(SettingsError::Denied));
        assert_eq!(store.read(1,2),Ok(light()));
        store.update(1,2,dark).unwrap();
        assert_eq!(store.read(1,2),Ok(dark));
    }
    #[test]fn invalid_speed_or_key_repeat_cannot_be_saved(){
        let mut store=Store::<1>::new();
        let mut bad=light();bad.pointer_speed=11;
        assert_eq!(store.register(1,2,bad),Err(SettingsError::Invalid));
        store.register(1,2,light()).unwrap();
        bad=light();bad.key_repeat=0;
        assert_eq!(store.update(1,2,bad),Err(SettingsError::Invalid));
        assert_eq!(store.read(1,2),Ok(light()));
    }
    #[test]fn retired_user_no_longer_exposes_profile(){
        let mut store=Store::<1>::new();store.register(1,2,light()).unwrap();
        store.retire(1,1);assert_eq!(store.read(1,2),Ok(light()));
        store.retire(1,2);assert_eq!(store.read(1,2),Err(SettingsError::NotFound));
    }
}

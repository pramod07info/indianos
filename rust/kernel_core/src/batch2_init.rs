//! R036/11: bounded dependency-ordered boot-service initialization policy.
//! Kernel-supervised startup must authenticate PIDs and perform real launches separately.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Service { pub id:u16, pub dependency:Option<u16>, pub essential:bool }
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum InitError { Invalid, Duplicate, Full, MissingDependency, Cycle, NotReady }
pub struct InitPlan<const N:usize> {
    entries:[Option<Service>;N], done:[bool;N],
}
impl<const N:usize> InitPlan<N> {
    pub const fn new()->Self { Self {entries:[None;N],done:[false;N]} }
    pub fn register(&mut self, service:Service)->Result<(),InitError>{
        if service.id==0 || service.dependency==Some(service.id){return Err(InitError::Invalid);}
        if self.entries.iter().flatten().any(|entry|entry.id==service.id){
            return Err(InitError::Duplicate);
        }
        let index=self.entries.iter().position(Option::is_none).ok_or(InitError::Full)?;
        self.entries[index]=Some(service);
        Ok(())
    }
    /// Returns None only after all configured services have reached ready.
    pub fn next(&self)->Result<Option<Service>,InitError>{
        let mut pending=false;
        for (i,item) in self.entries.iter().enumerate(){
            let Some(service)=item else {continue};
            if self.done[i]{continue;}
            pending=true;
            match service.dependency {
                None=>return Ok(Some(*service)),
                Some(parent)=>{
                    let index=self.entries.iter().position(|entry|
                        entry.is_some_and(|e|e.id==parent))
                        .ok_or(InitError::MissingDependency)?;
                    if self.done[index]{return Ok(Some(*service));}
                }
            }
        }
        if pending {Err(InitError::Cycle)} else {Ok(None)}
    }
    pub fn ready(&mut self,id:u16)->Result<(),InitError>{
        let scheduled=self.next()?.ok_or(InitError::NotReady)?;
        if scheduled.id!=id{return Err(InitError::NotReady);}
        let index=self.entries.iter().position(|item|
            item.is_some_and(|s|s.id==id)).ok_or(InitError::Invalid)?;
        self.done[index]=true;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn order_and_completion(){
        let mut p=InitPlan::<3>::new();
        p.register(Service{id:2,dependency:Some(1),essential:true}).unwrap();
        p.register(Service{id:1,dependency:None,essential:true}).unwrap();
        assert_eq!(p.next().unwrap().unwrap().id,1);
        assert_eq!(p.ready(2),Err(InitError::NotReady));
        p.ready(1).unwrap();
        p.ready(2).unwrap();
        assert_eq!(p.next(),Ok(None));
    }
    #[test] fn fail_closed_missing_and_cycle(){
        let mut p=InitPlan::<2>::new();
        p.register(Service{id:1,dependency:Some(2),essential:true}).unwrap();
        assert_eq!(p.next(),Err(InitError::MissingDependency));
        p.register(Service{id:2,dependency:Some(1),essential:false}).unwrap();
        assert_eq!(p.next(),Err(InitError::Cycle));
    }
    #[test] fn duplicate_and_capacity(){
        let mut p=InitPlan::<1>::new();
        p.register(Service{id:1,dependency:None,essential:true}).unwrap();
        assert_eq!(p.register(Service{id:1,dependency:None,essential:true}),Err(InitError::Duplicate));
        assert_eq!(p.register(Service{id:2,dependency:None,essential:true}),Err(InitError::Full));
    }
}

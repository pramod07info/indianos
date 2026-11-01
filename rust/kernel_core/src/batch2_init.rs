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



/// R085: validate the entire service dependency graph before starting PID1
/// services. Earlier next() may return a runnable root while another service
/// contains a missing dependency; validation fails closed before any launch.
impl<const N:usize> InitPlan<N> {
    pub fn validate_graph(&self)->Result<(),InitError>{
        for start in 0..N {
            let Some(_) = self.entries[start] else {continue};
            let mut seen=[false;N];
            let mut cursor=start;
            loop {
                if seen[cursor]{return Err(InitError::Cycle);}
                seen[cursor]=true;
                let current=self.entries[cursor].ok_or(InitError::MissingDependency)?;
                let Some(parent)=current.dependency else {break};
                cursor=self.entries.iter().position(|item|
                    item.is_some_and(|entry|entry.id==parent))
                    .ok_or(InitError::MissingDependency)?;
            }
        }
        Ok(())
    }
    pub fn next_validated(&self)->Result<Option<Service>,InitError>{
        self.validate_graph()?;
        self.next()
    }
    /// Register all boot services or publish none (including zero capacity).
    pub fn register_batch(&mut self,services:&[Service])->Result<usize,InitError>{
        if services.is_empty(){return Err(InitError::Invalid);}
        let mut staged=Self{entries:self.entries,done:self.done};
        for service in services{staged.register(*service)?;}
        staged.validate_graph()?;
        self.entries=staged.entries;
        self.done=staged.done;
        Ok(services.len())
    }
    /// Authenticate each planned readiness transition in dependency order;
    /// a late invalid ID cannot mark earlier services ready.
    pub fn ready_batch(&mut self,ids:&[u16])->Result<usize,InitError>{
        if ids.is_empty(){return Err(InitError::Invalid);}
        let mut staged=Self{entries:self.entries,done:self.done};
        staged.validate_graph()?;
        for &id in ids {staged.ready(id)?;}
        self.done=staged.done;
        Ok(ids.len())
    }
    pub fn essential_remaining(&self)->usize{
        self.entries.iter().enumerate().filter(|(i,entry)|
            entry.is_some_and(|service|service.essential)&&!self.done[*i]).count()
    }
    pub fn is_ready(&self,id:u16)->bool{
        id!=0&&self.entries.iter().enumerate().any(|(i,s)|
            s.is_some_and(|service|service.id==id)&&self.done[i])
    }
}
#[cfg(test)]
mod r085_init_acceptance {
    use super::*;
    #[test] fn graph_validation_catches_late_unknown_service_before_launch(){
        let mut p=InitPlan::<4>::new();
        p.register(Service{id:1,dependency:None,essential:true}).unwrap();
        p.register(Service{id:2,dependency:Some(99),essential:true}).unwrap();
        assert_eq!(p.next().unwrap().unwrap().id,1);
        assert_eq!(p.next_validated(),Err(InitError::MissingDependency));
        assert_eq!(p.essential_remaining(),2);
    }
    #[test] fn batches_are_atomic_and_only_ready_services_are_published(){
        let mut p=InitPlan::<3>::new();
        assert_eq!(p.register_batch(&[
            Service{id:1,dependency:None,essential:true},
            Service{id:2,dependency:Some(9),essential:true}
        ]),Err(InitError::MissingDependency));
        assert_eq!(p.next(),Ok(None));
        p.register_batch(&[
            Service{id:2,dependency:Some(1),essential:true},
            Service{id:1,dependency:None,essential:true}
        ]).unwrap();
        assert_eq!(p.ready_batch(&[1,9]),Err(InitError::NotReady));
        assert!(!p.is_ready(1));
        assert_eq!(p.essential_remaining(),2);
        p.ready_batch(&[1,2]).unwrap();
        assert_eq!(p.next_validated(),Ok(None));
        assert!(p.is_ready(2));
        assert_eq!(p.essential_remaining(),0);
    }
    #[test] fn cycle_rejected_even_with_a_runnable_root(){
        let mut p=InitPlan::<3>::new();
        p.register(Service{id:1,dependency:None,essential:true}).unwrap();
        p.register(Service{id:2,dependency:Some(3),essential:false}).unwrap();
        p.register(Service{id:3,dependency:Some(2),essential:false}).unwrap();
        assert_eq!(p.next_validated(),Err(InitError::Cycle));
    }
}


/// R085 extension: allocation-free service-plan inspection and preboot audit.
/// These operations do not launch a process; only trusted PID1 may consume
/// these results and initiate scheduler/loader work.
impl<const N:usize> InitPlan<N>{
    pub fn registered_count(&self)->usize{
        self.entries.iter().filter(|entry|entry.is_some()).count()
    }
    pub fn available_slots(&self)->usize{N-self.registered_count()}
    pub fn ready_count(&self)->usize{
        self.entries.iter().enumerate().filter(|(i,s)|s.is_some()&&self.done[*i]).count()
    }
    pub fn pending_count(&self)->usize{self.registered_count()-self.ready_count()}
    pub fn essential_count(&self)->usize{
        self.entries.iter().flatten().filter(|s|s.essential).count()
    }
    pub fn essential_ready_count(&self)->usize{
        self.entries.iter().enumerate().filter(|(i,s)|
            s.is_some_and(|v|v.essential)&&self.done[*i]).count()
    }
    pub fn optional_pending_count(&self)->usize{
        self.entries.iter().enumerate().filter(|(i,s)|
            s.is_some_and(|v|!v.essential)&&!self.done[*i]).count()
    }
    pub fn first_unready(&self)->Option<Service>{
        self.entries.iter().enumerate().find_map(|(i,s)|
            if !self.done[i]{*s}else{None})
    }
    pub fn last_unready(&self)->Option<Service>{
        self.entries.iter().enumerate().rev().find_map(|(i,s)|
            if !self.done[i]{*s}else{None})
    }
    pub fn first_ready(&self)->Option<Service>{
        self.entries.iter().enumerate().find_map(|(i,s)|
            if self.done[i]{*s}else{None})
    }
    pub fn get_service(&self,id:u16)->Option<Service>{
        if id==0{return None;}
        self.entries.iter().flatten().copied().find(|s|s.id==id)
    }
    pub fn has_service(&self,id:u16)->bool{self.get_service(id).is_some()}
    /// Detect a dangling direct dependency before attempting a boot step.
    pub fn dependency_ready(&self,id:u16)->Result<bool,InitError>{
        let service=self.get_service(id).ok_or(InitError::Invalid)?;
        let Some(parent)=service.dependency else{return Ok(true)};
        if !self.has_service(parent){return Err(InitError::MissingDependency);}
        Ok(self.is_ready(parent))
    }
    pub fn ready_service_ids(&self,out:&mut[u16])->Result<usize,InitError>{
        let count=self.ready_count();
        if out.len()<count{return Err(InitError::Full);}
        let mut n=0;
        for (i,service) in self.entries.iter().enumerate(){
            if self.done[i]{if let Some(service)=service{out[n]=service.id;n+=1;}}
        }
        Ok(n)
    }
    pub fn unready_service_ids(&self,out:&mut[u16])->Result<usize,InitError>{
        let count=self.pending_count();
        if out.len()<count{return Err(InitError::Full);}
        let mut n=0;
        for (i,service) in self.entries.iter().enumerate(){
            if !self.done[i]{if let Some(service)=service{out[n]=service.id;n+=1;}}
        }
        Ok(n)
    }
    pub fn essential_service_ids(&self,out:&mut[u16])->Result<usize,InitError>{
        let count=self.essential_count();
        if out.len()<count{return Err(InitError::Full);}
        let mut n=0;
        for service in self.entries.iter().flatten(){
            if service.essential{out[n]=service.id;n+=1;}
        }
        Ok(n)
    }
    pub fn pending_dependents(&self,id:u16)->Result<usize,InitError>{
        if !self.has_service(id){return Err(InitError::Invalid);}
        Ok(self.entries.iter().enumerate().filter(|(i,s)|
            !self.done[*i]&&s.is_some_and(|v|v.dependency==Some(id))).count())
    }
    /// Maximum chain depth is bounded by N; malformed graphs fail closed.
    pub fn dependency_depth(&self,id:u16)->Result<usize,InitError>{
        self.validate_graph()?;
        let mut node=self.get_service(id).ok_or(InitError::Invalid)?;
        let mut depth=0usize;
        while let Some(parent)=node.dependency{
            depth+=1;
            node=self.get_service(parent).ok_or(InitError::MissingDependency)?;
        }
        Ok(depth)
    }
    pub fn maximum_dependency_depth(&self)->Result<usize,InitError>{
        self.validate_graph()?;
        let mut max=0usize;
        for item in self.entries.iter().flatten(){
            max=max.max(self.dependency_depth(item.id)?);
        }
        Ok(max)
    }
    /// A ready child cannot be published before its dependency is ready.
    pub fn audit_ready_dependencies(&self)->Result<(),InitError>{
        self.validate_graph()?;
        for (i,item) in self.entries.iter().enumerate(){
            if self.done[i]{
                if let Some(item)=item{
                    if !self.dependency_ready(item.id)?{return Err(InitError::NotReady);}
                }
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod r085_extension_init_tests {
    use super::*;
    #[test]fn inventories_preserve_registration_and_readiness_order(){
        let mut p=InitPlan::<4>::new();
        p.register_batch(&[
            Service{id:3,dependency:Some(2),essential:false},
            Service{id:2,dependency:Some(1),essential:true},
            Service{id:1,dependency:None,essential:true},
        ]).unwrap();
        assert_eq!((p.registered_count(),p.available_slots(),p.ready_count(),
            p.pending_count(),p.essential_count(),p.essential_ready_count(),
            p.optional_pending_count()),(3,1,0,3,2,0,1));
        assert_eq!(p.first_unready().unwrap().id,3);
        assert_eq!(p.last_unready().unwrap().id,1);
        assert_eq!(p.first_ready(),None);
        assert!(p.has_service(2));assert!(!p.has_service(0));
        assert_eq!(p.get_service(7),None);
        assert_eq!(p.pending_dependents(2),Ok(1));
        assert_eq!(p.dependency_depth(3),Ok(2));
        assert_eq!(p.maximum_dependency_depth(),Ok(2));
        let mut tiny=[99u16;1];
        assert_eq!(p.unready_service_ids(&mut tiny),Err(InitError::Full));
        assert_eq!(tiny,[99]);
        p.ready_batch(&[1,2]).unwrap();
        assert_eq!(p.ready_count(),2);
        assert_eq!(p.first_ready().unwrap().id,2);
        assert_eq!(p.dependency_ready(3),Ok(true));
        assert_eq!(p.pending_dependents(2),Ok(1));
        let mut out=[0u16;4];
        assert_eq!(p.ready_service_ids(&mut out),Ok(2));
        assert_eq!(&out[..2],&[2,1]);
        assert_eq!(p.unready_service_ids(&mut out),Ok(1));
        assert_eq!(out[0],3);
        assert_eq!(p.essential_service_ids(&mut out),Ok(2));
        assert_eq!(&out[..2],&[2,1]);
        assert_eq!(p.audit_ready_dependencies(),Ok(()));
    }
    #[test]fn cyclic_and_dangling_graphs_cannot_produce_valid_depth(){
        let mut p=InitPlan::<3>::new();
        p.register(Service{id:1,dependency:Some(2),essential:true}).unwrap();
        assert_eq!(p.dependency_depth(1),Err(InitError::MissingDependency));
        assert_eq!(p.dependency_ready(1),Err(InitError::MissingDependency));
        p.register(Service{id:2,dependency:Some(1),essential:false}).unwrap();
        assert_eq!(p.maximum_dependency_depth(),Err(InitError::Cycle));
    }
}


/// R085 extension III: safe service-dependency planning. No method in this
/// block fabricates a live service start; only the real scheduler may confirm
/// readiness after preparing and authenticating the corresponding process.
impl<const N:usize> InitPlan<N>{
    pub fn root_count(&self)->usize{
        self.entries.iter().flatten().filter(|e|e.dependency.is_none()).count()
    }
    pub fn dependent_count(&self,id:u16)->Result<usize,InitError>{
        if !self.has_service(id){return Err(InitError::Invalid);}
        Ok(self.entries.iter().flatten().filter(|e|e.dependency==Some(id)).count())
    }
    pub fn root_ids(&self,out:&mut[u16])->Result<usize,InitError>{
        let count=self.root_count();
        if out.len()<count{return Err(InitError::Full);}
        let mut n=0;
        for e in self.entries.iter().flatten(){
            if e.dependency.is_none(){out[n]=e.id;n+=1;}
        }
        Ok(n)
    }
    pub fn dependent_ids(&self,id:u16,out:&mut[u16])->Result<usize,InitError>{
        let count=self.dependent_count(id)?;
        if out.len()<count{return Err(InitError::Full);}
        let mut n=0;
        for e in self.entries.iter().flatten(){
            if e.dependency==Some(id){out[n]=e.id;n+=1;}
        }
        Ok(n)
    }
    pub fn leaf_count(&self)->usize{
        self.entries.iter().flatten().filter(|s|!self.entries.iter().flatten()
            .any(|child|child.dependency==Some(s.id))).count()
    }
    pub fn leaf_ids(&self,out:&mut[u16])->Result<usize,InitError>{
        let count=self.leaf_count();
        if out.len()<count{return Err(InitError::Full);}
        let mut n=0;
        for s in self.entries.iter().flatten(){
            if !self.entries.iter().flatten().any(|c|c.dependency==Some(s.id)){
                out[n]=s.id;n+=1;
            }
        }
        Ok(n)
    }
    pub fn service_essential(&self,id:u16)->Result<bool,InitError>{
        Ok(self.get_service(id).ok_or(InitError::Invalid)?.essential)
    }
    pub fn service_pending(&self,id:u16)->Result<bool,InitError>{
        if !self.has_service(id){return Err(InitError::Invalid);}
        Ok(!self.is_ready(id))
    }
    pub fn all_essential_ready(&self)->bool{self.essential_remaining()==0}
    pub fn all_registered_ready(&self)->bool{self.pending_count()==0}
    pub fn parent_id(&self,id:u16)->Result<Option<u16>,InitError>{
        Ok(self.get_service(id).ok_or(InitError::Invalid)?.dependency)
    }
    pub fn parent_chain(&self,id:u16,out:&mut[u16])->Result<usize,InitError>{
        self.validate_graph()?;
        let mut current=self.get_service(id).ok_or(InitError::Invalid)?;
        let count=self.dependency_depth(id)?;
        if out.len()<count{return Err(InitError::Full);}
        let mut n=0;
        while let Some(p)=current.dependency{
            out[n]=p;n+=1;
            current=self.get_service(p).ok_or(InitError::MissingDependency)?;
        }
        Ok(n)
    }
    pub fn ancestor_of(&self,ancestor:u16,child:u16)->Result<bool,InitError>{
        if !self.has_service(ancestor){return Err(InitError::Invalid);}
        self.validate_graph()?;
        let mut item=self.get_service(child).ok_or(InitError::Invalid)?;
        while let Some(parent)=item.dependency{
            if parent==ancestor{return Ok(true);}
            item=self.get_service(parent).ok_or(InitError::MissingDependency)?;
        }
        Ok(false)
    }
    pub fn ready_direct_dependents(&self,id:u16)->Result<usize,InitError>{
        if !self.has_service(id){return Err(InitError::Invalid);}
        Ok(self.entries.iter().enumerate().filter(|(i,s)|
            self.done[*i]&&s.is_some_and(|e|e.dependency==Some(id))).count())
    }
    pub fn pending_roots(&self)->usize{
        self.entries.iter().enumerate().filter(|(i,s)|
            !self.done[*i]&&s.is_some_and(|e|e.dependency.is_none())).count()
    }
    pub fn next_ready_essential(&self)->Result<Option<Service>,InitError>{
        self.validate_graph()?;
        for (i,service) in self.entries.iter().enumerate(){
            if let Some(s)=service{
                if !self.done[i]&&s.essential&&self.dependency_ready(s.id)?{
                    return Ok(Some(*s));
                }
            }
        }
        Ok(None)
    }
    pub fn next_ready_optional(&self)->Result<Option<Service>,InitError>{
        self.validate_graph()?;
        for (i,service) in self.entries.iter().enumerate(){
            if let Some(s)=service{
                if !self.done[i]&&!s.essential&&self.dependency_ready(s.id)?{
                    return Ok(Some(*s));
                }
            }
        }
        Ok(None)
    }
    pub fn schedulable_count(&self)->Result<usize,InitError>{
        self.validate_graph()?;
        let mut n=0;
        for (i,e) in self.entries.iter().enumerate(){
            if let Some(s)=e{
                if !self.done[i]&&self.dependency_ready(s.id)?{n+=1;}
            }
        }
        Ok(n)
    }
    /// Snapshot runnable service IDs without changing any lifecycle state.
    pub fn schedulable_ids(&self,out:&mut[u16])->Result<usize,InitError>{
        let count=self.schedulable_count()?;
        if out.len()<count{return Err(InitError::Full);}
        let mut n=0;
        for (i,e) in self.entries.iter().enumerate(){
            if let Some(s)=e{
                if !self.done[i]&&self.dependency_ready(s.id)?{
                    out[n]=s.id;n+=1;
                }
            }
        }
        Ok(n)
    }
    /// Derive deterministic dependency order on a *temporary* plan. Never
    /// mark actual services ready or launch a process.
    pub fn topological_ids(&self,out:&mut[u16])->Result<usize,InitError>{
        self.validate_graph()?;
        let n=self.registered_count();
        if out.len()<n{return Err(InitError::Full);}
        let mut staged=Self{entries:self.entries,done:[false;N]};
        for slot in out.iter_mut().take(n){
            let service=staged.next_validated()?.ok_or(InitError::Cycle)?;
            *slot=service.id;
            staged.ready(service.id)?;
        }
        Ok(n)
    }
}
#[cfg(test)]mod r085_extension3_service_tests{
    use super::*;
    #[test]fn typed_dependency_views_and_deterministic_order(){
        let mut p=InitPlan::<5>::new();
        p.register_batch(&[
            Service{id:3,dependency:Some(2),essential:false},
            Service{id:2,dependency:Some(1),essential:true},
            Service{id:1,dependency:None,essential:true},
            Service{id:4,dependency:Some(1),essential:false},
        ]).unwrap();
        assert_eq!(p.root_count(),1);
        assert_eq!(p.dependent_count(1),Ok(2));
        assert_eq!(p.leaf_count(),2);
        assert_eq!(p.parent_id(3),Ok(Some(2)));
        assert_eq!(p.ancestor_of(1,3),Ok(true));
        assert_eq!(p.ancestor_of(3,1),Ok(false));
        assert_eq!(p.service_essential(2),Ok(true));
        assert_eq!(p.service_pending(2),Ok(true));
        assert!(!p.all_essential_ready());
        assert!(!p.all_registered_ready());
        assert_eq!(p.pending_roots(),1);
        assert_eq!(p.schedulable_count(),Ok(1));
        assert_eq!(p.next_ready_essential().unwrap().unwrap().id,1);
        assert_eq!(p.next_ready_optional(),Ok(None));
        let mut ids=[0u16;5];
        assert_eq!(p.root_ids(&mut ids),Ok(1));assert_eq!(ids[0],1);
        assert_eq!(p.dependent_ids(1,&mut ids),Ok(2));
        assert_eq!(&ids[..2],&[2,4]);
        assert_eq!(p.leaf_ids(&mut ids),Ok(2));
        assert_eq!(&ids[..2],&[3,4]);
        assert_eq!(p.parent_chain(3,&mut ids),Ok(2));
        assert_eq!(&ids[..2],&[2,1]);
        assert_eq!(p.schedulable_ids(&mut ids),Ok(1));
        assert_eq!(ids[0],1);
        assert_eq!(p.topological_ids(&mut ids),Ok(4));
        assert_eq!(&ids[..4],&[1,2,3,4]);
        assert_eq!(p.ready_count(),0);
        p.ready(1).unwrap();
        assert_eq!(p.ready_direct_dependents(1),Ok(0));
        assert_eq!(p.next_ready_optional().unwrap().unwrap().id,4);
        assert_eq!(p.schedulable_count(),Ok(2));
        assert_eq!(p.pending_roots(),0);
        p.ready(2).unwrap();p.ready(3).unwrap();p.ready(4).unwrap();
        assert!(p.all_essential_ready());
        assert!(p.all_registered_ready());
    }
    #[test]fn short_output_never_partially_publishes_dependency_chain(){
        let mut p=InitPlan::<2>::new();
        p.register_batch(&[
            Service{id:2,dependency:Some(1),essential:false},
            Service{id:1,dependency:None,essential:true},
        ]).unwrap();
        let mut short=[99u16;1];
        assert_eq!(p.topological_ids(&mut short),Err(InitError::Full));
        assert_eq!(short,[99]);
        assert_eq!(p.parent_chain(2,&mut []),Err(InitError::Full));
        assert_eq!(p.schedulable_ids(&mut []),Err(InitError::Full));
        assert_eq!(p.ready_count(),0);
    }
}

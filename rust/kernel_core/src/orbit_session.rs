//! Orbit R003 session ribbon ledger; no_std, no allocation, no direct restore.
//! This records authenticated *metadata only*. It does not persist data by
//! itself, capture process memory, relaunch executables or authorize new files.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Owner{pub pid:u32,pub generation:u64}
impl Owner{fn valid(self)->bool{self.pid!=0&&self.generation!=0}}
pub const MAX_SESSION_APPS:usize=4;
pub const WIRE_SIZE:usize=48;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{Invalid,Full,Duplicate,Denied,Missing,Expired,Corrupt,TooSmall}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Entry{
    pub slot:u32,pub owner:Owner,pub workspace:u8,pub expires:u64,
    pub apps:[u8;MAX_SESSION_APPS],pub count:u8
}
pub struct Ledger<const N:usize>{entries:[Option<Entry>;N]}
impl<const N:usize> Ledger<N>{
    pub const fn new()->Self{Self{entries:[None;N]}}
    // S1: authenticate exact generation and impose a finite session budget.
    pub fn open(&mut self,owner:Owner,slot:u32,workspace:u8,expires:u64)
        ->Result<(),Error>{
        if !owner.valid()||slot==0||workspace>3||expires==0{
            return Err(Error::Invalid);
        }
        if self.entries.iter().flatten().any(|e|e.slot==slot){
            return Err(Error::Duplicate);
        }
        let free=self.entries.iter_mut().find(|entry|entry.is_none())
            .ok_or(Error::Full)?;
        *free=Some(Entry{slot,owner,workspace,expires,
            apps:[0;MAX_SESSION_APPS],count:0});
        Ok(())
    }
    fn mine_mut(&mut self,owner:Owner,slot:u32)->Result<&mut Entry,Error>{
        let entry=self.entries.iter_mut().flatten().find(|e|e.slot==slot)
            .ok_or(Error::Missing)?;
        if !owner.valid()||entry.owner!=owner{return Err(Error::Denied);}
        Ok(entry)
    }
    // S2: store only approved symbolic app IDs (not executable paths or
    // environment strings, secrets, capabilities or framebuffer pointers).
    pub fn include_app(&mut self,owner:Owner,slot:u32,app:u8)
        ->Result<(),Error>{
        if !(1..=6).contains(&app){return Err(Error::Invalid);}
        let entry=self.mine_mut(owner,slot)?;
        if entry.apps[..entry.count as usize].contains(&app){
            return Err(Error::Duplicate);
        }
        if entry.count as usize==MAX_SESSION_APPS{return Err(Error::Full);}
        entry.apps[entry.count as usize]=app;entry.count+=1;
        Ok(())
    }
    // S3: preflight exact identity, expiry, caller policy and destination
    // before any output. This is a REVIEW PLAN, not actual app execution.
    pub fn plan(&self,owner:Owner,slot:u32,now:u64,
                permitted:&[u8],output:&mut[u8])->Result<usize,Error>{
        let entry=self.entries.iter().flatten().find(|e|e.slot==slot)
            .ok_or(Error::Missing)?;
        if !owner.valid()||entry.owner!=owner{return Err(Error::Denied);}
        if now>=entry.expires{return Err(Error::Expired);}
        let apps=&entry.apps[..entry.count as usize];
        if apps.iter().any(|x|!permitted.contains(x)){return Err(Error::Denied);}
        if output.len()<apps.len(){return Err(Error::TooSmall);}
        output[..apps.len()].copy_from_slice(apps);
        Ok(apps.len())
    }
    // S4: exact incarnation teardown or one-slot delete; stale recycled
    // PIDs cannot read/write another generation's saved metadata.
    pub fn forget(&mut self,owner:Owner,slot:u32)->Result<(),Error>{
        let entry=self.mine_mut(owner,slot)?;
        *entry=Entry{slot:0,owner:Owner{pid:0,generation:0},
            workspace:0,expires:0,apps:[0;MAX_SESSION_APPS],count:0};
        let target=self.entries.iter_mut().find(|item|
            item.is_some_and(|e|e.slot==0)).ok_or(Error::Missing)?;
        *target=None;Ok(())
    }
    pub fn purge_owner(&mut self,owner:Owner)->usize{
        if !owner.valid(){return 0;}
        let mut count=0;
        for item in self.entries.iter_mut(){
            if item.is_some_and(|e|e.owner==owner){*item=None;count+=1;}
        }
        count
    }
    /// Change the selected goal workspace for the same process incarnation.
    pub fn select_workspace(&mut self,owner:Owner,slot:u32,workspace:u8)
        ->Result<(),Error>{
        if workspace>3{return Err(Error::Invalid);}
        self.mine_mut(owner,slot)?.workspace=workspace;Ok(())
    }
    /// Drop a previously approved symbolic app from a session; scrub
    /// vacated storage to prevent stale metadata resurfacing.
    pub fn remove_app(&mut self,owner:Owner,slot:u32,app:u8)
        ->Result<(),Error>{
        let entry=self.mine_mut(owner,slot)?;
        let index=entry.apps[..entry.count as usize].iter().position(|v|*v==app)
            .ok_or(Error::Missing)?;
        let last=entry.count as usize-1;
        for i in index..last{entry.apps[i]=entry.apps[i+1];}
        entry.apps[last]=0;entry.count-=1;
        Ok(())
    }
    /// A session can reduce its deadline, never silently extend a grant.
    pub fn shorten_deadline(&mut self,owner:Owner,slot:u32,
                            now:u64,deadline:u64)->Result<(),Error>{
        let entry=self.mine_mut(owner,slot)?;
        if now>=entry.expires{return Err(Error::Expired);}
        if deadline<=now||deadline>entry.expires{return Err(Error::Invalid);}
        entry.expires=deadline;Ok(())
    }
    /// Complete owner-specific preview listing, with no partial output.
    pub fn inventory(&self,owner:Owner,out:&mut[Entry])
        ->Result<usize,Error>{
        if !owner.valid(){return Err(Error::Denied);}
        let total=self.entries.iter().flatten().filter(|x|x.owner==owner).count();
        if out.len()<total{return Err(Error::TooSmall);}
        let mut index=0;
        for item in self.entries.iter().flatten(){
            if item.owner==owner{out[index]=*item;index+=1;}
        }
        Ok(total)
    }
    /// Expire saved metadata, but do not run any application or read
    /// untrusted on-disk bytes as authenticated session state.
    pub fn expire(&mut self,now:u64)->usize{
        let mut removed=0;
        for item in &mut self.entries{
            if item.is_some_and(|x|now>=x.expires){
                *item=None;removed+=1;
            }
        }
        removed
    }
    pub fn view(&self,owner:Owner,slot:u32)->Result<Entry,Error>{
        let e=self.entries.iter().flatten().find(|e|e.slot==slot)
            .ok_or(Error::Missing)?;
        if !owner.valid()||e.owner!=owner{return Err(Error::Denied);}
        Ok(*e)
    }
}
// S5: versioned fixed-sized on-disk metadata encoding and corruption check.
// Noncryptographic checksum is only accidental-corruption detection: an
// authenticated durable VFS and trusted records/signer remain REQUIRED.
fn checksum(buf:&[u8])->u32{
    let mut h:u32=0x811c9dc5;
    for &byte in buf{h=(h^(byte as u32)).wrapping_mul(0x01000193);}
    h
}
pub fn encode(entry:Entry,out:&mut[u8])->Result<(),Error>{
    if out.len()<WIRE_SIZE{return Err(Error::TooSmall);}
    if !entry.owner.valid()||entry.slot==0||entry.workspace>3||
       entry.expires==0||entry.count as usize>MAX_SESSION_APPS{
        return Err(Error::Invalid);
    }
    if entry.apps[..entry.count as usize].iter().any(|&app|app==0||app>6){
        return Err(Error::Invalid);
    }
    let mut record=[0u8;WIRE_SIZE];
    record[..4].copy_from_slice(b"ORB1");
    record[4..8].copy_from_slice(&entry.slot.to_le_bytes());
    record[8..12].copy_from_slice(&entry.owner.pid.to_le_bytes());
    record[12..20].copy_from_slice(&entry.owner.generation.to_le_bytes());
    record[20..28].copy_from_slice(&entry.expires.to_le_bytes());
    record[28]=entry.workspace;
    record[29]=entry.count;
    record[30..34].copy_from_slice(&entry.apps);
    let digest=checksum(&record[..WIRE_SIZE-4]);
    record[WIRE_SIZE-4..].copy_from_slice(&digest.to_le_bytes());
    out[..WIRE_SIZE].copy_from_slice(&record);
    Ok(())
}
pub fn decode(raw:&[u8])->Result<Entry,Error>{
    if raw.len()!=WIRE_SIZE||&raw[..4]!=b"ORB1"{return Err(Error::Corrupt);}
    let expected=u32::from_le_bytes(raw[WIRE_SIZE-4..].try_into()
        .map_err(|_|Error::Corrupt)?);
    if checksum(&raw[..WIRE_SIZE-4])!=expected{return Err(Error::Corrupt);}
    let val=|i:usize|u32::from_le_bytes(raw[i..i+4].try_into().unwrap_or([0;4]));
    let wide=|i:usize|u64::from_le_bytes(raw[i..i+8].try_into().unwrap_or([0;8]));
    let mut apps=[0u8;MAX_SESSION_APPS];
    apps.copy_from_slice(&raw[30..34]);
    let entry=Entry{slot:val(4),owner:Owner{pid:val(8),generation:wide(12)},
        expires:wide(20),workspace:raw[28],count:raw[29],apps};
    if !entry.owner.valid()||entry.slot==0||entry.expires==0||
       entry.workspace>3||entry.count as usize>MAX_SESSION_APPS{
        return Err(Error::Corrupt);
    }
    for (i,&app) in apps.iter().enumerate(){
        if i<entry.count as usize{
            if !(1..=6).contains(&app)||apps[..i].contains(&app){
                return Err(Error::Corrupt);
            }
        }else if app!=0{return Err(Error::Corrupt);}
    }
    Ok(entry)
}
#[cfg(test)]mod tests{
    use super::*;
    fn owner()->Owner{Owner{pid:5,generation:3}}
    #[test]fn s1_session_ownership_and_capacity(){
        let mut l=Ledger::<1>::new();l.open(owner(),1,2,90).unwrap();
        assert_eq!(l.open(owner(),1,2,90),Err(Error::Duplicate));
        assert_eq!(l.open(owner(),2,2,90),Err(Error::Full));
        assert_eq!(l.view(Owner{generation:4,..owner()},1),Err(Error::Denied));
    }
    #[test]fn s2_app_ids_are_bounded_and_duplicate_free(){
        let mut l=Ledger::<1>::new();l.open(owner(),1,0,90).unwrap();
        for id in 1..=4{l.include_app(owner(),1,id).unwrap();}
        assert_eq!(l.include_app(owner(),1,4),Err(Error::Duplicate));
        assert_eq!(l.include_app(owner(),1,5),Err(Error::Full));
        assert_eq!(l.include_app(owner(),1,7),Err(Error::Invalid));
        assert_eq!(l.view(owner(),1).unwrap().apps,[1,2,3,4]);
    }
    #[test]fn s3_restore_plan_never_partially_publishes(){
        let mut l=Ledger::<1>::new();l.open(owner(),1,1,90).unwrap();
        l.include_app(owner(),1,1).unwrap();l.include_app(owner(),1,2).unwrap();
        let mut output=[77u8;2];
        assert_eq!(l.plan(owner(),1,10,&[1],&mut output),Err(Error::Denied));
        assert_eq!(output,[77;2]);
        assert_eq!(l.plan(owner(),1,90,&[1,2],&mut output),Err(Error::Expired));
        assert_eq!(l.plan(owner(),1,10,&[1,2],&mut output[..1]),Err(Error::TooSmall));
        assert_eq!(output,[77;2]);
        assert_eq!(l.plan(owner(),1,10,&[1,2],&mut output),Ok(2));
        assert_eq!(output,[1,2]);
    }
    #[test]fn s4_stale_process_revocation_and_forget(){
        let mut l=Ledger::<2>::new();
        let second=Owner{generation:4,..owner()};
        l.open(owner(),1,0,100).unwrap();l.open(second,2,0,100).unwrap();
        assert_eq!(l.forget(second,1),Err(Error::Denied));
        assert_eq!(l.purge_owner(owner()),1);
        assert_eq!(l.view(owner(),1),Err(Error::Missing));
        assert!(l.view(second,2).is_ok());
        l.forget(second,2).unwrap();
        assert_eq!(l.view(second,2),Err(Error::Missing));
    }
    #[test]fn s5_record_roundtrip_and_tamper_rejected(){
        let mut l=Ledger::<1>::new();l.open(owner(),2,1,1234).unwrap();
        l.include_app(owner(),2,1).unwrap();l.include_app(owner(),2,3).unwrap();
        let entry=l.view(owner(),2).unwrap();
        let mut bytes=[0u8;WIRE_SIZE];encode(entry,&mut bytes).unwrap();
        assert_eq!(decode(&bytes),Ok(entry));
        bytes[21]^=0x80;
        assert_eq!(decode(&bytes),Err(Error::Corrupt));
        bytes[21]^=0x80;bytes[0]=0;
        assert_eq!(decode(&bytes),Err(Error::Corrupt));
    }
    #[test]fn orbit_session_goal_switch_and_app_removal_are_owner_bound(){
        let mut l=Ledger::<2>::new();l.open(owner(),1,0,100).unwrap();
        l.include_app(owner(),1,1).unwrap();l.include_app(owner(),1,2).unwrap();
        l.select_workspace(owner(),1,3).unwrap();
        assert_eq!(l.view(owner(),1).unwrap().workspace,3);
        assert_eq!(l.select_workspace(owner(),1,4),Err(Error::Invalid));
        let stale=Owner{generation:9,..owner()};
        assert_eq!(l.remove_app(stale,1,1),Err(Error::Denied));
        l.remove_app(owner(),1,1).unwrap();
        assert_eq!(l.view(owner(),1).unwrap().apps,[2,0,0,0]);
    }
    #[test]fn orbit_session_deadline_and_inventory_no_partial_output(){
        let mut l=Ledger::<3>::new();
        l.open(owner(),1,0,100).unwrap();l.open(owner(),2,1,110).unwrap();
        let sentinel=Entry{slot:99,owner:owner(),workspace:3,expires:99,
            apps:[0;MAX_SESSION_APPS],count:0};
        let mut short=[sentinel];
        assert_eq!(l.inventory(owner(),&mut short),Err(Error::TooSmall));
        assert_eq!(short,[sentinel]);
        assert_eq!(l.shorten_deadline(owner(),1,1,120),Err(Error::Invalid));
        l.shorten_deadline(owner(),1,1,60).unwrap();
        assert_eq!(l.expire(60),1);
        assert_eq!(l.inventory(owner(),&mut short),Ok(1));
        assert_eq!(short[0].slot,2);
    }

}


/// Orbit R005: atomic session previews and safe metadata editing. These
/// functions never relaunch applications or recreate revoked capabilities.
impl<const N:usize> Ledger<N>{
 /// Replace one session's symbolic app list atomically. No token, address
 /// or executable path can be serialized through the app-id-only contract.
 pub fn replace_apps(&mut self,owner:Owner,slot:u32,apps:&[u8])
  ->Result<(),Error>{
  if apps.len()>MAX_SESSION_APPS||apps.iter().any(|v|!(1..=6).contains(v)){
   return Err(Error::Invalid);
  }
  for (i,&app) in apps.iter().enumerate(){
   if apps[..i].contains(&app){return Err(Error::Duplicate);}
  }
  let entry=self.mine_mut(owner,slot)?;
  let mut next=[0u8;MAX_SESSION_APPS];
  next[..apps.len()].copy_from_slice(apps);
  entry.apps=next;entry.count=apps.len() as u8;Ok(())
 }
 /// Review multiple session slots in one operation. The destination
 /// remains unchanged if any slot is stale, denied, expired or too small.
 pub fn plan_many(&self,owner:Owner,slots:&[u32],now:u64,
  permitted:&[u8],out:&mut[u8])->Result<usize,Error>{
  if !owner.valid()||slots.is_empty()||slots.len()>N{
   return Err(Error::Invalid);
  }
  let mut size=0usize;
  for (i,&slot) in slots.iter().enumerate(){
   if slot==0||slots[..i].contains(&slot){return Err(Error::Duplicate);}
   let entry=self.entries.iter().flatten().find(|e|e.slot==slot)
    .ok_or(Error::Missing)?;
   if entry.owner!=owner{return Err(Error::Denied);}
   if now>=entry.expires{return Err(Error::Expired);}
   let apps=&entry.apps[..entry.count as usize];
   if apps.iter().any(|a|!permitted.contains(a)){return Err(Error::Denied);}
   size=size.checked_add(apps.len()).ok_or(Error::TooSmall)?;
  }
  if size>out.len(){return Err(Error::TooSmall);}
  let mut pos=0;
  for &slot in slots{
   let entry=self.entries.iter().flatten().find(|e|e.slot==slot)
    .ok_or(Error::Missing)?;
   let slice=&entry.apps[..entry.count as usize];
   out[pos..pos+slice.len()].copy_from_slice(slice);pos+=slice.len();
  }
  Ok(size)
 }
 /// Atomically forget several snapshots; an invalid or foreign slot
 /// leaves every record untouched.
 pub fn forget_many(&mut self,owner:Owner,slots:&[u32])->Result<usize,Error>{
  if !owner.valid()||slots.is_empty()||slots.len()>N{
   return Err(Error::Invalid);
  }
  let mut next=Self{entries:self.entries};
  for (i,&slot) in slots.iter().enumerate(){
   if slots[..i].contains(&slot){return Err(Error::Duplicate);}
   next.forget(owner,slot)?;
  }
  self.entries=next.entries;Ok(slots.len())
 }
}
#[cfg(test)]
mod orbit_r005_session_tests{
 use super::*;
 fn a()->Owner{Owner{pid:7,generation:3}}
 #[test]fn preview_many_never_writes_partial_or_foreign_record(){
  let mut ledger=Ledger::<3>::new();ledger.open(a(),1,0,100).unwrap();
  ledger.open(a(),2,1,50).unwrap();
  ledger.include_app(a(),1,1).unwrap();ledger.include_app(a(),2,2).unwrap();
  let mut out=[99u8;3];
  assert_eq!(ledger.plan_many(a(),&[1,2],50,&[1,2],&mut out),Err(Error::Expired));
  assert_eq!(out,[99;3]);
  assert_eq!(ledger.plan_many(a(),&[1,2],1,&[1,2],&mut out),Ok(2));
  assert_eq!(&out[..2],&[1,2]);
  assert_eq!(ledger.plan_many(Owner{generation:4,..a()},&[1],1,&[1],&mut out),
   Err(Error::Denied));
 }
 #[test]fn replacement_scrubs_old_ids_and_detects_duplicates(){
  let mut l=Ledger::<1>::new();l.open(a(),1,0,100).unwrap();
  l.replace_apps(a(),1,&[1,2,3]).unwrap();
  assert_eq!(l.replace_apps(a(),1,&[3,3]),Err(Error::Duplicate));
  assert_eq!(l.view(a(),1).unwrap().count,3);
  l.replace_apps(a(),1,&[4]).unwrap();
  assert_eq!(l.view(a(),1).unwrap().apps,[4,0,0,0]);
 }
 #[test]fn transactional_forget_blocks_cross_owner_deletions(){
  let mut l=Ledger::<3>::new();l.open(a(),1,0,100).unwrap();
  l.open(a(),2,0,100).unwrap();
  let foreign=Owner{pid:8,generation:1};l.open(foreign,3,0,100).unwrap();
  assert_eq!(l.forget_many(a(),&[1,3]),Err(Error::Denied));
  assert!(l.view(a(),1).is_ok());assert!(l.view(a(),2).is_ok());
  assert_eq!(l.forget_many(a(),&[1,2]),Ok(2));
  assert!(l.view(foreign,3).is_ok());
 }
}

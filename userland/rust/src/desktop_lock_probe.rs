//! Opt-in ownership/revocation and blocking-lock acceptance; absent from product images.
use super::{call,exit,write};
fn send(pid:u32,payload:&[u8]){
 let mut packet=[0u8;68];packet[..4].copy_from_slice(&pid.to_le_bytes());
 packet[4..4+payload.len()].copy_from_slice(payload);
 if call(8,packet.as_ptr() as u64,(4+payload.len()) as u64)!=0{exit(122);}
}
fn receive(sender:u32,expected:&[u8]){
 let mut packet=[0u8;72];
 for _ in 0..16{
  let n=call(35,packet.as_mut_ptr() as u64,32);if n==0{continue;}
  if n!=expected.len() as u64+8||packet[..4]!=sender.to_le_bytes()||
     packet[4..8]!=(expected.len() as u32).to_le_bytes()||&packet[8..8+expected.len()]!=expected{exit(122);}
  return;
 }
 exit(122);
}
fn handle(operation:u64,path:&[u8],token:u64,argument:u64)->u64{
 let mut descriptor=[0u8;64];descriptor[..path.len()].copy_from_slice(path);
 descriptor[24..32].copy_from_slice(&token.to_le_bytes());descriptor[32..40].copy_from_slice(&argument.to_le_bytes());
 call(33,operation,descriptor.as_ptr() as u64)
}
fn lock(operation:u64,token:u64,rounds:u64)->u64{
 let mut descriptor=[0u8;64];descriptor[..4].copy_from_slice(&[1,0,64,0]);
 descriptor[8..16].copy_from_slice(&token.to_le_bytes());descriptor[24..32].copy_from_slice(&rounds.to_le_bytes());
 call(41,operation,descriptor.as_ptr() as u64)
}
#[cfg(any(feature="desktop-init",feature="desktop-files"))]
fn owner_probe(pid:u64){
 // The explicitly generated GPT fixture starts with legacy root:root/0777.
 // No other process may retain a descriptor while PID1 changes metadata.
 fn open()->u64{
  let token=handle(1,b"/shared/readme",0,1);
  if token==0||token==u64::MAX{exit(123);}token
 }
 fn change(operation:u64,value:u64){
  let token=open();
  if handle(operation,b"",token,value)!=0||handle(5,b"",token,0)!=u64::MAX{exit(123);}
 }
 fn reader(allowed:bool){
  let token=handle(1,b"/shared/readme",0,1);
  if !allowed{if token!=u64::MAX{exit(123);}return;}
  if token==0||token==u64::MAX{exit(123);}
  let mut bytes=[0u8;256];
  if super::file_handle_call(2,b"",token,0,&mut bytes,13)!=13||
     &bytes[..13]!=b"HELLO INDIANOS"||handle(5,b"",token,0)!=0{exit(123);}
 }
 if pid==1{
  if handle(37,b"",0,0)>>32&256==0{exit(123);}
  change(38,0o600);send(2,b"OWNERDENY");receive(2,b"OWNERDENIED");
  change(39,1000);send(2,b"OWNERALLOW");receive(2,b"OWNERREAD");
  change(39,0);send(2,b"OWNERREVOKE");receive(2,b"OWNERREVOKED");
  change(40,1000);change(38,0o640);
  send(2,b"GROUPALLOW");receive(2,b"GROUPREAD");
  change(38,0o600);send(2,b"GROUPREVOKE");receive(2,b"GROUPREVOKED");
  change(40,0);change(38,0o777);
  write(b"desktop ownership probe original owner group mode restored; stale tokens rejected");
 }else if pid==2{
  receive(1,b"OWNERDENY");reader(false);
  write(b"desktop ownership probe root-owned private file denied");send(1,b"OWNERDENIED");
  receive(1,b"OWNERALLOW");reader(true);
  write(b"desktop ownership probe new owner read exact committed bytes");send(1,b"OWNERREAD");
  receive(1,b"OWNERREVOKE");reader(false);
  write(b"desktop ownership probe retired owner denied");send(1,b"OWNERREVOKED");
  receive(1,b"GROUPALLOW");reader(true);
  write(b"desktop ownership probe new group read exact committed bytes");send(1,b"GROUPREAD");
  receive(1,b"GROUPREVOKE");reader(false);
  write(b"desktop ownership probe revoked group denied");send(1,b"GROUPREVOKED");
 }
}
pub(super) fn verify(){
 let pid=call(4,0,0);
 #[cfg(any(feature="desktop-init",feature="desktop-files"))]
 owner_probe(pid);
 match pid{
  1=>{
   if handle(37,b"",0,0)>>32&128==0{exit(122);}
   let writer=handle(1,b"/shared/readme",0,3);if writer==0||writer==u64::MAX{exit(122);}
   send(2,b"LOCKSTART");receive(2,b"LOCKHELD");
   if lock(35,writer,1)!=1{exit(122);}
   write(b"desktop lock probe writer timed out without replacing peer lock");
   send(2,b"LOCKRELEASE");
   if lock(35,writer,32)!=0{exit(122);}
   receive(2,b"LOCKRELEASED");
   if handle(36,b"",writer,0)!=0||handle(5,b"",writer,0)!=0{exit(122);}
   write(b"desktop lock probe writer resumed after peer unlock");
   for pid in [2,3,4]{send(pid,b"LOCKNORMAL");}
  },
  2=>{
   receive(1,b"LOCKSTART");
   let reader=handle(1,b"/shared/readme",0,1);if reader==0||reader==u64::MAX{exit(122);}
   if lock(34,reader,1)!=0||lock(35,reader,1)!=u64::MAX{exit(122);}
   send(1,b"LOCKHELD");receive(1,b"LOCKRELEASE");
   if handle(36,b"",reader,0)!=0||handle(5,b"",reader,0)!=0{exit(122);}
   write(b"desktop lock probe reader released original lock");
   send(1,b"LOCKRELEASED");receive(1,b"LOCKNORMAL");
  },
  3|4=>receive(1,b"LOCKNORMAL"),_=>exit(122),
 }
}

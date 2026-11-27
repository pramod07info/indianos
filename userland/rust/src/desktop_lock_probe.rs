//! Opt-in cross-process blocking-lock acceptance; absent from product images.
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
pub(super) fn verify(){
 match call(4,0,0){
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

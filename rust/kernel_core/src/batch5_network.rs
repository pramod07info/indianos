//! Batch 5 task 14: BOOTP/DHCP ACK header identity checks before parsing
//! lease options. Network stack, checksum validation and TLS are separate.
use crate::batch3_network_security::{self,Lease,NetworkError,TlsCertificateVerifier};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{Malformed,WrongClient,WrongTransaction,InvalidLease,NoAck,Unverified}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Client{pub xid:u32,pub mac:[u8;6]}
pub fn parse_ack(client:Client,packet:&[u8])->Result<Lease,Error>{
 // BOOTP fixed header through chaddr + server name + boot file + cookie.
 if packet.len()<241||packet.len()>1500||packet[0]!=2||packet[1]!=1||
  packet[2]!=6||packet[3]!=0{return Err(Error::Malformed);}
 if packet[4..8]!=client.xid.to_be_bytes(){return Err(Error::WrongTransaction);}
 if packet[28..34]!=client.mac{return Err(Error::WrongClient);}
 let address:[u8;4]=packet[16..20].try_into().map_err(|_|Error::Malformed)?;
 let options=&packet[236..];
 let lease=batch3_network_security::parse_lease(address,options)
  .map_err(|_|Error::InvalidLease)?;
 // Insist on DHCP message type ACK (53=5), a server identity (54)
 // and reject malformed, duplicate or truncated critical options.
 let mut i=4usize;let mut ack=false;let mut server=false;let mut ended=false;
 while i<options.len(){
  let typ=options[i];i+=1;
  if typ==255{ended=true;break;}
  if typ==0{continue;}
  if i>=options.len(){return Err(Error::Malformed);}
  let len=options[i] as usize;i+=1;
  if len>options.len()-i{return Err(Error::Malformed);}
  let v=&options[i..i+len];i+=len;
  match typ{
   53 if len==1&&!ack =>{if v[0]!=5{return Err(Error::NoAck);}ack=true;}
   54 if len==4&&!server =>{if v==[0u8;4]{return Err(Error::Malformed);}server=true;}
   53|54=>return Err(Error::Malformed),
   _=>{}
  }
 }
 if !ended||!ack||!server{return Err(Error::NoAck);}
 Ok(lease)
}
pub fn tls_authenticated<V:TlsCertificateVerifier>(v:&V,hostname:&[u8],
 cert:&[u8])->Result<(),Error>{
 match batch3_network_security::tls_peer_gate(v,hostname,cert){
  Ok(())=>Ok(()),
  Err(NetworkError::Unverified)=>Err(Error::Unverified),
  Err(_)=>Err(Error::Malformed),
 }
}
#[cfg(test)]mod tests{
 use super::*;
 fn packet()->[u8;271]{
  let mut p=[0u8;271];p[0]=2;p[1]=1;p[2]=6;
  p[4..8].copy_from_slice(&42u32.to_be_bytes());
  p[16..20].copy_from_slice(&[10,0,0,9]);
  p[28..34].copy_from_slice(&[1,2,3,4,5,6]);
  let opts=[99,130,83,99,1,4,255,255,255,0,3,4,10,0,0,1,
    51,4,0,0,0,30,53,1,5,54,4,10,0,0,1,255];
  p[236..236+opts.len()].copy_from_slice(&opts);p
 }
 #[test]fn client_and_xid_cannot_be_spoofed(){
  let id=Client{xid:42,mac:[1,2,3,4,5,6]};
  assert_eq!(parse_ack(id,&packet()).unwrap().address,[10,0,0,9]);
  let mut wrong=id;wrong.xid=43;
  assert_eq!(parse_ack(wrong,&packet()),Err(Error::WrongTransaction));
  wrong=id;wrong.mac=[0;6];
  assert_eq!(parse_ack(wrong,&packet()),Err(Error::WrongClient));
 }
 #[test]fn missing_ack_never_installs_lease(){
  let mut p=packet();p[260]=2;
  assert_eq!(parse_ack(Client{xid:42,mac:[1,2,3,4,5,6]},&p),
    Err(Error::NoAck));
 }
}
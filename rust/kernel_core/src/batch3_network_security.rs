//! R037/26: bounded DHCP lease decoding, simple TCP handshake policy and a
//! fail-closed TLS peer-verification hook. NOT a TCP/IP stack or TLS engine.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Lease{pub address:[u8;4],pub subnet:[u8;4],pub gateway:[u8;4],
    pub lease_seconds:u32}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum NetworkError{Malformed,Missing,Expired,State,Unverified}
/// Parse DHCP options from a previously verified BOOTP response. Caller MUST
/// validate transaction ID, peer and BOOTP fixed header before invoking this.
pub fn parse_lease(address:[u8;4],options:&[u8])->Result<Lease,NetworkError>{
    if address==[0;4]||options.len()>1024||options.len()<4||
        options[..4]!=[99,130,83,99]{return Err(NetworkError::Malformed);}
    let mut pos=4;let mut subnet=None;let mut gateway=None;let mut seconds=None;
    let mut end_seen=false;
    while pos<options.len(){
        let kind=options[pos];pos+=1;
        if kind==255{end_seen=true;break;}
        if kind==0{continue;}
        if pos>=options.len(){return Err(NetworkError::Malformed);}
        let len=options[pos] as usize;pos+=1;
        if len>options.len()-pos{return Err(NetworkError::Malformed);}
        let value=&options[pos..pos+len];pos+=len;
        match kind{
            1 if len==4 && subnet.is_none()=>subnet=Some(value.try_into().map_err(|_|NetworkError::Malformed)?),
            3 if len>=4 && len%4==0 && gateway.is_none()=>
                gateway=Some(value[..4].try_into().map_err(|_|NetworkError::Malformed)?),
            51 if len==4 && seconds.is_none()=>seconds=Some(u32::from_be_bytes(
                value.try_into().map_err(|_|NetworkError::Malformed)?)),
            1|3|51=>return Err(NetworkError::Malformed),
            _=>{}
        }
    }
    if !end_seen||options[pos..].iter().any(|&byte|byte!=0){
        return Err(NetworkError::Malformed);
    }
    let ttl=seconds.ok_or(NetworkError::Missing)?;
    if ttl==0{return Err(NetworkError::Expired);}
    let subnet=subnet.ok_or(NetworkError::Missing)?;
    let host_bits=!u32::from_be_bytes(subnet);
    // A normal IPv4 netmask is a one-run followed by a zero-run.
    if host_bits==u32::MAX||
        host_bits&(host_bits.wrapping_add(1))!=0{
        return Err(NetworkError::Malformed);
    }
    Ok(Lease{address,subnet,
        gateway:gateway.ok_or(NetworkError::Missing)?,lease_seconds:ttl})
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum TcpState{Closed,SynSent,Established}
pub struct TcpHandshake{state:TcpState,initial_seq:u32}
impl TcpHandshake{
    pub const fn new()->Self{Self{state:TcpState::Closed,initial_seq:0}}
    pub fn syn(&mut self,seq:u32)->Result<(),NetworkError>{
        if self.state!=TcpState::Closed{return Err(NetworkError::State);}
        self.initial_seq=seq;self.state=TcpState::SynSent;Ok(())
    }
    /// Caller must authenticate checksummed peer packet and expected port.
    pub fn syn_ack(&mut self,ack:u32,syn:bool,ack_flag:bool)
        ->Result<(),NetworkError>{
        if self.state!=TcpState::SynSent||!syn||!ack_flag||
            ack!=self.initial_seq.wrapping_add(1){return Err(NetworkError::State);}
        self.state=TcpState::Established;Ok(())
    }
    pub fn state(&self)->TcpState{self.state}
    pub fn close(&mut self){self.state=TcpState::Closed;}
}
pub trait TlsCertificateVerifier{
    fn verify_chain_and_name(&self,hostname:&[u8],peer_certificate:&[u8])->bool;
}
/// Strict ASCII DNS hostname preflight. The verifier must independently
/// validate the SAN, certificate chain, trust root and expiry; a well-formed
/// string alone is never peer authentication.
fn valid_dns_hostname(hostname:&[u8])->bool{
    if hostname.is_empty()||hostname.len()>253{return false;}
    let mut start=0usize;
    for (i,&byte) in hostname.iter().enumerate(){
        if byte==b'.'{
            if i==start||i-start>63||hostname[start]==b'-'||
                hostname[i-1]==b'-'{return false;}
            start=i+1;
        }else if !(byte.is_ascii_alphanumeric()||byte==b'-'){
            return false;
        }
    }
    let n=hostname.len()-start;
    n>0&&n<=63&&hostname[start]!=b'-'&&hostname[hostname.len()-1]!=b'-'
}
pub fn tls_peer_gate<V:TlsCertificateVerifier>(verifier:&V,
    hostname:&[u8],certificate:&[u8])->Result<(),NetworkError>{
    if !valid_dns_hostname(hostname)||certificate.is_empty()||
        certificate.len()>16*1024{return Err(NetworkError::Malformed);}
    if !verifier.verify_chain_and_name(hostname,certificate){
        return Err(NetworkError::Unverified);
    }
    Ok(())
}
/// Fail-closed *policy* for the transport's externally verified TLS 1.3
/// negotiation. Not a TLS engine: caller MUST run actual cryptographic
/// handshake, transcript authentication, record AEAD, replay protection,
/// certificate chain/expiry/SAN validation and trusted-root management.
pub fn tls13_peer_preflight<V:TlsCertificateVerifier>(
 verifier:&V,hostname:&[u8],certificate:&[u8],
 negotiated_version:u16,alpn:&[u8]
)->Result<(),NetworkError>{
 // RFC 8446 negotiated protocol version. Never silently downgrade to TLS 1.2.
 if negotiated_version!=0x0304{return Err(NetworkError::Unverified);}
 // For now only bounded printable protocol identifiers (e.g. h2, http/1.1).
 if alpn.is_empty()||alpn.len()>32||alpn.iter().any(|b|
   !b.is_ascii_alphanumeric()&&!matches!(*b,b'-'|b'_'|b'.'|b'/')){
  return Err(NetworkError::Malformed);
 }
 tls_peer_gate(verifier,hostname,certificate)
}
/// Application-bound TLS policy: reject a peer that negotiated a
/// different application protocol even when its certificate is trusted.
/// The trusted TLS engine must supply *authenticated* negotiated ALPN
/// from its handshake transcript, not untrusted network packet bytes.
pub fn tls13_expected_alpn<V:TlsCertificateVerifier>(
 verifier:&V,hostname:&[u8],certificate:&[u8],
 negotiated_version:u16,negotiated_alpn:&[u8],expected_alpn:&[u8]
)->Result<(),NetworkError>{
 if expected_alpn.is_empty()||expected_alpn.len()>32||
    expected_alpn!=negotiated_alpn{
  return Err(NetworkError::Unverified);
 }
 tls13_peer_preflight(verifier,hostname,certificate,
  negotiated_version,negotiated_alpn)
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn decodes_required_dhcp_lease_options(){
        let bytes=[99,130,83,99,1,4,255,255,255,0,3,4,10,0,0,1,
            51,4,0,0,0,30,255];
        assert_eq!(parse_lease([10,0,0,12],&bytes),Ok(Lease{
            address:[10,0,0,12],subnet:[255,255,255,0],
            gateway:[10,0,0,1],lease_seconds:30}));
        assert_eq!(parse_lease([10,0,0,12],&bytes[..bytes.len()-1]),
            Err(NetworkError::Malformed));
    }
    #[test]fn rejects_wrong_tcp_ack(){
        let mut tcp=TcpHandshake::new();tcp.syn(100).unwrap();
        assert_eq!(tcp.syn_ack(100,true,true),Err(NetworkError::State));
        tcp.syn_ack(101,true,true).unwrap();
        assert_eq!(tcp.state(),TcpState::Established);
        assert_eq!(tcp.syn(1),Err(NetworkError::State));
    }
    struct Deny;
    impl TlsCertificateVerifier for Deny{
        fn verify_chain_and_name(&self,_:&[u8],_:&[u8])->bool{false}
    }
    struct AllowForTests;
    impl TlsCertificateVerifier for AllowForTests{
        fn verify_chain_and_name(&self,_:&[u8],_:&[u8])->bool{true}
    }
    #[test]fn tls13_application_protocol_is_bound_to_authenticated_peer(){
        let v=AllowForTests;
        assert_eq!(tls13_expected_alpn(&v,b"example.org",b"cert",
          0x0304,b"h2",b"h2"),Ok(()));
        assert_eq!(tls13_expected_alpn(&v,b"example.org",b"cert",
          0x0304,b"http/1.1",b"h2"),Err(NetworkError::Unverified));
        assert_eq!(tls13_expected_alpn(&v,b"example.org",b"cert",
          0x0304,b"h2",b""),Err(NetworkError::Unverified));
        assert_eq!(tls13_expected_alpn(&v,b"example.org",b"cert",
          0x0303,b"h2",b"h2"),Err(NetworkError::Unverified));
        assert_eq!(tls13_expected_alpn(&Deny,b"example.org",b"cert",
          0x0304,b"h2",b"h2"),Err(NetworkError::Unverified));
    }
    #[test]fn tls_rejects_malformed_sni_before_trusting_verifier(){
        let verifier=AllowForTests;
        for name in [
          b"".as_slice(),b".example.org",b"example..org",b"example.org.",
          b"-example.org",b"example-.org",b"example.org\0.evil",
          b"example.org/path",b"*.example.org",
        ]{
          assert_eq!(tls_peer_gate(&verifier,name,b"cert"),
            Err(NetworkError::Malformed),"{name:?}");
        }
        assert_eq!(tls_peer_gate(&verifier,b"example.org",b"cert"),Ok(()));
        assert_eq!(tls_peer_gate(&verifier,b"example.org",&[]),
          Err(NetworkError::Malformed));
    }
    #[test]fn tls13_policy_requires_exact_version_alpn_and_verified_peer(){
        let verifier=AllowForTests;
        assert_eq!(tls13_peer_preflight(&verifier,b"example.org",b"cert",
          0x0304,b"h2"),Ok(()));
        assert_eq!(tls13_peer_preflight(&verifier,b"example.org",b"cert",
          0x0304,b"http/1.1"),Ok(()));
        for version in [0x0301,0x0302,0x0303,0x0305]{
            assert_eq!(tls13_peer_preflight(&verifier,b"example.org",b"cert",
              version,b"h2"),Err(NetworkError::Unverified));
        }
        for token in [b"".as_slice(),b"bad\0alpn",b"h2\n"]{
            assert_eq!(tls13_peer_preflight(&verifier,b"example.org",b"cert",
              0x0304,token),Err(NetworkError::Malformed));
        }
        assert_eq!(tls13_peer_preflight(&Deny,b"example.org",b"cert",
          0x0304,b"h2"),Err(NetworkError::Unverified));
        assert_eq!(tls13_peer_preflight(&verifier,b"example.org/",b"cert",
          0x0304,b"h2"),Err(NetworkError::Malformed));
    }
    #[test]fn tls_fails_closed_without_verified_certificate(){
        assert_eq!(tls_peer_gate(&Deny,b"example.org",b"cert"),
            Err(NetworkError::Unverified));
        assert_eq!(tls_peer_gate(&Deny,b"",b"cert"),Err(NetworkError::Malformed));
    }
}

#[cfg(test)]
mod ws26_dhcp_untrusted_options_tests{
 use super::*;
 #[test]fn rejects_bad_netmask_and_post_end_payload(){
  let mut options=[99,130,83,99,1,4,255,255,255,0,3,4,10,0,0,1,
      51,4,0,0,0,30,255,0,0];
  assert!(parse_lease([10,0,0,2],&options).is_ok());
  options[24]=42;
  assert_eq!(parse_lease([10,0,0,2],&options),Err(NetworkError::Malformed));
  options[24]=0;options[9]=0xf5;
  assert_eq!(parse_lease([10,0,0,2],&options),Err(NetworkError::Malformed));
 }
}


// PR #782 WS45-47: bounded DHCP ACK, on-link lease and peer-bound TCP policy.
/// WS45: Reject a configured gateway outside the verified DHCP address's
/// subnet, or a self-gateway. Does not claim packet-origin authentication.
pub fn require_onlink_gateway(lease:Lease)->Result<Lease,NetworkError>{
 let mask=u32::from_be_bytes(lease.subnet);
 let host=u32::from_be_bytes(lease.address);
 let router=u32::from_be_bytes(lease.gateway);
 if mask==0||router==0||host==router||(host&mask)!=(router&mask){
  return Err(NetworkError::Malformed);
 }
 Ok(lease)
}
/// WS46: Verify fixed BOOTP identity and exactly one DHCP ACK/server-ID
/// option before accepting the existing option parser's lease.
pub fn parse_verified_ack(packet:&[u8],expected_xid:u32,
 expected_mac:[u8;6])->Result<Lease,NetworkError>{
 if packet.len()<241||packet[0]!=2||packet[1]!=1||packet[2]!=6||
  expected_xid==0||&packet[4..8]!=expected_xid.to_be_bytes().as_slice()||
  &packet[28..34]!=expected_mac.as_slice()||expected_mac==[0;6]{
  return Err(NetworkError::Malformed);
 }
 let addr:[u8;4]=packet[16..20].try_into().map_err(|_|NetworkError::Malformed)?;
 let options=&packet[236..];
 let lease=parse_lease(addr,options)?;
 let mut at=4;let mut ack=false;let mut server=false;
 while at<options.len(){
  let kind=options[at];at+=1;
  if kind==255{break;}
  if kind==0{continue;}
  if at>=options.len(){return Err(NetworkError::Malformed);}
  let size=usize::from(options[at]);at+=1;
  if size>options.len()-at{return Err(NetworkError::Malformed);}
  if kind==53 {
   if ack||size!=1||options[at]!=5{return Err(NetworkError::Malformed);}
   ack=true;
  }
  if kind==54 {
   if server||size!=4||&options[at..at+4]==[0u8;4].as_slice(){
    return Err(NetworkError::Malformed);
   }
   server=true;
  }
  at+=size;
 }
 if !ack||!server{return Err(NetworkError::Missing);}
 require_onlink_gateway(lease)
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct PeerTuple{
 pub source_ip:[u8;4],pub source_port:u16,pub destination_port:u16
}
pub struct BoundTcpHandshake{
 expected:PeerTuple, inner:TcpHandshake
}
/// WS47: A SYN-ACK must match the same verified peer 4-tuple identity as
/// the outbound SYN; packet checksums, routing and peer validation are
/// the responsibility of the actual transport.
impl BoundTcpHandshake{
 pub const fn new(expected:PeerTuple)->Self{
  Self{expected,inner:TcpHandshake::new()}
 }
 pub fn syn(&mut self,sequence:u32)->Result<(),NetworkError>{
  if self.expected.source_ip==[0;4]||self.expected.source_port==0||
    self.expected.destination_port==0{return Err(NetworkError::State);}
  self.inner.syn(sequence)
 }
 pub fn syn_ack(&mut self,peer:PeerTuple,ack:u32,syn:bool,ack_flag:bool)
  ->Result<(),NetworkError>{
  if peer!=self.expected{return Err(NetworkError::State);}
  self.inner.syn_ack(ack,syn,ack_flag)
 }
 pub fn state(&self)->TcpState{self.inner.state()}
}
#[cfg(test)]
mod pr782_ws45_47_tests {
 use super::*;
 #[test]fn ws45_rejects_offlink_and_self_gateway(){
  let l=Lease{address:[192,168,1,10],subnet:[255,255,255,0],
   gateway:[192,168,1,1],lease_seconds:60};
  assert_eq!(require_onlink_gateway(l),Ok(l));
  assert_eq!(require_onlink_gateway(Lease{gateway:[192,168,2,1],..l}),
   Err(NetworkError::Malformed));
  assert_eq!(require_onlink_gateway(Lease{gateway:l.address,..l}),
   Err(NetworkError::Malformed));
 }
 fn ack()->[u8;268]{
  let mut p=[0u8;268];p[0]=2;p[1]=1;p[2]=6;
  p[4..8].copy_from_slice(&42u32.to_be_bytes());
  p[16..20].copy_from_slice(&[10,0,0,10]);
  p[28..34].copy_from_slice(&[1,2,3,4,5,6]);
  p[236..].copy_from_slice(&[
   99,130,83,99,
   53,1,5,54,4,10,0,0,1,
   1,4,255,255,255,0,
   3,4,10,0,0,1,
   51,4,0,0,0,60,255
  ]);p
 }
 #[test]fn ws46_rejects_wrong_transaction_and_duplicate_ack(){
  let packet=ack();
  assert!(parse_verified_ack(&packet,42,[1,2,3,4,5,6]).is_ok());
  assert_eq!(parse_verified_ack(&packet,43,[1,2,3,4,5,6]),
   Err(NetworkError::Malformed));
  let mut invalid=packet;invalid[242]=3;
  assert_eq!(parse_verified_ack(&invalid,42,[1,2,3,4,5,6]),
   Err(NetworkError::Malformed));
 }
 #[test]fn ws47_wrong_peer_cannot_establish_connection(){
  let tuple=PeerTuple{source_ip:[10,0,0,1],source_port:443,destination_port:1234};
  let mut h=BoundTcpHandshake::new(tuple);h.syn(5).unwrap();
  let other=PeerTuple{source_port:444,..tuple};
  assert_eq!(h.syn_ack(other,6,true,true),Err(NetworkError::State));
  assert_eq!(h.state(),TcpState::SynSent);
  assert_eq!(h.syn_ack(tuple,6,true,true),Ok(()));
  assert_eq!(h.state(),TcpState::Established);
 }
}


// PR782 WS111-115: strict bounded network ingress framing and lease
// maintenance. No raw sockets, TCP state machine, or cryptographic TLS.
/// WS111: Reject unspecified, loopback, broadcast, multicast and reserved
/// IPv4 endpoints before using them as an externally verified peer tuple.
pub fn require_ipv4_unicast(address:[u8;4])->Result<(),NetworkError>{
 let first=address[0];
 if first==0||first==127||first>=224||
   address==[255;4]||address==[0;4]{
  return Err(NetworkError::Malformed);
 }
 Ok(())
}
/// WS112: Verify a fixed IPv4 header, total length and one's-complement
/// checksum. Fragmented datagrams are rejected rather than reassembled.
pub fn strict_ipv4_header(packet:&[u8])->Result<u8,NetworkError>{
 if packet.len()<20||packet.len()>65535||packet[0]!=0x45||
  packet[1]&0x03!=0{return Err(NetworkError::Malformed);}
 let total=u16::from_be_bytes([packet[2],packet[3]]) as usize;
 if total!=packet.len()||packet[8]==0||
  !matches!(packet[9],6|17){return Err(NetworkError::Malformed);}
 let flags=u16::from_be_bytes([packet[6],packet[7]]);
 if flags&0xbfff!=0{return Err(NetworkError::Malformed);}
 require_ipv4_unicast(packet[12..16].try_into().map_err(|_|NetworkError::Malformed)?)?;
 require_ipv4_unicast(packet[16..20].try_into().map_err(|_|NetworkError::Malformed)?)?;
 let mut sum=0u32;
 for i in (0..20).step_by(2){
  sum+=u32::from(u16::from_be_bytes([packet[i],packet[i+1]]));
 }
 while sum>>16!=0{sum=(sum&65535)+(sum>>16);}
 if sum!=65535{return Err(NetworkError::Malformed);}
 Ok(packet[9])
}
/// WS113: Strict zero-allocation UDP payload-bounds check. Nonzero checksum
/// is mandatory for our ingress policy; authenticating the UDP pseudoheader
/// remains the job of the real IP transport layer.
pub fn strict_udp_datagram(packet:&[u8],dest_port:u16)
 ->Result<&[u8],NetworkError>{
 if strict_ipv4_header(packet)?!=17||packet.len()<28||dest_port==0{
  return Err(NetworkError::Malformed);
 }
 let src=u16::from_be_bytes([packet[20],packet[21]]);
 let dst=u16::from_be_bytes([packet[22],packet[23]]);
 let size=u16::from_be_bytes([packet[24],packet[25]]) as usize;
 let check=u16::from_be_bytes([packet[26],packet[27]]);
 if src==0||dst!=dest_port||check==0||size<8||size!=packet.len()-20{
  return Err(NetworkError::Malformed);
 }
 Ok(&packet[28..])
}
/// WS114: Checked finite retry schedule for a trusted socket manager. An
/// overflow/invalid attempt may never silently wrap into a short retry.
pub fn bounded_retransmit_ms(base:u32,attempt:u8,cap:u32)
 ->Result<u32,NetworkError>{
 if base==0||cap==0||cap>60000||base>cap||attempt>8{
  return Err(NetworkError::State);
 }
 let mut delay=base;
 for _ in 0..attempt{delay=delay.checked_mul(2).ok_or(NetworkError::State)?.min(cap);}
 Ok(delay)
}
/// WS115: Compute DHCP T1/T2 from an externally verified on-link lease,
/// preserving the renewal-before-rebind-before-expiry invariant.
pub fn renewal_schedule(lease:Lease)->Result<(u32,u32,u32),NetworkError>{
 require_onlink_gateway(lease)?;
 let expires=lease.lease_seconds;
 if expires<8{return Err(NetworkError::Expired);}
 let t1=expires/2;let t2=expires-expires/8;
 if t1==0||t1>=t2||t2>=expires{return Err(NetworkError::Expired);}
 Ok((t1,t2,expires))
}
#[cfg(test)]
mod pr782_ws111_115_tests{
 use super::*;
 fn datagram()->[u8;28]{
  let mut p=[0u8;28];p[0]=0x45;p[2..4].copy_from_slice(&28u16.to_be_bytes());
  p[8]=64;p[9]=17;p[12..16].copy_from_slice(&[10,0,0,1]);
  p[16..20].copy_from_slice(&[10,0,0,2]);
  p[20..22].copy_from_slice(&1234u16.to_be_bytes());
  p[22..24].copy_from_slice(&8080u16.to_be_bytes());
  p[24..26].copy_from_slice(&8u16.to_be_bytes());
  p[26..28].copy_from_slice(&1u16.to_be_bytes());
  let mut sum=0u32;
  for i in (0..20).step_by(2){sum+=u32::from(
    u16::from_be_bytes([p[i],p[i+1]]));}
  while sum>>16!=0{sum=(sum&65535)+(sum>>16);}
  p[10..12].copy_from_slice(&(!(sum as u16)).to_be_bytes());p
 }
 #[test]fn ws111_multicast_and_loopback_are_not_unicast(){
  assert_eq!(require_ipv4_unicast([10,0,0,1]),Ok(()));
  for ip in [[0,0,0,0],[127,0,0,1],[224,0,0,1],[255;4]]{
   assert_eq!(require_ipv4_unicast(ip),Err(NetworkError::Malformed));
  }
 }
 #[test]fn ws112_checksum_length_and_fragments_fail_closed(){
  let p=datagram();assert_eq!(strict_ipv4_header(&p),Ok(17));
  let mut bad=p;bad[10]^=1;
  assert_eq!(strict_ipv4_header(&bad),Err(NetworkError::Malformed));
  let mut fragment=p;fragment[6]=0x20;
  assert_eq!(strict_ipv4_header(&fragment),Err(NetworkError::Malformed));
  assert_eq!(strict_ipv4_header(&p[..27]),Err(NetworkError::Malformed));
 }
 #[test]fn ws113_udp_destination_and_length_are_exact(){
  let p=datagram();assert_eq!(strict_udp_datagram(&p,8080),Ok(&b""[..]));
  assert_eq!(strict_udp_datagram(&p,53),Err(NetworkError::Malformed));
  let mut bad=p;bad[25]=7;
  assert_eq!(strict_udp_datagram(&bad,8080),Err(NetworkError::Malformed));
 }
 #[test]fn ws114_bounded_backoff_saturates_without_wrapping(){
  assert_eq!(bounded_retransmit_ms(100,0,1000),Ok(100));
  assert_eq!(bounded_retransmit_ms(100,4,1000),Ok(1000));
  assert_eq!(bounded_retransmit_ms(100,9,1000),Err(NetworkError::State));
  assert_eq!(bounded_retransmit_ms(0,1,1000),Err(NetworkError::State));
 }
 #[test]fn ws115_renewal_schedule_precedes_rebinding_and_expiry(){
  let lease=Lease{address:[10,0,0,2],subnet:[255,255,255,0],
   gateway:[10,0,0,1],lease_seconds:80};
  assert_eq!(renewal_schedule(lease),Ok((40,70,80)));
  assert_eq!(renewal_schedule(Lease{lease_seconds:7,..lease}),
   Err(NetworkError::Expired));
  assert_eq!(renewal_schedule(Lease{gateway:[10,0,1,1],..lease}),
   Err(NetworkError::Malformed));
 }
}

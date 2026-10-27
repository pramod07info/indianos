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
    if !end_seen{return Err(NetworkError::Malformed);}
    let ttl=seconds.ok_or(NetworkError::Missing)?;
    if ttl==0{return Err(NetworkError::Expired);}
    Ok(Lease{address,subnet:subnet.ok_or(NetworkError::Missing)?,
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

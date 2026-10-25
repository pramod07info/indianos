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
pub fn tls_peer_gate<V:TlsCertificateVerifier>(verifier:&V,
    hostname:&[u8],certificate:&[u8])->Result<(),NetworkError>{
    if hostname.is_empty()||hostname.len()>253||certificate.is_empty()||
        certificate.len()>16*1024{return Err(NetworkError::Malformed);}
    if !verifier.verify_chain_and_name(hostname,certificate){
        return Err(NetworkError::Unverified);
    }
    Ok(())
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
    #[test]fn tls_fails_closed_without_verified_certificate(){
        assert_eq!(tls_peer_gate(&Deny,b"example.org",b"cert"),
            Err(NetworkError::Unverified));
        assert_eq!(tls_peer_gate(&Deny,b"",b"cert"),Err(NetworkError::Malformed));
    }
}

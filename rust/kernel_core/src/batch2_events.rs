//! R036/13: bounded process-owned event broker. No user-supplied sender spoofing.
pub const PAYLOAD_BYTES:usize=24;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Event{pub sender:u32,pub recipient:u32,pub generation:u64,
    pub kind:u16,pub len:u8,pub payload:[u8;PAYLOAD_BYTES]}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum BrokerError{Invalid,Full,Empty,Denied}
pub struct Broker<const N:usize>{queue:[Option<Event>;N],len:usize}
impl<const N:usize> Broker<N>{
    pub const fn new()->Self{Self{queue:[None;N],len:0}}
    pub fn send(&mut self,authenticated_sender:u32,recipient:u32,generation:u64,
        kind:u16,body:&[u8])->Result<(),BrokerError>{
        if authenticated_sender==0||recipient==0||generation==0||kind==0||
            body.is_empty()||body.len()>PAYLOAD_BYTES{return Err(BrokerError::Invalid);}
        if self.len>=N{return Err(BrokerError::Full);}
        let mut payload=[0u8;PAYLOAD_BYTES];payload[..body.len()].copy_from_slice(body);
        self.queue[self.len]=Some(Event{sender:authenticated_sender,
            recipient,generation,kind,len:body.len() as u8,payload});
        self.len+=1;Ok(())
    }
    pub fn receive(&mut self,authenticated_pid:u32,generation:u64)
        ->Result<Event,BrokerError>{
        if authenticated_pid==0||generation==0{return Err(BrokerError::Denied);}
        let index=self.queue[..self.len].iter().position(|item|
            item.is_some_and(|e|e.recipient==authenticated_pid&&e.generation==generation))
            .ok_or(BrokerError::Empty)?;
        let event=self.queue[index].ok_or(BrokerError::Empty)?;
        self.remove(index);Ok(event)
    }
    fn remove(&mut self,index:usize){
        for i in index..self.len-1 {self.queue[i]=self.queue[i+1];}
        self.len-=1;self.queue[self.len]=None;
    }
    pub fn retire(&mut self,pid:u32,generation:u64){
        let mut i=0;
        while i<self.len {
            if self.queue[i].is_some_and(|e|e.recipient==pid&&e.generation==generation){
                self.remove(i)
            }else{i+=1;}
        }
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn recipient_and_generation_isolation(){
        let mut b=Broker::<3>::new();
        b.send(1,2,4,10,b"hello").unwrap();
        assert_eq!(b.receive(1,4),Err(BrokerError::Empty));
        assert_eq!(b.receive(2,3),Err(BrokerError::Empty));
        let message=b.receive(2,4).unwrap();
        assert_eq!(&message.payload[..message.len as usize],b"hello");
        assert_eq!(message.sender,1);
    }
    #[test] fn bounded_and_teardown_safe(){
        let mut b=Broker::<2>::new();
        b.send(1,2,1,1,b"a").unwrap();
        b.send(1,3,1,1,b"b").unwrap();
        assert_eq!(b.send(1,2,1,1,b"c"),Err(BrokerError::Full));
        b.retire(2,1);
        assert_eq!(b.receive(2,1),Err(BrokerError::Empty));
        assert_eq!(b.receive(3,1).unwrap().payload[0],b'b');
    }
    #[test] fn rejects_invalid_and_oversized_payloads(){
        let mut b=Broker::<1>::new();
        assert_eq!(b.send(0,1,1,1,b"x"),Err(BrokerError::Invalid));
        assert_eq!(b.send(1,2,1,1,&[7;25]),Err(BrokerError::Invalid));
    }
}

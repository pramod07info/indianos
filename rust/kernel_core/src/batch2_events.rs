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



/// R085: transactionally publish or consume a bounded authenticated event
/// group. The owner PID is supplied by trusted ingress, never by an app.
impl<const N:usize> Broker<N>{
    pub fn pending_total(&self)->usize{self.len}
    pub fn pending_for(&self,pid:u32,generation:u64)->usize{
        if pid==0||generation==0{return 0;}
        self.queue[..self.len].iter().flatten().filter(|event|
            event.recipient==pid&&event.generation==generation).count()
    }
    /// A late malformed or over-capacity item cannot leak earlier messages.
    pub fn send_batch(&mut self,sender:u32,
        items:&[(u32,u64,u16,&[u8])])->Result<usize,BrokerError>{
        if items.is_empty(){return Err(BrokerError::Invalid);}
        let mut staged=Self{queue:self.queue,len:self.len};
        for &(recipient,generation,kind,payload) in items{
            staged.send(sender,recipient,generation,kind,payload)?;
        }
        self.queue=staged.queue;self.len=staged.len;
        Ok(items.len())
    }
    /// Short caller buffers never drain any event. Messages for unrelated
    /// recipients stay queued in original FIFO order.
    pub fn receive_batch(&mut self,pid:u32,generation:u64,
        output:&mut[Event])->Result<usize,BrokerError>{
        if pid==0||generation==0{return Err(BrokerError::Denied);}
        let count=self.pending_for(pid,generation);
        if count==0{return Err(BrokerError::Empty);}
        if output.len()<count{return Err(BrokerError::Full);}
        for item in output.iter_mut().take(count){
            *item=self.receive(pid,generation)?;
        }
        Ok(count)
    }
    /// Only the authenticated destination can cancel its queued kind.
    pub fn cancel_kind(&mut self,pid:u32,generation:u64,
        kind:u16)->Result<usize,BrokerError>{
        if pid==0||generation==0||kind==0{return Err(BrokerError::Denied);}
        let mut removed=0;let mut i=0;
        while i<self.len{
            if self.queue[i].is_some_and(|e|
                e.recipient==pid&&e.generation==generation&&e.kind==kind){
                self.remove(i);removed+=1;
            }else{i+=1;}
        }
        Ok(removed)
    }
}
#[cfg(test)]
mod r085_broker_acceptance{
 use super::*;
 const EMPTY:Event=Event{sender:0,recipient:0,generation:0,kind:0,len:0,
    payload:[0;PAYLOAD_BYTES]};
 #[test]fn publish_group_rolls_back_capacity_and_invalid_receiver(){
    let mut b=Broker::<2>::new();
    assert_eq!(b.send_batch(9,&[(2,1,1,b"one"),(0,1,2,b"bad")]),
        Err(BrokerError::Invalid));
    assert_eq!(b.pending_total(),0);
    assert_eq!(b.send_batch(9,&[(2,1,1,b"a"),(2,1,2,b"b"),(2,1,3,b"c")]),
        Err(BrokerError::Full));
    assert_eq!(b.pending_total(),0);
    assert_eq!(b.send_batch(9,&[(2,1,1,b"a"),(3,2,2,b"b")]),Ok(2));
    assert_eq!(b.pending_for(2,1),1);
    assert_eq!(b.pending_for(2,2),0);
 }
 #[test]fn receive_group_preflights_buffer_and_isolates_owners(){
    let mut b=Broker::<3>::new();
    b.send_batch(8,&[(2,3,10,b"a"),(4,3,10,b"x"),(2,3,11,b"b")]).unwrap();
    let mut small=[EMPTY;1];
    assert_eq!(b.receive_batch(2,3,&mut small),Err(BrokerError::Full));
    assert_eq!(b.pending_total(),3);
    let mut out=[EMPTY;2];
    assert_eq!(b.receive_batch(2,3,&mut out),Ok(2));
    assert_eq!(out[0].kind,10);
    assert_eq!(out[1].kind,11);
    assert_eq!(b.receive(4,3).unwrap().payload[0],b'x');
 }
 #[test]fn kind_cancel_cannot_erase_foreign_or_stale_generation(){
    let mut b=Broker::<3>::new();
    b.send_batch(1,&[(5,7,1,b"a"),(5,8,1,b"b"),(5,7,2,b"c")]).unwrap();
    assert_eq!(b.cancel_kind(5,7,1),Ok(1));
    assert_eq!(b.pending_for(5,8),1);
    assert_eq!(b.pending_for(5,7),1);
    assert_eq!(b.cancel_kind(0,7,1),Err(BrokerError::Denied));
 }
}


/// R085 extension: bounded mailbox inspection and exact-recipient selective
/// consumption. Caller must be authenticated by syscall ingress; global
/// sender summaries are trusted diagnostic interfaces only.
impl<const N:usize> Broker<N>{
    pub fn available_slots(&self)->usize{N-self.len}
    pub fn is_empty(&self)->bool{self.len==0}
    pub fn is_full(&self)->bool{self.len==N}
    pub fn count_sender(&self,sender:u32)->usize{
        if sender==0{return 0;}
        self.queue[..self.len].iter().flatten().filter(|e|e.sender==sender).count()
    }
    pub fn count_kind_for(&self,pid:u32,epoch:u64,kind:u16)->usize{
        if pid==0||epoch==0||kind==0{return 0;}
        self.queue[..self.len].iter().flatten().filter(|e|
            e.recipient==pid&&e.generation==epoch&&e.kind==kind).count()
    }
    pub fn count_from_for(&self,pid:u32,epoch:u64,sender:u32)->usize{
        if pid==0||epoch==0||sender==0{return 0;}
        self.queue[..self.len].iter().flatten().filter(|e|
            e.recipient==pid&&e.generation==epoch&&e.sender==sender).count()
    }
    pub fn count_sender_kind(&self,sender:u32,kind:u16)->usize{
        if sender==0||kind==0{return 0;}
        self.queue[..self.len].iter().flatten().filter(|e|
            e.sender==sender&&e.kind==kind).count()
    }
    pub fn first_for(&self,pid:u32,epoch:u64)->Option<Event>{
        if pid==0||epoch==0{return None;}
        self.queue[..self.len].iter().flatten().copied().find(|e|
            e.recipient==pid&&e.generation==epoch)
    }
    pub fn last_for(&self,pid:u32,epoch:u64)->Option<Event>{
        if pid==0||epoch==0{return None;}
        self.queue[..self.len].iter().rev().flatten().copied().find(|e|
            e.recipient==pid&&e.generation==epoch)
    }
    pub fn first_kind_for(&self,pid:u32,epoch:u64,kind:u16)->Option<Event>{
        if kind==0{return None;}
        self.queue[..self.len].iter().flatten().copied().find(|e|
            pid!=0&&epoch!=0&&e.recipient==pid&&e.generation==epoch&&e.kind==kind)
    }
    pub fn last_kind_for(&self,pid:u32,epoch:u64,kind:u16)->Option<Event>{
        if kind==0{return None;}
        self.queue[..self.len].iter().rev().flatten().copied().find(|e|
            pid!=0&&epoch!=0&&e.recipient==pid&&e.generation==epoch&&e.kind==kind)
    }
    pub fn peek_at_for(&self,pid:u32,epoch:u64,index:usize)->Option<Event>{
        if pid==0||epoch==0{return None;}
        self.queue[..self.len].iter().flatten().copied().filter(|e|
            e.recipient==pid&&e.generation==epoch).nth(index)
    }
    pub fn snapshot_for(&self,pid:u32,epoch:u64,out:&mut[Event])
        ->Result<usize,BrokerError>{
        if pid==0||epoch==0{return Err(BrokerError::Denied);}
        let count=self.pending_for(pid,epoch);
        if out.len()<count{return Err(BrokerError::Full);}
        let mut n=0;
        for event in self.queue[..self.len].iter().flatten(){
            if event.recipient==pid&&event.generation==epoch{
                out[n]=*event;n+=1;
            }
        }
        Ok(n)
    }
    pub fn snapshot_kind_for(&self,pid:u32,epoch:u64,kind:u16,
        out:&mut[Event])->Result<usize,BrokerError>{
        if pid==0||epoch==0||kind==0{return Err(BrokerError::Denied);}
        let count=self.count_kind_for(pid,epoch,kind);
        if out.len()<count{return Err(BrokerError::Full);}
        let mut n=0;
        for event in self.queue[..self.len].iter().flatten(){
            if event.recipient==pid&&event.generation==epoch&&event.kind==kind{
                out[n]=*event;n+=1;
            }
        }
        Ok(n)
    }
    pub fn receive_kind_for(&mut self,pid:u32,epoch:u64,kind:u16)
        ->Result<Event,BrokerError>{
        if pid==0||epoch==0||kind==0{return Err(BrokerError::Denied);}
        let index=self.queue[..self.len].iter().position(|item|item.is_some_and(|e|
            e.recipient==pid&&e.generation==epoch&&e.kind==kind))
            .ok_or(BrokerError::Empty)?;
        let event=self.queue[index].ok_or(BrokerError::Empty)?;
        self.remove(index);Ok(event)
    }
    pub fn receive_from_for(&mut self,pid:u32,epoch:u64,sender:u32)
        ->Result<Event,BrokerError>{
        if pid==0||epoch==0||sender==0{return Err(BrokerError::Denied);}
        let index=self.queue[..self.len].iter().position(|item|item.is_some_and(|e|
            e.recipient==pid&&e.generation==epoch&&e.sender==sender))
            .ok_or(BrokerError::Empty)?;
        let event=self.queue[index].ok_or(BrokerError::Empty)?;
        self.remove(index);Ok(event)
    }
    pub fn cancel_all_for(&mut self,pid:u32,epoch:u64)
        ->Result<usize,BrokerError>{
        if pid==0||epoch==0{return Err(BrokerError::Denied);}
        let removed=self.pending_for(pid,epoch);
        self.retire(pid,epoch);Ok(removed)
    }
    pub fn cancel_from_for(&mut self,pid:u32,epoch:u64,sender:u32)
        ->Result<usize,BrokerError>{
        if pid==0||epoch==0||sender==0{return Err(BrokerError::Denied);}
        let mut i=0;let mut count=0;
        while i<self.len{
            if self.queue[i].is_some_and(|e|e.recipient==pid&&
                e.generation==epoch&&e.sender==sender){
                self.remove(i);count+=1;
            }else{i+=1;}
        }
        Ok(count)
    }
    pub fn cancel_kinds_for(&mut self,pid:u32,epoch:u64,kinds:&[u16])
        ->Result<usize,BrokerError>{
        if pid==0||epoch==0||kinds.is_empty()||kinds.contains(&0){
            return Err(BrokerError::Denied);
        }
        let mut i=0;let mut count=0;
        while i<self.len{
            if self.queue[i].is_some_and(|e|e.recipient==pid&&
                e.generation==epoch&&kinds.contains(&e.kind)){
                self.remove(i);count+=1;
            }else{i+=1;}
        }
        Ok(count)
    }
    /// Trusted supervisor cancels messages from an exited sender globally.
    /// Receiver-side retire() separately drops the terminated recipient.
    pub fn drop_sender(&mut self,sender:u32)->Result<usize,BrokerError>{
        if sender==0{return Err(BrokerError::Denied);}
        let mut i=0;let mut count=0;
        while i<self.len{
            if self.queue[i].is_some_and(|e|e.sender==sender){
                self.remove(i);count+=1;
            }else{i+=1;}
        }
        Ok(count)
    }
}
#[cfg(test)]
mod r085_extension_broker_tests{
    use super::*;
    const EMPTY:Event=Event{sender:0,recipient:0,generation:0,kind:0,
        len:0,payload:[0;PAYLOAD_BYTES]};
    #[test]fn filtered_mailbox_read_and_consume_preserves_foreign_events(){
        let mut b=Broker::<5>::new();
        assert!(b.is_empty());assert_eq!(b.available_slots(),5);
        b.send_batch(8,&[(2,1,10,b"a"),(3,1,10,b"x"),
            (2,1,11,b"b")]).unwrap();
        b.send(9,2,2,10,b"other").unwrap();
        assert_eq!(b.pending_total(),4);
        assert_eq!(b.available_slots(),1);
        assert_eq!(b.count_sender(8),3);
        assert_eq!(b.count_kind_for(2,1,10),1);
        assert_eq!(b.count_from_for(2,1,8),2);
        assert_eq!(b.count_sender_kind(8,10),2);
        assert_eq!(b.first_for(2,1).unwrap().kind,10);
        assert_eq!(b.last_for(2,1).unwrap().kind,11);
        assert_eq!(b.first_kind_for(2,1,11).unwrap().payload[0],b'b');
        assert_eq!(b.last_kind_for(2,1,10).unwrap().payload[0],b'a');
        assert_eq!(b.peek_at_for(2,1,1).unwrap().kind,11);
        let mut small=[EMPTY;1];
        assert_eq!(b.snapshot_for(2,1,&mut small),Err(BrokerError::Full));
        assert_eq!(small[0],EMPTY);
        let mut out=[EMPTY;3];
        assert_eq!(b.snapshot_for(2,1,&mut out),Ok(2));
        assert_eq!(out[1].kind,11);
        assert_eq!(b.snapshot_kind_for(2,1,10,&mut out),Ok(1));
        assert_eq!(out[0].kind,10);
        assert_eq!(b.receive_kind_for(2,1,11).unwrap().payload[0],b'b');
        assert_eq!(b.receive_from_for(2,1,8).unwrap().kind,10);
        assert_eq!(b.pending_for(2,1),0);
        assert_eq!(b.pending_for(2,2),1);
        assert_eq!(b.cancel_from_for(2,2,8),Ok(0));
        assert_eq!(b.cancel_all_for(2,2),Ok(1));
        assert_eq!(b.receive(3,1).unwrap().payload[0],b'x');
        assert!(b.is_empty());
    }
    #[test]fn selective_revoke_preserves_unrelated_and_invalid_input_is_atomic(){
        let mut b=Broker::<3>::new();
        b.send_batch(7,&[(3,1,1,b"a"),(3,1,2,b"b"),(4,1,2,b"c")]).unwrap();
        assert!(b.is_full());
        assert_eq!(b.cancel_kinds_for(3,1,&[0]),Err(BrokerError::Denied));
        assert_eq!(b.pending_total(),3);
        assert_eq!(b.cancel_kinds_for(3,1,&[1]),Ok(1));
        assert_eq!(b.count_kind_for(3,1,1),0);
        assert_eq!(b.drop_sender(7),Ok(2));
        assert_eq!(b.pending_total(),0);
        assert_eq!(b.drop_sender(0),Err(BrokerError::Denied));
        assert_eq!(b.receive_kind_for(3,1,1),Err(BrokerError::Empty));
    }
}


/// R085 extension III: deterministic bounded broker admission, exact-owner
/// selective delivery, event replacement, and authenticated cancellation.
impl<const N:usize> Broker<N>{
    pub fn can_enqueue(&self,count:usize)->bool{
        count>0&&count<=self.available_slots()
    }
    pub fn owner_has_kind(&self,pid:u32,epoch:u64,kind:u16)->bool{
        self.count_kind_for(pid,epoch,kind)!=0
    }
    pub fn sender_has_kind(&self,sender:u32,kind:u16)->bool{
        self.count_sender_kind(sender,kind)!=0
    }
    pub fn pending_from_kind_for(&self,pid:u32,epoch:u64,
        sender:u32,kind:u16)->usize{
        if pid==0||epoch==0||sender==0||kind==0{return 0;}
        self.queue[..self.len].iter().flatten().filter(|e|
            e.recipient==pid&&e.generation==epoch&&
            e.sender==sender&&e.kind==kind).count()
    }
    pub fn pending_payload_bytes_for(&self,pid:u32,epoch:u64)->usize{
        if pid==0||epoch==0{return 0;}
        self.queue[..self.len].iter().flatten().filter(|e|
            e.recipient==pid&&e.generation==epoch)
            .map(|e|e.len as usize).sum()
    }
    pub fn total_payload_bytes(&self)->usize{
        self.queue[..self.len].iter().flatten().map(|e|e.len as usize).sum()
    }
    pub fn first_from_for(&self,pid:u32,epoch:u64,sender:u32)->Option<Event>{
        if pid==0||epoch==0||sender==0{return None;}
        self.queue[..self.len].iter().flatten().copied().find(|e|
            e.recipient==pid&&e.generation==epoch&&e.sender==sender)
    }
    pub fn last_from_for(&self,pid:u32,epoch:u64,sender:u32)->Option<Event>{
        if pid==0||epoch==0||sender==0{return None;}
        self.queue[..self.len].iter().rev().flatten().copied().find(|e|
            e.recipient==pid&&e.generation==epoch&&e.sender==sender)
    }
    pub fn first_matching_for(&self,pid:u32,epoch:u64,
        sender:u32,kind:u16)->Option<Event>{
        if pid==0||epoch==0||sender==0||kind==0{return None;}
        self.queue[..self.len].iter().flatten().copied().find(|e|
            e.recipient==pid&&e.generation==epoch&&
            e.sender==sender&&e.kind==kind)
    }
    pub fn receive_matching_for(&mut self,pid:u32,epoch:u64,
        sender:u32,kind:u16)->Result<Event,BrokerError>{
        if pid==0||epoch==0||sender==0||kind==0{
            return Err(BrokerError::Denied);
        }
        let i=self.queue[..self.len].iter().position(|s|s.is_some_and(|e|
            e.recipient==pid&&e.generation==epoch&&
            e.sender==sender&&e.kind==kind)).ok_or(BrokerError::Empty)?;
        let event=self.queue[i].ok_or(BrokerError::Empty)?;
        self.remove(i);Ok(event)
    }
    pub fn receive_up_to_for(&mut self,pid:u32,epoch:u64,
        limit:usize,out:&mut[Event])->Result<usize,BrokerError>{
        if pid==0||epoch==0{return Err(BrokerError::Denied);}
        if limit==0{return Err(BrokerError::Invalid);}
        let count=self.pending_for(pid,epoch).min(limit);
        if out.len()<count{return Err(BrokerError::Full);}
        if count==0{return Err(BrokerError::Empty);}
        for slot in out.iter_mut().take(count){
            *slot=self.receive(pid,epoch)?;
        }
        Ok(count)
    }
    pub fn receive_kind_up_to_for(&mut self,pid:u32,epoch:u64,
        kind:u16,limit:usize,out:&mut[Event])->Result<usize,BrokerError>{
        if pid==0||epoch==0||kind==0{return Err(BrokerError::Denied);}
        if limit==0{return Err(BrokerError::Invalid);}
        let count=self.count_kind_for(pid,epoch,kind).min(limit);
        if out.len()<count{return Err(BrokerError::Full);}
        if count==0{return Err(BrokerError::Empty);}
        for slot in out.iter_mut().take(count){
            *slot=self.receive_kind_for(pid,epoch,kind)?;
        }
        Ok(count)
    }
    /// Deduplicate only an exact sender/recipient/process epoch/event kind.
    pub fn send_if_absent(&mut self,sender:u32,pid:u32,epoch:u64,
        kind:u16,body:&[u8])->Result<bool,BrokerError>{
        if sender==0||pid==0||epoch==0||kind==0||
            body.is_empty()||body.len()>PAYLOAD_BYTES{
            return Err(BrokerError::Invalid);
        }
        if self.pending_from_kind_for(pid,epoch,sender,kind)>0{return Ok(false);}
        self.send(sender,pid,epoch,kind,body)?;
        Ok(true)
    }
    /// In-place update never consumes queue capacity; only the same sender
    /// may update a queued kind, and body validation precedes all mutation.
    pub fn replace_first_exact(&mut self,sender:u32,pid:u32,epoch:u64,
        kind:u16,body:&[u8])->Result<(),BrokerError>{
        if sender==0||pid==0||epoch==0||kind==0||
            body.is_empty()||body.len()>PAYLOAD_BYTES{
            return Err(BrokerError::Invalid);
        }
        let item=self.queue[..self.len].iter_mut().flatten().find(|e|
            e.sender==sender&&e.recipient==pid&&
            e.generation==epoch&&e.kind==kind).ok_or(BrokerError::Denied)?;
        item.payload=[0;PAYLOAD_BYTES];
        item.payload[..body.len()].copy_from_slice(body);
        item.len=body.len() as u8;
        Ok(())
    }
    pub fn cancel_matching_for(&mut self,pid:u32,epoch:u64,
        sender:u32,kind:u16)->Result<usize,BrokerError>{
        if pid==0||epoch==0||sender==0||kind==0{return Err(BrokerError::Denied);}
        let mut removed=0;let mut i=0;
        while i<self.len{
            if self.queue[i].is_some_and(|e|e.recipient==pid&&
                e.generation==epoch&&e.sender==sender&&e.kind==kind){
                self.remove(i);removed+=1;
            }else{i+=1;}
        }
        Ok(removed)
    }
    pub fn cancel_first_kind_for(&mut self,pid:u32,epoch:u64,
        kind:u16)->Result<bool,BrokerError>{
        match self.receive_kind_for(pid,epoch,kind){
            Ok(_)=>Ok(true),Err(BrokerError::Empty)=>Ok(false),Err(e)=>Err(e)
        }
    }
    pub fn discard_first_for(&mut self,pid:u32,epoch:u64)
        ->Result<bool,BrokerError>{
        match self.receive(pid,epoch){
            Ok(_)=>Ok(true),Err(BrokerError::Empty)=>Ok(false),Err(e)=>Err(e)
        }
    }
    /// Preserve other destinations when trimming this owner's queue.
    pub fn retain_last_for(&mut self,pid:u32,epoch:u64,
        limit:usize)->Result<usize,BrokerError>{
        if pid==0||epoch==0{return Err(BrokerError::Denied);}
        let mut excess=self.pending_for(pid,epoch).saturating_sub(limit);
        let removed=excess;
        while excess>0{self.receive(pid,epoch)?;excess-=1;}
        Ok(removed)
    }
    pub fn retain_first_for(&mut self,pid:u32,epoch:u64,
        limit:usize)->Result<usize,BrokerError>{
        if pid==0||epoch==0{return Err(BrokerError::Denied);}
        let mut excess=self.pending_for(pid,epoch).saturating_sub(limit);
        let removed=excess;
        while excess>0{
            let index=self.queue[..self.len].iter().rposition(|e|e.is_some_and(|x|
                x.recipient==pid&&x.generation==epoch)).ok_or(BrokerError::Empty)?;
            self.remove(index);excess-=1;
        }
        Ok(removed)
    }
    /// A trusted supervisor may drop a single retired destination kind,
    /// without affecting other owners or epochs.
    pub fn retire_kind(&mut self,pid:u32,epoch:u64,
        kind:u16)->Result<usize,BrokerError>{
        self.cancel_kind(pid,epoch,kind)
    }
}
#[cfg(test)]mod r085_extension3_broker_tests{
    use super::*;
    const ZERO:Event=Event{sender:0,recipient:0,generation:0,kind:0,
        len:0,payload:[0;PAYLOAD_BYTES]};
    #[test]fn exact_owner_batch_delivery_and_replacement_never_leak(){
        let mut b=Broker::<6>::new();
        assert!(b.can_enqueue(5));assert!(!b.can_enqueue(0));
        b.send_batch(7,&[(4,2,10,b"abc"),(4,2,10,b"d"),
            (5,2,10,b"secret"),(4,3,10,b"new")]).unwrap();
        assert_eq!(b.pending_from_kind_for(4,2,7,10),2);
        assert_eq!(b.pending_payload_bytes_for(4,2),4);
        assert_eq!(b.total_payload_bytes(),13);
        assert!(b.owner_has_kind(4,2,10));
        assert!(b.sender_has_kind(7,10));
        assert_eq!(b.first_from_for(4,2,7).unwrap().len,3);
        assert_eq!(b.last_from_for(4,2,7).unwrap().len,1);
        assert_eq!(b.first_matching_for(4,2,7,10).unwrap().len,3);
        assert_eq!(b.send_if_absent(7,4,2,10,b"x"),Ok(false));
        b.replace_first_exact(7,4,2,10,b"xy").unwrap();
        assert_eq!(b.first_for(4,2).unwrap().payload[0],b'x');
        assert_eq!(b.first_for(5,2).unwrap().payload[0],b's');
        let mut tiny=[ZERO;1];
        assert_eq!(b.receive_kind_up_to_for(4,2,10,2,&mut tiny),
            Err(BrokerError::Full));
        assert_eq!(tiny,[ZERO]);assert_eq!(b.pending_for(4,2),2);
        assert_eq!(b.receive_up_to_for(4,2,1,&mut tiny),Ok(1));
        assert_eq!(tiny[0].len,2);
        assert_eq!(b.receive_matching_for(4,2,7,10).unwrap().len,1);
        assert_eq!(b.pending_for(5,2),1);
    }
    #[test]fn trimming_and_exact_cancellation_preserve_other_destinations(){
        let mut b=Broker::<6>::new();
        b.send_batch(2,&[(3,1,7,b"a"),(3,1,8,b"b"),
            (4,1,7,b"other"),(3,1,7,b"c"),(3,1,7,b"d")]).unwrap();
        assert_eq!(b.retain_last_for(3,1,2),Ok(2));
        assert_eq!(b.pending_for(3,1),2);
        assert_eq!(b.retain_first_for(3,1,1),Ok(1));
        assert_eq!(b.count_kind_for(3,1,7),1);
        assert_eq!(b.cancel_matching_for(3,1,2,7),Ok(1));
        assert_eq!(b.cancel_first_kind_for(3,1,7),Ok(false));
        assert_eq!(b.discard_first_for(3,1),Ok(false));
        assert_eq!(b.retire_kind(4,1,7),Ok(1));
        assert!(b.is_empty());
    }
}

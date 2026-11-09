//! Bounded authenticated startup join for the three independent services.
#[derive(Default)]
pub struct Startup{ready:u8}
impl Startup{
 pub fn observe(&mut self,packet:&[u8])->Result<bool,()>{
  if !(8..=72).contains(&packet.len()){return Err(());}
  let sender=u32::from_le_bytes(packet[..4].try_into().map_err(|_|())?);
  let length=u32::from_le_bytes(packet[4..8].try_into().map_err(|_|())?) as usize;
  if length>64||packet.len()!=length+8{return Err(());}
  let bit=match (sender,&packet[8..]){
   (2,b"FILES")=>1,(3,b"TERMINAL")=>2,(4,b"COMPOSITOR")=>4,
   _=>return Err(()),
  };
  if self.ready&bit!=0{return Err(());}
  self.ready|=bit;Ok(self.ready==7)
 }
}

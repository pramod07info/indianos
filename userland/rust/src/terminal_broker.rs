//! Admission for one authenticated PID1 -> Terminal key/reply transaction.
//! Names and filesystem operations are validated independently by terminal_fs.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Decision{Complete(bool),Dispatch,Denied}
pub struct Exchange{may_request:bool,may_cancel:bool,requested:bool,complete:bool}
impl Exchange{
 pub fn new(key:u8,begin:bool)->Self{
  Self{may_request:!begin&&matches!(key,b'\r'|b'\n'),
       may_cancel:!begin&&key==b'\t',requested:false,complete:false}
 }
 pub fn observe(&mut self,message:&[u8])->Result<Decision,()>{
  if self.complete{return Err(());}
  match message{
   b"A"=>{self.complete=true;Ok(Decision::Complete(true))},
   b"C" if self.may_cancel=>{self.complete=true;Ok(Decision::Complete(false))},
   b"C"=>Err(()),
   _=>{
    // A single line submits at most one filesystem command, including a
    // failed/denied command. Never retry an ambiguous mutation in this turn.
    if self.requested{return Err(());}
    self.requested=true;
    Ok(if self.may_request{Decision::Dispatch}else{Decision::Denied})
   },
  }
 }
}

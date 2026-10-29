//! R034/20: kernel-side acceptance across all 19 workspace state and action features.
use super::{Error,agent_01,agent_02,agent_03,agent_04,agent_05,agent_06,
agent_07,agent_08,agent_09,agent_10,agent_11,agent_12,agent_13,
agent_14,agent_15,agent_16,agent_17,agent_18,agent_19};
pub fn smoke()->Result<(),Error>{
 let mut recent=agent_01::Recent::<3>::new(7);recent.visit(7,1)?;recent.visit(7,2)?;
 if recent.at(7,0)?!=2{return Err(Error::Conflict);}
 let mut nav=agent_02::Navigate::<3>::new();nav.push(1)?;nav.push(2)?;
 if nav.back()?!=1{return Err(Error::Conflict);}
 let mut parent=[0u8;32];
 let n=agent_03::parent(b"/p/src/main.rs",&mut parent)?;
 if &parent[..n]!=b"/p/src"{return Err(Error::Conflict);}
 if agent_04::select_lines(b"a\nb\nc",2,3)?!=(2,5){return Err(Error::Conflict);}
 if agent_05::next(b"a-b-a",b"a",2,true)?!=Some(4){return Err(Error::Conflict);}
 let mut symbol=[0u8;16];let n=agent_06::identifier(b"let alpha=1",6,&mut symbol)?;
 if &symbol[..n]!=b"alpha"{return Err(Error::Conflict);}
 agent_07::destination(b"/p",b"/p/old",b"/p/new")?;
 if agent_08::decide(2,1,true)!=agent_08::Decision::Conflict{return Err(Error::Conflict);}
 let mut events=agent_09::Ledger::<2>::new();
 events.push(agent_09::Event{seq:1,actor:7,operation:1})?;
 if events.len()!=1{return Err(Error::Conflict);}
 if !agent_10::extension(b"/p/main.rs",b".rs")?{return Err(Error::Conflict);}
 let mut hist=agent_11::Commands::<2>::new(7);hist.record(7,b"cat /p/a")?;
 if hist.count(7)?!=1{return Err(Error::Conflict);}
 let mut page=[0u8;10];let n=agent_12::page(b"a\nb\nc\n",1,1,&mut page)?;
 if &page[..n]!=b"b\n"{return Err(Error::Conflict);}
 let mut quota=agent_13::Quota::new(7,100);quota.reserve(7,50)?;quota.free(7,10)?;
 agent_14::validate(&[0,1,2])?;
 let mut dirty=[0usize;2];
 if agent_15::dirty(&[1,2],&[1,3],&mut dirty)?!=1||dirty[0]!=1{return Err(Error::Conflict);}
 if agent_16::aggregate(&[1,1,0])?.passed!=2{return Err(Error::Conflict);}
 let mut time=agent_17::Timepoint::new();
 if time.observe(true,10)?!=1{return Err(Error::Conflict);}
 if agent_18::transition(agent_18::State::Stopped,1,true)!=Ok(agent_18::State::Running){
 return Err(Error::Conflict);}
 let mut panels=agent_19::Panels::new(7);panels.show(7,2)?;
 if panels.focused()!=2{return Err(Error::Conflict);}
 Ok(())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn complete_r034_workspace_acceptance(){assert_eq!(smoke(),Ok(()));}
 #[test]fn foreign_session_rejected_by_workspace_history(){
 let mut r=agent_01::Recent::<2>::new(7);
 assert_eq!(r.visit(8,1),Err(Error::Denied));
 }
}

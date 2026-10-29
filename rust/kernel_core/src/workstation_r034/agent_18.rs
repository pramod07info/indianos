//! R034/18: Debugger step and pause state machine.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub enum State{Stopped,Running,Paused}pub fn transition(current:State,action:u8,trusted:bool)->Result<State,Error>{if !trusted{return Err(Error::Denied);}match(current,action){(State::Stopped,1)|(State::Paused,1)=>Ok(State::Running),(State::Running,2)=>Ok(State::Paused),(_,3)=>Ok(State::Stopped),_=>Err(Error::Conflict)}}
#[cfg(test)] mod tests {use super::*;#[test]fn functional_and_boundary_cases(){assert_eq!(transition(State::Stopped,1,true),Ok(State::Running));assert_eq!(transition(State::Running,2,true),Ok(State::Paused));}}

//! R035/14 — Native build DAG execution and trusted completion.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn build<const N:usize>(owner:u64,dependencies:&[u32],passes:&[bool])->Result<bool,Error>{if owner==0||dependencies.len()!=passes.len()||dependencies.len()>N{return Err(Error::Invalid);}let mut plan=crate::workstation_r032::agent_14::Plan::<N>::new(owner);for(i,&mask)in dependencies.iter().enumerate(){plan.add(owner,i as u8,mask).map_err(|_|Error::Invalid)?;}let mut order=[0u8;N];plan.order(owner,&mut order).map_err(|_|Error::Conflict)?;for _ in 0..dependencies.len(){let Some(id)=plan.next(owner).map_err(|_|Error::Conflict)? else{break;};plan.finish(owner,true,id,passes[id as usize]).map_err(|_|Error::Conflict)?;}Ok(plan.green())}
#[cfg(test)]mod tests{use super::*;#[test]fn real_workflow_and_failure_guard(){assert_eq!(build::<3>(7,&[0,1,2],&[true,true,true]),Ok(true));assert_eq!(build::<3>(7,&[0,1,2],&[true,false,true]),Ok(false));}}

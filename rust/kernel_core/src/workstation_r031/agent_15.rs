//! R031/15: explicit, non-executing bounded template expansion from session environment.
use super::Error;
use crate::workstation_r030::agent_13::Env;
/// Replaces only ${NAME}; never invokes a shell or executes expanded output.
pub fn expand<const N:usize>(env:&Env<N>,owner:u64,template:&[u8],
 out:&mut[u8])->Result<usize,Error>{
 if template.len()>512||template.contains(&0){return Err(Error::Invalid);}
 let mut staged=[0u8;1024];let (mut i,mut n)=(0usize,0usize);
 while i<template.len(){
  if template[i..].starts_with(b"${"){
   let start=i+2;
   let len=template[start..].iter().position(|b|*b==b'}')
    .ok_or(Error::Invalid)?;
   if len==0||len>16{return Err(Error::Invalid);}
   let mut bytes=[0u8;64];
   let k=env.read(owner,&template[start..start+len],&mut bytes)
    .map_err(|_|Error::Denied)?;
   if n+k>staged.len(){return Err(Error::Full);}
   staged[n..n+k].copy_from_slice(&bytes[..k]);n+=k;i=start+len+1;
  }else{
   if n==staged.len(){return Err(Error::Full);}
   staged[n]=template[i];n+=1;i+=1;
  }
 }
 if n>out.len(){return Err(Error::Full);}
 out[..n].copy_from_slice(&staged[..n]);Ok(n)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn terminal_template_expands_owner_specific_value_as_literal_data(){
  let mut env=Env::<1>::new(7);env.set(7,b"WORKSPACE",b"/p").unwrap();
  let mut output=[0;32];
  let n=expand(&env,7,b"open ${WORKSPACE}/a",&mut output).unwrap();
  assert_eq!(&output[..n],b"open /p/a");
 }
 #[test]fn missing_variable_or_foreign_owner_do_not_output_partial_expansion(){
  let env=Env::<1>::new(7);let mut output=[9u8;32];
  assert_eq!(expand(&env,7,b"X${MISSING}",&mut output),Err(Error::Denied));
  assert_eq!(expand(&env,8,b"${HOME}",&mut output),Err(Error::Denied));
  assert_eq!(output,[9;32]);
 }
}
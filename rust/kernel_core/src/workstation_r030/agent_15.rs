//! R030/15: deterministic dependency-aware job order; cycles stop execution.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Job {pub id:u8,pub dependencies:u32}
pub fn order(jobs:&[Job],out:&mut[u8])->Result<usize,Error>{
 if jobs.is_empty()||jobs.len()>32||jobs.len()>out.len(){return Err(Error::Full);}
 let mut present=0u32;
 for job in jobs{
  if job.id>=32{return Err(Error::Invalid);}
  let flag=1u32<<job.id;
  if present&flag!=0||job.dependencies&flag!=0{return Err(Error::Invalid);}
  present|=flag;
 }
 if jobs.iter().any(|j|j.dependencies&!present!=0){return Err(Error::Missing);}
 let mut done=0u32;let mut staged=[0u8;32];
 for i in 0..jobs.len(){
  let mut selected=None;
  for job in jobs{
   let flag=1u32<<job.id;
   if done&flag==0&&job.dependencies&!done==0&&
    selected.is_none_or(|id:u8|job.id<id){selected=Some(job.id);}
  }
  let id=selected.ok_or(Error::Conflict)?;staged[i]=id;done|=1u32<<id;
 }
 out[..jobs.len()].copy_from_slice(&staged[..jobs.len()]);Ok(jobs.len())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn build_jobs_are_sorted_by_dependencies_not_submission_order(){
  let jobs=[Job{id:2,dependencies:1<<1},Job{id:1,dependencies:1<<0},
   Job{id:0,dependencies:0}];let mut out=[0;3];
  assert_eq!(order(&jobs,&mut out),Ok(3));assert_eq!(out,[0,1,2]);
 }
 #[test]fn cycles_and_missing_jobs_cannot_dispatch(){
  let a=[Job{id:0,dependencies:2},Job{id:1,dependencies:1}];
  assert_eq!(order(&a,&mut[0;2]),Err(Error::Conflict));
  assert_eq!(order(&[Job{id:0,dependencies:4}],&mut[0;1]),Err(Error::Missing));
 }
}
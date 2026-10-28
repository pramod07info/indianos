//! R021/13: fail-closed three-way file merge preview with exact base revisions.
//! Produces plans only. Never overwrites a developer workspace automatically.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Resolution{Identical,TakeOurs,TakeTheirs,Conflict}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct FileState{pub path_hash:u64,pub digest:u64,pub revision:u64}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Decision{pub path_hash:u64,pub resolution:Resolution,
 pub output_digest:u64,pub base_revision:u64}
pub fn decide(base:FileState,ours:FileState,
 theirs:FileState)->Result<Decision,Error>{
 if base.path_hash==0||ours.path_hash!=base.path_hash||
  theirs.path_hash!=base.path_hash||base.revision==0||
  ours.revision<base.revision||theirs.revision<base.revision{
  return Err(Error::Invalid);
 }
 let (resolution,out)=if ours.digest==theirs.digest{
  (Resolution::Identical,ours.digest)
 }else if ours.digest==base.digest{
  (Resolution::TakeTheirs,theirs.digest)
 }else if theirs.digest==base.digest{
  (Resolution::TakeOurs,ours.digest)
 }else{(Resolution::Conflict,0)};
 Ok(Decision{path_hash:base.path_hash,resolution,
  output_digest:out,base_revision:base.revision})
}
pub fn plan<const N:usize>(bases:&[FileState],ours:&[FileState],
 theirs:&[FileState],out:&mut[Decision;N])->Result<usize,Error>{
 if bases.len()!=ours.len()||bases.len()!=theirs.len()||
   bases.len()>N{return Err(Error::Full);}
 let empty=Decision{path_hash:0,resolution:Resolution::Conflict,
  output_digest:0,base_revision:0};
 let mut staged=[empty;N];
 for i in 0..bases.len(){
  if bases[..i].iter().any(|x|x.path_hash==bases[i].path_hash){
   return Err(Error::Conflict);
  }
  staged[i]=decide(bases[i],ours[i],theirs[i])?;
 }
 *out=staged;Ok(bases.len())
}
pub fn all_resolved(items:&[Decision])->bool{
 items.iter().all(|d|d.resolution!=Resolution::Conflict&&d.path_hash!=0)
}
pub fn conflicts(items:&[Decision])->usize{
 items.iter().filter(|d|d.resolution==Resolution::Conflict).count()
}
pub fn approval_required(items:&[Decision])->bool{
 items.iter().any(|d|d.resolution==Resolution::TakeTheirs)
}
#[cfg(test)]mod tests{
 use super::*;
 const ZERO:Decision=Decision{path_hash:0,resolution:Resolution::Conflict,
  output_digest:0,base_revision:0};
 fn file(digest:u64)->FileState{FileState{path_hash:1,digest,revision:1}}
 #[test]fn automatic_fast_forward_only_when_other_side_unchanged(){
  assert_eq!(decide(file(10),file(11),file(10)).unwrap().resolution,
   Resolution::TakeOurs);
  assert_eq!(decide(file(10),file(10),file(12)).unwrap().resolution,
   Resolution::TakeTheirs);
 }
 #[test]fn divergent_file_edits_flag_human_conflict(){
  let d=decide(file(10),file(11),file(12)).unwrap();
  assert_eq!(d.resolution,Resolution::Conflict);
  assert!(!all_resolved(&[d]));assert_eq!(conflicts(&[d]),1);
 }
 #[test]fn invalid_multi_file_plan_never_publishes_partial_decisions(){
  let mut out=[ZERO;2];let mismatch=FileState{path_hash:2,digest:3,revision:1};
  assert_eq!(plan(&[file(1),file(2)],&[file(1),file(2)],
    &[file(1),mismatch],&mut out),Err(Error::Invalid));
  assert_eq!(out,[ZERO;2]);
 }
}

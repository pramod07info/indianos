//! R032/19: typed command palette with scoped actions, no arbitrary shell execution.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Action{Open,Save,Find,Build,Test,Debug,Terminal,Layout}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Item{pub action:Action,pub key:&'static[u8],pub privileged:bool}
const CATALOG:[Item;8]=[
 Item{action:Action::Open,key:b"open file",privileged:false},
 Item{action:Action::Save,key:b"save file",privileged:true},
 Item{action:Action::Find,key:b"find in project",privileged:false},
 Item{action:Action::Build,key:b"build project",privileged:true},
 Item{action:Action::Test,key:b"run tests",privileged:true},
 Item{action:Action::Debug,key:b"debug session",privileged:true},
 Item{action:Action::Terminal,key:b"open terminal",privileged:false},
 Item{action:Action::Layout,key:b"change layout",privileged:false}
];
fn contains_ci(haystack:&[u8],needle:&[u8])->bool{
 if needle.is_empty(){return false;}
 haystack.windows(needle.len()).any(|chunk|
  chunk.iter().zip(needle).all(|(a,b)|a.eq_ignore_ascii_case(b)))
}
pub fn search<const N:usize>(query:&[u8],can_write:bool,
 out:&mut[Action;N])->Result<usize,Error>{
 if query.is_empty()||query.len()>32||!query.iter().all(|b|
  b.is_ascii_alphanumeric()||*b==b' '||*b==b'-'){return Err(Error::Invalid);}
 let mut stage=[Action::Open;N];let mut n=0;
 for item in CATALOG{
  if item.privileged&&!can_write{continue;}
  if contains_ci(item.key,query){
   if n==N{return Err(Error::Full);}
   stage[n]=item.action;n+=1;
  }
 }
 *out=stage;Ok(n)
}
pub fn resolve(command:Action,can_write:bool)->Result<Action,Error>{
 if !can_write&&matches!(command,Action::Save|Action::Build|Action::Test|Action::Debug){
  return Err(Error::Denied);
 }
 Ok(command)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn quick_palette_discovers_build_test_and_editor_actions(){
  let mut out=[Action::Open;4];
  assert_eq!(search(b"build",true,&mut out),Ok(1));
  assert_eq!(out[0],Action::Build);
  assert_eq!(search(b"OPEN",true,&mut out),Ok(2));
 }
 #[test]fn read_only_user_cannot_dispatch_privileged_build_or_save(){
  assert_eq!(resolve(Action::Build,false),Err(Error::Denied));
  assert_eq!(resolve(Action::Find,false),Ok(Action::Find));
  assert_eq!(search(b"build",false,&mut[Action::Open;1]),Ok(0));
 }
}
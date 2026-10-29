//! R026/01: bounded project tree navigation of real kernel RAM files.
//! Direct child and descendant queries obey slash-delimited project roots.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Node{pub path:[u8;PATH_MAX],pub len:usize,pub depth:usize,pub bytes:usize,pub revision:u64}
pub const EMPTY:Node=Node{path:[0;PATH_MAX],len:0,depth:0,bytes:0,revision:0};
fn belongs(path:&[u8],root:&[u8])->bool{
 path.starts_with(root)&&(root==b"/"||path.get(root.len())==Some(&b'/'))
}
pub fn tree<const F:usize,const O:usize>(fs:&RamFs<F>,root:&[u8],
 max_depth:usize,out:&mut[Node;O])->Result<usize,Error>{
 if root.is_empty()||root[0]!=b'/'||root.len()>PATH_MAX||
 root.windows(2).any(|w|w==b".."){return Err(Error::Invalid);}
 let mut stage=[EMPTY;O];let mut count=0;
 for i in 0..fs.count(){
  let mut name=[0;PATH_MAX];
  let (n,size,rev)=fs.entry(i,&mut name).map_err(|_|Error::Invalid)?
   .ok_or(Error::Missing)?;
  if !belongs(&name[..n],root){continue;}
  let rest=&name[if root==b"/"{1}else{root.len()+1}..n];
  let depth=1+rest.iter().filter(|&&x|x==b'/').count();
  if depth>max_depth{continue;}
  if count==O{return Err(Error::Full);}
  stage[count]=Node{path:name,len:n,depth,bytes:size,revision:rev};count+=1;
 }
 stage[..count].sort_unstable_by(|a,b|a.path[..a.len].cmp(&b.path[..b.len]));
 *out=stage;Ok(count)
}
pub fn total_bytes(nodes:&[Node])->Result<usize,Error>{
 nodes.iter().try_fold(0usize,|sum,n|sum.checked_add(n.bytes).ok_or(Error::Overflow))
}
pub fn at<'a>(items:&'a[Node],path:&[u8])->Option<&'a Node>{
 items.iter().find(|n|&n.path[..n.len]==path)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn tree_filters_real_project_and_orders_nested_files(){
  let mut fs=RamFs::<3>::new();
  fs.replace(b"/p/src/a.rs",b"hi").unwrap();
  fs.replace(b"/p/README",b"x").unwrap();
  fs.replace(b"/px/secret",b"no").unwrap();
  let mut out=[EMPTY;3];
  assert_eq!(tree(&fs,b"/p",2,&mut out),Ok(2));
  assert_eq!(total_bytes(&out[..2]),Ok(3));
  assert_eq!(out[0].depth,1);
 }
 #[test]fn depth_limit_hides_nested_source_but_not_direct_project_file(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/p/src/a",b"x").unwrap();fs.replace(b"/p/b",b"y").unwrap();
  let mut nodes=[EMPTY;2];
  assert_eq!(tree(&fs,b"/p",1,&mut nodes),Ok(1));
  assert_eq!(at(&nodes[..1],b"/p/b").map(|x|x.bytes),Some(1));
 }
 #[test]fn insufficient_tree_capacity_preserves_previous_results(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/p/a",b"a").unwrap();
  fs.replace(b"/p/b",b"b").unwrap();let mut out=[Node{depth:77,..EMPTY};1];
  assert_eq!(tree(&fs,b"/p",2,&mut out),Err(Error::Full));
  assert_eq!(out[0].depth,77);
 }
}

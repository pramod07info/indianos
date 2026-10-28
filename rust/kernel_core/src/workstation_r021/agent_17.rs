//! R021/17: preflight reproducible native launch configurations.
//! Verified program descriptor and explicit owner grant required; does NOT exec ELF.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Target{Native,Simulator,WebPreview}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Recipe{pub id:u32,pub owner:u32,pub artifact:[u8;32],
 pub target:Target,pub cpu:u16,pub ram_mb:u32,pub timeout:u64,
 pub rights:u32,pub argv_count:u8,pub approved:bool}
pub struct Configurations<const N:usize>{recipes:[Option<Recipe>;N]}
impl<const N:usize> Configurations<N>{
 pub const fn new()->Self{Self{recipes:[None;N]}}
 fn index(&self,owner:u32,id:u32)->Option<usize>{
  self.recipes.iter().position(|r|r.is_some_and(|r|r.owner==owner&&r.id==id))
 }
 pub fn add(&mut self,mut recipe:Recipe)->Result<(),Error>{
  if recipe.owner==0||recipe.id==0||recipe.artifact==[0;32]||
   recipe.cpu==0||recipe.ram_mb==0||recipe.timeout==0||
   recipe.argv_count>16{return Err(Error::Invalid);}
  if self.index(recipe.owner,recipe.id).is_some(){return Err(Error::Conflict);}
  let at=self.recipes.iter().position(Option::is_none).ok_or(Error::Full)?;
  recipe.approved=false;self.recipes[at]=Some(recipe);Ok(())
 }
 pub fn approve(&mut self,owner:u32,id:u32,
  installed_digest:[u8;32],granted_rights:u32)->Result<(),Error>{
  let i=self.index(owner,id).ok_or(Error::Missing)?;
  let mut r=self.recipes[i].ok_or(Error::Missing)?;
  if r.artifact!=installed_digest||r.rights&granted_rights!=r.rights{
   return Err(Error::Denied);
  }
  r.approved=true;self.recipes[i]=Some(r);Ok(())
 }
 pub fn preflight(&self,owner:u32,id:u32,installed_digest:[u8;32],
  max_cpu:u16,max_ram:u32,now:u64)->Result<Recipe,Error>{
  if owner==0||now==u64::MAX{return Err(Error::Denied);}
  let i=self.index(owner,id).ok_or(Error::Missing)?;
  let r=self.recipes[i].ok_or(Error::Missing)?;
  if !r.approved||r.artifact!=installed_digest{return Err(Error::Denied);}
  if r.cpu>max_cpu||r.ram_mb>max_ram{return Err(Error::Full);}
  Ok(r)
 }
 pub fn revoke(&mut self,owner:u32,id:u32)->Result<(),Error>{
  let i=self.index(owner,id).ok_or(Error::Missing)?;
  let mut r=self.recipes[i].ok_or(Error::Missing)?;
  r.approved=false;self.recipes[i]=Some(r);Ok(())
 }
 pub fn remove(&mut self,owner:u32,id:u32)->Result<(),Error>{
  let i=self.index(owner,id).ok_or(Error::Missing)?;
  self.recipes[i]=None;Ok(())
 }
 pub fn count(&self,owner:u32)->usize{
  self.recipes.iter().flatten().filter(|r|r.owner==owner).count()
 }
}
#[cfg(test)]mod tests{
 use super::*;
 fn cfg()->Recipe{Recipe{id:1,owner:7,artifact:[5;32],
  target:Target::Native,cpu:2,ram_mb:64,timeout:100,
  rights:3,argv_count:2,approved:false}}
 #[test]fn verified_native_launch_checks_digest_rights_and_resource_quotas(){
  let mut c=Configurations::<1>::new();c.add(cfg()).unwrap();
  c.approve(7,1,[5;32],3).unwrap();
  assert_eq!(c.preflight(7,1,[5;32],2,64,1),Ok(Recipe{approved:true,..cfg()}));
 }
 #[test]fn forged_installed_artifact_denied_after_approval(){
  let mut c=Configurations::<1>::new();c.add(cfg()).unwrap();
  assert_eq!(c.approve(7,1,[6;32],3),Err(Error::Denied));
  c.approve(7,1,[5;32],3).unwrap();
  assert_eq!(c.preflight(7,1,[6;32],2,64,1),Err(Error::Denied));
 }
 #[test]fn excessive_cpu_or_revoked_config_cannot_launch(){
  let mut c=Configurations::<1>::new();c.add(cfg()).unwrap();
  c.approve(7,1,[5;32],3).unwrap();
  assert_eq!(c.preflight(7,1,[5;32],1,64,1),Err(Error::Full));
  c.revoke(7,1).unwrap();
  assert_eq!(c.preflight(7,1,[5;32],2,64,1),Err(Error::Denied));
 }
}

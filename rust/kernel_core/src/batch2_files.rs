//! R036/17: bounded per-process Files view model for trusted VFS results.
//! Metadata must be supplied by a separately authenticated VFS service.
pub const MAX_NAME:usize=24;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Entry{pub name:[u8;MAX_NAME],pub name_len:u8,pub bytes:u64,pub directory:bool}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum FilesError{Invalid,Duplicate,Full,Denied}
pub struct View<const N:usize>{
    pid:u32,generation:u64,entries:[Option<Entry>;N],count:usize,
}
impl<const N:usize> View<N>{
    pub fn new(pid:u32,generation:u64)->Result<Self,FilesError>{
        if pid==0||generation==0{return Err(FilesError::Invalid);}
        Ok(Self{pid,generation,entries:[None;N],count:0})
    }
    pub fn add_from_vfs(&mut self,name:&[u8],bytes:u64,directory:bool)
        ->Result<(),FilesError>{
        if name.is_empty()||name.len()>MAX_NAME||
            name.iter().any(|b|!b.is_ascii_graphic()||matches!(*b,b'/'|b'\\')){
            return Err(FilesError::Invalid);
        }
        if self.entries[..self.count].iter().flatten().any(|e|
            e.name_len as usize==name.len()&&&e.name[..name.len()]==name){
            return Err(FilesError::Duplicate);
        }
        if self.count==N{return Err(FilesError::Full);}
        let mut buf=[0;MAX_NAME];buf[..name.len()].copy_from_slice(name);
        self.entries[self.count]=Some(Entry{name:buf,name_len:name.len() as u8,bytes,directory});
        self.count+=1;Ok(())
    }
    /// Only a trusted VFS result for this exact process incarnation may
    /// update an existing displayed entry. Denied mutations are atomic.
    pub fn update_metadata(&mut self,pid:u32,generation:u64,name:&[u8],
        bytes:u64,directory:bool)->Result<(),FilesError>{
        if pid!=self.pid||generation!=self.generation{
            return Err(FilesError::Denied);
        }
        if name.is_empty()||name.len()>MAX_NAME||
            name.iter().any(|b|!b.is_ascii_graphic()||matches!(*b,b'/'|b'\\')){
            return Err(FilesError::Invalid);
        }
        let entry=self.entries[..self.count].iter_mut().flatten().find(|e|
            e.name_len as usize==name.len() && &e.name[..name.len()]==name)
            .ok_or(FilesError::Invalid)?;
        entry.bytes=bytes;entry.directory=directory;
        Ok(())
    }
    pub fn page(&self,pid:u32,generation:u64,start:usize,
        output:&mut[Option<Entry>])->Result<usize,FilesError>{
        if self.pid!=pid||self.generation!=generation{
            return Err(FilesError::Denied);
        }
        if start>=self.count{return Ok(0);}
        let size=output.len().min(self.count-start);
        for (to,from) in output[..size].iter_mut()
            .zip(self.entries[start..start+size].iter()){
            *to=*from;
        }
        Ok(size)
    }
    pub fn clear(&mut self,pid:u32,generation:u64)->Result<(),FilesError>{
        if pid!=self.pid||generation!=self.generation{return Err(FilesError::Denied);}
        self.entries=[None;N];self.count=0;Ok(())
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn paged_listing_does_not_leak_to_other_pid(){
        let mut view=View::<3>::new(10,2).unwrap();
        view.add_from_vfs(b"notes.txt",55,false).unwrap();
        view.add_from_vfs(b"docs",0,true).unwrap();
        let mut out=[None;1];
        assert_eq!(view.page(10,1,0,&mut out),Err(FilesError::Denied));
        assert_eq!(view.page(20,2,0,&mut out),Err(FilesError::Denied));
        assert_eq!(view.page(10,2,1,&mut out),Ok(1));
        assert_eq!(out[0].unwrap().name_len,4);
        assert_eq!(view.page(10,2,2,&mut out),Ok(0));
    }
    #[test]fn invalid_and_duplicate_names_fail_closed(){
        let mut view=View::<1>::new(1,1).unwrap();
        assert_eq!(view.add_from_vfs(b"../secret",0,false),Err(FilesError::Invalid));
        view.add_from_vfs(b"readme",1,false).unwrap();
        assert_eq!(view.add_from_vfs(b"readme",1,false),Err(FilesError::Duplicate));
        assert_eq!(view.add_from_vfs(b"next",2,false),Err(FilesError::Full));
    }
    #[test]fn owner_must_clear_snapshot(){
        let mut view=View::<1>::new(1,2).unwrap();
        view.add_from_vfs(b"a",1,false).unwrap();
        assert_eq!(view.clear(1,1),Err(FilesError::Denied));
        view.clear(1,2).unwrap();
        assert_eq!(view.page(1,2,0,&mut[None;1]),Ok(0));
    }
}

#[cfg(test)]
mod ws29_files_metadata_auth_tests{
 use super::*;
 #[test]fn only_current_incarnation_updates_trusted_vfs_entry(){
  let mut view=View::<2>::new(8,4).unwrap();
  view.add_from_vfs(b"project.rs",11,false).unwrap();
  assert_eq!(view.update_metadata(8,3,b"project.rs",99,false),Err(FilesError::Denied));
  assert_eq!(view.update_metadata(9,4,b"project.rs",99,false),Err(FilesError::Denied));
  assert_eq!(view.update_metadata(8,4,b"../path",99,false),Err(FilesError::Invalid));
  let mut page=[None;1];view.page(8,4,0,&mut page).unwrap();
  assert_eq!(page[0].unwrap().bytes,11);
  assert_eq!(view.update_metadata(8,4,b"project.rs",99,false),Ok(()));
  view.page(8,4,0,&mut page).unwrap();
  assert_eq!(page[0].unwrap().bytes,99);
 }
}

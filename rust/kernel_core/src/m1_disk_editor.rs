//! R019 WS04/WS02: disk-backed developer text buffer over durable HierVfs.
//! This service edits real file bytes, refusing stale writes and preserving
//! committed data on simulated crashes. It does not yet draw an IDE window.
//! The caller Identity MUST be derived from a trusted scheduler/session.
use crate::batch5_session::Identity;
use crate::block::BlockDevice;
use crate::durablefs::{FsError, MAX_FILE_BYTES};
use crate::hier_vfs::HierVfs;
use crate::vfs_path::normalize_absolute;
use crate::fs_write_guard::WriteGuard;

pub const EDITOR_PATH_MAX: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid, Busy, Closed, Unauthorized, Dirty, Stale, Full,
    Filesystem(FsError),
}
impl From<FsError> for Error {
    fn from(error: FsError) -> Self { Self::Filesystem(error) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    pub bytes: usize,
    pub dirty: bool,
    pub filesystem_generation: u64,
}

/// Snapshot of the previous edit for bounded, owner-only undo/redo.
#[derive(Clone, Copy)]
struct Snapshot { data:[u8;MAX_FILE_BYTES], used:usize }
/// No heap and no unverified filesystem cache: one owner-bound open document.
/// The filesystem itself uses separate CRC-protected copy-on-write banks.
pub struct DiskEditor {
    owner: Option<Identity>,
    path: [u8; EDITOR_PATH_MAX],
    path_len: usize,
    data: [u8; MAX_FILE_BYTES],
    used: usize,
    baseline: u64,
    dirty: bool,
    undo: Option<Snapshot>,
    redo: Option<Snapshot>,
}
impl DiskEditor {
    pub const fn new() -> Self {
        Self { owner: None, path: [0; EDITOR_PATH_MAX], path_len: 0,
            data: [0; MAX_FILE_BYTES], used: 0, baseline: 0, dirty: false,
            undo: None, redo: None }
    }
    fn authorize(&self, caller: Identity) -> Result<(), Error> {
        if caller.pid == 0 || caller.generation == 0 || self.owner != Some(caller) {
            return Err(Error::Unauthorized);
        }
        Ok(())
    }
    fn pathname(&self) -> &str {
        // A successful open uses the existing HierVfs ASCII path validator.
        core::str::from_utf8(&self.path[..self.path_len]).unwrap()
    }
    /// Open existing real VFS bytes; failure leaves any current session intact.
    pub fn open<D: BlockDevice>(
        &mut self, caller: Identity, fs: &mut HierVfs<D>, name: &str
    ) -> Result<Status, Error> {
        if self.owner.is_some() { return Err(Error::Busy); }
        if caller.pid == 0 || caller.generation == 0 { return Err(Error::Unauthorized); }
        let mut path = [0u8; EDITOR_PATH_MAX];
        let count = normalize_absolute(name, &mut path).map_err(|_| Error::Invalid)?;
        if count < 2 || count > EDITOR_PATH_MAX { return Err(Error::Invalid); }
        let validated = core::str::from_utf8(&path[..count]).map_err(|_| Error::Invalid)?;
        let mut fresh = [0u8; MAX_FILE_BYTES];
        // Whole-file checksum, parent path and bounds are validated by HierVfs.
        let len = fs.read(validated, &mut fresh)?;
        let generation = fs.generation();
        self.path = path;
        self.path_len = count;
        self.data = fresh;
        self.used = len;
        self.baseline = generation;
        self.dirty = false;
        self.undo = None; self.redo = None;
        self.owner = Some(caller);
        Ok(self.status(caller)?)
    }
    pub fn status(&self, caller: Identity) -> Result<Status, Error> {
        self.authorize(caller)?;
        Ok(Status { bytes: self.used, dirty: self.dirty,
            filesystem_generation: self.baseline })
    }
    /// Snapshot editor bytes atomically: short output remains untouched.
    pub fn read(&self, caller: Identity, out: &mut [u8]) -> Result<usize, Error> {
        self.authorize(caller)?;
        if out.len() < self.used { return Err(Error::Full); }
        out[..self.used].copy_from_slice(&self.data[..self.used]);
        Ok(self.used)
    }
    /// Replace byte range [from,to) without partial in-memory mutation.
    pub fn replace(&mut self, caller: Identity, from: usize, to: usize,
                   replacement: &[u8]) -> Result<usize, Error> {
        self.authorize(caller)?;
        if from > to || to > self.used { return Err(Error::Invalid); }
        let new_len = self.used.checked_sub(to - from)
            .and_then(|n| n.checked_add(replacement.len()))
            .ok_or(Error::Full)?;
        if new_len > MAX_FILE_BYTES { return Err(Error::Full); }
        let mut staged = [0u8; MAX_FILE_BYTES];
        staged[..from].copy_from_slice(&self.data[..from]);
        staged[from..from + replacement.len()].copy_from_slice(replacement);
        staged[from + replacement.len()..new_len]
            .copy_from_slice(&self.data[to..self.used]);
        if new_len==self.used && staged[..new_len]==self.data[..self.used]{
            return Ok(self.used);
        }
        self.undo=Some(Snapshot{data:self.data,used:self.used});
        self.redo=None;
        self.data = staged;
        self.used = new_len;
        self.dirty = true;
        Ok(new_len)
    }
    pub fn append(&mut self, caller: Identity, data: &[u8]) -> Result<usize, Error> {
        self.replace(caller, self.used, self.used, data)
    }
    /// Unicode-safe local source editing. The byte-oriented replace()
    /// remains available to kernel-owned binary tools; source routes use
    /// this explicit UTF-8/boundary-checked variant instead.
    pub fn replace_text(&mut self,caller:Identity,from:usize,to:usize,
        insert:&str)->Result<usize,Error>{
        self.authorize(caller)?;
        let text=core::str::from_utf8(&self.data[..self.used])
            .map_err(|_|Error::Invalid)?;
        if from>to||to>self.used||
           !text.is_char_boundary(from)||!text.is_char_boundary(to){
            return Err(Error::Invalid);
        }
        self.replace(caller,from,to,insert.as_bytes())
    }
    /// Literal Unicode replacement of every non-overlapping source
    /// occurrence in ONE unsaved buffer, with ONE reversible edit.
    /// Overflow or failed authority checks preserve undo/redo and bytes.
    pub fn replace_all_text(&mut self,caller:Identity,needle:&str,
        replacement:&str)->Result<usize,Error>{
        self.authorize(caller)?;
        if needle.is_empty(){return Err(Error::Invalid);}
        let text=core::str::from_utf8(&self.data[..self.used])
            .map_err(|_|Error::Invalid)?;
        let mut staged=[0u8;MAX_FILE_BYTES];
        let mut source=0usize;let mut target=0usize;let mut count=0usize;
        while source<self.used{
            if text[source..].starts_with(needle){
                let end=target.checked_add(replacement.len()).ok_or(Error::Full)?;
                if end>MAX_FILE_BYTES{return Err(Error::Full);}
                staged[target..end].copy_from_slice(replacement.as_bytes());
                target=end;source+=needle.len();count+=1;
            }else{
                let width=text[source..].chars().next().ok_or(Error::Invalid)?.len_utf8();
                if target+width>MAX_FILE_BYTES{return Err(Error::Full);}
                staged[target..target+width]
                    .copy_from_slice(&self.data[source..source+width]);
                target+=width;source+=width;
            }
        }
        if count==0{return Ok(0);}
        if target==self.used && staged[..target]==self.data[..self.used]{
            return Ok(count);
        }
        self.undo=Some(Snapshot{data:self.data,used:self.used});
        self.redo=None;
        self.data=staged;self.used=target;self.dirty=true;
        Ok(count)
    }
    /// Find a Unicode literal in the current unsaved buffer from a
    /// character boundary, returning a byte offset for source navigation.
    pub fn find_text(&self,caller:Identity,needle:&str,
        from:usize)->Result<Option<usize>,Error>{
        self.authorize(caller)?;
        if needle.is_empty(){return Err(Error::Invalid);}
        let text=core::str::from_utf8(&self.data[..self.used])
            .map_err(|_|Error::Invalid)?;
        if from>self.used||!text.is_char_boundary(from){return Err(Error::Invalid);}
        Ok(text[from..].find(needle).map(|offset|from+offset))
    }
    /// Translate 1-based source line and Unicode scalar column to the
    /// byte offset used by replace_text; valid at end of any line.
    pub fn text_offset(&self,caller:Identity,
        line:usize,column:usize)->Result<usize,Error>{
        self.authorize(caller)?;
        if line==0||column==0{return Err(Error::Invalid);}
        let text=core::str::from_utf8(&self.data[..self.used])
            .map_err(|_|Error::Invalid)?;
        let mut base=0;
        for (index,row) in text.split('\n').enumerate(){
            if index+1==line{
                let local=row.char_indices().nth(column-1).map(|x|x.0)
                   .or_else(||if column==row.chars().count()+1{
                       Some(row.len())
                   }else{None}).ok_or(Error::Invalid)?;
                return Ok(base+local);
            }
            base+=row.len()+1;
        }
        Err(Error::Invalid)
    }
    /// Undo exactly one successful local edit. The result is dirty even
    /// if the previous edit had been saved to durable storage.
    pub fn undo(&mut self,caller:Identity)->Result<usize,Error>{
        self.authorize(caller)?;
        let old=self.undo.take().ok_or(Error::Invalid)?;
        self.redo=Some(Snapshot{data:self.data,used:self.used});
        self.data=old.data;self.used=old.used;self.dirty=true;
        Ok(self.used)
    }
    /// Redo the single previously undone local change; a new edit clears it.
    pub fn redo(&mut self,caller:Identity)->Result<usize,Error>{
        self.authorize(caller)?;
        let next=self.redo.take().ok_or(Error::Invalid)?;
        self.undo=Some(Snapshot{data:self.data,used:self.used});
        self.data=next.data;self.used=next.used;self.dirty=true;
        Ok(self.used)
    }
    /// A concurrent volume generation invalidates the session. This is
    /// conservative (unrelated file updates also require reload) but it
    /// prevents lost source edits until per-file versions are supported.
    pub fn save<D: BlockDevice>(
        &mut self, caller: Identity, fs: &mut HierVfs<D>
    ) -> Result<u64, Error> {
        self.authorize(caller)?;
        if fs.generation() != self.baseline { return Err(Error::Stale); }
        if self.dirty {
            // This direct Editor path bypasses FileHandles' S0 write guard.
            // Deny OS/recovery/key namespaces before any durable COW write.
            WriteGuard::allow_path(self.pathname()).map_err(|_|Error::Unauthorized)?;
            fs.overwrite(self.pathname(), &self.data[..self.used])?;
            self.baseline = fs.generation();
            self.dirty = false;
        }
        Ok(self.baseline)
    }
    /// Save current edits as a separate durable file without replacing the
    /// original or bypassing reserved OS paths. A failed create does not
    /// change the editor's original path, data, dirty flag or history.
    pub fn save_as<D:BlockDevice>(&mut self,caller:Identity,
        fs:&mut HierVfs<D>,destination:&str)->Result<u64,Error>{
        self.authorize(caller)?;
        if fs.generation()!=self.baseline {return Err(Error::Stale);}
        let mut normalized=[0u8;EDITOR_PATH_MAX];
        let n=normalize_absolute(destination,&mut normalized)
            .map_err(|_|Error::Invalid)?;
        if n<2 || n>EDITOR_PATH_MAX {return Err(Error::Invalid);}
        let destination=core::str::from_utf8(&normalized[..n])
            .map_err(|_|Error::Invalid)?;
        WriteGuard::allow_path(destination).map_err(|_|Error::Unauthorized)?;
        if destination==self.pathname() {return Err(Error::Invalid);}
        fs.create(destination,&self.data[..self.used])?;
        self.path=normalized;self.path_len=n;
        self.baseline=fs.generation();self.dirty=false;
        self.undo=None;self.redo=None;
        Ok(self.baseline)
    }
    /// Explicit conflict resolution: discard local changes and reread
    /// committed bytes; failed read never modifies the existing buffer.
    pub fn reload<D: BlockDevice>(
        &mut self, caller: Identity, fs: &mut HierVfs<D>
    ) -> Result<Status, Error> {
        self.authorize(caller)?;
        let mut fresh = [0u8; MAX_FILE_BYTES];
        let size = fs.read(self.pathname(), &mut fresh)?;
        let generation = fs.generation();
        self.data = fresh;
        self.used = size;
        self.baseline = generation;
        self.dirty = false;
        self.undo = None;self.redo = None;
        self.status(caller)
    }
    /// Closing dirty buffers requires explicit discard consent.
    pub fn close(&mut self, caller: Identity, discard: bool) -> Result<(), Error> {
        self.authorize(caller)?;
        if self.dirty && !discard { return Err(Error::Dirty); }
        *self = Self::new();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{BlockError, SECTOR_BYTES};
    const SECTORS: usize = 100;
    struct Disk {
        sectors: [[u8; SECTOR_BYTES]; SECTORS],
        writes_remaining: Option<usize>,
    }
    impl Disk {
        fn new() -> Self {
            Self { sectors: [[0u8; SECTOR_BYTES]; SECTORS],
                writes_remaining: None }
        }
    }
    impl BlockDevice for Disk {
        fn sector_count(&self) -> u64 { SECTORS as u64 }
        fn read_sector(&mut self, lba: u64, out: &mut [u8; SECTOR_BYTES])
            -> Result<(), BlockError> {
            *out = *self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?;
            Ok(())
        }
        fn write_sector(&mut self, lba: u64, bytes: &[u8; SECTOR_BYTES])
            -> Result<(), BlockError> {
            if let Some(remaining) = &mut self.writes_remaining {
                if *remaining == 0 { return Err(BlockError::Io); }
                *remaining -= 1;
            }
            *self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)? = *bytes;
            Ok(())
        }
    }
    fn owner() -> Identity { Identity { pid: 2, generation: 3 } }
    fn other() -> Identity { Identity { pid: 2, generation: 4 } }
    fn volume() -> HierVfs<Disk> {
        let mut fs = HierVfs::format_blank(Disk::new()).unwrap();
        fs.create("/source.rs", b"fn main() {}").unwrap();
        fs
    }

    #[test]
    fn actual_source_edits_survive_volume_reboot_and_remount() {
        let mut fs = volume();
        let mut editor = DiskEditor::new();
        assert_eq!(editor.open(owner(), &mut fs, "/source.rs").unwrap().bytes, 12);
        assert_eq!(editor.append(owner(), b"\n// saved"), Ok(21));
        assert!(editor.status(owner()).unwrap().dirty);
        editor.save(owner(), &mut fs).unwrap();
        editor.close(owner(), false).unwrap();
        // No in-memory editor state survives. Data comes from the sector backend.
        let disk = fs.into_device();
        let mut remounted = HierVfs::mount(disk).unwrap();
        let mut reopened = DiskEditor::new();
        reopened.open(owner(), &mut remounted, "/source.rs").unwrap();
        let mut out = [0u8; 40];
        assert_eq!(reopened.read(owner(), &mut out).unwrap(), 21);
        assert_eq!(&out[..21], b"fn main() {}\n// saved");
    }
    #[test]
    fn stale_editor_can_never_overwrite_external_writer() {
        let mut fs = volume();
        let mut editor = DiskEditor::new();
        editor.open(owner(), &mut fs, "/source.rs").unwrap();
        editor.replace(owner(), 0, 2, b"FN").unwrap();
        fs.overwrite("/source.rs", b"external").unwrap();
        assert_eq!(editor.save(owner(), &mut fs), Err(Error::Stale));
        let mut on_disk = [0u8; 32];
        assert_eq!(fs.read("/source.rs", &mut on_disk), Ok(8));
        assert_eq!(&on_disk[..8], b"external");
        assert_eq!(editor.reload(owner(), &mut fs).unwrap().bytes, 8);
        assert!(!editor.status(owner()).unwrap().dirty);
    }
    #[test]
    fn process_generation_and_dirty_close_are_enforced() {
        let mut fs = volume();
        let mut editor = DiskEditor::new();
        editor.open(owner(), &mut fs, "/source.rs").unwrap();
        assert_eq!(editor.append(other(), b"bad"), Err(Error::Unauthorized));
        assert_eq!(editor.save(other(), &mut fs), Err(Error::Unauthorized));
        assert_eq!(editor.close(other(), true), Err(Error::Unauthorized));
        editor.append(owner(), b"!").unwrap();
        assert_eq!(editor.close(owner(), false), Err(Error::Dirty));
        editor.close(owner(), true).unwrap();
        assert_eq!(editor.status(owner()), Err(Error::Unauthorized));
        let mut data = [0u8; 32];
        assert_eq!(fs.read("/source.rs", &mut data), Ok(12));
    }
    #[test]
    fn too_long_or_invalid_edit_never_mutates_buffer_or_caller_output() {
        let mut fs = volume();
        let mut editor = DiskEditor::new();
        editor.open(owner(), &mut fs, "/source.rs").unwrap();
        let mut out = [0xAAu8; 4];
        assert_eq!(editor.read(owner(), &mut out), Err(Error::Full));
        assert_eq!(out, [0xAA; 4]);
        assert_eq!(editor.replace(owner(), 5, 3, b"new"), Err(Error::Invalid));
        assert_eq!(editor.append(owner(), &[b'x'; MAX_FILE_BYTES]), Err(Error::Full));
        assert_eq!(editor.status(owner()).unwrap().bytes, 12);
        assert!(!editor.status(owner()).unwrap().dirty);
    }
    #[test]
    fn corrupt_or_missing_volume_paths_do_not_open_session() {
        let mut fs = volume();
        let mut editor = DiskEditor::new();
        assert_eq!(editor.open(owner(), &mut fs, "/../escape"), Err(Error::Invalid));
        assert!(matches!(editor.open(owner(), &mut fs, "/missing"),
                         Err(Error::Filesystem(FsError::NotFound))));
        assert_eq!(editor.open(owner(), &mut fs, "/source.rs").unwrap().bytes, 12);
        assert_eq!(editor.open(owner(), &mut fs, "/source.rs"), Err(Error::Busy));
    }
    #[test]
    fn injected_disk_write_failure_preserves_last_committed_source() {
        let mut fs = volume();
        let mut editor = DiskEditor::new();
        editor.open(owner(), &mut fs, "/source.rs").unwrap();
        editor.append(owner(), b" failure").unwrap();
        let mut disk = fs.into_device();
        disk.writes_remaining = Some(1);
        let mut fs = HierVfs::mount(disk).unwrap();
        assert!(matches!(editor.save(owner(), &mut fs),
                         Err(Error::Filesystem(FsError::Io))));
        assert!(editor.status(owner()).unwrap().dirty);
        let mut disk = fs.into_device();
        disk.writes_remaining = None;
        let mut fs = HierVfs::mount(disk).unwrap();
        let mut on_disk = [0u8; 32];
        assert_eq!(fs.read("/source.rs", &mut on_disk), Ok(12));
        assert_eq!(&on_disk[..12], b"fn main() {}");
    }
    #[test]
    fn failed_reload_keeps_current_unsaved_bytes_intact() {
        let mut fs = volume();
        let mut editor = DiskEditor::new();
        editor.open(owner(), &mut fs, "/source.rs").unwrap();
        editor.append(owner(), b"!").unwrap();
        fs.unlink("/source.rs").unwrap();
        assert!(matches!(editor.reload(owner(), &mut fs),
                         Err(Error::Filesystem(FsError::NotFound))));
        assert_eq!(editor.status(owner()).unwrap().bytes, 13);
        assert!(editor.status(owner()).unwrap().dirty);
        let mut data = [0u8; 13];
        assert_eq!(editor.read(owner(), &mut data), Ok(13));
        assert_eq!(&data[..], b"fn main() {}!");
    }
    #[test]
    fn r039_undo_redo_owner_bound_and_durable_after_save(){
        let mut fs=volume();let mut e=DiskEditor::new();
        e.open(owner(),&mut fs,"/source.rs").unwrap();
        assert_eq!(e.undo(owner()),Err(Error::Invalid));
        e.append(owner(),b"!").unwrap();
        assert_eq!(e.undo(other()),Err(Error::Unauthorized));
        assert_eq!(e.undo(owner()),Ok(12));
        assert_eq!(e.redo(owner()),Ok(13));
        e.save(owner(),&mut fs).unwrap();
        assert_eq!(e.undo(owner()),Ok(12));
        assert!(e.status(owner()).unwrap().dirty);
        e.save(owner(),&mut fs).unwrap();
        e.close(owner(),false).unwrap();
        let mut fs=HierVfs::mount(fs.into_device()).unwrap();
        let mut b=[0;32];assert_eq!(fs.read("/source.rs",&mut b),Ok(12));
        assert_eq!(&b[..12],b"fn main() {}");
    }
    #[test]
    fn r039_invalid_edit_keeps_undo_and_new_edit_discards_redo(){
        let mut fs=volume();let mut e=DiskEditor::new();
        e.open(owner(),&mut fs,"/source.rs").unwrap();
        e.append(owner(),b"!").unwrap();
        assert_eq!(e.replace(owner(),5,3,b"bad"),Err(Error::Invalid));
        assert_eq!(e.undo(owner()),Ok(12));
        assert_eq!(e.redo(owner()),Ok(13));
        e.undo(owner()).unwrap();
        e.append(owner(),b"?").unwrap();
        assert_eq!(e.redo(owner()),Err(Error::Invalid));
        e.reload(owner(),&mut fs).unwrap();
        assert_eq!(e.undo(owner()),Err(Error::Invalid));
    }

    #[test]
    fn r040_reserved_os_namespaces_cannot_be_modified_by_editor(){
        let mut fs=volume();
        fs.mkdir("/system").unwrap();
        fs.create("/system/secret",b"trusted").unwrap();
        let mut editor=DiskEditor::new();
        editor.open(owner(),&mut fs,"/system/secret").unwrap();
        editor.append(owner(),b" tampered").unwrap();
        let generation=fs.generation();
        assert_eq!(editor.save(owner(),&mut fs),Err(Error::Unauthorized));
        assert_eq!(editor.save_as(owner(),&mut fs,"/boot/init"),Err(Error::Unauthorized));
        assert_eq!(fs.generation(),generation);
        let mut bytes=[0u8;32];
        assert_eq!(fs.read("/system/secret",&mut bytes),Ok(7));
        assert_eq!(&bytes[..7],b"trusted");
        assert!(editor.status(owner()).unwrap().dirty);
    }
    #[test]
    fn r040_save_as_preserves_source_and_commits_new_document_after_remount(){
        let mut fs=volume();let mut editor=DiskEditor::new();
        editor.open(owner(),&mut fs,"/source.rs").unwrap();
        editor.append(owner(),b" new").unwrap();
        assert_eq!(editor.save_as(other(),&mut fs,"/copy.rs"),Err(Error::Unauthorized));
        assert_eq!(editor.save_as(owner(),&mut fs,"/source.rs"),Err(Error::Invalid));
        editor.save_as(owner(),&mut fs,"/copy.rs").unwrap();
        assert!(!editor.status(owner()).unwrap().dirty);
        assert_eq!(editor.undo(owner()),Err(Error::Invalid));
        editor.close(owner(),false).unwrap();
        let mut fs=HierVfs::mount(fs.into_device()).unwrap();
        let mut a=[0;32];let mut b=[0;32];
        assert_eq!(fs.read("/source.rs",&mut a),Ok(12));
        assert_eq!(&a[..12],b"fn main() {}");
        assert_eq!(fs.read("/copy.rs",&mut b),Ok(16));
        assert_eq!(&b[..16],b"fn main() {} new");
    }
    #[test]
    fn r040_failed_save_as_is_atomic_for_existing_and_stale_destinations(){
        let mut fs=volume();
        fs.create("/exists.rs",b"safe").unwrap();
        let mut editor=DiskEditor::new();
        editor.open(owner(),&mut fs,"/source.rs").unwrap();
        editor.append(owner(),b"!").unwrap();
        let generation=fs.generation();
        assert_eq!(editor.save_as(owner(),&mut fs,"/exists.rs"),
            Err(Error::Filesystem(FsError::Exists)));
        assert_eq!(fs.generation(),generation);
        let mut bytes=[0;32];
        assert_eq!(editor.read(owner(),&mut bytes),Ok(13));
        assert_eq!(&bytes[..13],b"fn main() {}!");
        fs.overwrite("/exists.rs",b"external").unwrap();
        assert_eq!(editor.save_as(owner(),&mut fs,"/new.rs"),Err(Error::Stale));
    }

    #[test]
    fn r043_editor_unicode_navigation_replacement_and_remount(){
        let mut fs=volume();
        let mut editor=DiskEditor::new();
        editor.open(owner(),&mut fs,"/source.rs").unwrap();
        editor.append(owner(),"\nnaïve 🍀\n".as_bytes()).unwrap();
        let position=editor.find_text(owner(),"ï",0).unwrap().unwrap();
        assert!(editor.replace_text(owner(),position+1,position+2,"x").is_err());
        assert_eq!(editor.replace_text(owner(),position,position+"ï".len(),"i"),Ok(24));
        assert_eq!(editor.find_text(owner(),"naive",0),Ok(Some(13)));
        assert_eq!(editor.text_offset(owner(),2,6),Ok(18));
        editor.save(owner(),&mut fs).unwrap();
        editor.close(owner(),false).unwrap();
        let mut fs=HierVfs::mount(fs.into_device()).unwrap();
        let mut next=DiskEditor::new();
        next.open(owner(),&mut fs,"/source.rs").unwrap();
        assert_eq!(next.find_text(owner(),"naive",0),Ok(Some(13)));
    }
    #[test]
    fn r043_editor_replace_all_is_one_undo_and_failure_is_atomic(){
        let mut fs=volume();
        let mut editor=DiskEditor::new();
        editor.open(owner(),&mut fs,"/source.rs").unwrap();
        editor.replace_text(owner(),0,12,"α α α").unwrap();
        let base=fs.generation();
        assert_eq!(editor.replace_all_text(other(),"α","β"),Err(Error::Unauthorized));
        assert_eq!(editor.replace_all_text(owner(),"α",&"x".repeat(MAX_FILE_BYTES)),
            Err(Error::Full));
        assert_eq!(fs.generation(),base);
        assert_eq!(editor.replace_all_text(owner(),"α","β"),Ok(3));
        let mut out=[0u8;32];
        assert_eq!(editor.read(owner(),&mut out),Ok("β β β".len()));
        assert_eq!(&out[..8],"β β β".as_bytes());
        assert_eq!(editor.undo(owner()),Ok("α α α".len()));
        assert_eq!(editor.redo(owner()),Ok("β β β".len()));
        editor.save(owner(),&mut fs).unwrap();
        let mut fs=HierVfs::mount(fs.into_device()).unwrap();
        let mut bytes=[0u8;24];
        assert_eq!(fs.read("/source.rs",&mut bytes),Ok(8));
        assert_eq!(&bytes[..8],"β β β".as_bytes());
    }
    #[test]
    fn r043_editor_invalid_utf8_text_offsets_never_mutate(){
        let mut fs=volume();let mut editor=DiskEditor::new();
        editor.open(owner(),&mut fs,"/source.rs").unwrap();
        let before=fs.generation();
        assert_eq!(editor.replace_text(owner(),0,1,"🍀"),Ok(15));
        assert_eq!(editor.replace_text(owner(),1,2,"bad"),Err(Error::Invalid));
        assert_eq!(editor.text_offset(owner(),1,0),Err(Error::Invalid));
        assert_eq!(editor.find_text(owner(),"🍀",1),Err(Error::Invalid));
        assert_eq!(fs.generation(),before);
        let mut data=[0;32];
        assert_eq!(editor.read(owner(),&mut data),Ok(15));
        assert_eq!(&data[..4],"🍀".as_bytes());
    }

}


/// R060: one owner-authorized, UTF-8-safe multi-cursor source edit.
/// Edits use offsets in the ORIGINAL buffer, sorted and non-overlapping.
/// Overflow, stale owner and invalid character boundaries publish NOTHING;
/// a successful group is a single undo/redo step and one durable save.
impl DiskEditor{
 pub fn replace_ranges_text(&mut self,caller:Identity,
    edits:&[(usize,usize,&str)])->Result<usize,Error>{
  self.authorize(caller)?;
  if edits.is_empty()||edits.len()>16{return Err(Error::Invalid);}
  let text=core::str::from_utf8(&self.data[..self.used])
   .map_err(|_|Error::Invalid)?;
  let mut staged=[0u8;MAX_FILE_BYTES];
  let mut cursor=0usize;
  let mut written=0usize;
  for &(start,end,replacement) in edits{
   if start<cursor||start>end||end>self.used||
      !text.is_char_boundary(start)||!text.is_char_boundary(end){
    return Err(Error::Invalid);
   }
   let unchanged=&self.data[cursor..start];
   let next=written.checked_add(unchanged.len())
     .and_then(|n|n.checked_add(replacement.len()))
     .ok_or(Error::Full)?;
   if next>MAX_FILE_BYTES{return Err(Error::Full);}
   staged[written..written+unchanged.len()].copy_from_slice(unchanged);
   staged[written+unchanged.len()..next]
       .copy_from_slice(replacement.as_bytes());
   cursor=end;
   written=next;
  }
  let total=written.checked_add(self.used-cursor).ok_or(Error::Full)?;
  if total>MAX_FILE_BYTES{return Err(Error::Full);}
  staged[written..total].copy_from_slice(&self.data[cursor..self.used]);
  if total==self.used&&staged[..total]==self.data[..self.used]{
   return Ok(self.used);
  }
  self.undo=Some(Snapshot{data:self.data,used:self.used});
  self.redo=None;
  self.data=staged;self.used=total;self.dirty=true;
  Ok(total)
 }
}
#[cfg(test)]
mod r060_multi_cursor_tests{
 use super::*;
 use crate::block::{BlockError,SECTOR_BYTES};
 struct TestDisk{data:[[u8;SECTOR_BYTES];100]}
 impl BlockDevice for TestDisk{
  fn sector_count(&self)->u64{100}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *out=*self.data.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,lba:u64,bytes:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *self.data.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*bytes;Ok(())
  }
 }
 fn ready()->HierVfs<TestDisk>{
  let mut fs=HierVfs::format_blank(TestDisk{data:[[0;SECTOR_BYTES];100]}).unwrap();
  fs.create("/source.rs",b"fn main() {}").unwrap();fs
 }
 fn owner()->Identity{Identity{pid:8,generation:3}}
 #[test]fn multi_edit_single_undo_and_real_remount(){
  let mut fs=ready();let mut e=DiskEditor::new();
  e.open(owner(),&mut fs,"/source.rs").unwrap();
  assert_eq!(e.replace_ranges_text(owner(),
    &[(0,2,"pub fn"),(3,7,"start")]),Ok(17));
  let mut out=[0u8;40];let n=e.read(owner(),&mut out).unwrap();
  assert_eq!(&out[..n],b"pub fn start() {}");
  assert_eq!(e.undo(owner()),Ok(12));
  assert_eq!(e.redo(owner()),Ok(17));
  e.save(owner(),&mut fs).unwrap();
  let mut mounted=HierVfs::mount(fs.into_device()).unwrap();
  let mut back=[0u8;40];
  assert_eq!(mounted.read("/source.rs",&mut back),Ok(17));
  assert_eq!(&back[..17],b"pub fn start() {}");
 }
 #[test]fn overlap_bad_utf8_boundary_and_overflow_never_apply_first_edit(){
  let mut fs=ready();let mut e=DiskEditor::new();
  e.open(owner(),&mut fs,"/source.rs").unwrap();
  e.replace_text(owner(),0,2,"β").unwrap();
  let mut before=[0u8;40];let n=e.read(owner(),&mut before).unwrap();
  assert_eq!(e.replace_ranges_text(owner(),
     &[(0,2,"ok"),(1,3,"bad")]),Err(Error::Invalid));
  assert_eq!(e.replace_ranges_text(owner(),
     &[(1,2,"x")]),Err(Error::Invalid));
  let huge=[b'x';MAX_FILE_BYTES];
  let huge=core::str::from_utf8(&huge).unwrap();
  assert_eq!(e.replace_ranges_text(owner(),
     &[(0,0,huge)]),Err(Error::Full));
  let mut after=[0u8;40];
  assert_eq!(e.read(owner(),&mut after),Ok(n));
  assert_eq!(&before[..n],&after[..n]);
  assert_eq!(e.replace_ranges_text(
    Identity{generation:4,..owner()},&[(0,2,"bad")]),
    Err(Error::Unauthorized));
 }
}


/// R061: duplicate a complete UTF-8 source row immediately below itself.
/// All offsets refer to one authenticated unsaved buffer. An invalid row,
/// binary buffer, or full editor is left untouched, including undo/redo.
/// The underlying save operation still uses a durable single COW overwrite.
impl DiskEditor{
 pub fn duplicate_source_line(&mut self,caller:Identity,
    line:usize)->Result<usize,Error>{
  self.authorize(caller)?;
  if line==0{return Err(Error::Invalid);}
  let text=core::str::from_utf8(&self.data[..self.used])
      .map_err(|_|Error::Invalid)?;
  let mut start=0usize;
  let mut chosen=None;
  for (index,row) in text.split_terminator('\n').enumerate(){
   if index+1==line{
    let end=start+row.len();
    let terminated=end<self.used&&self.data[end]==b'\n';
    chosen=Some((start,end,terminated));
    break;
   }
   start=start.checked_add(row.len()+1).ok_or(Error::Full)?;
  }
  let (begin,end,terminated)=chosen.ok_or(Error::Invalid)?;
  let copy_len=end-begin+1;
  let next=self.used.checked_add(copy_len).ok_or(Error::Full)?;
  if next>MAX_FILE_BYTES{return Err(Error::Full);}
  let mut scratch=[0u8;MAX_FILE_BYTES];
  let offset=if terminated{
   scratch[..end-begin].copy_from_slice(&self.data[begin..end]);
   scratch[end-begin]=b'\n';
   end+1
  }else{
   scratch[0]=b'\n';
   scratch[1..copy_len].copy_from_slice(&self.data[begin..end]);
   end
  };
  self.replace(caller,offset,offset,&scratch[..copy_len])
 }
}
#[cfg(test)]
mod r061_line_duplicate_tests{
 use super::*;
 use crate::block::{BlockError,SECTOR_BYTES};
 struct Disk{data:[[u8;SECTOR_BYTES];100]}
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{100}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *out=*self.data.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,lba:u64,bytes:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *self.data.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*bytes;Ok(())
  }
 }
 fn owner()->Identity{Identity{pid:7,generation:9}}
 fn volume()->HierVfs<Disk>{
  let mut fs=HierVfs::format_blank(Disk{data:[[0;SECTOR_BYTES];100]}).unwrap();
  fs.create("/code", "α\nβ".as_bytes()).unwrap();fs
 }
 #[test]fn unicode_row_duplication_is_single_undo_and_survives_remount(){
  let mut fs=volume();let mut editor=DiskEditor::new();
  editor.open(owner(),&mut fs,"/code").unwrap();
  assert_eq!(editor.duplicate_source_line(owner(),1),Ok(8));
  let mut out=[0u8;20];
  assert_eq!(editor.read(owner(),&mut out),Ok(8));
  assert_eq!(&out[..8],"α\nα\nβ".as_bytes());
  assert_eq!(editor.undo(owner()),Ok(5));
  assert_eq!(editor.redo(owner()),Ok(8));
  editor.save(owner(),&mut fs).unwrap();
  let mut mounted=HierVfs::mount(fs.into_device()).unwrap();
  let mut saved=[0u8;20];assert_eq!(mounted.read("/code",&mut saved),Ok(8));
  assert_eq!(&saved[..8],"α\nα\nβ".as_bytes());
 }
 #[test]fn last_line_and_terminal_newline_have_consistent_boundaries(){
  let mut fs=volume();
  fs.create("/newline",b"a\n").unwrap();
  let mut editor=DiskEditor::new();
  editor.open(owner(),&mut fs,"/newline").unwrap();
  assert_eq!(editor.duplicate_source_line(owner(),1),Ok(4));
  let mut out=[0u8;8];editor.read(owner(),&mut out).unwrap();
  assert_eq!(&out[..4],b"a\na\n");
  assert_eq!(editor.duplicate_source_line(owner(),3),Err(Error::Invalid));
  editor.close(owner(),true).unwrap();
  editor.open(owner(),&mut fs,"/code").unwrap();
  assert_eq!(editor.duplicate_source_line(owner(),2),Ok(8));
  editor.read(owner(),&mut out).unwrap();
  assert_eq!(&out[..8],"α\nβ\nβ".as_bytes());
 }
 #[test]fn invalid_line_binary_file_and_foreign_owner_never_mutate(){
  let mut fs=volume();fs.create("/binary",&[0xff,0xfe]).unwrap();
  let mut editor=DiskEditor::new();
  editor.open(owner(),&mut fs,"/code").unwrap();
  let mut out=[0x6fu8;20];
  assert_eq!(editor.duplicate_source_line(owner(),0),Err(Error::Invalid));
  assert_eq!(editor.duplicate_source_line(owner(),99),Err(Error::Invalid));
  assert_eq!(editor.duplicate_source_line(
     Identity{generation:10,..owner()},1),Err(Error::Unauthorized));
  assert_eq!(editor.read(owner(),&mut out),Ok(5));
  assert_eq!(&out[..5],"α\nβ".as_bytes());
  assert!(!editor.status(owner()).unwrap().dirty);
  editor.close(owner(),false).unwrap();
  editor.open(owner(),&mut fs,"/binary").unwrap();
  assert_eq!(editor.duplicate_source_line(owner(),1),Err(Error::Invalid));
  assert!(!editor.status(owner()).unwrap().dirty);
 }
}


/// R062: safely delete one complete UTF-8 source line. A terminated line
/// removes its following newline, while the final unterminated line removes
/// its preceding separator. This prevents accidental empty phantom rows.
/// Undo/redo and disk save remain single operations; invalid requests
/// never change text, history or the COW filesystem generation.
impl DiskEditor{
 pub fn delete_source_line(&mut self,caller:Identity,
     line:usize)->Result<usize,Error>{
  self.authorize(caller)?;
  if line==0{return Err(Error::Invalid);}
  let text=core::str::from_utf8(&self.data[..self.used])
    .map_err(|_|Error::Invalid)?;
  let mut start=0usize;
  for (index,row) in text.split_terminator('\n').enumerate(){
   let end=start.checked_add(row.len()).ok_or(Error::Full)?;
   if index+1==line{
    let (from,to)=if end<self.used&&self.data[end]==b'\n'{
     (start,end+1)
    }else if start>0{
     (start-1,end)
    }else{
     (start,end)
    };
    return self.replace(caller,from,to,b"");
   }
   start=end.checked_add(1).ok_or(Error::Full)?;
  }
  Err(Error::Invalid)
 }
}
#[cfg(test)]
mod r062_source_line_deletion_tests{
 use super::*;
 use crate::block::{BlockError,SECTOR_BYTES};
 struct TestDisk{data:[[u8;SECTOR_BYTES];100]}
 impl BlockDevice for TestDisk{
  fn sector_count(&self)->u64{100}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *out=*self.data.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,lba:u64,bytes:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *self.data.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*bytes;Ok(())
  }
 }
 fn actor()->Identity{Identity{pid:8,generation:11}}
 fn fresh()->HierVfs<TestDisk>{
  let mut fs=HierVfs::format_blank(TestDisk{data:[[0;SECTOR_BYTES];100]}).unwrap();
  fs.create("/source","α\nβ\ngamma".as_bytes()).unwrap();fs
 }
 #[test]fn removes_unicode_middle_row_and_undo_redo_survive_disk_remount(){
  let mut fs=fresh();let mut e=DiskEditor::new();
  e.open(actor(),&mut fs,"/source").unwrap();
  assert_eq!(e.delete_source_line(actor(),2),Ok(8));
  let mut data=[0u8;32];
  assert_eq!(e.read(actor(),&mut data),Ok(8));
  assert_eq!(&data[..8],"α\ngamma".as_bytes());
  assert_eq!(e.undo(actor()),Ok(11));
  assert_eq!(e.redo(actor()),Ok(8));
  e.save(actor(),&mut fs).unwrap();
  let mut mounted=HierVfs::mount(fs.into_device()).unwrap();
  assert_eq!(mounted.read("/source",&mut data),Ok(8));
  assert_eq!(&data[..8],"α\ngamma".as_bytes());
 }
 #[test]fn delete_first_last_and_only_row_without_separator_corruption(){
  let mut fs=fresh();fs.create("/one",b"single").unwrap();
  let mut e=DiskEditor::new();
  e.open(actor(),&mut fs,"/source").unwrap();
  assert_eq!(e.delete_source_line(actor(),1),Ok(8));
  let mut out=[0u8;32];e.read(actor(),&mut out).unwrap();
  assert_eq!(&out[..8],b"\xce\xb2\ngamma");
  assert_eq!(e.delete_source_line(actor(),2),Ok(2));
  e.read(actor(),&mut out).unwrap();
  assert_eq!(&out[..2],"β".as_bytes());
  e.close(actor(),true).unwrap();
  e.open(actor(),&mut fs,"/one").unwrap();
  assert_eq!(e.delete_source_line(actor(),1),Ok(0));
  assert_eq!(e.read(actor(),&mut out),Ok(0));
 }
 #[test]fn out_of_range_wrong_identity_and_bad_utf8_preserve_all_bytes(){
  let mut fs=fresh();fs.create("/binary",&[0xff,0x80]).unwrap();
  let mut e=DiskEditor::new();
  e.open(actor(),&mut fs,"/source").unwrap();
  for row in [0,4,99]{
   assert_eq!(e.delete_source_line(actor(),row),Err(Error::Invalid));
  }
  assert_eq!(e.delete_source_line(Identity{generation:12,..actor()},1),
     Err(Error::Unauthorized));
  assert!(!e.status(actor()).unwrap().dirty);
  e.close(actor(),false).unwrap();
  e.open(actor(),&mut fs,"/binary").unwrap();
  assert_eq!(e.delete_source_line(actor(),1),Err(Error::Invalid));
  assert!(!e.status(actor()).unwrap().dirty);
 }
}


/// R063: insert a new UTF-8 source row before a 1-based existing row or
/// immediately after the final row. A source row must not contain a newline,
/// CR or NUL. The edit is ONE undoable transaction and remains unsaved until
/// an authorized COW save. Nonempty final rows gain the required separator.
impl DiskEditor{
 pub fn insert_source_line(&mut self,caller:Identity,line:usize,
    content:&str)->Result<usize,Error>{
  self.authorize(caller)?;
  if line==0||content.bytes().any(|b|b==b'\n'||b==b'\r'||b==0){
   return Err(Error::Invalid);
  }
  let text=core::str::from_utf8(&self.data[..self.used])
     .map_err(|_|Error::Invalid)?;
  let total=text.split_terminator('\n').count();
  if line>total.checked_add(1).ok_or(Error::Full)?{
   return Err(Error::Invalid);
  }
  // Preflight capacity BEFORE touching the fixed-size scratch buffer.
  let separator=usize::from(self.used!=0);
  let addition=content.len().checked_add(separator).ok_or(Error::Full)?;
  if self.used.checked_add(addition).is_none_or(|n|n>MAX_FILE_BYTES){
   return Err(Error::Full);
  }
  let mut prefix=[0u8;MAX_FILE_BYTES];
  let (offset,len)=if line<=total{
   let mut pos=0usize;
   for (index,row) in text.split_terminator('\n').enumerate(){
    if index+1==line{break;}
    pos=pos.checked_add(row.len()+1).ok_or(Error::Full)?;
   }
   prefix[..content.len()].copy_from_slice(content.as_bytes());
   prefix[content.len()]=b'\n';
   (pos,content.len()+1)
  }else if self.used==0{
   prefix[..content.len()].copy_from_slice(content.as_bytes());
   (0,content.len())
  }else if self.data[self.used-1]==b'\n'{
   prefix[..content.len()].copy_from_slice(content.as_bytes());
   prefix[content.len()]=b'\n';
   (self.used,content.len()+1)
  }else{
   prefix[0]=b'\n';
   prefix[1..content.len()+1].copy_from_slice(content.as_bytes());
   (self.used,content.len()+1)
  };
  self.replace(caller,offset,offset,&prefix[..len])
 }
}
#[cfg(test)]
mod r063_source_line_insert_tests{
 use super::*;
 use crate::block::{BlockError,SECTOR_BYTES};
 struct Disk{data:[[u8;SECTOR_BYTES];100]}
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{100}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *out=*self.data.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,lba:u64,bytes:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *self.data.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*bytes;Ok(())
  }
 }
 fn actor()->Identity{Identity{pid:9,generation:5}}
 fn newfs()->HierVfs<Disk>{
  let mut fs=HierVfs::format_blank(Disk{data:[[0;SECTOR_BYTES];100]}).unwrap();
  fs.create("/code","α\nγ".as_bytes()).unwrap();
  fs.create("/empty",b"").unwrap();
  fs.create("/lines",b"a\n").unwrap();fs
 }
 #[test]fn insert_unicode_line_is_single_undo_and_survives_disk_remount(){
  let mut fs=newfs();let mut editor=DiskEditor::new();
  editor.open(actor(),&mut fs,"/code").unwrap();
  assert_eq!(editor.insert_source_line(actor(),2,"β"),Ok(8));
  let mut data=[0u8;32];
  assert_eq!(editor.read(actor(),&mut data),Ok(8));
  assert_eq!(&data[..8],"α\nβ\nγ".as_bytes());
  assert_eq!(editor.undo(actor()),Ok(5));
  assert_eq!(editor.redo(actor()),Ok(8));
  editor.save(actor(),&mut fs).unwrap();
  let mut mounted=HierVfs::mount(fs.into_device()).unwrap();
  assert_eq!(mounted.read("/code",&mut data),Ok(8));
  assert_eq!(&data[..8],"α\nβ\nγ".as_bytes());
 }
 #[test]fn append_last_source_row_and_insert_into_empty_buffer(){
  let mut fs=newfs();let mut editor=DiskEditor::new();
  editor.open(actor(),&mut fs,"/code").unwrap();
  assert_eq!(editor.insert_source_line(actor(),3,"ω"),Ok(8));
  let mut output=[0u8;32];editor.read(actor(),&mut output).unwrap();
  assert_eq!(&output[..8],"α\nγ\nω".as_bytes());
  editor.close(actor(),true).unwrap();
  editor.open(actor(),&mut fs,"/empty").unwrap();
  assert_eq!(editor.insert_source_line(actor(),1,"first"),Ok(5));
  editor.close(actor(),true).unwrap();
  editor.open(actor(),&mut fs,"/lines").unwrap();
  assert_eq!(editor.insert_source_line(actor(),2,"b"),Ok(4));
  editor.read(actor(),&mut output).unwrap();
  assert_eq!(&output[..4],b"a\nb\n");
 }
 #[test]fn invalid_index_control_payload_and_stale_owner_do_not_edit(){
  let mut fs=newfs();let mut editor=DiskEditor::new();
  editor.open(actor(),&mut fs,"/code").unwrap();
  assert_eq!(editor.insert_source_line(actor(),0,"x"),Err(Error::Invalid));
  assert_eq!(editor.insert_source_line(actor(),4,"x"),Err(Error::Invalid));
  assert_eq!(editor.insert_source_line(actor(),2,"x\nbad"),Err(Error::Invalid));
  assert_eq!(editor.insert_source_line(actor(),2,"x\r"),Err(Error::Invalid));
  let huge=[b'x';MAX_FILE_BYTES+1];
  let huge=core::str::from_utf8(&huge).unwrap();
  assert_eq!(editor.insert_source_line(actor(),2,huge),Err(Error::Full));
  assert_eq!(editor.insert_source_line(
   Identity{generation:6,..actor()},2,"bad"),Err(Error::Unauthorized));
  assert!(!editor.status(actor()).unwrap().dirty);
  let mut output=[0u8;20];editor.read(actor(),&mut output).unwrap();
  assert_eq!(&output[..5],"α\nγ".as_bytes());
 }
}


/// R064: replace a whole 1-based UTF-8 source row without modifying its
/// line separator. Preserves terminated/unterminated final rows and
/// executes as ONE undoable, authorization-checked source edit.
/// Unsaved edits only become durable through the existing COW save path.
impl DiskEditor{
 pub fn replace_source_line(&mut self,caller:Identity,line:usize,
    replacement:&str)->Result<usize,Error>{
  self.authorize(caller)?;
  if line==0||replacement.bytes().any(|b|b==b'\n'||b==b'\r'||b==0){
   return Err(Error::Invalid);
  }
  let text=core::str::from_utf8(&self.data[..self.used])
    .map_err(|_|Error::Invalid)?;
  let mut start=0usize;
  for (index,row) in text.split_terminator('\n').enumerate(){
   let end=start.checked_add(row.len()).ok_or(Error::Full)?;
   if index+1==line{
    return self.replace_text(caller,start,end,replacement);
   }
   start=end.checked_add(1).ok_or(Error::Full)?;
  }
  Err(Error::Invalid)
 }
}
#[cfg(test)]
mod r064_source_line_replace_tests{
 use super::*;
 use crate::block::{BlockError,SECTOR_BYTES};
 struct Disk{sectors:[[u8;SECTOR_BYTES];100]}
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{100}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *out=*self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,lba:u64,bytes:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*bytes;Ok(())
  }
 }
 fn actor()->Identity{Identity{pid:11,generation:7}}
 fn fresh()->HierVfs<Disk>{
  let mut fs=HierVfs::format_blank(Disk{sectors:[[0;SECTOR_BYTES];100]}).unwrap();
  fs.create("/code","α\nβ\nomega".as_bytes()).unwrap();
  fs.create("/terminated",b"first\nlast\n").unwrap();
  fs.create("/binary",&[0xff,0x80]).unwrap();
  fs
 }
 #[test]fn unicode_row_replacement_is_one_undo_and_persists_after_remount(){
  let mut fs=fresh();let mut editor=DiskEditor::new();
  editor.open(actor(),&mut fs,"/code").unwrap();
  assert_eq!(editor.replace_source_line(actor(),2,"γδ"),Ok(13));
  let mut out=[0u8;32];
  assert_eq!(editor.read(actor(),&mut out),Ok(13));
  assert_eq!(&out[..13],"α\nγδ\nomega".as_bytes());
  assert_eq!(editor.undo(actor()),Ok(11));
  assert_eq!(editor.redo(actor()),Ok(13));
  editor.save(actor(),&mut fs).unwrap();
  let mut mounted=HierVfs::mount(fs.into_device()).unwrap();
  assert_eq!(mounted.read("/code",&mut out),Ok(13));
  assert_eq!(&out[..13],"α\nγδ\nomega".as_bytes());
 }
 #[test]fn preserves_terminated_final_row_and_noop_revision(){
  let mut fs=fresh();let mut editor=DiskEditor::new();
  editor.open(actor(),&mut fs,"/terminated").unwrap();
  assert_eq!(editor.replace_source_line(actor(),2,"tail"),Ok(11));
  let mut out=[0u8;32];
  editor.read(actor(),&mut out).unwrap();
  assert_eq!(&out[..11],b"first\ntail\n");
  editor.close(actor(),true).unwrap();
  editor.open(actor(),&mut fs,"/code").unwrap();
  assert_eq!(editor.replace_source_line(actor(),2,"β"),Ok(11));
  assert!(!editor.status(actor()).unwrap().dirty);
 }
 #[test]fn invalid_rows_bad_utf8_stale_owner_and_size_leave_buffer_clean(){
  let mut fs=fresh();let mut editor=DiskEditor::new();
  editor.open(actor(),&mut fs,"/code").unwrap();
  for row in [0,4,200]{
   assert_eq!(editor.replace_source_line(actor(),row,"safe"),Err(Error::Invalid));
  }
  assert_eq!(editor.replace_source_line(actor(),1,"bad\nline"),Err(Error::Invalid));
  assert_eq!(editor.replace_source_line(actor(),1,"bad\r"),Err(Error::Invalid));
  let huge=[b'x';MAX_FILE_BYTES+1];
  assert_eq!(editor.replace_source_line(actor(),1,
   core::str::from_utf8(&huge).unwrap()),Err(Error::Full));
  assert_eq!(editor.replace_source_line(
   Identity{generation:8,..actor()},1,"bad"),Err(Error::Unauthorized));
  assert!(!editor.status(actor()).unwrap().dirty);
  editor.close(actor(),false).unwrap();
  editor.open(actor(),&mut fs,"/binary").unwrap();
  assert_eq!(editor.replace_source_line(actor(),1,"safe"),Err(Error::Invalid));
 }
}


/// R065: source-editor multi-line comment and uncomment. Uses exactly one
/// owner-authenticated, UTF-8-safe replacement for all selected rows and
/// therefore one undo/redo step. Blank rows are preserved; uncommenting
/// a nonblank row without the expected prefix fails the whole operation.
/// COW persistence still requires a later authorized save().
impl DiskEditor{
 pub fn comment_source_range(&mut self,caller:Identity,first:usize,
     last:usize,uncomment:bool)->Result<usize,Error>{
  self.authorize(caller)?;
  if first==0||last<first{return Err(Error::Invalid);}
  let text=core::str::from_utf8(&self.data[..self.used])
    .map_err(|_|Error::Invalid)?;
  let mut staged=[0u8;MAX_FILE_BYTES];
  let mut used=0usize;let mut rows=0usize;let mut changed=0usize;
  for original in text.split_inclusive('\n'){
   rows=rows.checked_add(1).ok_or(Error::Full)?;
   let (line,separator)=match original.strip_suffix('\n'){
    Some(value)=>(value,true),
    None=>(original,false),
   };
   let selected=rows>=first&&rows<=last&&!line.is_empty();
   let next=if selected&&uncomment{
    let content=line.strip_prefix("// ").ok_or(Error::Invalid)?;
    changed+=1;content
   }else{line};
   let addition=next.len()
      .checked_add(if selected&&!uncomment{3}else{0})
      .and_then(|n|n.checked_add(usize::from(separator)))
      .ok_or(Error::Full)?;
   let end=used.checked_add(addition).ok_or(Error::Full)?;
   if end>MAX_FILE_BYTES{return Err(Error::Full);}
   if selected&&!uncomment{
    staged[used..used+3].copy_from_slice(b"// ");
    used+=3;changed+=1;
   }
   staged[used..used+next.len()].copy_from_slice(next.as_bytes());
   used+=next.len();
   if separator{staged[used]=b'\n';used+=1;}
  }
  if last>rows{return Err(Error::Invalid);}
  if changed==0{return Ok(0);}
  let result=core::str::from_utf8(&staged[..used])
    .map_err(|_|Error::Invalid)?;
  self.replace_text(caller,0,self.used,result)?;
  Ok(changed)
 }
}
#[cfg(test)]
mod r065_source_comment_tests{
 use super::*;
 use crate::block::{BlockError,SECTOR_BYTES};
 struct Disk{data:[[u8;SECTOR_BYTES];100]}
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{100}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *out=*self.data.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,lba:u64,bytes:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *self.data.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*bytes;Ok(())
  }
 }
 fn actor()->Identity{Identity{pid:12,generation:3}}
 fn newfs()->HierVfs<Disk>{
  let mut fs=HierVfs::format_blank(Disk{data:[[0;SECTOR_BYTES];100]}).unwrap();
  fs.create("/code","α\nβ\nomega".as_bytes()).unwrap();
  fs.create("/blank",b"a\n\nb\n").unwrap();
  fs.create("/binary",&[0xff]).unwrap();
  fs
 }
 #[test]fn comments_two_unicode_lines_with_one_undo_and_survives_remount(){
  let mut fs=newfs();let mut editor=DiskEditor::new();
  editor.open(actor(),&mut fs,"/code").unwrap();
  assert_eq!(editor.comment_source_range(actor(),1,2,false),Ok(2));
  let mut data=[0u8;32];
  assert_eq!(editor.read(actor(),&mut data),Ok(17));
  assert_eq!(&data[..17],"// α\n// β\nomega".as_bytes());
  assert_eq!(editor.undo(actor()),Ok(11));
  assert_eq!(editor.redo(actor()),Ok(17));
  editor.save(actor(),&mut fs).unwrap();
  let mut remounted=HierVfs::mount(fs.into_device()).unwrap();
  assert_eq!(remounted.read("/code",&mut data),Ok(17));
  assert_eq!(&data[..17],"// α\n// β\nomega".as_bytes());
 }
 #[test]fn preserves_blank_rows_and_terminating_newline_when_uncommenting(){
  let mut fs=newfs();let mut editor=DiskEditor::new();
  editor.open(actor(),&mut fs,"/blank").unwrap();
  assert_eq!(editor.comment_source_range(actor(),1,3,false),Ok(2));
  let mut out=[0u8;32];
  assert_eq!(editor.read(actor(),&mut out),Ok(11));
  assert_eq!(&out[..11],b"// a\n\n// b\n");
  assert_eq!(editor.comment_source_range(actor(),1,3,true),Ok(2));
  assert_eq!(editor.read(actor(),&mut out),Ok(5));
  assert_eq!(&out[..5],b"a\n\nb\n");
 }
 #[test]fn malformed_rows_unauthorized_uncomment_and_binary_reject_atomically(){
  let mut fs=newfs();let mut editor=DiskEditor::new();
  editor.open(actor(),&mut fs,"/code").unwrap();
  for (a,b,uncomment) in [(0,1,false),(1,4,false),(2,1,false),(1,2,true)]{
   assert_eq!(editor.comment_source_range(actor(),a,b,uncomment),
     Err(Error::Invalid));
  }
  assert_eq!(editor.comment_source_range(
   Identity{generation:4,..actor()},1,1,false),Err(Error::Unauthorized));
  assert!(!editor.status(actor()).unwrap().dirty);
  let mut out=[0u8;32];
  assert_eq!(editor.read(actor(),&mut out),Ok(11));
  assert_eq!(&out[..11],"α\nβ\nomega".as_bytes());
  editor.close(actor(),false).unwrap();
  editor.open(actor(),&mut fs,"/binary").unwrap();
  assert_eq!(editor.comment_source_range(actor(),1,1,false),Err(Error::Invalid));
 }
}


/// R066: replace an inclusive, one-based source-line selection with one
/// or more UTF-8 rows. The existing final newline of the selected range
/// is preserved; replacement must not inject a trailing phantom row.
/// All validation and growth checks go through one authenticated replace(),
/// preserving a single undo/redo step and disk COW save semantics.
impl DiskEditor{
 pub fn replace_source_lines(&mut self,caller:Identity,
    first:usize,last:usize,replacement:&str)->Result<usize,Error>{
  self.authorize(caller)?;
  if first==0||last<first||replacement.ends_with('\n')||
     replacement.bytes().any(|b|b==b'\r'||b==0){
    return Err(Error::Invalid);
  }
  let text=core::str::from_utf8(&self.data[..self.used])
    .map_err(|_|Error::Invalid)?;
  let mut cursor=0usize;
  let mut from=None;let mut to=None;
  for (index,row) in text.split_terminator('\n').enumerate(){
   let number=index+1;
   let end=cursor.checked_add(row.len()).ok_or(Error::Full)?;
   if number==first{from=Some(cursor);}
   if number==last{to=Some(end);}
   cursor=end.checked_add(1).ok_or(Error::Full)?;
  }
  let start=from.ok_or(Error::Invalid)?;
  let end=to.ok_or(Error::Invalid)?;
  self.replace_text(caller,start,end,replacement)
 }
}
#[cfg(test)]
mod r066_unicode_source_range_tests{
 use super::*;
 use crate::block::{BlockError,SECTOR_BYTES};
 struct Disk{data:[[u8;SECTOR_BYTES];100]}
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{100}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *out=*self.data.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,lba:u64,bytes:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *self.data.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*bytes;Ok(())
  }
 }
 fn actor()->Identity{Identity{pid:6,generation:4}}
 fn setup()->HierVfs<Disk>{
  let mut fs=HierVfs::format_blank(Disk{data:[[0;SECTOR_BYTES];100]}).unwrap();
  fs.create("/code","α\nβ\nomega".as_bytes()).unwrap();
  fs.create("/rows",b"first\nmiddle\nlast\n").unwrap();
  fs.create("/binary",&[0xff,0x80]).unwrap();
  fs
 }
 #[test]fn batch_source_replacement_is_one_undo_and_survives_disk_remount(){
  let mut fs=setup();let mut editor=DiskEditor::new();
  editor.open(actor(),&mut fs,"/code").unwrap();
  let expected="first\nsecond\nthird\nomega".as_bytes();
  assert_eq!(editor.replace_source_lines(
    actor(),1,2,"first\nsecond\nthird"),Ok(expected.len()));
  let mut out=[0u8;64];
  assert_eq!(editor.read(actor(),&mut out),Ok(expected.len()));
  assert_eq!(&out[..expected.len()],expected);
  assert_eq!(editor.undo(actor()),Ok("α\nβ\nomega".len()));
  assert_eq!(editor.redo(actor()),Ok(expected.len()));
  editor.save(actor(),&mut fs).unwrap();
  let mut remounted=HierVfs::mount(fs.into_device()).unwrap();
  assert_eq!(remounted.read("/code",&mut out),Ok(expected.len()));
  assert_eq!(&out[..expected.len()],expected);
 }
 #[test]fn final_terminated_line_keeps_newline_and_noop_is_clean(){
  let mut fs=setup();let mut editor=DiskEditor::new();
  editor.open(actor(),&mut fs,"/rows").unwrap();
  assert_eq!(editor.replace_source_lines(actor(),2,3,"M\nN"),Ok(10));
  let mut out=[0u8;32];editor.read(actor(),&mut out).unwrap();
  assert_eq!(&out[..10],b"first\nM\nN\n");
  editor.close(actor(),true).unwrap();
  editor.open(actor(),&mut fs,"/code").unwrap();
  assert_eq!(editor.replace_source_lines(actor(),2,2,"β"),
    Ok("α\nβ\nomega".len()));
  assert!(!editor.status(actor()).unwrap().dirty);
 }
 #[test]fn invalid_range_control_bytes_oversize_identity_and_binary_fail_closed(){
  let mut fs=setup();let mut editor=DiskEditor::new();
  editor.open(actor(),&mut fs,"/code").unwrap();
  for (first,last) in [(0,1),(2,1),(1,4),(3,4)]{
   assert_eq!(editor.replace_source_lines(actor(),first,last,"bad"),
     Err(Error::Invalid));
  }
  assert_eq!(editor.replace_source_lines(actor(),1,1,"bad\n"),
    Err(Error::Invalid));
  assert_eq!(editor.replace_source_lines(actor(),1,1,"bad\r"),
    Err(Error::Invalid));
  let big=[b'X';MAX_FILE_BYTES+1];
  assert_eq!(editor.replace_source_lines(actor(),1,1,
    core::str::from_utf8(&big).unwrap()),Err(Error::Full));
  assert_eq!(editor.replace_source_lines(
    Identity{generation:5,..actor()},1,2,"bad"),Err(Error::Unauthorized));
  assert!(!editor.status(actor()).unwrap().dirty);
  editor.close(actor(),false).unwrap();
  editor.open(actor(),&mut fs,"/binary").unwrap();
  assert_eq!(editor.replace_source_lines(actor(),1,1,"safe"),
    Err(Error::Invalid));
 }
}


/// R069: remove trailing horizontal whitespace from every source row in
/// one reversible buffer edit. The editor stays unsaved until authorized
/// save(), which uses the existing generation and system-path guards.
impl DiskEditor {
    pub fn trim_source_trailing_whitespace(&mut self, caller: Identity)
        -> Result<usize, Error> {
        self.authorize(caller)?;
        let text = core::str::from_utf8(&self.data[..self.used]).map_err(|_| Error::Invalid)?;
        let mut staged = [0u8; MAX_FILE_BYTES];
        let mut used = 0usize;
        for row in text.split_inclusive('\n') {
            let newline = row.ends_with('\n');
            let body = if newline { &row[..row.len() - 1] } else { row };
            let clean = body.trim_end_matches(|ch| ch == ' ' || ch == '\t');
            let end = used.checked_add(clean.len() + usize::from(newline)).ok_or(Error::Full)?;
            if end > MAX_FILE_BYTES { return Err(Error::Full); }
            staged[used..used + clean.len()].copy_from_slice(clean.as_bytes());
            used += clean.len();
            if newline { staged[used] = b'\n'; used += 1; }
        }
        if used == self.used && staged[..used] == self.data[..self.used] {
            return Ok(self.used);
        }
        self.replace(caller, 0, self.used, &staged[..used])
    }
}

#[cfg(test)]
mod r069_editor_whitespace_tests {
    use super::*;
    use crate::block::{BlockError, SECTOR_BYTES};
    struct Disk { data: [[u8; SECTOR_BYTES]; 100] }
    impl BlockDevice for Disk {
        fn sector_count(&self) -> u64 { 100 }
        fn read_sector(&mut self, lba: u64, out: &mut [u8; SECTOR_BYTES]) -> Result<(), BlockError> {
            *out = *self.data.get(lba as usize).ok_or(BlockError::OutOfRange)?; Ok(())
        }
        fn write_sector(&mut self, lba: u64, bytes: &[u8; SECTOR_BYTES]) -> Result<(), BlockError> {
            *self.data.get_mut(lba as usize).ok_or(BlockError::OutOfRange)? = *bytes; Ok(())
        }
    }
    fn id() -> Identity { Identity { pid: 6, generation: 4 } }
    fn volume() -> HierVfs<Disk> {
        let mut fs = HierVfs::format_blank(Disk { data: [[0; SECTOR_BYTES]; 100] }).unwrap();
        fs.create("/source", "α  \nβ\t \nend \t".as_bytes()).unwrap();
        fs.create("/binary", &[0xff, 0xfe]).unwrap();
        fs
    }
    #[test]
    fn cleanup_is_one_undo_redo_and_survives_save_and_remount() {
        let mut fs = volume(); let mut editor = DiskEditor::new();
        editor.open(id(), &mut fs, "/source").unwrap();
        let clean = "α\nβ\nend".as_bytes();
        let old = "α  \nβ\t \nend \t".as_bytes();
        assert_eq!(editor.trim_source_trailing_whitespace(id()), Ok(clean.len()));
        assert_eq!(editor.undo(id()), Ok(old.len()));
        assert_eq!(editor.redo(id()), Ok(clean.len()));
        let mut buf = [0u8; 40];
        assert_eq!(editor.read(id(), &mut buf), Ok(clean.len()));
        assert_eq!(&buf[..clean.len()], clean);
        editor.save(id(), &mut fs).unwrap();
        let mut fs = HierVfs::mount(fs.into_device()).unwrap();
        assert_eq!(fs.read("/source", &mut buf), Ok(clean.len()));
        assert_eq!(&buf[..clean.len()], clean);
    }
    #[test]
    fn clean_source_is_noop_without_dirtying_editor() {
        let mut fs = volume(); let mut editor = DiskEditor::new();
        editor.open(id(), &mut fs, "/source").unwrap();
        editor.trim_source_trailing_whitespace(id()).unwrap();
        editor.save(id(), &mut fs).unwrap();
        assert_eq!(editor.trim_source_trailing_whitespace(id()), Ok("α\nβ\nend".len()));
        assert!(!editor.status(id()).unwrap().dirty);
    }
    #[test]
    fn invalid_owner_and_binary_content_fail_closed() {
        let mut fs = volume(); let mut editor = DiskEditor::new();
        editor.open(id(), &mut fs, "/binary").unwrap();
        let generation = fs.generation();
        assert_eq!(editor.trim_source_trailing_whitespace(id()), Err(Error::Invalid));
        assert_eq!(editor.trim_source_trailing_whitespace(
            Identity { generation: 5, ..id() }), Err(Error::Unauthorized));
        assert!(!editor.status(id()).unwrap().dirty);
        assert_eq!(fs.generation(), generation);
    }
}

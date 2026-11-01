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

    /// R076/15: inverse of text_offset, using 1-based Unicode scalar columns.
    /// The newline byte is the end position of the preceding source row.
    pub fn text_position(&self, who:Identity, at:usize)->Result<(usize,usize),Error>{
        self.authorize(who)?;
        let text=core::str::from_utf8(&self.data[..self.used]).map_err(|_|Error::Invalid)?;
        if at>self.used||!text.is_char_boundary(at){return Err(Error::Invalid);}
        let prefix=&text[..at];
        Ok((prefix.bytes().filter(|b|*b==b'\n').count()+1,
            prefix.rsplit('\n').next().unwrap_or("").chars().count()+1))
    }
    /// R076/17: previous complete literal match ending before this position.
    pub fn find_previous_text(&self,who:Identity,needle:&str,before:usize)
        ->Result<Option<usize>,Error>{
        self.authorize(who)?;
        if needle.is_empty(){return Err(Error::Invalid);}
        let text=core::str::from_utf8(&self.data[..self.used]).map_err(|_|Error::Invalid)?;
        if before>self.used||!text.is_char_boundary(before){return Err(Error::Invalid);}
        Ok(text[..before].rfind(needle))
    }
    /// R076/17: bounded pages of literal match positions, non-overlapping.
    /// next is the byte offset of the first match omitted from this page.
    pub fn find_text_page(&self,who:Identity,needle:&str,from:usize,out:&mut[usize])
        ->Result<(usize,Option<usize>),Error>{
        self.authorize(who)?;
        if needle.is_empty(){return Err(Error::Invalid);}
        let text=core::str::from_utf8(&self.data[..self.used]).map_err(|_|Error::Invalid)?;
        if from>self.used||!text.is_char_boundary(from){return Err(Error::Invalid);}
        if out.is_empty(){return Err(Error::Full);}
        let mut cursor=from;let mut count=0;
        while let Some(delta)=text[cursor..].find(needle){
            let at=cursor+delta;
            if count==out.len(){return Ok((count,Some(at)));}
            out[count]=at;count+=1;cursor=at+needle.len();
        }
        Ok((count,None))
    }
    /// R076/16: source selections are UTF-8-boundary checked and owner-bound.
    /// The caller owns the clipboard and must retire it with its session.
    pub fn copy_text_selection(&self,who:Identity,from:usize,to:usize,out:&mut[u8])
        ->Result<usize,Error>{
        self.authorize(who)?;
        let text=core::str::from_utf8(&self.data[..self.used]).map_err(|_|Error::Invalid)?;
        if from>to||to>self.used||!text.is_char_boundary(from)||
            !text.is_char_boundary(to){return Err(Error::Invalid);}
        let count=to-from;
        if out.len()<count{return Err(Error::Full);}
        out[..count].copy_from_slice(&self.data[from..to]);Ok(count)
    }
    /// R076/16: preflight both outputs; a failed cut preserves clipboard,
    /// source and undo/redo. Saving still requires the normal VFS generation.
    pub fn cut_text_selection(&mut self,who:Identity,from:usize,to:usize,out:&mut[u8])
        ->Result<usize,Error>{
        self.authorize(who)?;
        let mut staged=[0u8;MAX_FILE_BYTES];
        let n=self.copy_text_selection(who,from,to,&mut staged)?;
        if out.len()<n{return Err(Error::Full);}
        self.replace_text(who,from,to,"")?;
        out[..n].copy_from_slice(&staged[..n]);Ok(n)
    }
    pub fn paste_text_selection(&mut self,who:Identity,from:usize,to:usize,text:&str)
        ->Result<usize,Error>{
        self.replace_text(who,from,to,text)
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
    fn r076_utf8_bidirectional_cursor_and_durable_remount(){
        let mut fs=volume();let mut editor=DiskEditor::new();
        editor.open(owner(),&mut fs,"/source.rs").unwrap();
        editor.replace_text(owner(),0,12,"α\n🍀z\n").unwrap();
        assert_eq!(editor.text_position(owner(),0),Ok((1,1)));
        assert_eq!(editor.text_position(owner(),2),Ok((1,2)));
        assert_eq!(editor.text_position(owner(),3),Ok((2,1)));
        assert_eq!(editor.text_position(owner(),7),Ok((2,2)));
        assert_eq!(editor.text_position(owner(),9),Ok((3,1)));
        assert_eq!(editor.text_offset(owner(),2,2),Ok(7));
        assert_eq!(editor.text_position(owner(),4),Err(Error::Invalid));
        assert_eq!(editor.text_position(other(),3),Err(Error::Unauthorized));
        editor.save(owner(),&mut fs).unwrap();
        let mut fs=HierVfs::mount(fs.into_device()).unwrap();
        let mut next=DiskEditor::new();
        next.open(owner(),&mut fs,"/source.rs").unwrap();
        assert_eq!(next.text_position(owner(),7),Ok((2,2)));
    }
    #[test]
    fn r076_literal_search_backward_and_pages_are_bounded(){
        let mut fs=volume();let mut e=DiskEditor::new();
        e.open(owner(),&mut fs,"/source.rs").unwrap();
        e.replace_text(owner(),0,12,"α α α\n").unwrap();
        assert_eq!(e.find_previous_text(owner(),"α",9),Ok(Some(6)));
        assert_eq!(e.find_previous_text(owner(),"α",6),Ok(Some(3)));
        assert_eq!(e.find_previous_text(owner(),"α",3),Ok(Some(0)));
        assert_eq!(e.find_previous_text(owner(),"α",0),Ok(None));
        let mut page=[99usize;2];
        assert_eq!(e.find_text_page(owner(),"α",0,&mut page),Ok((2,Some(6))));
        assert_eq!(page,[0,3]);
        assert_eq!(e.find_text_page(owner(),"α",6,&mut page),Ok((1,None)));
        assert_eq!(page[0],6);
        let mut untouched=[99usize;2];
        assert_eq!(e.find_text_page(owner(),"α",1,&mut untouched),Err(Error::Invalid));
        assert_eq!(e.find_text_page(other(),"α",0,&mut untouched),Err(Error::Unauthorized));
        assert_eq!(e.find_text_page(owner(),"",0,&mut untouched),Err(Error::Invalid));
        assert_eq!(untouched,[99,99]);
    }
    #[test]
    fn r076_unicode_selection_copy_cut_paste_is_durable(){
        let mut fs=volume();let mut e=DiskEditor::new();
        e.open(owner(),&mut fs,"/source.rs").unwrap();
        e.replace_text(owner(),0,12,"α β γ").unwrap();
        let mut too_small=[0xa5u8;1];
        assert_eq!(e.cut_text_selection(owner(),3,5,&mut too_small),Err(Error::Full));
        assert_eq!(too_small,[0xa5]);
        let mut clipboard=[0xa5u8;8];
        assert_eq!(e.copy_text_selection(other(),3,5,&mut clipboard),Err(Error::Unauthorized));
        assert_eq!(e.copy_text_selection(owner(),4,5,&mut clipboard),Err(Error::Invalid));
        assert_eq!(clipboard,[0xa5;8]);
        assert_eq!(e.copy_text_selection(owner(),3,5,&mut clipboard),Ok(2));
        assert_eq!(&clipboard[..2],"β".as_bytes());
        assert_eq!(e.cut_text_selection(owner(),3,5,&mut clipboard),Ok(2));
        assert_eq!(e.paste_text_selection(owner(),3,3,
            core::str::from_utf8(&clipboard[..2]).unwrap()),Ok(8));
        assert_eq!(e.paste_text_selection(owner(),8,8,"🍀"),Ok(12));
        e.save(owner(),&mut fs).unwrap();
        let mut fs=HierVfs::mount(fs.into_device()).unwrap();
        let mut saved=[0u8;32];
        assert_eq!(fs.read("/source.rs",&mut saved),Ok(12));
        assert_eq!(&saved[..12],"α β γ🍀".as_bytes());
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


/// R070: indent or strictly unindent an inclusive 1-based UTF-8 source
/// line range. Blank rows remain unchanged. A failed row, invalid range,
/// overflowing buffer or recycled owner never touches the undo snapshot.
impl DiskEditor {
    pub fn indent_source_range(&mut self, caller: Identity, first: usize,
        last: usize, remove: bool) -> Result<usize, Error> {
        self.authorize(caller)?;
        if first == 0 || last < first { return Err(Error::Invalid); }
        let text = core::str::from_utf8(&self.data[..self.used])
            .map_err(|_| Error::Invalid)?;
        let mut staged = [0u8; MAX_FILE_BYTES];
        let mut used = 0usize;
        let mut rows = 0usize;
        let mut changed = 0usize;
        for row in text.split_inclusive('\n') {
            rows += 1;
            let body = row.strip_suffix('\n').unwrap_or(row);
            let selected = rows >= first && rows <= last && !body.is_empty();
            let contents = if selected && remove {
                body.strip_prefix("    ").ok_or(Error::Invalid)?
            } else { body };
            let addition = contents.len()
                .checked_add(if selected && !remove { 4 } else { 0 })
                .and_then(|n| n.checked_add(usize::from(row.ends_with('\n'))))
                .ok_or(Error::Full)?;
            let end = used.checked_add(addition).ok_or(Error::Full)?;
            if end > MAX_FILE_BYTES { return Err(Error::Full); }
            if selected && !remove {
                staged[used..used + 4].copy_from_slice(b"    ");
                used += 4;
            }
            staged[used..used + contents.len()].copy_from_slice(contents.as_bytes());
            used += contents.len();
            if row.ends_with('\n') { staged[used] = b'\n'; used += 1; }
            if selected { changed += 1; }
        }
        if rows < last { return Err(Error::Invalid); }
        if changed == 0 { return Ok(0); }
        self.replace(caller, 0, self.used, &staged[..used])?;
        Ok(changed)
    }
}

#[cfg(test)]
mod r070_source_indentation_tests {
    use super::*;
    use crate::block::{BlockError, SECTOR_BYTES};
    struct Disk { sectors: [[u8; SECTOR_BYTES]; 100] }
    impl BlockDevice for Disk {
        fn sector_count(&self) -> u64 { 100 }
        fn read_sector(&mut self, lba: u64, out: &mut [u8; SECTOR_BYTES])
            -> Result<(), BlockError> {
            *out = *self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?;
            Ok(())
        }
        fn write_sector(&mut self, lba: u64, data: &[u8; SECTOR_BYTES])
            -> Result<(), BlockError> {
            *self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)? = *data;
            Ok(())
        }
    }
    fn id() -> Identity { Identity { pid: 9, generation: 2 } }
    fn setup() -> (HierVfs<Disk>, DiskEditor) {
        let mut fs = HierVfs::format_blank(Disk { sectors: [[0; SECTOR_BYTES]; 100] }).unwrap();
        fs.create("/src", "α\nβ\nγ\n".as_bytes()).unwrap();
        fs.create("/raw", &[0xff, 0xfe]).unwrap();
        let mut editor = DiskEditor::new();
        editor.open(id(), &mut fs, "/src").unwrap();
        (fs, editor)
    }
    #[test]
    fn indent_undo_redo_save_and_real_remount() {
        let (mut fs, mut editor) = setup();
        let expected = "    α\n    β\nγ\n".as_bytes();
        assert_eq!(editor.indent_source_range(id(), 1, 2, false), Ok(2));
        let mut out = [0u8; 40];
        assert_eq!(editor.read(id(), &mut out), Ok(expected.len()));
        assert_eq!(&out[..expected.len()], expected);
        assert_eq!(editor.undo(id()), Ok("α\nβ\nγ\n".len()));
        assert_eq!(editor.redo(id()), Ok(expected.len()));
        editor.save(id(), &mut fs).unwrap();
        let mut remounted = HierVfs::mount(fs.into_device()).unwrap();
        assert_eq!(remounted.read("/src", &mut out), Ok(expected.len()));
        assert_eq!(&out[..expected.len()], expected);
    }
    #[test]
    fn unindent_is_atomic_and_noop_does_not_dirty() {
        let (_fs, mut editor) = setup();
        assert_eq!(editor.indent_source_range(id(), 1, 2, true), Err(Error::Invalid));
        assert!(!editor.status(id()).unwrap().dirty);
        assert_eq!(editor.indent_source_range(id(), 1, 4, false), Err(Error::Invalid));
        assert!(!editor.status(id()).unwrap().dirty);
        editor.indent_source_range(id(), 1, 2, false).unwrap();
        assert_eq!(editor.indent_source_range(id(), 1, 2, true), Ok(2));
        assert_eq!(editor.read(id(), &mut [0u8; 2]), Err(Error::Full));
    }
    #[test]
    fn recycled_owner_and_binary_source_are_rejected() {
        let (mut fs, mut editor) = setup();
        assert_eq!(editor.indent_source_range(
            Identity { generation: 3, ..id() }, 1, 1, false), Err(Error::Unauthorized));
        editor.close(id(), false).unwrap();
        editor.open(id(), &mut fs, "/raw").unwrap();
        assert_eq!(editor.indent_source_range(id(), 1, 1, false), Err(Error::Invalid));
        assert!(!editor.status(id()).unwrap().dirty);
    }
}


/// R071: atomically convert Windows CRLF rows to LF in one undoable,
/// unsaved source edit. Stray CR and invalid UTF-8 never change editor state.
impl DiskEditor {
    pub fn normalize_source_crlf(&mut self, caller: Identity) -> Result<usize, Error> {
        self.authorize(caller)?;
        core::str::from_utf8(&self.data[..self.used]).map_err(|_| Error::Invalid)?;
        let mut staged = [0u8; MAX_FILE_BYTES];
        let mut i = 0usize;
        let mut n = 0usize;
        let mut changed = 0usize;
        while i < self.used {
            if self.data[i] == b'\r' {
                if self.data.get(i + 1) != Some(&b'\n') { return Err(Error::Invalid); }
                staged[n] = b'\n';
                i += 2;
                changed += 1;
            } else {
                staged[n] = self.data[i];
                i += 1;
            }
            n += 1;
        }
        if changed > 0 { self.replace(caller, 0, self.used, &staged[..n])?; }
        Ok(changed)
    }
}

#[cfg(test)]
mod r071_editor_crlf_tests {
    use super::*;
    use crate::block::{BlockError, SECTOR_BYTES};
    struct Disk { sectors: [[u8; SECTOR_BYTES]; 100] }
    impl BlockDevice for Disk {
        fn sector_count(&self) -> u64 { 100 }
        fn read_sector(&mut self, lba: u64, out: &mut [u8; SECTOR_BYTES]) -> Result<(), BlockError> {
            *out = *self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?;
            Ok(())
        }
        fn write_sector(&mut self, lba: u64, data: &[u8; SECTOR_BYTES]) -> Result<(), BlockError> {
            *self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)? = *data; Ok(())
        }
    }
    fn id() -> Identity { Identity { pid: 7, generation: 3 } }
    fn setup() -> (HierVfs<Disk>, DiskEditor) {
        let mut fs = HierVfs::format_blank(Disk { sectors: [[0; SECTOR_BYTES]; 100] }).unwrap();
        fs.create("/src", "α\r\nβ\r\nend\n".as_bytes()).unwrap();
        fs.create("/bad", b"first\rbroken").unwrap();
        fs.create("/binary", &[0xff, 0xfe]).unwrap();
        let mut editor = DiskEditor::new();
        editor.open(id(), &mut fs, "/src").unwrap();
        (fs, editor)
    }
    #[test]
    fn converts_unicode_lines_undo_redo_and_remount() {
        let (mut fs, mut e) = setup();
        assert_eq!(e.normalize_source_crlf(id()), Ok(2));
        assert_eq!(e.undo(id()), Ok("α\r\nβ\r\nend\n".len()));
        assert_eq!(e.redo(id()), Ok("α\nβ\nend\n".len()));
        e.save(id(), &mut fs).unwrap();
        assert_eq!(e.normalize_source_crlf(id()), Ok(0));
        assert!(!e.status(id()).unwrap().dirty);
        let mut fs = HierVfs::mount(fs.into_device()).unwrap();
        let mut out = [0u8; 32];
        assert_eq!(fs.read("/src", &mut out), Ok("α\nβ\nend\n".len()));
        assert_eq!(&out[.."α\nβ\nend\n".len()], "α\nβ\nend\n".as_bytes());
    }
    #[test]
    fn bad_cr_binary_and_recycled_owner_are_fail_closed() {
        let (mut fs, mut e) = setup();
        let generation = fs.generation();
        assert_eq!(e.normalize_source_crlf(Identity { generation: 4, ..id() }),
            Err(Error::Unauthorized));
        assert!(!e.status(id()).unwrap().dirty);
        e.close(id(), false).unwrap();
        e.open(id(), &mut fs, "/bad").unwrap();
        assert_eq!(e.normalize_source_crlf(id()), Err(Error::Invalid));
        assert!(!e.status(id()).unwrap().dirty);
        e.close(id(), false).unwrap();
        e.open(id(), &mut fs, "/binary").unwrap();
        assert_eq!(e.normalize_source_crlf(id()), Err(Error::Invalid));
        assert_eq!(fs.generation(), generation);
    }
}


/// R072: expand TAB indentation at the beginning of each source row to
/// four ASCII spaces. Interior tabs (e.g. inside source literals) are left
/// unchanged. Stage every byte before one reversible in-memory edit.
impl DiskEditor {
    pub fn expand_leading_source_tabs(&mut self, caller: Identity) -> Result<usize, Error> {
        self.authorize(caller)?;
        core::str::from_utf8(&self.data[..self.used]).map_err(|_| Error::Invalid)?;
        let mut staged = [0u8; MAX_FILE_BYTES];
        let mut at = 0usize;
        let mut changed = 0usize;
        let mut in_indent = true;
        for &byte in &self.data[..self.used] {
            if byte == b'\t' && in_indent {
                let last = at.checked_add(4).ok_or(Error::Full)?;
                if last > staged.len() { return Err(Error::Full); }
                staged[at..last].fill(b' ');
                at = last;
                changed += 1;
            } else {
                if at == staged.len() { return Err(Error::Full); }
                staged[at] = byte;
                at += 1;
                if byte == b'\n' { in_indent = true; }
                else if byte != b' ' { in_indent = false; }
            }
        }
        if changed != 0 { self.replace(caller, 0, self.used, &staged[..at])?; }
        Ok(changed)
    }
}

#[cfg(test)]
mod r072_editor_tabs_tests {
    use super::*;
    use crate::block::{BlockError, SECTOR_BYTES};
    struct Disk { sectors: [[u8; SECTOR_BYTES]; 100] }
    impl BlockDevice for Disk {
        fn sector_count(&self) -> u64 { 100 }
        fn read_sector(&mut self, lba: u64, out: &mut [u8; SECTOR_BYTES]) -> Result<(), BlockError> {
            *out = *self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?; Ok(())
        }
        fn write_sector(&mut self, lba: u64, data: &[u8; SECTOR_BYTES]) -> Result<(), BlockError> {
            *self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)? = *data; Ok(())
        }
    }
    fn id() -> Identity { Identity { pid: 8, generation: 3 } }
    fn volume() -> HierVfs<Disk> {
        let mut fs = HierVfs::format_blank(Disk { sectors: [[0; SECTOR_BYTES]; 100] }).unwrap();
        fs.create("/src", "\tα\n \tβ\tz\n\t\tγ\n".as_bytes()).unwrap();
        fs.create("/binary", &[0xff, b'\t']).unwrap();
        fs
    }
    #[test]
    fn expands_leading_tabs_only_and_survives_undo_redo_and_reboot() {
        let mut fs = volume();
        let mut e = DiskEditor::new();
        e.open(id(), &mut fs, "/src").unwrap();
        let old = "\tα\n \tβ\tz\n\t\tγ\n";
        let next = "    α\n     β\tz\n        γ\n";
        assert_eq!(e.expand_leading_source_tabs(id()), Ok(4));
        assert_eq!(e.undo(id()), Ok(old.len()));
        assert_eq!(e.redo(id()), Ok(next.len()));
        e.save(id(), &mut fs).unwrap();
        let mut fs = HierVfs::mount(fs.into_device()).unwrap();
        let mut out = [0u8; 48];
        assert_eq!(fs.read("/src", &mut out), Ok(next.len()));
        assert_eq!(&out[..next.len()], next.as_bytes());
    }
    #[test]
    fn no_tabs_is_noop_binary_and_recycled_id_are_denied() {
        let mut fs = volume();
        let mut e = DiskEditor::new();
        e.open(id(), &mut fs, "/src").unwrap();
        assert_eq!(e.expand_leading_source_tabs(
            Identity { generation: 4, ..id() }), Err(Error::Unauthorized));
        assert!(!e.status(id()).unwrap().dirty);
        e.expand_leading_source_tabs(id()).unwrap();
        e.save(id(), &mut fs).unwrap();
        assert_eq!(e.expand_leading_source_tabs(id()), Ok(0));
        assert!(!e.status(id()).unwrap().dirty);
        e.close(id(), false).unwrap();
        e.open(id(), &mut fs, "/binary").unwrap();
        assert_eq!(e.expand_leading_source_tabs(id()), Err(Error::Invalid));
    }
}


/// R073 source hygiene: strip a UTF-8 BOM at file start and ensure nonempty
/// UTF-8 source ends in LF. Stages both changes as one undoable edit;
/// invalid UTF-8, NUL, size overflow and foreign owners fail closed.
/// Returns the number of hygiene actions (BOM removed and/or LF appended).
impl DiskEditor {
    pub fn clean_source_encoding_and_eof(&mut self, caller: Identity) -> Result<usize, Error> {
        self.authorize(caller)?;
        let original = core::str::from_utf8(&self.data[..self.used])
            .map_err(|_| Error::Invalid)?;
        if original.as_bytes().contains(&0) { return Err(Error::Invalid); }
        let bom = original.as_bytes().starts_with(&[0xef, 0xbb, 0xbf]);
        let start = if bom { 3 } else { 0 };
        let body = &self.data[start..self.used];
        let add_lf = !body.is_empty() && !body.ends_with(b"\n");
        let target = body.len() + usize::from(add_lf);
        if target > MAX_FILE_BYTES { return Err(Error::Full); }
        let count = usize::from(bom) + usize::from(add_lf);
        if count == 0 { return Ok(0); }
        let mut staged = [0u8; MAX_FILE_BYTES];
        staged[..body.len()].copy_from_slice(body);
        if add_lf { staged[body.len()] = b'\n'; }
        self.replace(caller, 0, self.used, &staged[..target])?;
        Ok(count)
    }
}

#[cfg(test)]
mod r073_source_hygiene_tests {
    use super::*;
    use crate::block::{BlockError, SECTOR_BYTES};
    struct Disk { sectors: [[u8; SECTOR_BYTES]; 100] }
    impl BlockDevice for Disk {
        fn sector_count(&self) -> u64 { 100 }
        fn read_sector(&mut self, lba: u64, out: &mut [u8; SECTOR_BYTES]) -> Result<(), BlockError> {
            *out = *self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?;
            Ok(())
        }
        fn write_sector(&mut self, lba: u64, data: &[u8; SECTOR_BYTES]) -> Result<(), BlockError> {
            *self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)? = *data;
            Ok(())
        }
    }
    fn id() -> Identity { Identity { pid: 4, generation: 5 } }
    fn volume() -> HierVfs<Disk> {
        let mut fs = HierVfs::format_blank(Disk { sectors: [[0; SECTOR_BYTES]; 100] }).unwrap();
        fs.create("/src", "\u{feff}α\nβ".as_bytes()).unwrap();
        fs.create("/binary", &[0xff, 0xfe]).unwrap();
        fs.create("/nul", b"bad\0data").unwrap();
        fs
    }
    #[test]
    fn removes_bom_appends_lf_undoes_redoes_and_remounts() {
        let mut fs = volume();
        let mut e = DiskEditor::new();
        e.open(id(), &mut fs, "/src").unwrap();
        let old = "\u{feff}α\nβ";
        let next = "α\nβ\n";
        assert_eq!(e.clean_source_encoding_and_eof(id()), Ok(2));
        assert_eq!(e.undo(id()), Ok(old.len()));
        assert_eq!(e.redo(id()), Ok(next.len()));
        e.save(id(), &mut fs).unwrap();
        assert_eq!(e.clean_source_encoding_and_eof(id()), Ok(0));
        assert!(!e.status(id()).unwrap().dirty);
        let mut fs = HierVfs::mount(fs.into_device()).unwrap();
        let mut out = [0u8; 32];
        assert_eq!(fs.read("/src", &mut out), Ok(next.len()));
        assert_eq!(&out[..next.len()], next.as_bytes());
    }
    #[test]
    fn corrupt_and_untrusted_sources_cannot_change_disk_or_editor() {
        let mut fs = volume();
        let mut e = DiskEditor::new();
        e.open(id(), &mut fs, "/src").unwrap();
        let generation = fs.generation();
        assert_eq!(e.clean_source_encoding_and_eof(
            Identity { generation: 6, ..id() }), Err(Error::Unauthorized));
        assert!(!e.status(id()).unwrap().dirty);
        e.close(id(), false).unwrap();
        e.open(id(), &mut fs, "/binary").unwrap();
        assert_eq!(e.clean_source_encoding_and_eof(id()), Err(Error::Invalid));
        e.close(id(), false).unwrap();
        e.open(id(), &mut fs, "/nul").unwrap();
        assert_eq!(e.clean_source_encoding_and_eof(id()), Err(Error::Invalid));
        assert_eq!(fs.generation(), generation);
    }
}


/// R074: move a source row up or down by swapping its UTF-8 payload with
/// the adjacent row while retaining newline delimiters at the two positions.
/// Works even when the final row lacks a terminating LF. The entire swap is
/// one undoable unsaved operation; bad rows, binary data and stale owners
/// preserve both document contents and disk state.
impl DiskEditor {
    pub fn move_source_line_one(&mut self, caller: Identity, line: usize,
        down: bool) -> Result<bool, Error> {
        self.authorize(caller)?;
        let source = core::str::from_utf8(&self.data[..self.used])
            .map_err(|_| Error::Invalid)?;
        if line == 0 { return Err(Error::Invalid); }
        let mut ranges = [(0usize, 0usize, 0usize); 2];
        let (first, second) = if down {
            (line, line.checked_add(1).ok_or(Error::Invalid)?)
        } else {
            if line <= 1 { return Err(Error::Invalid); }
            (line - 1, line)
        };
        let mut offset = 0usize;
        for (i, row) in source.split_inclusive('\n').enumerate() {
            let ordinal = i + 1;
            let end = offset + row.len();
            let body_end = if row.ends_with('\n') { end - 1 } else { end };
            if ordinal == first { ranges[0] = (offset, body_end, end); }
            if ordinal == second { ranges[1] = (offset, body_end, end); }
            offset = end;
        }
        let (a, a_end, a_row_end) = ranges[0];
        let (b, b_end, b_row_end) = ranges[1];
        if a_row_end == 0 || b_row_end == 0 || a_row_end != b {
            return Err(Error::Invalid);
        }
        let mut staged = [0u8; MAX_FILE_BYTES];
        let mut used = 0usize;
        // Preceding source rows.
        staged[..a].copy_from_slice(&self.data[..a]);
        used += a;
        // The second row's body occupies the first row's newline slot.
        staged[used..used + b_end-b].copy_from_slice(&self.data[b..b_end]);
        used += b_end-b;
        staged[used..used + a_row_end-a_end]
            .copy_from_slice(&self.data[a_end..a_row_end]);
        used += a_row_end-a_end;
        // The first row's body occupies the second row's newline slot.
        staged[used..used + a_end-a].copy_from_slice(&self.data[a..a_end]);
        used += a_end-a;
        staged[used..used + b_row_end-b_end]
            .copy_from_slice(&self.data[b_end..b_row_end]);
        used += b_row_end-b_end;
        staged[used..self.used].copy_from_slice(&self.data[b_row_end..self.used]);
        if staged[..self.used] == self.data[..self.used] { return Ok(false); }
        self.replace(caller, 0, self.used, &staged[..self.used])?;
        Ok(true)
    }
}

#[cfg(test)]
mod r074_move_source_rows_tests {
    use super::*;
    use crate::block::{BlockError, SECTOR_BYTES};
    struct Disk { sectors: [[u8; SECTOR_BYTES]; 100] }
    impl BlockDevice for Disk {
        fn sector_count(&self) -> u64 { 100 }
        fn read_sector(&mut self, lba: u64, out: &mut [u8; SECTOR_BYTES]) -> Result<(), BlockError> {
            *out = *self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?;
            Ok(())
        }
        fn write_sector(&mut self, lba: u64, data: &[u8; SECTOR_BYTES]) -> Result<(), BlockError> {
            *self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)? = *data;
            Ok(())
        }
    }
    fn owner() -> Identity { Identity { pid: 11, generation: 3 } }
    fn ready() -> HierVfs<Disk> {
        let mut fs = HierVfs::format_blank(Disk { sectors: [[0; SECTOR_BYTES]; 100] }).unwrap();
        fs.create("/source", "α\nβ\nγ".as_bytes()).unwrap();
        fs.create("/binary", &[0xff, 0x80]).unwrap();
        fs
    }
    #[test]
    fn moves_unicode_rows_with_nonterminated_final_row_and_survives_remount() {
        let mut fs = ready();
        let mut editor = DiskEditor::new();
        editor.open(owner(), &mut fs, "/source").unwrap();
        let original = "α\nβ\nγ";
        let moved = "α\nγ\nβ";
        assert_eq!(editor.move_source_line_one(owner(), 2, true), Ok(true));
        assert_eq!(editor.undo(owner()), Ok(original.len()));
        assert_eq!(editor.redo(owner()), Ok(moved.len()));
        editor.save(owner(), &mut fs).unwrap();
        let mut mounted = HierVfs::mount(fs.into_device()).unwrap();
        let mut output = [0u8; 32];
        assert_eq!(mounted.read("/source", &mut output), Ok(moved.len()));
        assert_eq!(&output[..moved.len()], moved.as_bytes());
    }
    #[test]
    fn wrong_row_stale_identity_and_binary_source_never_modify_editor() {
        let mut fs = ready();
        let mut editor = DiskEditor::new();
        editor.open(owner(), &mut fs, "/source").unwrap();
        let before = fs.generation();
        assert_eq!(editor.move_source_line_one(owner(), 3, true), Err(Error::Invalid));
        assert_eq!(editor.move_source_line_one(owner(), 1, false), Err(Error::Invalid));
        assert_eq!(editor.move_source_line_one(
            Identity { generation: 4, ..owner() }, 2, true), Err(Error::Unauthorized));
        assert!(!editor.status(owner()).unwrap().dirty);
        editor.close(owner(), false).unwrap();
        editor.open(owner(), &mut fs, "/binary").unwrap();
        assert_eq!(editor.move_source_line_one(owner(), 1, true), Err(Error::Invalid));
        assert_eq!(fs.generation(), before);
    }
}


/// R075: reverse a contiguous range of source-line *bodies*. Newline
/// delimiters stay at the same row positions, including an unterminated
/// final line. Stage one complete UTF-8 edit for owner-bound undo/redo.
impl DiskEditor {
    pub fn reverse_source_lines(&mut self, caller: Identity,
        first: usize, last: usize) -> Result<usize, Error> {
        self.authorize(caller)?;
        let text = core::str::from_utf8(&self.data[..self.used])
            .map_err(|_| Error::Invalid)?;
        if first == 0 || last <= first { return Err(Error::Invalid); }
        // The editor supports a bounded 128-row reversal to protect kernel stacks.
        let mut boundaries = [(0usize, 0usize, false); 128];
        let mut rows = 0usize;
        let mut pos = 0usize;
        for row in text.split_inclusive('\n') {
            if rows == boundaries.len() { return Err(Error::Full); }
            let ends_in_lf = row.ends_with('\n');
            let end = pos + row.len() - usize::from(ends_in_lf);
            boundaries[rows] = (pos, end, ends_in_lf);
            rows += 1;
            pos += row.len();
        }
        if last > rows { return Err(Error::Invalid); }
        let mut staged = [0u8; MAX_FILE_BYTES];
        let mut used = 0usize;
        for index in 0..rows {
            let from_index = if (first-1..last).contains(&index) {
                first-1 + last-1 - index
            } else { index };
            let (start, end, _) = boundaries[from_index];
            let len = end - start;
            staged[used..used+len].copy_from_slice(&self.data[start..end]);
            used += len;
            if boundaries[index].2 {
                staged[used] = b'\n'; used += 1;
            }
        }
        debug_assert_eq!(used, self.used);
        if staged[..used] == self.data[..self.used] { return Ok(0); }
        self.replace(caller, 0, self.used, &staged[..used])?;
        Ok(last-first+1)
    }
}

#[cfg(test)]
mod r075_editor_reverse_source_lines_tests {
    use super::*;
    use crate::block::{BlockError, SECTOR_BYTES};
    struct Disk { sectors: [[u8; SECTOR_BYTES]; 100] }
    impl BlockDevice for Disk {
        fn sector_count(&self) -> u64 { 100 }
        fn read_sector(&mut self, lba: u64, out: &mut [u8; SECTOR_BYTES]) -> Result<(), BlockError> {
            *out = *self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?; Ok(())
        }
        fn write_sector(&mut self, lba: u64, data: &[u8; SECTOR_BYTES]) -> Result<(), BlockError> {
            *self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)? = *data; Ok(())
        }
    }
    fn owner() -> Identity { Identity { pid: 9, generation: 4 } }
    fn setup() -> (HierVfs<Disk>, DiskEditor) {
        let mut fs = HierVfs::format_blank(Disk { sectors: [[0; SECTOR_BYTES]; 100] }).unwrap();
        fs.create("/src", "α\nbeta\nγ\nend".as_bytes()).unwrap();
        fs.create("/binary", &[0xff, 0xfe]).unwrap();
        let mut e = DiskEditor::new();
        e.open(owner(), &mut fs, "/src").unwrap();
        (fs, e)
    }
    #[test]
    fn reverses_unicode_line_bodies_once_then_undo_redo_and_remount() {
        let (mut fs, mut e) = setup();
        let before = "α\nbeta\nγ\nend";
        let after = "α\nend\nγ\nbeta";
        assert_eq!(e.reverse_source_lines(owner(), 2, 4), Ok(3));
        assert_eq!(e.undo(owner()), Ok(before.len()));
        assert_eq!(e.redo(owner()), Ok(after.len()));
        e.save(owner(), &mut fs).unwrap();
        let mut disk = HierVfs::mount(fs.into_device()).unwrap();
        let mut out = [0u8; 40];
        assert_eq!(disk.read("/src", &mut out), Ok(after.len()));
        assert_eq!(&out[..after.len()], after.as_bytes());
    }
    #[test]
    fn invalid_rows_recycled_id_and_binary_are_fail_closed() {
        let (mut fs, mut e) = setup();
        assert_eq!(e.reverse_source_lines(owner(), 1, 8), Err(Error::Invalid));
        assert_eq!(e.reverse_source_lines(owner(), 2, 2), Err(Error::Invalid));
        assert_eq!(e.reverse_source_lines(Identity { generation: 5, ..owner() },
            1, 3), Err(Error::Unauthorized));
        assert!(!e.status(owner()).unwrap().dirty);
        e.close(owner(), false).unwrap();
        e.open(owner(), &mut fs, "/binary").unwrap();
        assert_eq!(e.reverse_source_lines(owner(), 1, 2), Err(Error::Invalid));
    }
}


// R075 expanded workstream A: three additional real source-cleanup APIs.
impl DiskEditor {
    /// Remove blank/whitespace-only rows as one reversible UTF-8 edit.
    pub fn remove_source_blank_rows(&mut self, owner: Identity) -> Result<usize, Error> {
        self.authorize(owner)?;
        let text = core::str::from_utf8(&self.data[..self.used])
            .map_err(|_| Error::Invalid)?;
        let mut staged = [0u8; MAX_FILE_BYTES];
        let mut target = 0usize;
        let mut removed = 0usize;
        for row in text.split_inclusive('\n') {
            let body = row.strip_suffix('\n').unwrap_or(row);
            if body.bytes().all(|b| b == b' ' || b == b'\t') {
                removed += 1;
                continue;
            }
            let end = target + row.len();
            staged[target..end].copy_from_slice(row.as_bytes());
            target = end;
        }
        if removed != 0 { self.replace(owner, 0, self.used, &staged[..target])?; }
        Ok(removed)
    }

    /// Convert LF-only Unicode source to Windows CRLF, atomically.
    /// Reject existing CR bytes instead of silently changing mixed endings.
    pub fn convert_source_lf_to_crlf(&mut self, owner: Identity) -> Result<usize, Error> {
        self.authorize(owner)?;
        let text = core::str::from_utf8(&self.data[..self.used])
            .map_err(|_| Error::Invalid)?;
        if text.as_bytes().contains(&b'\r') { return Err(Error::Invalid); }
        let mut staged = [0u8; MAX_FILE_BYTES];
        let mut at = 0usize;
        let mut converted = 0usize;
        for byte in text.bytes() {
            if byte == b'\n' {
                if at + 2 > MAX_FILE_BYTES { return Err(Error::Full); }
                staged[at] = b'\r'; staged[at + 1] = b'\n';
                at += 2;
                converted += 1;
            } else {
                if at == MAX_FILE_BYTES { return Err(Error::Full); }
                staged[at] = byte;
                at += 1;
            }
        }
        if converted != 0 { self.replace(owner, 0, self.used, &staged[..at])?; }
        Ok(converted)
    }

    /// Collapse only byte-identical *adjacent* Unicode source rows.
    /// This is a file-local content operation, not a deduplication of
    /// distant rows or a comment/string-aware source transformation.
    pub fn deduplicate_adjacent_source_rows(&mut self, owner: Identity)
        -> Result<usize, Error> {
        self.authorize(owner)?;
        let text = core::str::from_utf8(&self.data[..self.used])
            .map_err(|_| Error::Invalid)?;
        let mut staged = [0u8; MAX_FILE_BYTES];
        let mut written = 0usize;
        let mut previous = None::<&str>;
        let mut skipped = 0usize;
        for row in text.split_inclusive('\n') {
            let body = row.strip_suffix('\n').unwrap_or(row);
            if previous == Some(body) { skipped += 1; continue; }
            staged[written..written+row.len()].copy_from_slice(row.as_bytes());
            written += row.len();
            previous = Some(body);
        }
        if skipped != 0 { self.replace(owner, 0, self.used, &staged[..written])?; }
        Ok(skipped)
    }
}

#[cfg(test)]
mod r075_expanded_editor_cleanup_tests {
    use super::*;
    use crate::block::{BlockError, SECTOR_BYTES};
    struct Disk { sectors: [[u8; SECTOR_BYTES]; 100] }
    impl BlockDevice for Disk {
        fn sector_count(&self) -> u64 { 100 }
        fn read_sector(&mut self, lba: u64, out: &mut [u8; SECTOR_BYTES]) -> Result<(), BlockError> {
            *out = *self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?; Ok(())
        }
        fn write_sector(&mut self, lba: u64, data: &[u8; SECTOR_BYTES]) -> Result<(), BlockError> {
            *self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)? = *data; Ok(())
        }
    }
    fn id() -> Identity { Identity { pid: 19, generation: 3 } }
    fn fresh() -> HierVfs<Disk> {
        let mut fs = HierVfs::format_blank(Disk { sectors: [[0; SECTOR_BYTES]; 100] }).unwrap();
        fs.create("/src", "α\n   \nα\nα\nβ\n".as_bytes()).unwrap();
        fs.create("/bad", &[0xff, b'\n']).unwrap();
        fs
    }
    #[test]
    fn blank_removal_and_adjacent_dedup_are_reversible_and_durable() {
        let mut fs = fresh(); let mut editor = DiskEditor::new();
        editor.open(id(), &mut fs, "/src").unwrap();
        assert_eq!(editor.remove_source_blank_rows(id()), Ok(1));
        assert_eq!(editor.deduplicate_adjacent_source_rows(id()), Ok(2));
        let expected = "α\nβ\n";
        let mut bytes = [0u8; 32];
        assert_eq!(editor.read(id(), &mut bytes), Ok(expected.len()));
        assert_eq!(&bytes[..expected.len()], expected.as_bytes());
        assert_eq!(editor.undo(id()), Ok("α\nα\nα\nβ\n".len()));
        assert_eq!(editor.redo(id()), Ok(expected.len()));
        editor.save(id(), &mut fs).unwrap();
        let mut disk = HierVfs::mount(fs.into_device()).unwrap();
        assert_eq!(disk.read("/src", &mut bytes), Ok(expected.len()));
    }
    #[test]
    fn crlf_is_one_atomic_unicode_edit_and_bad_identity_is_rejected() {
        let mut fs = fresh(); let mut editor = DiskEditor::new();
        editor.open(id(), &mut fs, "/src").unwrap();
        assert_eq!(editor.convert_source_lf_to_crlf(id()), Ok(5));
        assert_eq!(editor.normalize_source_crlf(id()), Ok(5));
        assert_eq!(editor.convert_source_lf_to_crlf(
            Identity { generation: 4, ..id() }), Err(Error::Unauthorized));
        // Intentionally discard the unsaved conversion before switching files.
        editor.close(id(), true).unwrap();
        editor.open(id(), &mut fs, "/bad").unwrap();
        assert_eq!(editor.remove_source_blank_rows(id()), Err(Error::Invalid));
    }
}


// R076: owner-authorized Unicode navigation, bounded selections and search.
// These APIs operate on the actual unsaved DiskEditor buffer; they do not
// claim that a graphical selection widget or multi-document IDE is complete.
impl DiskEditor {
    /// Find forward from a valid cursor, wrapping to the document start.
    pub fn find_text_wrapping(&self, caller: Identity, needle: &str,
        from: usize) -> Result<Option<usize>, Error> {
        if let Some(at) = self.find_text(caller, needle, from)? {
            return Ok(Some(at));
        }
        self.find_text(caller, needle, 0)
    }

    /// Find backward before a valid cursor, wrapping to the final match.
    pub fn find_previous_text_wrapping(&self, caller: Identity,
        needle: &str, before: usize) -> Result<Option<usize>, Error> {
        if let Some(at) = self.find_previous_text(caller, needle, before)? {
            return Ok(Some(at));
        }
        self.find_previous_text(caller, needle, self.used)
    }

    /// Copy a UTF-8 selection into caller-provided bounded storage.
    /// Invalid ranges, owners, or short outputs leave output unchanged.
    pub fn copy_text(&self, caller: Identity, from: usize, to: usize,
        out: &mut [u8]) -> Result<usize, Error> {
        self.authorize(caller)?;
        let text = core::str::from_utf8(&self.data[..self.used])
            .map_err(|_| Error::Invalid)?;
        if from > to || to > self.used
            || !text.is_char_boundary(from) || !text.is_char_boundary(to) {
            return Err(Error::Invalid);
        }
        let len = to - from;
        if out.len() < len { return Err(Error::Full); }
        out[..len].copy_from_slice(&self.data[from..to]);
        Ok(len)
    }

    /// Atomically copy and remove a Unicode selection as one undoable edit.
    /// No source mutation occurs when the clipboard is undersized.
    pub fn cut_text(&mut self, caller: Identity, from: usize, to: usize,
        out: &mut [u8]) -> Result<usize, Error> {
        let len = self.copy_text(caller, from, to, out)?;
        if len > 0 { self.replace_text(caller, from, to, "")?; }
        Ok(len)
    }

    /// Insert clipboard UTF-8 at a valid cursor or replace a selection.
    /// The existing bounded editor transaction supplies undo/redo.
    pub fn paste_text(&mut self, caller: Identity, from: usize, to: usize,
        clipboard: &str) -> Result<usize, Error> {
        self.replace_text(caller, from, to, clipboard)
    }
}

#[cfg(test)]
mod r076_editor_navigation_tests {
    use super::*;
    use crate::block::{BlockError, SECTOR_BYTES};

    struct Disk { sectors: [[u8; SECTOR_BYTES]; 100] }
    impl BlockDevice for Disk {
        fn sector_count(&self) -> u64 { 100 }
        fn read_sector(&mut self, lba: u64, out: &mut [u8; SECTOR_BYTES])
            -> Result<(), BlockError> {
            *out = *self.sectors.get(lba as usize)
                .ok_or(BlockError::OutOfRange)?;
            Ok(())
        }
        fn write_sector(&mut self, lba: u64, bytes: &[u8; SECTOR_BYTES])
            -> Result<(), BlockError> {
            *self.sectors.get_mut(lba as usize)
                .ok_or(BlockError::OutOfRange)? = *bytes;
            Ok(())
        }
    }
    fn owner() -> Identity { Identity { pid: 26, generation: 1 } }
    fn recycled() -> Identity { Identity { pid: 26, generation: 2 } }
    fn ready() -> (HierVfs<Disk>, DiskEditor) {
        let mut fs = HierVfs::format_blank(Disk {
            sectors: [[0; SECTOR_BYTES]; 100],
        }).unwrap();
        fs.create("/src", "α😀\nalpha alpha\n".as_bytes()).unwrap();
        fs.create("/bin", &[0xff, 0xfe]).unwrap();
        let mut editor = DiskEditor::new();
        editor.open(owner(), &mut fs, "/src").unwrap();
        (fs, editor)
    }

    #[test]
    fn scalar_cursor_inverse_and_bidirectional_wrapped_search() {
        let (_fs, editor) = ready();
        let text = "α😀\nalpha alpha\n";
        for offset in 0..=text.len() {
            if text.is_char_boundary(offset) {
                let (line, column) = editor.text_position(owner(), offset).unwrap();
                assert_eq!(editor.text_offset(owner(), line, column), Ok(offset));
            } else {
                assert_eq!(editor.text_position(owner(), offset),
                    Err(Error::Invalid));
            }
        }
        let first = text.find("alpha").unwrap();
        let last = text.rfind("alpha").unwrap();
        assert_eq!(editor.find_previous_text(owner(), "alpha", last),
            Ok(Some(first)));
        assert_eq!(editor.find_previous_text(owner(), "alpha", first), Ok(None));
        assert_eq!(editor.find_previous_text_wrapping(owner(), "alpha", first),
            Ok(Some(last)));
        assert_eq!(editor.find_text_wrapping(owner(), "alpha", text.len()),
            Ok(Some(first)));
        assert_eq!(editor.find_previous_text(owner(), "", text.len()),
            Err(Error::Invalid));
        assert_eq!(editor.find_text_wrapping(owner(), "not-found", 0), Ok(None));
    }

    #[test]
    fn selection_copy_cut_paste_undo_and_real_remount() {
        let (mut fs, mut editor) = ready();
        let mut clipboard = [0xabu8; 32];
        let selected = "α😀".len();
        assert_eq!(editor.copy_text(owner(), 0, selected, &mut clipboard),
            Ok(selected));
        assert_eq!(&clipboard[..selected], "α😀".as_bytes());
        assert_eq!(editor.cut_text(owner(), 0, selected, &mut clipboard),
            Ok(selected));
        assert_eq!(editor.undo(owner()), Ok("α😀\nalpha alpha\n".len()));
        assert_eq!(editor.redo(owner()), Ok("\nalpha alpha\n".len()));
        assert_eq!(editor.paste_text(owner(), 0, 0,
            core::str::from_utf8(&clipboard[..selected]).unwrap()),
            Ok("α😀\nalpha alpha\n".len()));
        editor.save(owner(), &mut fs).unwrap();
        let mut fs = HierVfs::mount(fs.into_device()).unwrap();
        let mut bytes = [0u8; 64];
        let len = fs.read("/src", &mut bytes).unwrap();
        assert_eq!(&bytes[..len], "α😀\nalpha alpha\n".as_bytes());
    }

    #[test]
    fn denied_identity_invalid_boundaries_and_small_clipboard_are_atomic() {
        let (mut fs, mut editor) = ready();
        let mut clipboard = [0x5au8; 3];
        assert_eq!(editor.cut_text(owner(), 0, "α😀".len(), &mut clipboard),
            Err(Error::Full));
        assert_eq!(clipboard, [0x5a; 3]);
        assert_eq!(editor.copy_text(recycled(), 0, 1, &mut clipboard),
            Err(Error::Unauthorized));
        assert_eq!(editor.copy_text(owner(), 1, 2, &mut clipboard),
            Err(Error::Invalid));
        assert_eq!(editor.find_previous_text(owner(), "a", 1),
            Err(Error::Invalid));
        assert_eq!(editor.paste_text(recycled(), 0, 0, "evil"),
            Err(Error::Unauthorized));
        assert!(!editor.status(owner()).unwrap().dirty);
        editor.close(owner(), false).unwrap();
        editor.open(owner(), &mut fs, "/bin").unwrap();
        assert_eq!(editor.text_position(owner(), 0), Err(Error::Invalid));
    }
}


// R076: two actual concurrent disk-backed source documents.
// Every operation is owner-generation checked by its contained DiskEditor.
// No heap allocation and no assumption that different source files have
// independent VFS generation counters.
pub const EDITOR_POOL_SLOTS: usize = 2;
pub struct EditorPool {
    docs: [DiskEditor; EDITOR_POOL_SLOTS],
}
impl EditorPool {
    pub const fn new() -> Self {
        Self { docs: [const { DiskEditor::new() }; EDITOR_POOL_SLOTS] }
    }
    pub fn open<D: BlockDevice>(&mut self, who: Identity,
        fs: &mut HierVfs<D>, slot: usize, path: &str)
        -> Result<Status, Error> {
        let doc = self.docs.get_mut(slot).ok_or(Error::Invalid)?;
        doc.open(who, fs, path)
    }
    pub fn read(&self, who: Identity, slot: usize, out: &mut [u8])
        -> Result<usize, Error> {
        self.docs.get(slot).ok_or(Error::Invalid)?.read(who, out)
    }
    pub fn replace_text(&mut self, who: Identity, slot: usize,
        from: usize, to: usize, text: &str) -> Result<usize, Error> {
        self.docs.get_mut(slot).ok_or(Error::Invalid)?
            .replace_text(who, from, to, text)
    }
    pub fn close(&mut self, who: Identity, slot: usize, discard: bool)
        -> Result<(), Error> {
        self.docs.get_mut(slot).ok_or(Error::Invalid)?.close(who, discard)
    }
    /// Save every owned open source buffer and return one result per slot.
    /// Reject stale/unauthorized/duplicate-path batches *before any writes*.
    /// The COW VFS is transaction-atomic per file, not across the entire batch.
    /// The exclusive &mut VFS permits refreshing peer generation only for
    /// writes performed within this very save-all batch.
    pub fn save_all<D: BlockDevice>(&mut self, who: Identity,
        fs: &mut HierVfs<D>)
        -> Result<[Option<Result<u64, Error>>; EDITOR_POOL_SLOTS], Error> {
        if who.pid == 0 || who.generation == 0 { return Err(Error::Unauthorized); }
        let original = fs.generation();
        for i in 0..EDITOR_POOL_SLOTS {
            let doc = &self.docs[i];
            if doc.owner.is_some() && doc.owner != Some(who) {
                return Err(Error::Unauthorized);
            }
            if doc.owner.is_some() && doc.dirty && doc.baseline != original {
                return Err(Error::Stale);
            }
            if doc.owner == Some(who) && doc.dirty {
                WriteGuard::allow_path(doc.pathname()).map_err(|_|Error::Unauthorized)?;
            }
            if doc.owner.is_some() {
                for peer in &self.docs[..i] {
                    if peer.owner.is_some() && peer.pathname() == doc.pathname() {
                        return Err(Error::Invalid);
                    }
                }
            }
        }
        let mut results: [Option<Result<u64,Error>>; EDITOR_POOL_SLOTS] =
            [None; EDITOR_POOL_SLOTS];
        for (i, doc) in self.docs.iter_mut().enumerate() {
            if doc.owner == Some(who) {
                if doc.dirty && doc.baseline == original {
                    doc.baseline = fs.generation();
                }
                results[i] = Some(doc.save(who,fs));
            }
        }
        Ok(results)
    }
}

#[cfg(test)]
mod r076_multi_document_editor_tests {
    use super::*;
    use crate::block::{BlockError, SECTOR_BYTES};
    struct Disk { sectors: [[u8; SECTOR_BYTES]; 100] }
    impl BlockDevice for Disk {
        fn sector_count(&self) -> u64 { 100 }
        fn read_sector(&mut self, lba: u64, out: &mut [u8; SECTOR_BYTES])
            -> Result<(), BlockError> {
            *out = *self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?;
            Ok(())
        }
        fn write_sector(&mut self, lba: u64, data: &[u8; SECTOR_BYTES])
            -> Result<(), BlockError> {
            *self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)? = *data;
            Ok(())
        }
    }
    fn owner() -> Identity { Identity { pid: 30, generation: 1 } }
    fn recycled() -> Identity { Identity { pid: 30, generation: 2 } }
    fn setup() -> (HierVfs<Disk>, EditorPool) {
        let mut fs = HierVfs::format_blank(Disk { sectors: [[0; SECTOR_BYTES];100] }).unwrap();
        fs.create("/one",b"hello").unwrap();
        fs.create("/two","world β".as_bytes()).unwrap();
        let mut pool = EditorPool::new();
        pool.open(owner(),&mut fs,0,"/one").unwrap();
        pool.open(owner(),&mut fs,1,"/two").unwrap();
        (fs,pool)
    }
    #[test]
    fn two_dirty_documents_save_without_self_staleness_and_survive_remount() {
        let (mut fs, mut pool) = setup();
        pool.replace_text(owner(),0,5,5,"!").unwrap();
        pool.replace_text(owner(),1,0,0,"α ").unwrap();
        let saved = pool.save_all(owner(),&mut fs).unwrap();
        assert!(matches!(saved[0],Some(Ok(_))));
        assert!(matches!(saved[1],Some(Ok(_))));
        let mut fs = HierVfs::mount(fs.into_device()).unwrap();
        let mut a = [0u8;20]; let mut b = [0u8;20];
        assert_eq!(fs.read("/one",&mut a),Ok(6));
        assert_eq!(&a[..6],b"hello!");
        let expected="α world β";
        assert_eq!(fs.read("/two",&mut b),Ok(expected.len()));
        assert_eq!(&b[..expected.len()],expected.as_bytes());
    }
    #[test]
    fn recycled_owner_stale_external_writer_and_duplicate_paths_fail_closed() {
        let (mut fs, mut pool) = setup();
        assert_eq!(pool.replace_text(recycled(),0,0,1,"bad"),Err(Error::Unauthorized));
        assert_eq!(pool.open(owner(),&mut fs,2,"/one"),Err(Error::Invalid));
        pool.replace_text(owner(),0,0,1,"H").unwrap();
        fs.overwrite("/one",b"external").unwrap();
        assert_eq!(pool.save_all(owner(),&mut fs),Err(Error::Stale));
        let mut out = [0u8;20];
        assert_eq!(fs.read("/one",&mut out),Ok(8));
        assert_eq!(&out[..8],b"external");
        let (mut fs, mut again) = setup();
        again.close(owner(),1,false).unwrap();
        again.open(owner(),&mut fs,1,"/one").unwrap();
        assert_eq!(again.save_all(owner(),&mut fs),Err(Error::Invalid));
    }
}


// R077 workstream A: make the existing two-document editor usable for
// independent, authenticated undo/redo and status inspection.
impl EditorPool {
    pub fn status(&self, who: Identity, slot: usize) -> Result<Status, Error> {
        self.docs.get(slot).ok_or(Error::Invalid)?.status(who)
    }
    pub fn undo(&mut self, who: Identity, slot: usize) -> Result<usize, Error> {
        self.docs.get_mut(slot).ok_or(Error::Invalid)?.undo(who)
    }
    pub fn redo(&mut self, who: Identity, slot: usize) -> Result<usize, Error> {
        self.docs.get_mut(slot).ok_or(Error::Invalid)?.redo(who)
    }
}

#[cfg(test)]
mod r077_editor_pool_tests {
    use super::*;
    use crate::block::{BlockError, SECTOR_BYTES};
    struct Disk { sectors: [[u8; SECTOR_BYTES]; 100] }
    impl BlockDevice for Disk {
        fn sector_count(&self) -> u64 { 100 }
        fn read_sector(&mut self, lba: u64, out: &mut [u8; SECTOR_BYTES])
            -> Result<(), BlockError> {
            *out = *self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?;
            Ok(())
        }
        fn write_sector(&mut self, lba: u64, bytes: &[u8; SECTOR_BYTES])
            -> Result<(), BlockError> {
            *self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)? = *bytes;
            Ok(())
        }
    }
    fn owner() -> Identity { Identity { pid: 35, generation: 1 } }
    fn recycled() -> Identity { Identity { pid: 35, generation: 2 } }

    #[test]
    fn undo_redo_are_independent_across_two_real_documents() {
        let mut fs = HierVfs::format_blank(Disk { sectors: [[0; SECTOR_BYTES]; 100] }).unwrap();
        fs.create("/one", b"one").unwrap();
        fs.create("/two", b"two").unwrap();
        let mut pool = EditorPool::new();
        pool.open(owner(), &mut fs, 0, "/one").unwrap();
        pool.open(owner(), &mut fs, 1, "/two").unwrap();
        pool.replace_text(owner(), 0, 3, 3, "1").unwrap();
        pool.replace_text(owner(), 1, 3, 3, "2").unwrap();
        assert_eq!(pool.undo(owner(), 0), Ok(3));
        assert_eq!(pool.status(owner(), 1).unwrap().bytes, 4);
        assert_eq!(pool.redo(owner(), 0), Ok(4));
        assert_eq!(pool.undo(owner(), 1), Ok(3));
        pool.save_all(owner(), &mut fs).unwrap();
        let mut mounted = HierVfs::mount(fs.into_device()).unwrap();
        let mut bytes = [0u8; 8];
        assert_eq!(mounted.read("/one", &mut bytes), Ok(4));
        assert_eq!(&bytes[..4], b"one1");
        assert_eq!(mounted.read("/two", &mut bytes), Ok(3));
        assert_eq!(&bytes[..3], b"two");
    }

    #[test]
    fn rejected_slot_and_recycled_identity_never_change_a_document() {
        let mut fs = HierVfs::format_blank(Disk { sectors: [[0; SECTOR_BYTES]; 100] }).unwrap();
        fs.create("/one", b"base").unwrap();
        let mut pool = EditorPool::new();
        pool.open(owner(), &mut fs, 0, "/one").unwrap();
        pool.replace_text(owner(), 0, 4, 4, "!").unwrap();
        assert_eq!(pool.undo(recycled(), 0), Err(Error::Unauthorized));
        assert_eq!(pool.redo(recycled(), 0), Err(Error::Unauthorized));
        assert_eq!(pool.status(owner(), 2), Err(Error::Invalid));
        let mut bytes = [0u8; 8];
        assert_eq!(pool.read(owner(), 0, &mut bytes), Ok(5));
        assert_eq!(&bytes[..5], b"base!");
    }
}


// M1/R078 workstream 1 (tasks 01-04): real multi-document source operations.
impl EditorPool {
    /// Search only the named open document; never leak a peer document's bytes.
    pub fn find_text(&self, caller:Identity, slot:usize, needle:&str, from:usize)
        ->Result<Option<usize>,Error>{
        self.docs.get(slot).ok_or(Error::Invalid)?.find_text(caller,needle,from)
    }
    /// Unicode selection copied from one authenticated editor session.
    pub fn copy_text(&self, caller:Identity, slot:usize, from:usize,to:usize,
        out:&mut[u8])->Result<usize,Error>{
        self.docs.get(slot).ok_or(Error::Invalid)?
            .copy_text_selection(caller,from,to,out)
    }
    /// A selection cut modifies only its owning slot and is undoable.
    pub fn cut_text(&mut self, caller:Identity, slot:usize, from:usize,to:usize,
        out:&mut[u8])->Result<usize,Error>{
        self.docs.get_mut(slot).ok_or(Error::Invalid)?
            .cut_text_selection(caller,from,to,out)
    }
    /// Clipboard data comes from an authorized caller; no global clipboard.
    pub fn paste_text(&mut self, caller:Identity, slot:usize, from:usize,to:usize,
        data:&str)->Result<usize,Error>{
        self.docs.get_mut(slot).ok_or(Error::Invalid)?
            .paste_text_selection(caller,from,to,data)
    }
}

#[cfg(test)]
mod r078_editor_pool_source_tests {
    use super::*;
    use crate::block::{BlockError,SECTOR_BYTES};
    struct Disk{sectors:[[u8;SECTOR_BYTES];100]}
    impl BlockDevice for Disk {
        fn sector_count(&self)->u64{100}
        fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
            ->Result<(),BlockError>{
            *out=*self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?;
            Ok(())
        }
        fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])
            ->Result<(),BlockError>{
            *self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*data;
            Ok(())
        }
    }
    fn id()->Identity{Identity{pid:42,generation:3}}
    fn setup()->(HierVfs<Disk>,EditorPool){
        let mut fs=HierVfs::format_blank(Disk{sectors:[[0;SECTOR_BYTES];100]}).unwrap();
        fs.create("/one","α hello".as_bytes()).unwrap();
        fs.create("/two",b"second").unwrap();
        let mut pool=EditorPool::new();
        pool.open(id(),&mut fs,0,"/one").unwrap();
        pool.open(id(),&mut fs,1,"/two").unwrap();
        (fs,pool)
    }
    #[test]
    fn owned_search_copy_cut_paste_and_undo_are_slot_isolated_and_durable(){
        let (mut fs,mut pool)=setup();
        assert_eq!(pool.find_text(id(),0,"hello",0),Ok(Some(3)));
        assert_eq!(pool.find_text(id(),1,"hello",0),Ok(None));
        let mut clip=[0u8;16];
        assert_eq!(pool.copy_text(id(),0,0,2,&mut clip),Ok(2));
        assert_eq!(&clip[..2],"α".as_bytes());
        assert_eq!(pool.cut_text(id(),0,0,2,&mut clip),Ok(2));
        assert_eq!(pool.undo(id(),0),Ok("α hello".len()));
        assert_eq!(pool.paste_text(id(),1,6,6,"!"),Ok(7));
        pool.save_all(id(),&mut fs).unwrap();
        let mut mounted=HierVfs::mount(fs.into_device()).unwrap();
        let mut output=[0u8;16];
        assert_eq!(mounted.read("/one",&mut output),Ok("α hello".len()));
        assert_eq!(&output[.."α hello".len()],"α hello".as_bytes());
        assert_eq!(mounted.read("/two",&mut output),Ok(7));
        assert_eq!(&output[..7],b"second!");
    }
    #[test]
    fn invalid_unicode_short_clip_wrong_owner_and_bad_slots_leave_buffers_clean(){
        let (_fs,mut pool)=setup();
        let mut clip=[0x55u8;1];
        assert_eq!(pool.copy_text(id(),0,0,2,&mut clip),Err(Error::Full));
        assert_eq!(pool.cut_text(id(),0,1,2,&mut clip),Err(Error::Invalid));
        assert_eq!(pool.paste_text(id(),0,1,2,"X"),Err(Error::Invalid));
        assert_eq!(pool.find_text(id(),2,"a",0),Err(Error::Invalid));
        assert_eq!(pool.copy_text(Identity{generation:4,..id()},1,0,1,&mut clip),
            Err(Error::Unauthorized));
        assert_eq!(clip,[0x55]);
        assert!(!pool.status(id(),0).unwrap().dirty);
        assert!(!pool.status(id(),1).unwrap().dirty);
    }
}


// M1/R079 workstation 1: four independently authorized multi-buffer
// operations over actual active source buffers, never stale disk snapshots.
impl EditorPool {
    /// Replace every literal Unicode match within a single selected slot,
    /// producing exactly one reversible unsaved edit.
    pub fn replace_all_text(&mut self,who:Identity,slot:usize,
        needle:&str,replacement:&str)->Result<usize,Error>{
        self.docs.get_mut(slot).ok_or(Error::Invalid)?
            .replace_all_text(who,needle,replacement)
    }
    /// Previous match bounded to the selected document and a UTF-8 cursor.
    pub fn find_previous_text(&self,who:Identity,slot:usize,
        needle:&str,before:usize)->Result<Option<usize>,Error>{
        self.docs.get(slot).ok_or(Error::Invalid)?
            .find_previous_text(who,needle,before)
    }
    /// Stream a bounded page of nonoverlapping source match byte offsets.
    pub fn find_text_page(&self,who:Identity,slot:usize,needle:&str,
        from:usize,out:&mut[usize])->Result<(usize,Option<usize>),Error>{
        self.docs.get(slot).ok_or(Error::Invalid)?
            .find_text_page(who,needle,from,out)
    }
    /// Convert an active document's byte offset into one-based Unicode
    /// line/scalar-column coordinates without copying source bytes.
    pub fn text_position(&self,who:Identity,slot:usize,at:usize)
        ->Result<(usize,usize),Error>{
        self.docs.get(slot).ok_or(Error::Invalid)?.text_position(who,at)
    }
}

#[cfg(test)]
mod r079_editor_multibuffer_tests {
    use super::*;
    use crate::block::{BlockError,SECTOR_BYTES};
    struct Disk{sectors:[[u8;SECTOR_BYTES];100]}
    impl BlockDevice for Disk{
        fn sector_count(&self)->u64{100}
        fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
            ->Result<(),BlockError>{
            *out=*self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?;
            Ok(())
        }
        fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])
            ->Result<(),BlockError>{
            *self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*data;Ok(())
        }
    }
    fn who()->Identity{Identity{pid:60,generation:1}}
    fn setup()->(HierVfs<Disk>,EditorPool){
        let mut fs=HierVfs::format_blank(Disk{sectors:[[0;SECTOR_BYTES];100]}).unwrap();
        fs.create("/one","α cat cat\n".as_bytes()).unwrap();
        fs.create("/two",b"dog dog").unwrap();
        let mut p=EditorPool::new();
        p.open(who(),&mut fs,0,"/one").unwrap();
        p.open(who(),&mut fs,1,"/two").unwrap();
        (fs,p)
    }
    #[test]
    fn replacements_pages_positions_and_previous_search_use_own_slot_and_remount(){
        let (mut fs,mut p)=setup();
        assert_eq!(p.text_position(who(),0,3),Ok((1,3)));
        assert_eq!(p.find_previous_text(who(),0,"cat",10),Ok(Some(7)));
        let mut matches=[usize::MAX;1];
        assert_eq!(p.find_text_page(who(),0,"cat",0,&mut matches),Ok((1,Some(7))));
        assert_eq!(matches,[3]);
        assert_eq!(p.find_text_page(who(),0,"cat",7,&mut matches),Ok((1,None)));
        assert_eq!(matches,[7]);
        assert_eq!(p.replace_all_text(who(),0,"cat","猫"),Ok(2));
        assert_eq!(p.replace_all_text(who(),1,"dog","fox"),Ok(2));
        assert_eq!(p.undo(who(),1),Ok(7));
        p.save_all(who(),&mut fs).unwrap();
        let mut fs=HierVfs::mount(fs.into_device()).unwrap();
        let mut out=[0u8;32];
        let n=fs.read("/one",&mut out).unwrap();
        assert_eq!(&out[..n],"α 猫 猫\n".as_bytes());
        assert_eq!(fs.read("/two",&mut out),Ok(7));
        assert_eq!(&out[..7],b"dog dog");
    }
    #[test]
    fn stale_owner_invalid_boundaries_and_short_page_keep_data_and_results(){
        let (_fs,mut p)=setup();
        let generation=Identity{generation:2,..who()};
        let mut matched=[77usize;0];
        assert_eq!(p.find_text_page(who(),0,"cat",0,&mut matched),Err(Error::Full));
        assert_eq!(p.find_previous_text(who(),0,"cat",1),Err(Error::Invalid));
        assert_eq!(p.text_position(who(),0,1),Err(Error::Invalid));
        assert_eq!(p.replace_all_text(generation,0,"cat","oops"),Err(Error::Unauthorized));
        assert_eq!(p.replace_all_text(who(),2,"cat","oops"),Err(Error::Invalid));
        assert!(!p.status(who(),0).unwrap().dirty);
        assert_eq!(matched,[]);
    }
}


// R080 workstream A: active multi-document source reconciliation.
// These operations work on authenticated in-memory edits, never only an
// outdated disk view. Durable changes still require explicit save_all().
impl EditorPool {
    /// First changed byte across two open, valid UTF-8 source buffers.
    pub fn compare_open_text(&self,who:Identity,left:usize,right:usize)
        ->Result<Option<usize>,Error>{
        let a=self.docs.get(left).ok_or(Error::Invalid)?;
        let b=self.docs.get(right).ok_or(Error::Invalid)?;
        a.authorize(who)?;b.authorize(who)?;
        core::str::from_utf8(&a.data[..a.used]).map_err(|_|Error::Invalid)?;
        core::str::from_utf8(&b.data[..b.used]).map_err(|_|Error::Invalid)?;
        let common=core::cmp::min(a.used,b.used);
        for i in 0..common{
            if a.data[i]!=b.data[i]{return Ok(Some(i));}
        }
        Ok(if a.used==b.used{None}else{Some(common)})
    }
    /// Copy selected Unicode text from one document into another as ONE
    /// undoable destination edit. Source bytes and history stay unchanged.
    pub fn transfer_selection(&mut self,who:Identity,source:usize,
        from:usize,to:usize,target:usize,target_from:usize,target_to:usize)
        ->Result<usize,Error>{
        if source==target{return Err(Error::Invalid);}
        let mut scratch=[0u8;MAX_FILE_BYTES];
        let len=self.docs.get(source).ok_or(Error::Invalid)?
            .copy_text_selection(who,from,to,&mut scratch)?;
        let dest=self.docs.get_mut(target).ok_or(Error::Invalid)?;
        dest.authorize(who)?;
        let slice=core::str::from_utf8(&scratch[..len]).map_err(|_|Error::Invalid)?;
        dest.paste_text_selection(who,target_from,target_to,slice)
    }
    /// Refresh one document from verified durable bytes, requiring explicit
    /// discard if it has unsaved edits. Peer documents are untouched.
    pub fn reload_slot<D:BlockDevice>(&mut self,who:Identity,
        fs:&mut HierVfs<D>,slot:usize,discard:bool)->Result<Status,Error>{
        let doc=self.docs.get_mut(slot).ok_or(Error::Invalid)?;
        doc.authorize(who)?;
        if doc.dirty&&!discard{return Err(Error::Dirty);}
        doc.reload(who,fs)
    }
    /// Close all owned documents only after *every* slot passes authority
    /// and dirty-buffer checks, avoiding a half-closed workspace.
    pub fn close_all(&mut self,who:Identity,discard:bool)->Result<usize,Error>{
        let mut active=0;
        for doc in &self.docs{
            if doc.owner.is_some(){
                doc.authorize(who)?;
                if doc.dirty&&!discard{return Err(Error::Dirty);}
                active+=1;
            }
        }
        for doc in &mut self.docs{
            if doc.owner.is_some(){doc.close(who,discard)?;}
        }
        Ok(active)
    }
}

#[cfg(test)]
mod r080_editor_reconciliation_tests {
    use super::*;
    use crate::block::{BlockError,SECTOR_BYTES};
    struct Disk{sectors:[[u8;SECTOR_BYTES];100]}
    impl BlockDevice for Disk{
        fn sector_count(&self)->u64{100}
        fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
            *out=*self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
        }
        fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
            *self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*data;Ok(())
        }
    }
    fn id()->Identity{Identity{pid:71,generation:1}}
    fn setup()->(HierVfs<Disk>,EditorPool){
        let mut fs=HierVfs::format_blank(Disk{sectors:[[0;SECTOR_BYTES];100]}).unwrap();
        fs.create("/first","α text".as_bytes()).unwrap();
        fs.create("/second",b"notes").unwrap();
        let mut p=EditorPool::new();
        p.open(id(),&mut fs,0,"/first").unwrap();
        p.open(id(),&mut fs,1,"/second").unwrap();
        (fs,p)
    }
    #[test]
    fn compare_transfer_and_reload_real_unicode_source_survive_remount(){
        let (mut fs,mut pool)=setup();
        assert_eq!(pool.compare_open_text(id(),0,1),Ok(Some(0)));
        assert_eq!(pool.transfer_selection(id(),0,0,2,1,5,5),Ok(7));
        assert_eq!(pool.compare_open_text(id(),0,1),Ok(Some(0)));
        pool.save_all(id(),&mut fs).unwrap();
        let mut mounted=HierVfs::mount(fs.into_device()).unwrap();
        let mut buf=[0u8;20];
        assert_eq!(mounted.read("/second",&mut buf),Ok(7));
        assert_eq!(&buf[..7],"notesα".as_bytes());
        // The existing in-memory session may explicitly refresh after reboot.
        assert_eq!(pool.reload_slot(id(),&mut mounted,1,false).unwrap().bytes,7);
        assert_eq!(pool.close_all(id(),false),Ok(2));
    }
    #[test]
    fn rejected_transfer_wrong_pid_dirty_close_and_reload_leave_peers_unchanged(){
        let (mut fs,mut pool)=setup();
        assert_eq!(pool.transfer_selection(id(),0,1,2,1,0,0),Err(Error::Invalid));
        assert_eq!(pool.transfer_selection(id(),0,0,2,0,0,0),Err(Error::Invalid));
        assert_eq!(pool.transfer_selection(
            Identity{generation:2,..id()},0,0,2,1,0,0),Err(Error::Unauthorized));
        pool.replace_text(id(),1,0,0,"!").unwrap();
        assert_eq!(pool.reload_slot(id(),&mut fs,1,false),Err(Error::Dirty));
        assert_eq!(pool.close_all(id(),false),Err(Error::Dirty));
        assert_eq!(pool.status(id(),0).unwrap().bytes,"α text".len());
        assert_eq!(pool.status(id(),1).unwrap().bytes,6);
        assert_eq!(pool.reload_slot(id(),&mut fs,1,true).unwrap().bytes,5);
    }
}


// M1/R081 workstream A: six real unsaved multi-document operations.
// All edits remain owner-bound and use the same single-step undo machinery.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct PoolTextMetrics{
    pub bytes:usize,pub lines:usize,pub scalars:usize,pub fnv64:u64,pub dirty:bool,
}
impl EditorPool{
    /// Inspect the current unsaved UTF-8 buffer, not an outdated disk copy.
    pub fn text_metrics(&self,who:Identity,slot:usize)->Result<PoolTextMetrics,Error>{
        let doc=self.docs.get(slot).ok_or(Error::Invalid)?;
        doc.authorize(who)?;
        let text=core::str::from_utf8(&doc.data[..doc.used]).map_err(|_|Error::Invalid)?;
        let hash=doc.data[..doc.used].iter().fold(0xcbf29ce484222325u64,
            |h,b|(h^(*b as u64)).wrapping_mul(0x100000001b3));
        Ok(PoolTextMetrics{bytes:doc.used,lines:if doc.used==0{0}else{
            text.split_inclusive('\n').count()},scalars:text.chars().count(),
            fnv64:hash,dirty:doc.dirty})
    }
    /// Search the two open slots from a selected starting slot and byte
    /// cursor; later slots begin at offset zero.
    pub fn find_across_slots(&self,who:Identity,needle:&str,
        start_slot:usize,from:usize)->Result<Option<(usize,usize)>,Error>{
        if start_slot>=EDITOR_POOL_SLOTS{return Err(Error::Invalid);}
        for i in start_slot..EDITOR_POOL_SLOTS{
            let doc=&self.docs[i];
            if let Some(at)=doc.find_text(who,needle,if i==start_slot{from}else{0})?{
                return Ok(Some((i,at)));
            }
        }
        Ok(None)
    }
    /// Replace a Unicode selection only if its visible unsaved text exactly
    /// matches an expected literal; rejected edits do not clear redo history.
    pub fn replace_selection_if_equal(&mut self,who:Identity,slot:usize,
        from:usize,to:usize,expected:&str,new_text:&str)->Result<usize,Error>{
        let doc=self.docs.get_mut(slot).ok_or(Error::Invalid)?;
        doc.authorize(who)?;
        let text=core::str::from_utf8(&doc.data[..doc.used]).map_err(|_|Error::Invalid)?;
        if text.get(from..to)!=Some(expected){return Err(Error::Stale);}
        doc.replace_text(who,from,to,new_text)
    }
    /// Insert one whole source row from a different editor slot as a single
    /// undoable target edit. Selection preserves UTF-8 and its newline.
    pub fn insert_line_from_slot(&mut self,who:Identity,
        source:usize,one_based_line:usize,target:usize,at:usize)
        ->Result<usize,Error>{
        if one_based_line==0||source==target{return Err(Error::Invalid);}
        let doc=self.docs.get(source).ok_or(Error::Invalid)?;
        doc.authorize(who)?;
        let text=core::str::from_utf8(&doc.data[..doc.used]).map_err(|_|Error::Invalid)?;
        let row=text.split_inclusive('\n').nth(one_based_line-1)
            .ok_or(Error::Invalid)?;
        let row_len=row.len();
        let mut temporary=[0u8;MAX_FILE_BYTES];
        temporary[..row_len].copy_from_slice(row.as_bytes());
        let other=self.docs.get_mut(target).ok_or(Error::Invalid)?;
        other.replace_text(who,at,at,
            core::str::from_utf8(&temporary[..row_len]).map_err(|_|Error::Invalid)?)
    }
    /// Mirror an active UTF-8 source into another slot only if the target has
    /// the exact revision the UI previously observed.
    pub fn mirror_slot_if_checksum(&mut self,who:Identity,source:usize,
        target:usize,expected:u64)->Result<usize,Error>{
        if source==target{return Err(Error::Invalid);}
        let snapshot=self.text_metrics(who,target)?;
        if snapshot.fnv64!=expected{return Err(Error::Stale);}
        let doc=self.docs.get(source).ok_or(Error::Invalid)?;
        doc.authorize(who)?;
        let text=core::str::from_utf8(&doc.data[..doc.used]).map_err(|_|Error::Invalid)?;
        let text_len=text.len();
        let mut staged=[0u8;MAX_FILE_BYTES];
        staged[..text_len].copy_from_slice(text.as_bytes());
        let other=self.docs.get_mut(target).ok_or(Error::Invalid)?;
        let original=other.used;
        other.replace_text(who,0,original,
            core::str::from_utf8(&staged[..text_len]).map_err(|_|Error::Invalid)?)
    }
    /// Undo all edited open documents together; validate all required undo
    /// snapshots and identities before changing any buffer.
    pub fn undo_all_ready(&mut self,who:Identity)->Result<usize,Error>{
        let mut count=0;
        for doc in &self.docs{
            if doc.owner.is_some(){
                doc.authorize(who)?;
                if doc.undo.is_none(){return Err(Error::Invalid);}
                count+=1;
            }
        }
        if count==0{return Err(Error::Invalid);}
        for doc in &mut self.docs{
            if doc.owner.is_some(){doc.undo(who)?;}
        }
        Ok(count)
    }
}

#[cfg(test)]
mod r081_pool_source_tests{
    use super::*;
    use crate::block::{BlockError,SECTOR_BYTES};
    struct Disk{sectors:[[u8;SECTOR_BYTES];100]}
    impl BlockDevice for Disk{
        fn sector_count(&self)->u64{100}
        fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
            *out=*self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
        }
        fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
            *self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*data;Ok(())
        }
    }
    fn id()->Identity{Identity{pid:81,generation:1}}
    fn setup()->(HierVfs<Disk>,EditorPool){
        let mut fs=HierVfs::format_blank(Disk{sectors:[[0;SECTOR_BYTES];100]}).unwrap();
        fs.create("/one","α one\nsecond\n".as_bytes()).unwrap();
        fs.create("/two",b"second").unwrap();
        let mut pool=EditorPool::new();
        pool.open(id(),&mut fs,0,"/one").unwrap();
        pool.open(id(),&mut fs,1,"/two").unwrap();
        (fs,pool)
    }
    #[test]
    fn six_live_source_features_are_unicode_and_owner_safe(){
        let (mut fs,mut pool)=setup();
        let before=pool.text_metrics(id(),1).unwrap();
        assert_eq!(before.bytes,6);
        assert_eq!(pool.find_across_slots(id(),"second",0,0),Ok(Some((0,7))));
        assert_eq!(pool.find_across_slots(id(),"second",0,13),Ok(Some((1,0))));
        assert_eq!(pool.replace_selection_if_equal(id(),0,3,6,"one","first"),Ok(16));
        assert_eq!(pool.insert_line_from_slot(id(),0,1,1,0),Ok(15));
        let digest=pool.text_metrics(id(),1).unwrap().fnv64;
        assert_eq!(pool.mirror_slot_if_checksum(id(),0,1,digest),Ok(16));
        assert_eq!(pool.undo_all_ready(id()),Ok(2));
        let mut out=[0u8;32];
        assert_eq!(pool.read(id(),0,&mut out),Ok(14));
        assert_eq!(&out[..14],"α one\nsecond\n".as_bytes());
        assert_eq!(pool.read(id(),1,&mut out),Ok(15));
        assert_eq!(&out[..15],"α first\nsecond".as_bytes());
        pool.save_all(id(),&mut fs).unwrap();
        let mut fs=HierVfs::mount(fs.into_device()).unwrap();
        assert_eq!(fs.read("/one",&mut out),Ok(14));
    }
    #[test]
    fn all_or_nothing_undo_rejects_missing_history_and_wrong_owner(){
        let (_fs,mut pool)=setup();
        pool.replace_text(id(),0,0,0,"!").unwrap();
        assert_eq!(pool.undo_all_ready(id()),Err(Error::Invalid));
        assert_eq!(pool.replace_selection_if_equal(id(),1,0,2,"bad","x"),Err(Error::Stale));
        assert_eq!(pool.insert_line_from_slot(id(),0,0,1,0),Err(Error::Invalid));
        assert_eq!(pool.mirror_slot_if_checksum(id(),0,1,0),Err(Error::Stale));
        assert_eq!(pool.text_metrics(Identity{generation:2,..id()},0),Err(Error::Unauthorized));
        assert_eq!(pool.status(id(),0).unwrap().bytes,15);
        assert!(!pool.status(id(),1).unwrap().dirty);
    }
}


// M1/R082 stream A — ten owner-bound active source-buffer functions.
// These are live unsaved buffers; durable writes still use save_all().
impl EditorPool {
    /// Count nonoverlapping Unicode literals within the authenticated slot.
    pub fn count_literal(&self,who:Identity,slot:usize,needle:&str)->Result<usize,Error>{
        if needle.is_empty(){return Err(Error::Invalid);}
        let d=self.docs.get(slot).ok_or(Error::Invalid)?;d.authorize(who)?;
        let text=core::str::from_utf8(&d.data[..d.used]).map_err(|_|Error::Invalid)?;
        Ok(text.match_indices(needle).count())
    }
    /// Find last Unicode source match at a complete character boundary.
    pub fn last_literal(&self,who:Identity,slot:usize,needle:&str)
        ->Result<Option<usize>,Error>{
        let d=self.docs.get(slot).ok_or(Error::Invalid)?;
        d.find_previous_text(who,needle,d.used)
    }
    /// Read one complete 1-based source row with a short-buffer preflight.
    pub fn read_source_row(&self,who:Identity,slot:usize,row:usize,
        out:&mut[u8])->Result<usize,Error>{
        let d=self.docs.get(slot).ok_or(Error::Invalid)?;d.authorize(who)?;
        if row==0{return Err(Error::Invalid);}
        let text=core::str::from_utf8(&d.data[..d.used]).map_err(|_|Error::Invalid)?;
        let line=text.split_inclusive('\n').nth(row-1).ok_or(Error::Invalid)?;
        if out.len()<line.len(){return Err(Error::Full);}
        out[..line.len()].copy_from_slice(line.as_bytes());Ok(line.len())
    }
    /// Append source text to one currently open buffer as a reversible edit.
    pub fn append_source_text(&mut self,who:Identity,slot:usize,source:&str)
        ->Result<usize,Error>{
        let d=self.docs.get_mut(slot).ok_or(Error::Invalid)?;
        d.replace_text(who,d.used,d.used,source)
    }
    /// Prepend source text without affecting another document's undo state.
    pub fn prepend_source_text(&mut self,who:Identity,slot:usize,source:&str)
        ->Result<usize,Error>{
        self.docs.get_mut(slot).ok_or(Error::Invalid)?
            .replace_text(who,0,0,source)
    }
    /// Replace precisely the last occurrence, not every identical symbol.
    pub fn replace_last_literal(&mut self,who:Identity,slot:usize,
        old:&str,new:&str)->Result<Option<usize>,Error>{
        let d=self.docs.get_mut(slot).ok_or(Error::Invalid)?;
        if old.is_empty(){return Err(Error::Invalid);}
        let Some(pos)=d.find_previous_text(who,old,d.used)? else{return Ok(None)};
        d.replace_text(who,pos,pos+old.len(),new)?;Ok(Some(pos))
    }
    /// Ensure one terminal LF exists without creating a second empty row.
    pub fn ensure_final_newline(&mut self,who:Identity,slot:usize)
        ->Result<bool,Error>{
        let d=self.docs.get_mut(slot).ok_or(Error::Invalid)?;d.authorize(who)?;
        core::str::from_utf8(&d.data[..d.used]).map_err(|_|Error::Invalid)?;
        if d.data[..d.used].ends_with(b"\n"){return Ok(false);}
        d.replace_text(who,d.used,d.used,"\n")?;Ok(true)
    }
    /// Remove only one final LF; preserve other rows and UTF-8 characters.
    pub fn remove_final_newline(&mut self,who:Identity,slot:usize)
        ->Result<bool,Error>{
        let d=self.docs.get_mut(slot).ok_or(Error::Invalid)?;d.authorize(who)?;
        core::str::from_utf8(&d.data[..d.used]).map_err(|_|Error::Invalid)?;
        if !d.data[..d.used].ends_with(b"\n"){return Ok(false);}
        d.replace_text(who,d.used-1,d.used,"")?;Ok(true)
    }
    /// Edit all nonoverlapping matches only at the active unsaved revision.
    pub fn replace_all_at_revision(&mut self,who:Identity,slot:usize,
        expected:u64,old:&str,new:&str)->Result<usize,Error>{
        let version=self.text_metrics(who,slot)?;
        if version.fnv64!=expected{return Err(Error::Stale);}
        self.docs.get_mut(slot).ok_or(Error::Invalid)?
            .replace_all_text(who,old,new)
    }
    /// Redo all open documents after preflighting every owner and snapshot.
    pub fn redo_all_ready(&mut self,who:Identity)->Result<usize,Error>{
        let mut count=0;
        for d in &self.docs{
            if d.owner.is_some(){
                d.authorize(who)?;
                if d.redo.is_none(){return Err(Error::Invalid);}
                count+=1;
            }
        }
        if count==0{return Err(Error::Invalid);}
        for d in &mut self.docs{
            if d.owner.is_some(){d.redo(who)?;}
        }
        Ok(count)
    }
}

#[cfg(test)]
mod r082_editor_pool_function_tests{
    use super::*;
    use crate::block::{BlockError,SECTOR_BYTES};
    struct Disk{sectors:[[u8;SECTOR_BYTES];100]}
    impl BlockDevice for Disk{
        fn sector_count(&self)->u64{100}
        fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
            ->Result<(),BlockError>{
            *out=*self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
        }
        fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])
            ->Result<(),BlockError>{
            *self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*data;Ok(())
        }
    }
    fn who()->Identity{Identity{pid:91,generation:1}}
    fn setup()->(HierVfs<Disk>,EditorPool){
        let mut fs=HierVfs::format_blank(Disk{sectors:[[0;SECTOR_BYTES];100]}).unwrap();
        fs.create("/one","α cat\ncat\n".as_bytes()).unwrap();
        fs.create("/two",b"dog").unwrap();
        let mut pool=EditorPool::new();
        pool.open(who(),&mut fs,0,"/one").unwrap();
        pool.open(who(),&mut fs,1,"/two").unwrap();
        (fs,pool)
    }
    #[test]
    fn ten_editor_actions_remain_owner_safe_and_disk_persistent(){
        let (mut fs,mut pool)=setup();
        assert_eq!(pool.count_literal(who(),0,"cat"),Ok(2));
        assert_eq!(pool.last_literal(who(),0,"cat"),Ok(Some(7)));
        let mut out=[0xaau8;20];
        assert_eq!(pool.read_source_row(who(),0,1,&mut out),Ok(7));
        assert_eq!(&out[..7],"α cat\n".as_bytes());
        assert_eq!(pool.append_source_text(who(),1," fish"),Ok(8));
        assert_eq!(pool.prepend_source_text(who(),1,"big "),Ok(12));
        assert_eq!(pool.replace_last_literal(who(),1,"fish","bird"),Ok(Some(8)));
        assert_eq!(pool.ensure_final_newline(who(),1),Ok(true));
        assert_eq!(pool.ensure_final_newline(who(),1),Ok(false));
        assert_eq!(pool.remove_final_newline(who(),1),Ok(true));
        let hash=pool.text_metrics(who(),0).unwrap().fnv64;
        assert_eq!(pool.replace_all_at_revision(who(),0,hash,"cat","fox"),Ok(2));
        assert_eq!(pool.undo_all_ready(who()),Ok(2));
        assert_eq!(pool.redo_all_ready(who()),Ok(2));
        pool.save_all(who(),&mut fs).unwrap();
        let mut fs=HierVfs::mount(fs.into_device()).unwrap();
        assert_eq!(fs.read("/one",&mut out),Ok(11));
        assert_eq!(&out[..11],"α fox\nfox\n".as_bytes());
        assert_eq!(fs.read("/two",&mut out),Ok(12));
        assert_eq!(&out[..12],b"big dog bird");
    }
    #[test]
    fn stale_revision_short_row_invalid_owner_and_rejected_redo_do_not_mutate(){
        let (_fs,mut pool)=setup();let before=pool.text_metrics(who(),0).unwrap().fnv64;
        let mut tiny=[0xa5u8;1];
        assert_eq!(pool.read_source_row(who(),0,1,&mut tiny),Err(Error::Full));
        assert_eq!(tiny,[0xa5]);
        assert_eq!(pool.read_source_row(who(),0,0,&mut tiny),Err(Error::Invalid));
        assert_eq!(pool.replace_all_at_revision(who(),0,before^1,"cat","oops"),
            Err(Error::Stale));
        assert_eq!(pool.redo_all_ready(who()),Err(Error::Invalid));
        assert_eq!(pool.count_literal(Identity{generation:2,..who()},0,"cat"),
            Err(Error::Unauthorized));
        assert_eq!(pool.status(who(),0).unwrap().dirty,false);
    }
}


// M1/R083 Stream A — 20 per-slot operations over authenticated unsaved text.
// Navigation never touches disk. Successful transforms use existing single
// undo snapshots, not fake UI-only source state.
impl EditorPool{
    fn r083_text(&self,who:Identity,slot:usize)->Result<&str,Error>{
        let d=self.docs.get(slot).ok_or(Error::Invalid)?;
        d.authorize(who)?;
        core::str::from_utf8(&d.data[..d.used]).map_err(|_|Error::Invalid)
    }
    pub fn buffer_contains_literal(&self,who:Identity,slot:usize,needle:&str)->Result<bool,Error>{
        let t=self.r083_text(who,slot)?;
        if needle.is_empty(){return Err(Error::Invalid);} Ok(t.contains(needle))
    }
    pub fn buffer_starts_with(&self,who:Identity,slot:usize,prefix:&str)->Result<bool,Error>{
        let t=self.r083_text(who,slot)?;
        if prefix.is_empty(){return Err(Error::Invalid);} Ok(t.starts_with(prefix))
    }
    pub fn buffer_ends_with(&self,who:Identity,slot:usize,suffix:&str)->Result<bool,Error>{
        let t=self.r083_text(who,slot)?;
        if suffix.is_empty(){return Err(Error::Invalid);} Ok(t.ends_with(suffix))
    }
    pub fn buffer_row_count(&self,who:Identity,slot:usize)->Result<usize,Error>{
        let t=self.r083_text(who,slot)?;
        Ok(if t.is_empty(){0}else{t.split_inclusive('\n').count()})
    }
    pub fn buffer_lines_with(&self,who:Identity,slot:usize,needle:&str)->Result<usize,Error>{
        let t=self.r083_text(who,slot)?;
        if needle.is_empty(){return Err(Error::Invalid);} Ok(t.split_inclusive('\n').filter(|r|r.contains(needle)).count())
    }
    pub fn buffer_first_line_with(&self,who:Identity,slot:usize,needle:&str)->Result<Option<usize>,Error>{
        let t=self.r083_text(who,slot)?;
        if needle.is_empty(){return Err(Error::Invalid);} Ok(t.split_inclusive('\n').position(|r|r.contains(needle)).map(|i|i+1))
    }
    pub fn buffer_last_line_with(&self,who:Identity,slot:usize,needle:&str)->Result<Option<usize>,Error>{
        let t=self.r083_text(who,slot)?;
        if needle.is_empty(){return Err(Error::Invalid);} Ok(t.split_inclusive('\n').enumerate().filter(|(_,r)|r.contains(needle)).map(|(i,_)|i+1).last())
    }
    pub fn buffer_tab_count(&self,who:Identity,slot:usize)->Result<usize,Error>{
        let t=self.r083_text(who,slot)?;
        Ok(t.bytes().filter(|b|*b==b'\t').count())
    }
    pub fn buffer_crlf_count(&self,who:Identity,slot:usize)->Result<usize,Error>{
        let t=self.r083_text(who,slot)?;
        Ok(t.as_bytes().windows(2).filter(|w|*w==b"\r\n").count())
    }
    pub fn buffer_trailing_spaces(&self,who:Identity,slot:usize)->Result<usize,Error>{
        let t=self.r083_text(who,slot)?;
        Ok(t.split_inclusive('\n').map(|r|r.trim_end_matches('\n').bytes().rev().take_while(|b|*b==b' ').count()).sum())
    }

    /// Replace a literal opening sequence in one undoable buffer edit.
    pub fn buffer_replace_prefix(&mut self,who:Identity,slot:usize,
        old:&str,new:&str)->Result<bool,Error>{
        if old.is_empty(){return Err(Error::Invalid);}
        let t=self.r083_text(who,slot)?;
        if !t.starts_with(old){return Ok(false);}
        self.docs[slot].replace_text(who,0,old.len(),new)?;Ok(true)
    }
    pub fn buffer_replace_suffix(&mut self,who:Identity,slot:usize,
        old:&str,new:&str)->Result<bool,Error>{
        if old.is_empty(){return Err(Error::Invalid);}
        let t=self.r083_text(who,slot)?;
        if !t.ends_with(old){return Ok(false);}
        let from=t.len()-old.len();let end=t.len();
        self.docs[slot].replace_text(who,from,end,new)?;Ok(true)
    }
    pub fn buffer_erase_first_literal(&mut self,who:Identity,slot:usize,
        needle:&str)->Result<Option<usize>,Error>{
        if needle.is_empty(){return Err(Error::Invalid);}
        let t=self.r083_text(who,slot)?;
        let Some(pos)=t.find(needle) else{return Ok(None)};
        self.docs[slot].replace_text(who,pos,pos+needle.len(),"")?;Ok(Some(pos))
    }
    pub fn buffer_erase_last_literal(&mut self,who:Identity,slot:usize,
        needle:&str)->Result<Option<usize>,Error>{
        if needle.is_empty(){return Err(Error::Invalid);}
        let t=self.r083_text(who,slot)?;
        let Some(pos)=t.rfind(needle) else{return Ok(None)};
        self.docs[slot].replace_text(who,pos,pos+needle.len(),"")?;Ok(Some(pos))
    }
    pub fn buffer_insert_before_first(&mut self,who:Identity,slot:usize,
        needle:&str,insert:&str)->Result<Option<usize>,Error>{
        if needle.is_empty()||insert.is_empty(){return Err(Error::Invalid);}
        let t=self.r083_text(who,slot)?;
        let Some(pos)=t.find(needle) else{return Ok(None)};
        self.docs[slot].replace_text(who,pos,pos,insert)?;Ok(Some(pos))
    }
    pub fn buffer_insert_after_last(&mut self,who:Identity,slot:usize,
        needle:&str,insert:&str)->Result<Option<usize>,Error>{
        if needle.is_empty()||insert.is_empty(){return Err(Error::Invalid);}
        let t=self.r083_text(who,slot)?;
        let Some(pos)=t.rfind(needle) else{return Ok(None)};
        let end=pos+needle.len();
        self.docs[slot].replace_text(who,end,end,insert)?;Ok(Some(end))
    }
    pub fn buffer_uppercase_ascii(&mut self,who:Identity,slot:usize)
        ->Result<usize,Error>{
        let t=self.r083_text(who,slot)?;
        let n=t.len();let mut stage=[0u8;MAX_FILE_BYTES];let mut changes=0;
        for (i,b) in t.bytes().enumerate(){
            stage[i]=b.to_ascii_uppercase();
            if stage[i]!=b{changes+=1;}
        }
        if changes>0{
            let value=core::str::from_utf8(&stage[..n]).map_err(|_|Error::Invalid)?;
            self.docs[slot].replace_text(who,0,n,value)?;
        }
        Ok(changes)
    }
    pub fn buffer_lowercase_ascii(&mut self,who:Identity,slot:usize)
        ->Result<usize,Error>{
        let t=self.r083_text(who,slot)?;
        let n=t.len();let mut stage=[0u8;MAX_FILE_BYTES];let mut changes=0;
        for (i,b) in t.bytes().enumerate(){
            stage[i]=b.to_ascii_lowercase();
            if stage[i]!=b{changes+=1;}
        }
        if changes>0{
            let value=core::str::from_utf8(&stage[..n]).map_err(|_|Error::Invalid)?;
            self.docs[slot].replace_text(who,0,n,value)?;
        }
        Ok(changes)
    }
    pub fn buffer_trim_trailing_spaces(&mut self,who:Identity,slot:usize)
        ->Result<usize,Error>{
        let t=self.r083_text(who,slot)?;
        let n=t.len();let mut stage=[0u8;MAX_FILE_BYTES];let mut used=0;let mut removed=0;
        for row in t.split_inclusive('\n'){
            let haslf=row.ends_with('\n');
            let content=if haslf{&row[..row.len()-1]}else{row};
            let trim=content.trim_end_matches(' ');
            stage[used..used+trim.len()].copy_from_slice(trim.as_bytes());used+=trim.len();
            removed+=content.len()-trim.len();
            if haslf{stage[used]=b'\n';used+=1;}
        }
        if removed>0{
            let value=core::str::from_utf8(&stage[..used]).map_err(|_|Error::Invalid)?;
            self.docs[slot].replace_text(who,0,n,value)?;
        }
        Ok(removed)
    }
    pub fn buffer_normalize_crlf(&mut self,who:Identity,slot:usize)
        ->Result<usize,Error>{
        let t=self.r083_text(who,slot)?;
        let n=t.len();let mut stage=[0u8;MAX_FILE_BYTES];let mut used=0;let mut i=0;
        let mut count=0;
        let source=t.as_bytes();
        while i<n{
            if i+1<n&&source[i]==b'\r'&&source[i+1]==b'\n'{
                stage[used]=b'\n';used+=1;i+=2;count+=1;
            }else{stage[used]=source[i];used+=1;i+=1;}
        }
        if count>0{
            let value=core::str::from_utf8(&stage[..used]).map_err(|_|Error::Invalid)?;
            self.docs[slot].replace_text(who,0,n,value)?;
        }
        Ok(count)
    }
}

#[cfg(test)]
mod r083_editor_tests{
    use super::*;
    use crate::block::{BlockError,SECTOR_BYTES};
    struct Disk{sectors:[[u8;SECTOR_BYTES];100]}
    impl BlockDevice for Disk{
        fn sector_count(&self)->u64{100}
        fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
            *out=*self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
        }
        fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
            *self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*data;Ok(())
        }
    }
    fn who()->Identity{Identity{pid:104,generation:1}}
    fn setup()->(HierVfs<Disk>,EditorPool){
        let mut fs=HierVfs::format_blank(Disk{sectors:[[0;SECTOR_BYTES];100]}).unwrap();
        fs.create("/main",b"CAT  \r\none\t2\r\nCAT\n").unwrap();
        let mut p=EditorPool::new();p.open(who(),&mut fs,0,"/main").unwrap();(fs,p)
    }
    #[test]
    fn twenty_live_operations_produce_one_undoable_utf8_result_per_edit(){
        let (mut fs,mut p)=setup();
        assert_eq!(p.buffer_contains_literal(who(),0,"CAT"),Ok(true));
        assert_eq!(p.buffer_starts_with(who(),0,"CAT"),Ok(true));
        assert_eq!(p.buffer_ends_with(who(),0,"CAT\n"),Ok(true));
        assert_eq!(p.buffer_row_count(who(),0),Ok(3));
        assert_eq!(p.buffer_lines_with(who(),0,"CAT"),Ok(2));
        assert_eq!(p.buffer_first_line_with(who(),0,"CAT"),Ok(Some(1)));
        assert_eq!(p.buffer_last_line_with(who(),0,"CAT"),Ok(Some(3)));
        assert_eq!(p.buffer_tab_count(who(),0),Ok(1));
        assert_eq!(p.buffer_crlf_count(who(),0),Ok(2));
        assert_eq!(p.buffer_trailing_spaces(who(),0),Ok(0));
        assert_eq!(p.buffer_replace_prefix(who(),0,"CAT","DOG"),Ok(true));
        assert_eq!(p.buffer_replace_suffix(who(),0,"CAT\n","FOX\n"),Ok(true));
        assert_eq!(p.buffer_insert_before_first(who(),0,"DOG","!"),Ok(Some(0)));
        assert!(p.buffer_insert_after_last(who(),0,"FOX","!").unwrap().is_some());
        assert_eq!(p.buffer_erase_first_literal(who(),0,"!"),Ok(Some(0)));
        assert!(p.buffer_erase_last_literal(who(),0,"!").unwrap().is_some());
        assert!(p.buffer_lowercase_ascii(who(),0).unwrap()>0);
        assert!(p.buffer_uppercase_ascii(who(),0).unwrap()>0);
        assert_eq!(p.buffer_normalize_crlf(who(),0),Ok(2));
        assert_eq!(p.buffer_trim_trailing_spaces(who(),0),Ok(2));
        p.save_all(who(),&mut fs).unwrap();
        let mut fs=HierVfs::mount(fs.into_device()).unwrap();
        let mut output=[0u8;50];let n=fs.read("/main",&mut output).unwrap();
        assert_eq!(&output[..n],b"DOG\nONE\t2\nFOX\n");
    }
    #[test]
    fn unauthorized_invalid_search_and_miss_keep_unsaved_data(){
        let (_fs,mut p)=setup();
        assert_eq!(p.buffer_contains_literal(Identity{generation:2,..who()},0,"CAT"),Err(Error::Unauthorized));
        assert_eq!(p.buffer_replace_suffix(who(),0,"NO","new"),Ok(false));
        assert_eq!(p.buffer_erase_first_literal(who(),0,""),Err(Error::Invalid));
        assert_eq!(p.buffer_insert_after_last(who(),0,"missing","!"),Ok(None));
        assert_eq!(p.status(who(),0).unwrap().dirty,false);
    }
}

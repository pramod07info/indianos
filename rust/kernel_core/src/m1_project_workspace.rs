//! R019 WS01/WS03: authenticated project tree on the existing durable HierVfs.
//!
//! Source files and directories are REAL persistent sector-backed objects.
//! The allocation-free R027 volume currently supports eight objects, so this
//! is a bounded service, not an unlimited developer workspace or a GUI.
//! A trusted Desktop grants read/write permissions before dispatching here.
use crate::batch5_session::Identity;
use crate::block::BlockDevice;
use crate::durablefs::{FsError, MAX_FILE_BYTES};
use crate::hier_vfs::HierVfs;

pub const NAME_MAX: usize = 12;
pub const PROJECT_PATH_MAX: usize = 22;
const PARENT: &str = "/work";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Unauthorized,
    Invalid,
    NotOpen,
    AlreadyOpen,
    Stale,
    Full,
    Dirty,
    Fs(FsError),
}
impl From<FsError> for Error { fn from(e: FsError) -> Self { Self::Fs(e) } }

/// A detached session does not retain disk bytes or become a source of truth.
/// The owned process generation and filesystem generation must both match.
pub struct ProjectTree {
    owner: Option<Identity>,
    path: [u8; PROJECT_PATH_MAX],
    length: usize,
    generation: u64,
}
impl ProjectTree {
    pub const fn new() -> Self {
        Self { owner: None, path: [0; PROJECT_PATH_MAX],
               length: 0, generation: 0 }
    }
    fn check(&self, caller: Identity) -> Result<(), Error> {
        if caller.pid == 0 || caller.generation == 0 || self.owner != Some(caller) {
            return Err(Error::Unauthorized);
        }
        Ok(())
    }
    fn check_fresh<D: BlockDevice>(&self, caller: Identity,
        fs: &HierVfs<D>) -> Result<(), Error> {
        self.check(caller)?;
        if fs.generation() != self.generation { return Err(Error::Stale); }
        Ok(())
    }
    fn valid(name: &str) -> Result<(), Error> {
        if name.is_empty() || name.len() > NAME_MAX ||
           name.bytes().any(|b| !(b.is_ascii_alphanumeric() || b == b'-' || b == b'_')) {
            return Err(Error::Invalid);
        }
        Ok(())
    }
    fn project_path(name: &str) -> Result<([u8; PROJECT_PATH_MAX], usize), Error> {
        Self::valid(name)?;
        let bytes = name.as_bytes();
        let size = PARENT.len() + 1 + bytes.len();
        if size > PROJECT_PATH_MAX { return Err(Error::Invalid); }
        let mut path = [0u8; PROJECT_PATH_MAX];
        path[..PARENT.len()].copy_from_slice(PARENT.as_bytes());
        path[PARENT.len()] = b'/';
        path[PARENT.len() + 1..size].copy_from_slice(bytes);
        Ok((path, size))
    }
    fn pathname(&self) -> &str {
        core::str::from_utf8(&self.path[..self.length]).unwrap()
    }
    fn child(&self, name: &str) -> Result<([u8; PROJECT_PATH_MAX], usize), Error> {
        Self::valid(name)?;
        let len = self.length + 1 + name.len();
        if len > PROJECT_PATH_MAX { return Err(Error::Invalid); }
        let mut buf = [0; PROJECT_PATH_MAX];
        buf[..self.length].copy_from_slice(&self.path[..self.length]);
        buf[self.length] = b'/';
        buf[self.length + 1..len].copy_from_slice(name.as_bytes());
        Ok((buf, len))
    }
    fn str_path(path: &[u8], len: usize) -> &str {
        core::str::from_utf8(&path[..len]).unwrap()
    }

    /// Create a real COW-backed project directory, then attach to it.
    /// Creating the shared parent may succeed independently if subsequent
    /// project creation exhausts disk capacity; it never deletes user files.
    pub fn create<D: BlockDevice>(&mut self, caller: Identity,
        fs: &mut HierVfs<D>, name: &str) -> Result<(), Error> {
        if self.owner.is_some() { return Err(Error::AlreadyOpen); }
        if caller.pid == 0 || caller.generation == 0 { return Err(Error::Unauthorized); }
        let (path, len) = Self::project_path(name)?;
        match fs.mkdir(PARENT) {
            Ok(()) | Err(FsError::Exists) => (),
            Err(e) => return Err(e.into()),
        };
        fs.mkdir(Self::str_path(&path, len))?;
        self.path = path;
        self.length = len;
        self.generation = fs.generation();
        self.owner = Some(caller);
        Ok(())
    }
    /// Attach an existing persistent project only if it is a real directory.
    /// No arbitrary paths or .. traversal are accepted from an application.
    pub fn open<D: BlockDevice>(&mut self, caller: Identity,
        fs: &mut HierVfs<D>, name: &str) -> Result<(), Error> {
        if self.owner.is_some() { return Err(Error::AlreadyOpen); }
        if caller.pid == 0 || caller.generation == 0 { return Err(Error::Unauthorized); }
        let (path, len) = Self::project_path(name)?;
        let mut listing = [0u8; 256];
        fs.list_dir(Self::str_path(&path, len), &mut listing)?;
        self.path = path;
        self.length = len;
        self.generation = fs.generation();
        self.owner = Some(caller);
        Ok(())
    }
    /// Guard against unobserved concurrent modifications of the same volume.
    /// Explicit refresh is required before using the updated project tree.
    pub fn refresh<D: BlockDevice>(&mut self, caller: Identity,
        fs: &mut HierVfs<D>) -> Result<u64, Error> {
        self.check(caller)?;
        let mut listing = [0u8; 256];
        fs.list_dir(self.pathname(), &mut listing)?;
        self.generation = fs.generation();
        Ok(self.generation)
    }
    /// Copy actual on-disk directory entries into a caller-owned buffer.
    pub fn list<D: BlockDevice>(&self, caller: Identity,
        fs: &mut HierVfs<D>, out: &mut [u8]) -> Result<usize, Error> {
        self.check_fresh(caller, fs)?;
        fs.list_dir(self.pathname(), out).map_err(Error::Fs)
    }
    /// Real durable create; safe leaf names cannot escape project root.
    pub fn create_file<D: BlockDevice>(&mut self, caller: Identity,
        fs: &mut HierVfs<D>, name: &str, data: &[u8]) -> Result<(), Error> {
        self.check_fresh(caller, fs)?;
        if data.len() > MAX_FILE_BYTES { return Err(Error::Full); }
        let (path, len) = self.child(name)?;
        fs.create(Self::str_path(&path, len), data)?;
        self.generation = fs.generation();
        Ok(())
    }
    pub fn read_file<D: BlockDevice>(&self, caller: Identity,
        fs: &mut HierVfs<D>, name: &str, out: &mut [u8]) -> Result<usize, Error> {
        self.check_fresh(caller, fs)?;
        let (path, len) = self.child(name)?;
        fs.read(Self::str_path(&path, len), out).map_err(Error::Fs)
    }
    /// Replaces the whole committed file transactionally via the COW backend.
    pub fn save_file<D: BlockDevice>(&mut self, caller: Identity,
        fs: &mut HierVfs<D>, name: &str, data: &[u8]) -> Result<(), Error> {
        self.check_fresh(caller, fs)?;
        if data.len() > MAX_FILE_BYTES { return Err(Error::Full); }
        let (path, len) = self.child(name)?;
        fs.overwrite(Self::str_path(&path, len), data)?;
        self.generation = fs.generation();
        Ok(())
    }
    pub fn rename_file<D: BlockDevice>(&mut self, caller: Identity,
        fs: &mut HierVfs<D>, source: &str, target: &str) -> Result<(), Error> {
        self.check_fresh(caller, fs)?;
        if source == target { return Err(Error::Invalid); }
        let (a, alen) = self.child(source)?;
        let (b, blen) = self.child(target)?;
        fs.rename_file(Self::str_path(&a, alen), Self::str_path(&b, blen))?;
        self.generation = fs.generation();
        Ok(())
    }
    pub fn delete_file<D: BlockDevice>(&mut self, caller: Identity,
        fs: &mut HierVfs<D>, name: &str) -> Result<(), Error> {
        self.check_fresh(caller, fs)?;
        let (path, len) = self.child(name)?;
        fs.unlink(Self::str_path(&path, len))?;
        self.generation = fs.generation();
        Ok(())
    }
    /// Atomic directory-tree rename is delegated to the existing R031.4 COW
    /// implementation; no file data is copied into a temporary RAM directory.
    pub fn rename_project<D: BlockDevice>(&mut self, caller: Identity,
        fs: &mut HierVfs<D>, name: &str) -> Result<(), Error> {
        self.check_fresh(caller, fs)?;
        let (target, len) = Self::project_path(name)?;
        fs.rename_dir(self.pathname(), Self::str_path(&target, len))?;
        self.path = target;
        self.length = len;
        self.generation = fs.generation();
        Ok(())
    }
    /// Remove ONLY an empty project; existing files are never recursively
    /// destroyed. An explicit caller command must opt in to the deletion.
    pub fn remove_empty_project<D: BlockDevice>(&mut self, caller: Identity,
        fs: &mut HierVfs<D>) -> Result<(), Error> {
        self.check_fresh(caller, fs)?;
        fs.rmdir(self.pathname())?;
        *self = Self::new();
        Ok(())
    }
    /// Read a bounded UTF-8 source file and publish only complete bytes.
    pub fn open_source<D: BlockDevice>(&self, caller: Identity,
        fs: &mut HierVfs<D>, name: &str, out: &mut [u8]
    ) -> Result<usize, Error> {
        self.check_fresh(caller, fs)?;
        let (p, n) = self.child(name)?;
        let mut staging = [0u8; MAX_FILE_BYTES];
        let count = fs.read(Self::str_path(&p, n), &mut staging)?;
        if core::str::from_utf8(&staging[..count]).is_err() { return Err(Error::Invalid); }
        if count > out.len() { return Err(Error::Full); }
        out[..count].copy_from_slice(&staging[..count]);
        Ok(count)
    }

    /// Revision-locked Unicode-safe edit committed through durable COW VFS.
    /// No write occurs until every byte boundary, size and authority check passes.
    pub fn edit_range<D: BlockDevice>(&mut self, caller: Identity,
        fs: &mut HierVfs<D>, name: &str, start: usize, end: usize, insert: &[u8]
    ) -> Result<usize, Error> {
        self.check_fresh(caller, fs)?;
        let (p, n) = self.child(name)?;
        let mut old = [0u8; MAX_FILE_BYTES];
        let size = fs.read(Self::str_path(&p, n), &mut old)?;
        let text = core::str::from_utf8(&old[..size]).map_err(|_|Error::Invalid)?;
        if start > end || end > size ||
            !text.is_char_boundary(start) || !text.is_char_boundary(end) ||
            core::str::from_utf8(insert).is_err() { return Err(Error::Invalid); }
        let next_size = size - (end - start) + insert.len();
        if next_size > MAX_FILE_BYTES { return Err(Error::Full); }
        let mut next = [0u8; MAX_FILE_BYTES];
        next[..start].copy_from_slice(&old[..start]);
        next[start..start + insert.len()].copy_from_slice(insert);
        next[start + insert.len()..next_size].copy_from_slice(&old[end..size]);
        fs.overwrite(Self::str_path(&p, n), &next[..next_size])?;
        self.generation = fs.generation();
        Ok(next_size)
    }

    /// Append text to a real disk-backed source file, not the volatile RamFs.
    pub fn append_source<D: BlockDevice>(&mut self, caller: Identity,
        fs: &mut HierVfs<D>, name: &str, suffix: &[u8]
    ) -> Result<usize, Error> {
        self.check_fresh(caller, fs)?;
        if core::str::from_utf8(suffix).is_err() { return Err(Error::Invalid); }
        let (p, n) = self.child(name)?;
        let mut original = [0u8; MAX_FILE_BYTES];
        let size = fs.read(Self::str_path(&p, n), &mut original)?;
        if core::str::from_utf8(&original[..size]).is_err() ||
           suffix.len() > MAX_FILE_BYTES - size { return Err(Error::Full); }
        original[size..size + suffix.len()].copy_from_slice(suffix);
        fs.overwrite(Self::str_path(&p, n), &original[..size + suffix.len()])?;
        self.generation = fs.generation();
        Ok(size + suffix.len())
    }

    /// Create a second durable COW file without overwriting an existing leaf.
    pub fn duplicate_file<D: BlockDevice>(&mut self, caller: Identity,
        fs: &mut HierVfs<D>, source: &str, destination: &str
    ) -> Result<usize, Error> {
        self.check_fresh(caller, fs)?;
        if source == destination { return Err(Error::Invalid); }
        let (a, alen) = self.child(source)?;
        let (b, blen) = self.child(destination)?;
        let mut data = [0u8; MAX_FILE_BYTES];
        let len = fs.read(Self::str_path(&a, alen), &mut data)?;
        fs.create(Self::str_path(&b, blen), &data[..len])?;
        self.generation = fs.generation();
        Ok(len)
    }

    /// Replace all non-overlapping literal Unicode occurrences within ONE
    /// project file, committing with exactly one COW overwrite.
    /// A failed bound/identity/version check cannot partially change disk.
    pub fn replace_all<D:BlockDevice>(&mut self,caller:Identity,
        fs:&mut HierVfs<D>,name:&str,needle:&str,replacement:&str
    )->Result<usize,Error>{
        self.check_fresh(caller,fs)?;
        if needle.is_empty(){return Err(Error::Invalid);}
        let (p,n)=self.child(name)?;
        let mut previous=[0u8;MAX_FILE_BYTES];
        let size=fs.read(Self::str_path(&p,n),&mut previous)?;
        let text=core::str::from_utf8(&previous[..size]).map_err(|_|Error::Invalid)?;
        let mut updated=[0u8;MAX_FILE_BYTES];
        let mut input=0usize;let mut output=0usize;let mut count=0usize;
        while input<size{
            if text[input..].starts_with(needle){
                let end=output.checked_add(replacement.len()).ok_or(Error::Full)?;
                if end>MAX_FILE_BYTES{return Err(Error::Full);}
                updated[output..end].copy_from_slice(replacement.as_bytes());
                output=end;input+=needle.len();count+=1;
            }else{
                let width=text[input..].chars().next().unwrap().len_utf8();
                if output+width>MAX_FILE_BYTES{return Err(Error::Full);}
                updated[output..output+width].copy_from_slice(&previous[input..input+width]);
                output+=width;input+=width;
            }
        }
        if count==0||updated[..output]==previous[..size]{return Ok(count);}
        fs.overwrite(Self::str_path(&p,n),&updated[..output])?;
        self.generation=fs.generation();
        Ok(count)
    }
    /// Navigate to an absolute source character line/column, respecting UTF-8.
    pub fn source_offset<D: BlockDevice>(&self, caller: Identity,
        fs: &mut HierVfs<D>, name: &str, line: usize, column: usize
    ) -> Result<usize, Error> {
        self.check_fresh(caller, fs)?;
        if line == 0 || column == 0 { return Err(Error::Invalid); }
        let (p, n) = self.child(name)?;
        let mut bytes = [0u8; MAX_FILE_BYTES];
        let count = fs.read(Self::str_path(&p, n), &mut bytes)?;
        let source = core::str::from_utf8(&bytes[..count]).map_err(|_|Error::Invalid)?;
        let mut at = 0;
        // Reject phantom final row after a newline, consistent with source_lines().
        for (index, row) in source.split_terminator('\n').enumerate() {
            if index + 1 == line {
                let offset = row.char_indices().nth(column - 1).map(|x| x.0)
                    .or_else(|| if column == row.chars().count() + 1 {
                        Some(row.len()) } else { None }).ok_or(Error::Invalid)?;
                return Ok(at + offset);
            }
            at += row.len() + 1;
        }
        Err(Error::Invalid)
    }

    /// Find a source term from a byte-aligned cursor without mutating the VFS.
    pub fn find_source<D: BlockDevice>(&self, caller: Identity,
        fs: &mut HierVfs<D>, name: &str, query: &[u8],
        from: usize, out: &mut [usize]
    ) -> Result<usize, Error> {
        self.check_fresh(caller, fs)?;
        if query.is_empty() || core::str::from_utf8(query).is_err() {
            return Err(Error::Invalid);
        }
        let (p, n) = self.child(name)?;
        let mut data = [0u8; MAX_FILE_BYTES];
        let count = fs.read(Self::str_path(&p, n), &mut data)?;
        let text = core::str::from_utf8(&data[..count]).map_err(|_|Error::Invalid)?;
        if from > count || !text.is_char_boundary(from) { return Err(Error::Invalid); }
        let mut matches = [0usize; MAX_FILE_BYTES];
        let mut total = 0;
        for (offset, part) in data[from..count].windows(query.len()).enumerate() {
            if part == query && text.is_char_boundary(from + offset) {
                if total == out.len() { return Err(Error::Full); }
                matches[total] = from + offset;
                total += 1;
            }
        }
        out[..total].copy_from_slice(&matches[..total]);
        Ok(total)
    }

    /// Count actual source lines in the persisted file.
    pub fn source_lines<D: BlockDevice>(&self, caller: Identity,
        fs: &mut HierVfs<D>, name: &str
    ) -> Result<usize, Error> {
        self.check_fresh(caller, fs)?;
        let (p, n) = self.child(name)?;
        let mut data = [0u8; MAX_FILE_BYTES];
        let count = fs.read(Self::str_path(&p, n), &mut data)?;
        core::str::from_utf8(&data[..count]).map_err(|_|Error::Invalid)?;
        if count == 0 { return Ok(0); }
        let newlines = data[..count].iter().filter(|b| **b == b'\n').count();
        Ok(newlines + usize::from(data[count - 1] != b'\n'))
    }

    /// Return one Unicode source line, without its newline delimiter.
    /// Output remains unchanged on short destination and invalid input.
    pub fn read_source_line<D:BlockDevice>(&self,caller:Identity,
        fs:&mut HierVfs<D>,name:&str,line:usize,out:&mut[u8]
    )->Result<usize,Error>{
        self.check_fresh(caller,fs)?;
        if line==0{return Err(Error::Invalid);}
        let (p,n)=self.child(name)?;
        let mut bytes=[0u8;MAX_FILE_BYTES];
        let count=fs.read(Self::str_path(&p,n),&mut bytes)?;
        let text=core::str::from_utf8(&bytes[..count]).map_err(|_|Error::Invalid)?;
        let row=text.split_terminator('\n').nth(line-1).ok_or(Error::Invalid)?;
        if row.len()>out.len(){return Err(Error::Full);}
        out[..row.len()].copy_from_slice(row.as_bytes());
        Ok(row.len())
    }
    /// Insert a full UTF-8 line before the one-based destination row.
    /// Commits ONE bounded file replacement, not an intermediate file.
    pub fn insert_source_line<D:BlockDevice>(&mut self,caller:Identity,
        fs:&mut HierVfs<D>,name:&str,line:usize,source:&str
    )->Result<usize,Error>{
        self.check_fresh(caller,fs)?;
        if line==0||source.contains('\n')||source.contains('\r'){
            return Err(Error::Invalid);
        }
        let (p,n)=self.child(name)?;
        let mut old=[0u8;MAX_FILE_BYTES];
        let count=fs.read(Self::str_path(&p,n),&mut old)?;
        let text=core::str::from_utf8(&old[..count]).map_err(|_|Error::Invalid)?;
        let mut offset=0;
        for _ in 1..line{
            offset+=text[offset..].find('\n').ok_or(Error::Invalid)?+1;
        }
        let needed=source.len().checked_add(1).ok_or(Error::Full)?;
        if count.checked_add(needed).is_none_or(|n|n>MAX_FILE_BYTES){
            return Err(Error::Full);
        }
        let mut next=[0u8;MAX_FILE_BYTES];
        next[..source.len()].copy_from_slice(source.as_bytes());
        next[source.len()]=b'\n';
        self.edit_range(caller,fs,name,offset,offset,&next[..needed])
    }
    /// Delete exactly one complete source line and its trailing newline.
    /// No recursive deletion, file removal, or other-line rewrite.
    pub fn delete_source_line<D:BlockDevice>(&mut self,caller:Identity,
        fs:&mut HierVfs<D>,name:&str,line:usize
    )->Result<usize,Error>{
        self.check_fresh(caller,fs)?;
        if line==0{return Err(Error::Invalid);}
        let (p,n)=self.child(name)?;
        let mut old=[0u8;MAX_FILE_BYTES];
        let count=fs.read(Self::str_path(&p,n),&mut old)?;
        let text=core::str::from_utf8(&old[..count]).map_err(|_|Error::Invalid)?;
        let mut start=0;
        for _ in 1..line{
            start+=text[start..].find('\n').ok_or(Error::Invalid)?+1;
        }
        if start==count{return Err(Error::Invalid);}
        let end=start+text[start..].find('\n').map(|n|n+1)
            .unwrap_or(count-start);
        self.edit_range(caller,fs,name,start,end,b"")
    }
    /// Read-only source checksum for a revision-sensitive build cache.
    /// FNV is not cryptographic and must never authenticate executable bytes.
    pub fn source_checksum<D: BlockDevice>(&self, caller: Identity,
        fs: &mut HierVfs<D>, name: &str
    ) -> Result<u64, Error> {
        self.check_fresh(caller, fs)?;
        let (p, n) = self.child(name)?;
        let mut data = [0u8; MAX_FILE_BYTES];
        let count = fs.read(Self::str_path(&p, n), &mut data)?;
        let hash = data[..count].iter().fold(0xcbf29ce484222325u64,
            |h, b| (h ^ (*b as u64)).wrapping_mul(0x100000001b3));
        Ok(hash)
    }

    pub fn close(&mut self, caller: Identity) -> Result<(), Error> {
        self.check(caller)?;
        *self = Self::new(); Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{BlockError, SECTOR_BYTES};
    const SECTORS: usize = 100;
    struct Disk { sectors: [[u8; SECTOR_BYTES]; SECTORS] }
    impl Disk { fn new() -> Self { Self { sectors: [[0; SECTOR_BYTES]; SECTORS] } } }
    impl BlockDevice for Disk {
        fn sector_count(&self) -> u64 { SECTORS as u64 }
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
    fn a() -> Identity { Identity { pid: 2, generation: 1 } }
    fn stale_id() -> Identity { Identity { pid: 2, generation: 2 } }
    fn fresh() -> HierVfs<Disk> {
        HierVfs::format_blank(Disk::new()).unwrap()
    }
    #[test]
    fn r043_project_line_insertion_read_and_removal_survive_remount(){
        let mut fs=fresh();let mut p=ProjectTree::new();
        p.create(a(),&mut fs,"demo").unwrap();
        p.create_file(a(),&mut fs,"main","one\nthree\n".as_bytes()).unwrap();
        assert_eq!(p.insert_source_line(a(),&mut fs,"main",2,"two 🍀"),
            Ok("one\ntwo 🍀\nthree\n".len()));
        let mut row=[0u8;40];
        assert_eq!(p.read_source_line(a(),&mut fs,"main",2,&mut row),Ok(8));
        assert_eq!(&row[..8],"two 🍀".as_bytes());
        assert_eq!(p.delete_source_line(a(),&mut fs,"main",1),
            Ok("two 🍀\nthree\n".len()));
        let mut fs=HierVfs::mount(fs.into_device()).unwrap();
        let mut next=ProjectTree::new();
        next.open(a(),&mut fs,"demo").unwrap();
        let mut data=[0u8;48];
        assert_eq!(next.read_file(a(),&mut fs,"main",&mut data),Ok(15));
        assert_eq!(&data[..15],"two 🍀\nthree\n".as_bytes());
    }
    #[test]
    fn r055_trailing_newline_cannot_expose_phantom_source_row(){
        let mut fs=fresh();let mut p=ProjectTree::new();
        p.create(a(),&mut fs,"demo").unwrap();
        p.create_file(a(),&mut fs,"main",b"first\nsecond\n").unwrap();
        assert_eq!(p.source_lines(a(),&mut fs,"main"),Ok(2));
        let mut row=[0xaau8;16];
        assert_eq!(p.read_source_line(a(),&mut fs,"main",2,&mut row),Ok(6));
        assert_eq!(&row[..6],b"second");
        let original=row;
        assert_eq!(p.read_source_line(a(),&mut fs,"main",3,&mut row),
            Err(Error::Invalid));
        assert_eq!(row,original);
        assert_eq!(p.source_offset(a(),&mut fs,"main",3,1),Err(Error::Invalid));
        assert_eq!(p.source_offset(a(),&mut fs,"main",2,7),Ok(12));
    }
    #[test]
    fn r055_empty_source_has_zero_visible_rows(){
        let mut fs=fresh();let mut p=ProjectTree::new();
        p.create(a(),&mut fs,"demo").unwrap();
        p.create_file(a(),&mut fs,"empty",b"").unwrap();
        let mut out=[0x55;4];
        assert_eq!(p.source_lines(a(),&mut fs,"empty"),Ok(0));
        assert_eq!(p.read_source_line(a(),&mut fs,"empty",1,&mut out),
            Err(Error::Invalid));
        assert_eq!(p.source_offset(a(),&mut fs,"empty",1,1),Err(Error::Invalid));
        assert_eq!(out,[0x55;4]);
    }
    #[test]
    fn r043_project_line_errors_and_short_output_are_atomic(){
        let mut fs=fresh();let mut p=ProjectTree::new();
        p.create(a(),&mut fs,"demo").unwrap();
        p.create_file(a(),&mut fs,"main",b"alpha\nbeta").unwrap();
        let before=fs.generation();
        let mut out=[0x5a;2];
        assert_eq!(p.read_source_line(a(),&mut fs,"main",1,&mut out),Err(Error::Full));
        assert_eq!(out,[0x5a;2]);
        assert_eq!(p.insert_source_line(a(),&mut fs,"main",5,"x"),Err(Error::Invalid));
        assert_eq!(p.insert_source_line(a(),&mut fs,"main",1,"invalid\nline"),
            Err(Error::Invalid));
        assert_eq!(p.delete_source_line(a(),&mut fs,"main",8),Err(Error::Invalid));
        assert_eq!(p.insert_source_line(stale_id(),&mut fs,"main",1,"evil"),
            Err(Error::Unauthorized));
        assert_eq!(fs.generation(),before);
    }
    #[test]
    fn persistent_project_create_save_rename_and_reboot() {
        let mut fs = fresh(); let mut p = ProjectTree::new();
        p.create(a(), &mut fs, "demo").unwrap();
        p.create_file(a(), &mut fs, "main", b"fn main(){}").unwrap();
        p.save_file(a(), &mut fs, "main", b"fn main(){42}").unwrap();
        p.rename_file(a(), &mut fs, "main", "src").unwrap();
        p.rename_project(a(), &mut fs, "next").unwrap();
        p.close(a()).unwrap();
        let disk = fs.into_device();
        let mut remounted = HierVfs::mount(disk).unwrap();
        let mut again = ProjectTree::new();
        again.open(a(), &mut remounted, "next").unwrap();
        let mut out = [0u8; 32];
        assert_eq!(again.read_file(a(), &mut remounted, "src", &mut out), Ok(13));
        assert_eq!(&out[..13], b"fn main(){42}");
        let mut list = [0u8; 32];
        let len = again.list(a(), &mut remounted, &mut list).unwrap();
        assert_eq!(&list[..len], b"f:src\n");
    }
    #[test]
    fn traversal_and_identity_stale_access_cannot_modify_files() {
        let mut fs = fresh(); let mut p = ProjectTree::new();
        p.create(a(), &mut fs, "demo").unwrap();
        assert_eq!(p.create_file(stale_id(), &mut fs, "x", b"x"), Err(Error::Unauthorized));
        assert_eq!(p.create_file(a(), &mut fs, "../sys", b"x"), Err(Error::Invalid));
        assert_eq!(p.create_file(a(), &mut fs, "bad/file", b"x"), Err(Error::Invalid));
        p.create_file(a(), &mut fs, "file", b"original").unwrap();
        assert_eq!(p.save_file(stale_id(), &mut fs, "file", b"bad"), Err(Error::Unauthorized));
        let mut out = [0u8; 8];
        assert_eq!(p.read_file(a(), &mut fs, "file", &mut out), Ok(8));
        assert_eq!(&out, b"original");
    }
    #[test]
    fn external_write_requires_refresh_not_lost_update() {
        let mut fs = fresh(); let mut p = ProjectTree::new();
        p.create(a(), &mut fs, "demo").unwrap();
        p.create_file(a(), &mut fs, "file", b"old").unwrap();
        fs.overwrite("/work/demo/file", b"new").unwrap();
        assert_eq!(p.save_file(a(), &mut fs, "file", b"bad"), Err(Error::Stale));
        p.refresh(a(), &mut fs).unwrap();
        let mut out = [0u8; 16];
        assert_eq!(p.read_file(a(), &mut fs, "file", &mut out), Ok(3));
        assert_eq!(&out[..3], b"new");
    }
    #[test]
    fn empty_project_removal_never_deletes_nonempty_content() {
        let mut fs = fresh(); let mut p = ProjectTree::new();
        p.create(a(), &mut fs, "demo").unwrap();
        p.create_file(a(), &mut fs, "code", b"important").unwrap();
        assert!(p.remove_empty_project(a(), &mut fs).is_err());
        let mut out = [0u8; 16];
        assert_eq!(p.read_file(a(), &mut fs, "code", &mut out), Ok(9));
        assert_eq!(&out[..9], b"important");
        p.delete_file(a(), &mut fs, "code").unwrap();
        p.remove_empty_project(a(), &mut fs).unwrap();
        assert_eq!(p.close(a()), Err(Error::Unauthorized));
    }
    #[test]
    fn bad_projects_or_missing_parent_are_not_attached() {
        let mut fs = fresh(); let mut p = ProjectTree::new();
        assert_eq!(p.open(a(), &mut fs, "missing"), Err(Error::Fs(FsError::NotFound)));
        assert_eq!(p.create(a(), &mut fs, "bad.name"), Err(Error::Invalid));
        p.create(a(), &mut fs, "demo").unwrap();
        assert_eq!(p.create(a(), &mut fs, "other"), Err(Error::AlreadyOpen));
        assert_eq!(p.open(a(), &mut fs, "demo"), Err(Error::AlreadyOpen));
    }
    #[test]
    fn unicode_edit_survives_remount_and_cursor_navigation() {
        let mut fs = fresh(); let mut p = ProjectTree::new();
        p.create(a(), &mut fs, "demo").unwrap();
        p.create_file(a(), &mut fs, "source", "aβc\n".as_bytes()).unwrap();
        assert_eq!(p.source_offset(a(), &mut fs, "source", 1, 3), Ok(3));
        assert_eq!(p.edit_range(a(), &mut fs, "source", 1, 3, b"XYZ"), Ok(6));
        assert_eq!(p.source_lines(a(), &mut fs, "source"), Ok(1));
        p.close(a()).unwrap();
        let mut remount = HierVfs::mount(fs.into_device()).unwrap();
        let mut reopened = ProjectTree::new();
        reopened.open(a(), &mut remount, "demo").unwrap();
        let mut out = [0; 16];
        assert_eq!(reopened.open_source(a(), &mut remount, "source", &mut out), Ok(6));
        assert_eq!(&out[..6], b"aXYZc\n");
    }
    #[test]
    fn edit_invalid_unicode_boundaries_and_owner_fail_without_writes() {
        let mut fs = fresh(); let mut p = ProjectTree::new();
        p.create(a(), &mut fs, "demo").unwrap();
        p.create_file(a(), &mut fs, "source", "αβ".as_bytes()).unwrap();
        let generation = fs.generation();
        assert_eq!(p.edit_range(a(), &mut fs, "source", 1, 2, b"x"), Err(Error::Invalid));
        assert_eq!(p.edit_range(stale_id(), &mut fs, "source", 0, 2, b"x"), Err(Error::Unauthorized));
        assert_eq!(fs.generation(), generation);
        let mut out = [0; 8];
        let n = p.open_source(a(), &mut fs, "source", &mut out).unwrap();
        assert_eq!(&out[..n], "αβ".as_bytes());
    }
    #[test]
    fn append_duplicate_and_find_across_persistent_project() {
        let mut fs = fresh(); let mut p = ProjectTree::new();
        p.create(a(), &mut fs, "demo").unwrap();
        p.create_file(a(), &mut fs, "main", b"hello\n").unwrap();
        assert_eq!(p.append_source(a(), &mut fs, "main", b"hello\n"), Ok(12));
        assert_eq!(p.duplicate_file(a(), &mut fs, "main", "copy"), Ok(12));
        let mut offsets = [0usize; 2];
        assert_eq!(p.find_source(a(), &mut fs, "main", b"hello", 0, &mut offsets), Ok(2));
        assert_eq!(offsets, [0, 6]);
        assert_eq!(p.duplicate_file(a(), &mut fs, "main", "copy"),
                   Err(Error::Fs(FsError::Exists)));
        let mut data = [0; 16];
        assert_eq!(p.read_file(a(), &mut fs, "copy", &mut data), Ok(12));
        assert_eq!(&data[..12], b"hello\nhello\n");
    }
    #[test]
    fn find_short_output_is_atomic_and_stale_generation_blocks_all_edits() {
        let mut fs = fresh(); let mut p = ProjectTree::new();
        p.create(a(), &mut fs, "demo").unwrap();
        p.create_file(a(), &mut fs, "main", b"aa aa").unwrap();
        let mut short = [99usize; 1];
        assert_eq!(p.find_source(a(), &mut fs, "main", b"aa", 0, &mut short), Err(Error::Full));
        assert_eq!(short, [99]);
        fs.overwrite("/work/demo/main", b"changed").unwrap();
        assert_eq!(p.append_source(a(), &mut fs, "main", b"bad"), Err(Error::Stale));
        assert_eq!(p.edit_range(a(), &mut fs, "main", 0, 1, b"bad"), Err(Error::Stale));
        assert_eq!(p.source_checksum(a(), &mut fs, "main"), Err(Error::Stale));
    }
    #[test]
    fn checksum_tracks_real_edits_and_duplicate_does_not_modify_source() {
        let mut fs = fresh(); let mut p = ProjectTree::new();
        p.create(a(), &mut fs, "demo").unwrap();
        p.create_file(a(), &mut fs, "main", b"old").unwrap();
        let first = p.source_checksum(a(), &mut fs, "main").unwrap();
        p.duplicate_file(a(), &mut fs, "main", "backup").unwrap();
        let second = p.source_checksum(a(), &mut fs, "main").unwrap();
        assert_eq!(first, second);
        p.save_file(a(), &mut fs, "main", b"new").unwrap();
        assert_ne!(p.source_checksum(a(), &mut fs, "main").unwrap(), first);
    }
    #[test]
    fn listing_short_destination_preserves_caller_bytes() {
        let mut fs = fresh(); let mut p = ProjectTree::new();
        p.create(a(), &mut fs, "demo").unwrap();
        p.create_file(a(), &mut fs, "code", b"x").unwrap();
        let mut short = [0xAAu8; 1];
        assert!(p.list(a(), &mut fs, &mut short).is_err());
        assert_eq!(short, [0xAA]);
    }
    #[test]
    fn r039_unicode_replace_all_survives_disk_remount(){
        let mut fs=fresh();let mut p=ProjectTree::new();
        p.create(a(),&mut fs,"demo").unwrap();
        p.create_file(a(),&mut fs,"code","α β α α".as_bytes()).unwrap();
        assert_eq!(p.replace_all(a(),&mut fs,"code","α","lambda"),Ok(3));
        let generation=fs.generation();
        assert_eq!(p.replace_all(a(),&mut fs,"code","missing","value"),Ok(0));
        assert_eq!(fs.generation(),generation);
        p.close(a()).unwrap();
        let mut fs=HierVfs::mount(fs.into_device()).unwrap();
        let mut p=ProjectTree::new();p.open(a(),&mut fs,"demo").unwrap();
        let mut out=[0;64];let n=p.open_source(a(),&mut fs,"code",&mut out).unwrap();
        assert_eq!(&out[..n],b"lambda \xce\xb2 lambda lambda");
    }
    #[test]
    fn r039_replace_all_invalid_oversize_and_stale_owner_are_atomic(){
        let mut fs=fresh();let mut p=ProjectTree::new();
        p.create(a(),&mut fs,"demo").unwrap();
        p.create_file(a(),&mut fs,"code",b"aaaa").unwrap();
        let generation=fs.generation();
        assert_eq!(p.replace_all(stale_id(),&mut fs,"code","a","X"),Err(Error::Unauthorized));
        assert_eq!(p.replace_all(a(),&mut fs,"code","","X"),Err(Error::Invalid));
        assert_eq!(p.replace_all(a(),&mut fs,"code","a",&"X".repeat(MAX_FILE_BYTES)),Err(Error::Full));
        assert_eq!(fs.generation(),generation);
        fs.overwrite("/work/demo/code",b"external").unwrap();
        assert_eq!(p.replace_all(a(),&mut fs,"code","e","E"),Err(Error::Stale));
    }

}

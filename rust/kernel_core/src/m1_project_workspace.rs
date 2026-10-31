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
    /// Replace a consecutive set of complete one-based source lines with
    /// UTF-8 text through one durable COW commit. The insertion may itself
    /// contain several newline-separated lines. No changes occur for missing
    /// lines, expired owner generations, invalid ranges or oversize results.
    /// Both the first and last selected line are included.
    pub fn replace_source_lines<D:BlockDevice>(&mut self,caller:Identity,
        fs:&mut HierVfs<D>,name:&str,first_line:usize,remove_count:usize,
        replacement:&str
    )->Result<usize,Error>{
        self.check_fresh(caller,fs)?;
        if first_line==0||remove_count==0{return Err(Error::Invalid);}
        let last=first_line.checked_add(remove_count-1).ok_or(Error::Invalid)?;
        let (path,n)=self.child(name)?;
        let mut old=[0u8;MAX_FILE_BYTES];
        let used=fs.read(Self::str_path(&path,n),&mut old)?;
        let source=core::str::from_utf8(&old[..used]).map_err(|_|Error::Invalid)?;
        let mut cursor=0usize;
        let mut start=None;
        let mut end=None;
        // split_inclusive does not fabricate a trailing empty line after
        // a final newline, consistent with source_lines() and read_source_line().
        for (i,line) in source.split_inclusive('\n').enumerate(){
            let number=i+1;
            if number==first_line{start=Some(cursor);}
            cursor+=line.len(); // already bounded by MAX_FILE_BYTES
            if number==last{end=Some(cursor);break;}
        }
        let (Some(start),Some(end))=(start,end)else{return Err(Error::Invalid)};
        self.edit_range(caller,fs,name,start,end,replacement.as_bytes())
    }

    /// Reorder a complete block of UTF-8 source lines within one file.
    /// Destination is the original one-based line before which the block
    /// should appear, or line_count+1 to move it to EOF. All movement is
    /// committed with ONE durable COW overwrite and cannot expose a
    /// partially moved project file after a power loss.
    pub fn move_source_lines<D:BlockDevice>(&mut self,caller:Identity,
        fs:&mut HierVfs<D>,name:&str,first:usize,count:usize,before:usize
    )->Result<usize,Error>{
        self.check_fresh(caller,fs)?;
        if first==0||count==0||before==0{return Err(Error::Invalid);}
        let last=first.checked_add(count-1).ok_or(Error::Invalid)?;
        let (p,n)=self.child(name)?;
        let mut original=[0u8;MAX_FILE_BYTES];
        let used=fs.read(Self::str_path(&p,n),&mut original)?;
        let text=core::str::from_utf8(&original[..used]).map_err(|_|Error::Invalid)?;
        // A file without a final newline can have ambiguous row boundaries
        // after moving the last row. Deny instead of concatenating lines.
        if !text.ends_with('\n'){return Err(Error::Invalid);}
        let mut start=None;
        let mut end=None;
        let mut destination=None;
        let mut pos=0usize;
        let mut total=0usize;
        for row in text.split_inclusive('\n'){
            total+=1;
            if total==first{start=Some(pos);}
            if total==before{destination=Some(pos);}
            pos+=row.len();
            if total==last{end=Some(pos);}
        }
        if before==total.checked_add(1).ok_or(Error::Invalid)?{
            destination=Some(used);
        }
        let (Some(start),Some(end),Some(dest))=(start,end,destination)
            else{return Err(Error::Invalid)};
        // Moving before a line inside the block or directly after the
        // block has no visible effect and must not advance disk generation.
        if dest>=start&&dest<=end{return Ok(used);}
        let mut staged=[0u8;MAX_FILE_BYTES];
        if dest<start{
            staged[..dest].copy_from_slice(&original[..dest]);
            let moved=end-start;
            staged[dest..dest+moved].copy_from_slice(&original[start..end]);
            staged[dest+moved..end].copy_from_slice(&original[dest..start]);
            staged[end..used].copy_from_slice(&original[end..used]);
        }else{
            staged[..start].copy_from_slice(&original[..start]);
            staged[start..start+dest-end].copy_from_slice(&original[end..dest]);
            staged[start+dest-end..dest].copy_from_slice(&original[start..end]);
            staged[dest..used].copy_from_slice(&original[dest..used]);
        }
        fs.overwrite(Self::str_path(&p,n),&staged[..used])?;
        self.generation=fs.generation();
        Ok(used)
    }

    /// Atomically indent or unindent a range of complete UTF-8 source rows.
    /// Positive delta adds ASCII spaces; negative delta removes that many
    /// leading spaces from EACH selected row (otherwise rejects the edit).
    /// No intermediate COW generation or half-indented source is exposed.
    pub fn adjust_source_indent<D:BlockDevice>(&mut self,caller:Identity,
        fs:&mut HierVfs<D>,name:&str,first:usize,count:usize,delta:i8
    )->Result<usize,Error>{
        self.check_fresh(caller,fs)?;
        if first==0||count==0||delta==0||!(-8..=8).contains(&delta){
            return Err(Error::Invalid);
        }
        let last=first.checked_add(count-1).ok_or(Error::Invalid)?;
        let (p,n)=self.child(name)?;
        let mut original=[0u8;MAX_FILE_BYTES];
        let used=fs.read(Self::str_path(&p,n),&mut original)?;
        let source=core::str::from_utf8(&original[..used]).map_err(|_|Error::Invalid)?;
        if source.is_empty(){return Err(Error::Invalid);}
        let depth=delta.unsigned_abs() as usize;
        let mut updated=[0u8;MAX_FILE_BYTES];
        let mut length=0usize;
        let mut seen=0usize;
        for (i,row) in source.split_inclusive('\n').enumerate(){
            let selected=(first..=last).contains(&(i+1));
            let bytes=row.as_bytes();
            let contents=if selected&&delta<0{
                if bytes.len()<depth||!bytes[..depth].iter().all(|b|*b==b' '){
                    return Err(Error::Invalid);
                }
                &bytes[depth..]
            }else{bytes};
            let padding=if selected&&delta>0{depth}else{0};
            let next=length.checked_add(padding)
                .and_then(|at|at.checked_add(contents.len())).ok_or(Error::Full)?;
            if next>MAX_FILE_BYTES{return Err(Error::Full);}
            updated[length..length+padding].fill(b' ');
            updated[length+padding..next].copy_from_slice(contents);
            length=next;
            if selected{seen+=1;}
        }
        if seen!=count{return Err(Error::Invalid);}
        fs.overwrite(Self::str_path(&p,n),&updated[..length])?;
        self.generation=fs.generation();
        Ok(length)
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
    fn m1_r056_replace_complete_unicode_lines_survives_real_remount(){
        let mut fs=fresh();let mut p=ProjectTree::new();
        p.create(a(),&mut fs,"demo").unwrap();
        p.create_file(a(),&mut fs,"main","one\nβeta\nthree\n".as_bytes()).unwrap();
        let wanted="one\nB\nC\nthree\n";
        assert_eq!(p.replace_source_lines(a(),&mut fs,"main",2,1,"B\nC\n"),
                   Ok(wanted.len()));
        p.close(a()).unwrap();
        let mut remounted=HierVfs::mount(fs.into_device()).unwrap();
        let mut reopened=ProjectTree::new();
        reopened.open(a(),&mut remounted,"demo").unwrap();
        let mut out=[0u8;64];
        let len=reopened.open_source(a(),&mut remounted,"main",&mut out).unwrap();
        assert_eq!(&out[..len],wanted.as_bytes());
        assert_eq!(reopened.source_lines(a(),&mut remounted,"main"),Ok(4));
        assert_eq!(reopened.replace_source_lines(a(),&mut remounted,
           "main",3,2,"last"),Ok("one\nB\nlast".len()));
        let len=reopened.open_source(a(),&mut remounted,"main",&mut out).unwrap();
        assert_eq!(&out[..len],b"one\nB\nlast");
    }
    #[test]
    fn m1_r056_invalid_multiline_edit_never_changes_durable_generation(){
        let mut fs=fresh();let mut p=ProjectTree::new();
        p.create(a(),&mut fs,"demo").unwrap();
        p.create_file(a(),&mut fs,"main",b"alpha\nbeta\n").unwrap();
        let generation=fs.generation();
        assert_eq!(p.replace_source_lines(a(),&mut fs,"main",3,1,"bad\n"),
                   Err(Error::Invalid));
        assert_eq!(p.replace_source_lines(a(),&mut fs,"main",1,0,"bad\n"),
                   Err(Error::Invalid));
        assert_eq!(p.replace_source_lines(a(),&mut fs,"main",usize::MAX,2,"bad\n"),
                   Err(Error::Invalid));
        assert_eq!(p.replace_source_lines(stale_id(),&mut fs,"main",1,1,"bad\n"),
                   Err(Error::Unauthorized));
        assert_eq!(p.replace_source_lines(a(),&mut fs,"main",1,1,
                    &"x".repeat(MAX_FILE_BYTES+1)),Err(Error::Full));
        assert_eq!(fs.generation(),generation);
        let mut out=[0u8;32];
        let n=p.open_source(a(),&mut fs,"main",&mut out).unwrap();
        assert_eq!(&out[..n],b"alpha\nbeta\n");
    }
    #[test]
    fn r058_move_unicode_source_lines_forward_and_backward_survives_remount(){
        let mut fs=fresh();let mut p=ProjectTree::new();
        p.create(a(),&mut fs,"demo").unwrap();
        p.create_file(a(),&mut fs,"main","one\nβeta\nthree\n四\n".as_bytes()).unwrap();
        let n=p.move_source_lines(a(),&mut fs,"main",2,1,5).unwrap();
        assert_eq!(n,"one\nthree\n四\nβeta\n".len());
        let mut out=[0u8;64];
        p.open_source(a(),&mut fs,"main",&mut out).unwrap();
        assert_eq!(&out[..n],"one\nthree\n四\nβeta\n".as_bytes());
        p.close(a()).unwrap();
        let mut remounted=HierVfs::mount(fs.into_device()).unwrap();
        let mut reopened=ProjectTree::new();
        reopened.open(a(),&mut remounted,"demo").unwrap();
        let size=reopened.move_source_lines(a(),&mut remounted,"main",4,1,1).unwrap();
        assert_eq!(size,n);
        let count=reopened.open_source(a(),&mut remounted,"main",&mut out).unwrap();
        assert_eq!(&out[..count],"βeta\none\nthree\n四\n".as_bytes());
    }
    #[test]
    fn r058_line_move_noop_invalid_and_stale_are_disk_atomic(){
        let mut fs=fresh();let mut p=ProjectTree::new();
        p.create(a(),&mut fs,"demo").unwrap();
        p.create_file(a(),&mut fs,"main",b"a\nb\nc\n").unwrap();
        let generation=fs.generation();
        assert_eq!(p.move_source_lines(a(),&mut fs,"main",2,1,2),Ok(6));
        assert_eq!(p.move_source_lines(a(),&mut fs,"main",2,1,3),Ok(6));
        assert_eq!(p.move_source_lines(a(),&mut fs,"main",0,1,2),Err(Error::Invalid));
        assert_eq!(p.move_source_lines(a(),&mut fs,"main",2,2,5),Err(Error::Invalid));
        assert_eq!(p.move_source_lines(stale_id(),&mut fs,"main",2,1,1),
                   Err(Error::Unauthorized));
        assert_eq!(fs.generation(),generation);
        let mut out=[0u8;8];
        assert_eq!(p.open_source(a(),&mut fs,"main",&mut out),Ok(6));
        assert_eq!(&out[..6],b"a\nb\nc\n");
    }
    #[test]
    fn r059_durable_unicode_indent_then_unindent_survives_remount(){
        let mut fs=fresh();let mut p=ProjectTree::new();
        p.create(a(),&mut fs,"demo").unwrap();
        p.create_file(a(),&mut fs,"main","alpha\nβeta\nγamma\nz".as_bytes()).unwrap();
        let n=p.adjust_source_indent(a(),&mut fs,"main",2,2,2).unwrap();
        let mut out=[0u8;64];
        assert_eq!(p.open_source(a(),&mut fs,"main",&mut out),Ok(n));
        assert_eq!(&out[..n],"alpha\n  βeta\n  γamma\nz".as_bytes());
        p.close(a()).unwrap();
        let mut disk=HierVfs::mount(fs.into_device()).unwrap();
        let mut reopened=ProjectTree::new();
        reopened.open(a(),&mut disk,"demo").unwrap();
        let n=reopened.adjust_source_indent(a(),&mut disk,"main",2,2,-2).unwrap();
        assert_eq!(reopened.open_source(a(),&mut disk,"main",&mut out),Ok(n));
        assert_eq!(&out[..n],"alpha\nβeta\nγamma\nz".as_bytes());
    }
    #[test]
    fn r059_bad_indent_and_oversize_leave_disk_generation_unchanged(){
        let mut fs=fresh();let mut p=ProjectTree::new();
        p.create(a(),&mut fs,"demo").unwrap();
        p.create_file(a(),&mut fs,"main",b"plain\n  indented\n").unwrap();
        let generation=fs.generation();
        assert_eq!(p.adjust_source_indent(a(),&mut fs,"main",1,2,-2),
                   Err(Error::Invalid));
        assert_eq!(p.adjust_source_indent(a(),&mut fs,"main",1,3,2),
                   Err(Error::Invalid));
        assert_eq!(p.adjust_source_indent(stale_id(),&mut fs,"main",1,1,2),
                   Err(Error::Unauthorized));
        assert_eq!(p.adjust_source_indent(a(),&mut fs,"main",1,1,0),
                   Err(Error::Invalid));
        assert_eq!(fs.generation(),generation);
        let mut out=[0u8;32];let n=p.open_source(a(),&mut fs,"main",&mut out).unwrap();
        assert_eq!(&out[..n],b"plain\n  indented\n");
        p.create_file(a(),&mut fs,"full",&[b'x';MAX_FILE_BYTES]).unwrap();
        let generation=fs.generation();
        assert_eq!(p.adjust_source_indent(a(),&mut fs,"full",1,1,1),
                   Err(Error::Full));
        assert_eq!(fs.generation(),generation);
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


/// R067: durable source-code snippet extraction to a NEW project file.
/// The source range is 1-based and consists of complete UTF-8 lines,
/// including existing newline separators. The destination is committed
/// once through the existing non-clobbering checksummed COW create.
impl ProjectTree{
 pub fn copy_source_lines<D:BlockDevice>(&mut self,caller:Identity,
  fs:&mut HierVfs<D>,source:&str,target:&str,first:usize,count:usize)
  ->Result<usize,Error>{
  self.check_fresh(caller,fs)?;
  if first==0||count==0||source==target{return Err(Error::Invalid);}
  let last=first.checked_add(count-1).ok_or(Error::Invalid)?;
  let (from,from_len)=self.child(source)?;
  let (to,to_len)=self.child(target)?;
  let mut bytes=[0u8;MAX_FILE_BYTES];
  let len=fs.read(Self::str_path(&from,from_len),&mut bytes)?;
  let text=core::str::from_utf8(&bytes[..len]).map_err(|_|Error::Invalid)?;
  let mut start=None;let mut finish=None;let mut cursor=0usize;
  for (index,row) in text.split_inclusive('\n').enumerate(){
   let line=index+1;
   if line==first{start=Some(cursor);}
   cursor+=row.len();
   if line==last{finish=Some(cursor);break;}
  }
  let (Some(a),Some(b))=(start,finish)else{return Err(Error::Invalid)};
  fs.create(Self::str_path(&to,to_len),&bytes[a..b])?;
  self.generation=fs.generation();
  Ok(b-a)
 }
}
#[cfg(test)]
mod r067_project_extract_tests{
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
 fn id()->Identity{Identity{pid:8,generation:4}}
 fn setup()->(ProjectTree,HierVfs<Disk>){
  let mut fs=HierVfs::format_blank(Disk{data:[[0;SECTOR_BYTES];100]}).unwrap();
  let mut p=ProjectTree::new();
  p.create(id(),&mut fs,"demo").unwrap();
  p.create_file(id(),&mut fs,"src","α\nβ\nend".as_bytes()).unwrap();
  (p,fs)
 }
 #[test]fn extracts_whole_utf8_rows_and_reboots_without_modifying_source(){
  let (mut project,mut fs)=setup();let before=fs.generation();
  assert_eq!(project.copy_source_lines(id(),&mut fs,"src","slice",2,2),Ok(6));
  assert_eq!(fs.generation(),before+1);
  let mut mounted=HierVfs::mount(fs.into_device()).unwrap();
  let mut out=[0u8;32];
  assert_eq!(mounted.read("/work/demo/slice",&mut out),Ok(6));
  assert_eq!(&out[..6],"β\nend".as_bytes());
  assert_eq!(mounted.read("/work/demo/src",&mut out),Ok(9));
  assert_eq!(&out[..9],"α\nβ\nend".as_bytes());
 }
 #[test]fn wrong_rows_and_existing_target_leave_both_files_untouched(){
  let (mut p,mut fs)=setup();p.create_file(id(),&mut fs,"exists",b"safe").unwrap();
  let generation=fs.generation();
  assert_eq!(p.copy_source_lines(id(),&mut fs,"src","new",0,1),Err(Error::Invalid));
  assert_eq!(p.copy_source_lines(id(),&mut fs,"src","new",2,5),Err(Error::Invalid));
  assert_eq!(p.copy_source_lines(id(),&mut fs,"src","src",1,1),Err(Error::Invalid));
  assert_eq!(p.copy_source_lines(id(),&mut fs,"src","../oops",1,1),Err(Error::Invalid));
  assert_eq!(p.copy_source_lines(id(),&mut fs,"src","exists",1,1),
   Err(Error::Fs(FsError::Exists)));
  assert_eq!(fs.generation(),generation);
  let mut out=[0u8;12];
  assert_eq!(fs.read("/work/demo/exists",&mut out),Ok(4));
  assert_eq!(&out[..4],b"safe");
 }
 #[test]fn stale_volume_and_recycled_pid_must_not_copy_project_data(){
  let (mut p,mut fs)=setup();let generation=fs.generation();
  assert_eq!(p.copy_source_lines(Identity{generation:5,..id()},
    &mut fs,"src","copy",1,1),Err(Error::Unauthorized));
  fs.create("/external",b"other").unwrap();
  assert_eq!(p.copy_source_lines(id(),&mut fs,"src","copy",1,1),Err(Error::Stale));
  assert_eq!(fs.generation(),generation+1);
 }
}


/// R068: exact byte compare-and-save on a durable project file. The
/// project generation AND original data must agree before any COW write.
impl ProjectTree{
 pub fn save_file_if_bytes<D:BlockDevice>(&mut self,caller:Identity,
  fs:&mut HierVfs<D>,name:&str,expected:&[u8],next:&[u8])
  ->Result<usize,Error>{
  self.check_fresh(caller,fs)?;
  if expected.len()>MAX_FILE_BYTES||next.len()>MAX_FILE_BYTES{
   return Err(Error::Full);
  }
  let (path,n)=self.child(name)?;
  let path=Self::str_path(&path,n);
  let mut current=[0u8;MAX_FILE_BYTES];
  let size=fs.read(path,&mut current)?;
  if size!=expected.len()||&current[..size]!=expected{
   return Err(Error::Stale);
  }
  if expected==next{return Ok(size);}
  fs.overwrite(path,next)?;
  self.generation=fs.generation();
  Ok(next.len())
 }
}
#[cfg(test)]
mod r068_project_compare_save_tests{
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
 fn id()->Identity{Identity{pid:8,generation:4}}
 fn setup()->(ProjectTree,HierVfs<Disk>){
  let mut fs=HierVfs::format_blank(Disk{sectors:[[0;SECTOR_BYTES];100]}).unwrap();
  let mut p=ProjectTree::new();
  p.create(id(),&mut fs,"demo").unwrap();
  p.create_file(id(),&mut fs,"main",b"old code").unwrap();
  (p,fs)
 }
 #[test]fn exact_old_bytes_commit_new_source_across_remount(){
  let (mut p,mut fs)=setup();let before=fs.generation();
  assert_eq!(p.save_file_if_bytes(id(),&mut fs,"main",b"old code",b"new code"),
   Ok(8));
  assert_eq!(fs.generation(),before+1);
  let mut mounted=HierVfs::mount(fs.into_device()).unwrap();
  let mut out=[0u8;16];
  assert_eq!(mounted.read("/work/demo/main",&mut out),Ok(8));
  assert_eq!(&out[..8],b"new code");
 }
 #[test]fn wrong_expected_bytes_and_identical_noop_do_not_advance_generation(){
  let (mut p,mut fs)=setup();let before=fs.generation();
  assert_eq!(p.save_file_if_bytes(id(),&mut fs,"main",b"other",b"bad"),
    Err(Error::Stale));
  assert_eq!(p.save_file_if_bytes(id(),&mut fs,"main",b"old code",b"old code"),
    Ok(8));
  assert_eq!(fs.generation(),before);
  let mut out=[0u8;16];fs.read("/work/demo/main",&mut out).unwrap();
  assert_eq!(&out[..8],b"old code");
 }
 #[test]fn foreign_pid_bad_path_and_external_write_fail_closed(){
  let (mut p,mut fs)=setup();let before=fs.generation();
  assert_eq!(p.save_file_if_bytes(Identity{generation:5,..id()},
   &mut fs,"main",b"old code",b"bad"),Err(Error::Unauthorized));
  assert_eq!(p.save_file_if_bytes(id(),&mut fs,"../bad",b"old code",b"bad"),
   Err(Error::Invalid));
  fs.create("/external",b"data").unwrap();
  assert_eq!(p.save_file_if_bytes(id(),&mut fs,"main",b"old code",b"bad"),
   Err(Error::Stale));
  assert_eq!(fs.generation(),before+1);
 }
}


/// R069: replace complete ASCII programming identifiers in a durable
/// UTF-8 project source file. This is lexical, not language-parser aware:
/// comments and string literals are included, but identifier prefixes are
/// never silently replaced. One successful refactor = one COW generation.
impl ProjectTree {
    pub fn rename_source_identifier<D: BlockDevice>(&mut self, caller: Identity,
        fs: &mut HierVfs<D>, name: &str, old: &str, new: &str
    ) -> Result<usize, Error> {
        self.check_fresh(caller, fs)?;
        fn valid_identifier(id: &str) -> bool {
            let b = id.as_bytes();
            !b.is_empty() && (b[0].is_ascii_alphabetic() || b[0] == b'_')
                && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_')
        }
        fn word(c: u8) -> bool { c.is_ascii_alphanumeric() || c == b'_' }
        if !valid_identifier(old) || !valid_identifier(new) { return Err(Error::Invalid); }
        let (p, length) = self.child(name)?;
        let path = Self::str_path(&p, length);
        let mut previous = [0u8; MAX_FILE_BYTES];
        let size = fs.read(path, &mut previous)?;
        let source = core::str::from_utf8(&previous[..size]).map_err(|_| Error::Invalid)?;
        let mut staged = [0u8; MAX_FILE_BYTES];
        let mut pos = 0usize;
        let mut written = 0usize;
        let mut matches = 0usize;
        while pos < size {
            let last = pos.checked_add(old.len()).ok_or(Error::Full)?;
            let found = last <= size && source[pos..].starts_with(old)
                && (pos == 0 || !word(previous[pos - 1]))
                && (last == size || !word(previous[last]));
            if found {
                let end = written.checked_add(new.len()).ok_or(Error::Full)?;
                if end > MAX_FILE_BYTES { return Err(Error::Full); }
                staged[written..end].copy_from_slice(new.as_bytes());
                written = end;
                pos = last;
                matches += 1;
            } else {
                let width = source[pos..].chars().next().ok_or(Error::Invalid)?.len_utf8();
                let end = written.checked_add(width).ok_or(Error::Full)?;
                if end > MAX_FILE_BYTES { return Err(Error::Full); }
                staged[written..end].copy_from_slice(&previous[pos..pos + width]);
                written = end;
                pos += width;
            }
        }
        if matches == 0 || (written == size && staged[..written] == previous[..size]) {
            return Ok(matches);
        }
        fs.overwrite(path, &staged[..written])?;
        self.generation = fs.generation();
        Ok(matches)
    }
}

#[cfg(test)]
mod r069_source_identifier_tests {
    use super::*;
    use crate::block::{BlockError, SECTOR_BYTES};
    struct Disk { sectors: [[u8; SECTOR_BYTES]; 100] }
    impl BlockDevice for Disk {
        fn sector_count(&self) -> u64 { 100 }
        fn read_sector(&mut self, lba: u64, out: &mut [u8; SECTOR_BYTES]) -> Result<(), BlockError> {
            *out = *self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?; Ok(())
        }
        fn write_sector(&mut self, lba: u64, bytes: &[u8; SECTOR_BYTES]) -> Result<(), BlockError> {
            *self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)? = *bytes; Ok(())
        }
    }
    fn user() -> Identity { Identity { pid: 8, generation: 4 } }
    fn setup() -> (ProjectTree, HierVfs<Disk>) {
        let mut fs = HierVfs::format_blank(Disk { sectors: [[0; SECTOR_BYTES]; 100] }).unwrap();
        let mut p = ProjectTree::new();
        p.create(user(), &mut fs, "demo").unwrap();
        p.create_file(user(), &mut fs, "main", "foo foobar foo_2 (foo)\nα_foo\n".as_bytes()).unwrap();
        (p, fs)
    }
    #[test]
    fn renames_whole_identifiers_and_survives_remount() {
        let (mut p, mut fs) = setup();
        let before = fs.generation();
        assert_eq!(p.rename_source_identifier(user(), &mut fs, "main", "foo", "bar"), Ok(2));
        assert_eq!(fs.generation(), before + 1);
        let mut fs = HierVfs::mount(fs.into_device()).unwrap();
        let mut out = [0; 64];
        let expected = "bar foobar foo_2 (bar)\nα_foo\n".as_bytes();
        assert_eq!(fs.read("/work/demo/main", &mut out), Ok(expected.len()));
        assert_eq!(&out[..expected.len()], expected);
    }
    #[test]
    fn invalid_identifier_and_repeated_refactor_do_not_commit() {
        let (mut p, mut fs) = setup();
        let gen = fs.generation();
        assert_eq!(p.rename_source_identifier(user(), &mut fs, "main", "foo", "foo"), Ok(2));
        assert_eq!(p.rename_source_identifier(user(), &mut fs, "main", "foo.bar", "new"), Err(Error::Invalid));
        assert_eq!(p.rename_source_identifier(user(), &mut fs, "main", "foobarX", "new"), Ok(0));
        assert_eq!(fs.generation(), gen);
    }
    #[test]
    fn stale_session_and_bad_path_leave_disk_unchanged() {
        let (mut p, mut fs) = setup();
        let before = fs.generation();
        assert_eq!(p.rename_source_identifier(Identity { generation: 5, ..user() },
            &mut fs, "main", "foo", "x"), Err(Error::Unauthorized));
        assert_eq!(p.rename_source_identifier(user(), &mut fs, "../bad", "foo", "x"), Err(Error::Invalid));
        fs.create("/foreign", b"data").unwrap();
        assert_eq!(p.rename_source_identifier(user(), &mut fs, "main", "foo", "x"), Err(Error::Stale));
        assert_eq!(fs.generation(), before + 1);
    }
}


/// R070: report one-based source line numbers matching a literal Unicode
/// query in the actual mounted project file. Each row is reported once,
/// and the output buffer is not mutated until every capacity check passes.
impl ProjectTree {
    pub fn find_source_lines<D: BlockDevice>(&self, caller: Identity,
        fs: &mut HierVfs<D>, name: &str, needle: &str,
        out: &mut [usize]) -> Result<usize, Error> {
        self.check_fresh(caller, fs)?;
        if needle.is_empty() || needle.contains('\n') || needle.contains('\r') {
            return Err(Error::Invalid);
        }
        let (p, n) = self.child(name)?;
        let mut bytes = [0u8; MAX_FILE_BYTES];
        let count = fs.read(Self::str_path(&p, n), &mut bytes)?;
        let source = core::str::from_utf8(&bytes[..count]).map_err(|_| Error::Invalid)?;
        let total = source.split_terminator('\n')
            .filter(|row| row.contains(needle)).count();
        if out.len() < total { return Err(Error::Full); }
        let mut index = 0usize;
        for (row_number, row) in source.split_terminator('\n').enumerate() {
            if row.contains(needle) {
                out[index] = row_number + 1;
                index += 1;
            }
        }
        Ok(total)
    }
}

#[cfg(test)]
mod r070_project_source_lines_tests {
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
    fn id() -> Identity { Identity { pid: 7, generation: 2 } }
    fn setup() -> (HierVfs<Disk>, ProjectTree) {
        let mut fs = HierVfs::format_blank(Disk { sectors: [[0; SECTOR_BYTES]; 100] }).unwrap();
        let mut p = ProjectTree::new();
        p.create(id(), &mut fs, "demo").unwrap();
        p.create_file(id(), &mut fs, "main", "α foo\nbeta\nα α\n".as_bytes()).unwrap();
        (fs, p)
    }
    #[test]
    fn unicode_line_matches_survive_disk_remount() {
        let (fs, mut p) = setup();
        let mut fs = HierVfs::mount(fs.into_device()).unwrap();
        p.refresh(id(), &mut fs).unwrap();
        let mut rows = [0usize; 3];
        assert_eq!(p.find_source_lines(id(), &mut fs, "main", "α", &mut rows), Ok(2));
        assert_eq!(&rows[..2], &[1, 3]);
        assert_eq!(p.find_source_lines(id(), &mut fs, "main", "missing", &mut rows), Ok(0));
    }
    #[test]
    fn small_output_bad_query_and_stale_owner_are_atomic() {
        let (mut fs, p) = setup();
        let before = fs.generation();
        let mut rows = [99usize; 1];
        assert_eq!(p.find_source_lines(id(), &mut fs, "main", "α", &mut rows), Err(Error::Full));
        assert_eq!(rows, [99]);
        assert_eq!(p.find_source_lines(id(), &mut fs, "main", "", &mut rows), Err(Error::Invalid));
        assert_eq!(p.find_source_lines(id(), &mut fs, "main", "a\nb", &mut rows), Err(Error::Invalid));
        assert_eq!(p.find_source_lines(
            Identity { generation: 3, ..id() }, &mut fs, "main", "α", &mut rows),
            Err(Error::Unauthorized));
        assert_eq!(rows, [99]);
        assert_eq!(fs.generation(), before);
    }
}

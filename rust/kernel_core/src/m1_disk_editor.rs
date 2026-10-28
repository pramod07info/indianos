//! R019 WS04/WS02: disk-backed developer text buffer over durable HierVfs.
//! This service edits real file bytes, refusing stale writes and preserving
//! committed data on simulated crashes. It does not yet draw an IDE window.
//! The caller Identity MUST be derived from a trusted scheduler/session.
use crate::batch5_session::Identity;
use crate::block::BlockDevice;
use crate::durablefs::{FsError, MAX_FILE_BYTES};
use crate::hier_vfs::HierVfs;
use crate::vfs_path::normalize_absolute;

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
}
impl DiskEditor {
    pub const fn new() -> Self {
        Self { owner: None, path: [0; EDITOR_PATH_MAX], path_len: 0,
            data: [0; MAX_FILE_BYTES], used: 0, baseline: 0, dirty: false }
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
        self.data = staged;
        self.used = new_len;
        self.dirty = true;
        Ok(new_len)
    }
    pub fn append(&mut self, caller: Identity, data: &[u8]) -> Result<usize, Error> {
        self.replace(caller, self.used, self.used, data)
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
            fs.overwrite(self.pathname(), &self.data[..self.used])?;
            self.baseline = fs.generation();
            self.dirty = false;
        }
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
}

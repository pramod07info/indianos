//! IndianOS initramfs v1, immutable and uncompressed.
//!
//! Complete bounded validation runs before any file pointer is returned.
//! No allocator, filesystem driver, kernel heap, or dynamic archives.

use core::{ptr, slice};

const MAX_FILES: usize = 16;
const MAX_NAME: usize = 128;
const MAX_BYTES: usize = 32 * 1024 * 1024;
const HEADER: usize = 16;
const ENTRY_HEADER: usize = 8;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Ramfile {
    pub bytes: *const u8,
    pub length: u32,
}

fn le16(data: &[u8], at: usize) -> Option<u16> {
    let b = data.get(at..at + 2)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}
fn le32(data: &[u8], at: usize) -> Option<u32> {
    let b = data.get(at..at + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn path_valid(name: &[u8]) -> bool {
    if name.len() < 2 || name.len() > MAX_NAME ||
        name[0] != b'/' || name[name.len() - 1] == b'/' {
        return false;
    }
    let mut segment_start = 1;
    for index in 1..=name.len() {
        if index == name.len() || name[index] == b'/' {
            let component = &name[segment_start..index];
            if component.is_empty() || component == b"." || component == b".." {
                return false;
            }
            segment_start = index + 1;
            continue;
        }
        let ch = name[index];
        if !(ch.is_ascii_alphanumeric() || ch == b'-' ||
             ch == b'_' || ch == b'.') {
            return false;
        }
    }
    true
}

/// Return None for invalid archive, Some(None) for valid archive with no
/// matching file, or Some(Some(file)) after validating every archive entry.
fn walk(bytes: &[u8], wanted: Option<&[u8]>) -> Option<Option<Ramfile>> {
    if bytes.len() < HEADER || bytes.len() > MAX_BYTES ||
       bytes.get(..8)? != b"INDRAM01" {
        return None;
    }
    let count = le32(bytes, 8)? as usize;
    if count == 0 || count > MAX_FILES ||
       le32(bytes, 12)? as usize != bytes.len() {
        return None;
    }
    if let Some(w) = wanted {
        if !path_valid(w) { return None; }
    }
    let mut seen = [(0usize, 0usize); MAX_FILES];
    let mut cursor = HEADER;
    let mut found: Option<Ramfile> = None;
    for index in 0..count {
        let header = bytes.get(cursor..cursor.checked_add(ENTRY_HEADER)?)?;
        let name_len = le16(header, 0)? as usize;
        let flags = le16(header, 2)?;
        let content_len = le32(header, 4)?;
        if flags != 0 { return None; }
        cursor += ENTRY_HEADER;
        let name = bytes.get(cursor..cursor.checked_add(name_len)?)?;
        if !path_valid(name) { return None; }
        for &(previous_start, previous_size) in &seen[..index] {
            if previous_size == name.len() &&
               bytes.get(previous_start..previous_start + previous_size)? == name {
                return None; // full archive is invalid, not merely this entry
            }
        }
        seen[index] = (cursor, name_len);
        cursor += name_len;
        let start = cursor;
        cursor = cursor.checked_add(content_len as usize)?;
        if bytes.get(start..cursor).is_none() { return None; }
        if wanted == Some(name) {
            found = Some(Ramfile {
                // SAFETY: start <= bytes.len(); the caller must retain
                // this immutable buffer for the returned borrowed pointer.
                bytes: unsafe { bytes.as_ptr().add(start) },
                length: content_len,
            });
        }
    }
    if cursor != bytes.len() { return None; }
    Some(found)
}

/// # Safety
/// \`archive\` is NULL or a valid readable kernel buffer of \`size\` bytes.
#[no_mangle]
pub unsafe extern "C" fn indianos_initramfs_validate(
    archive: *const u8, size: usize,
) -> bool {
    if archive.is_null() || !(HEADER..=MAX_BYTES).contains(&size) {
        return false;
    }
    walk(unsafe { slice::from_raw_parts(archive, size) }, None).is_some()
}

/// # Safety
/// The caller provides valid \`archive\`, \`path\` (NUL-terminated within
/// MAX_NAME+1 bytes), and writable \`out\` pointers or NULL. The returned
/// file borrows the archive bytes and must not outlive them.
#[no_mangle]
pub unsafe extern "C" fn indianos_initramfs_lookup(
    archive: *const u8, size: usize, path: *const core::ffi::c_char,
    out: *mut Ramfile,
) -> bool {
    if archive.is_null() || path.is_null() || out.is_null() ||
       !(HEADER..=MAX_BYTES).contains(&size) {
        return false;
    }
    let mut name_size = 0;
    while name_size <= MAX_NAME {
        let b = unsafe { *path.add(name_size) };
        if b == 0 { break; }
        name_size += 1;
    }
    if name_size > MAX_NAME { return false; }
    let name = unsafe { slice::from_raw_parts(path.cast::<u8>(), name_size) };
    let bytes = unsafe { slice::from_raw_parts(archive, size) };
    let Some(Some(candidate)) = walk(bytes, Some(name)) else { return false; };
    unsafe { ptr::write(out, candidate); }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{size_of, align_of};
    fn make_archive(paths: &[(&[u8], &[u8])]) -> std::vec::Vec<u8> {
        let mut out = std::vec::Vec::from(&b"INDRAM01"[..]);
        out.extend_from_slice(&(paths.len() as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        for (name, content) in paths {
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&(content.len() as u32).to_le_bytes());
            out.extend_from_slice(name);
            out.extend_from_slice(content);
        }
        let size = out.len() as u32;
        out[12..16].copy_from_slice(&size.to_le_bytes());
        out
    }

    #[test]
    fn same_layout_and_valid_borrowed_file() {
        assert_eq!(size_of::<Ramfile>(), 16);
        assert_eq!(align_of::<Ramfile>(), 8);
        let archive = make_archive(&[
            (b"/bin/init", b"first"), (b"/bin/worker", b"second"),
        ]);
        assert!(walk(&archive, None).is_some());
        let found = walk(&archive, Some(b"/bin/worker")).unwrap().unwrap();
        let actual = unsafe { slice::from_raw_parts(found.bytes, found.length as usize) };
        assert_eq!(actual, b"second");
        assert!(walk(&archive, Some(b"/bin/missing")).unwrap().is_none());
    }

    #[test]
    fn reject_duplicates_bad_names_truncation_or_trailing_bytes() {
        let valid = make_archive(&[(b"/bin/init", b"first")]);
        assert!(walk(&valid, None).is_some());
        assert!(walk(&valid[..valid.len()-1], None).is_none());
        let mut extra = valid.clone();
        extra.push(0);
        assert!(walk(&extra, None).is_none());
        let duplicate = make_archive(&[(b"/bin/init", b"first"), (b"/bin/init", b"second")]);
        assert!(walk(&duplicate, None).is_none());
        for bad in [&b"/bin/../oops"[..], &b"/bin//init"[..], &b"/"[..], &b"/bin/./init"[..], &b"/bin/space here"[..]] {
            assert!(walk(&make_archive(&[(bad, b"x")]), None).is_none());
        }
        assert!(walk(&valid, Some(b"../../secret")).is_none());
    }
}

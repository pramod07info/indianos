//! Safe, no-heap absolute-path normalization for a future VFS.
//!
//! No filesystem access occurs here. The normalized bytes borrow neither the
//! input nor global state; callers receive the written length and must use
//! only output[..len]. Errors leave the caller's output buffer untouched.
//! Dot-dot traversal is rejected rather than resolved, including at root.
pub const MAX_COMPONENT_BYTES: usize = 63;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathError {
    Empty,
    Relative,
    EmbeddedNul,
    ParentTraversal,
    ComponentTooLong,
    OutputTooSmall,
    LengthOverflow,
}

/// Normalize an absolute path into a caller-provided fixed byte buffer.
///
/// Repeated slashes and `.` segments are removed. `..` is always rejected.
/// The root path is `/`. Component limits count bytes, not Unicode chars.
/// The output is not NUL-terminated. This function never allocates.
pub fn normalize_absolute(path: &str, output: &mut [u8]) -> Result<usize, PathError> {
    let bytes = path.as_bytes();
    if bytes.is_empty() { return Err(PathError::Empty); }
    if bytes[0] != b'/' { return Err(PathError::Relative); }
    if bytes.contains(&0) { return Err(PathError::EmbeddedNul); }

    // Validate and size the complete path first, so an error never exposes
    // a partially normalized pathname in a previously populated buffer.
    let mut length = 1usize;
    let mut segments = 0usize;
    let mut cursor = 1usize;
    while let Some((start, end)) = next_component(bytes, &mut cursor) {
        let component = &bytes[start..end];
        if component == b"." { continue; }
        if component == b".." { return Err(PathError::ParentTraversal); }
        if component.len() > MAX_COMPONENT_BYTES {
            return Err(PathError::ComponentTooLong);
        }
        let separator = usize::from(segments > 0);
        length = length.checked_add(separator)
            .and_then(|value| value.checked_add(component.len()))
            .ok_or(PathError::LengthOverflow)?;
        segments += 1;
    }
    if output.len() < length { return Err(PathError::OutputTooSmall); }

    output[0] = b'/';
    let mut written = 1usize;
    let mut second_cursor = 1usize;
    let mut copied = 0usize;
    while let Some((start, end)) = next_component(bytes, &mut second_cursor) {
        let component = &bytes[start..end];
        if component == b"." { continue; }
        if copied > 0 {
            output[written] = b'/';
            written += 1;
        }
        let finish = written + component.len();
        output[written..finish].copy_from_slice(component);
        written = finish;
        copied += 1;
    }
    Ok(written)
}

/// Return the next nonempty component without ever allocating or indexing
/// outside the input. All components are separated by ASCII slash.
fn next_component(input: &[u8], cursor: &mut usize) -> Option<(usize, usize)> {
    while *cursor < input.len() && input[*cursor] == b'/' {
        *cursor += 1;
    }
    if *cursor == input.len() { return None; }
    let start = *cursor;
    while *cursor < input.len() && input[*cursor] != b'/' {
        *cursor += 1;
    }
    Some((start, *cursor))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_dot_and_repeated_slashes() {
        let mut output = [0xcc; 32];
        for path in ["/", "////", "/././", "//.///"] {
            let n = normalize_absolute(path, &mut output).unwrap();
            assert_eq!(&output[..n], b"/");
        }
        let n = normalize_absolute("//alpha///./beta/.", &mut output).unwrap();
        assert_eq!(&output[..n], b"/alpha/beta");
    }

    #[test]
    fn traversal_relative_and_nul_are_rejected_atomically() {
        let mut output = [0x55; 32];
        for (input, error) in [
            ("", PathError::Empty),
            ("abc", PathError::Relative),
            ("./hello", PathError::Relative),
            ("/a/../b", PathError::ParentTraversal),
            ("/..", PathError::ParentTraversal),
            ("//./../", PathError::ParentTraversal),
            ("/a\0b", PathError::EmbeddedNul),
        ] {
            assert_eq!(normalize_absolute(input, &mut output), Err(error));
            assert_eq!(output, [0x55; 32]);
        }
    }

    #[test]
    fn exact_capacity_preserves_outside_output_and_no_truncation() {
        let mut exact = [0xa5; 4];
        let written = normalize_absolute("/abc", &mut exact).unwrap();
        assert_eq!(written, 4);
        assert_eq!(&exact, b"/abc");
        let mut short = [0x33; 3];
        assert_eq!(normalize_absolute("/abc", &mut short), Err(PathError::OutputTooSmall));
        assert_eq!(short, [0x33; 3]);
        assert_eq!(normalize_absolute("/", &mut []), Err(PathError::OutputTooSmall));
        let mut longer = [0xbb; 10];
        let n = normalize_absolute("/a//b", &mut longer).unwrap();
        assert_eq!(&longer[..n], b"/a/b");
        assert!(longer[n..].iter().all(|byte| *byte == 0xbb));
    }

    #[test]
    fn component_limits_apply_to_bytes_including_utf8() {
        let mut output = [0xee; 90];
        let valid = format!("/{}", "a".repeat(MAX_COMPONENT_BYTES));
        let n = normalize_absolute(&valid, &mut output).unwrap();
        assert_eq!(n, 64);
        let invalid = format!("/{}", "a".repeat(MAX_COMPONENT_BYTES + 1));
        assert_eq!(normalize_absolute(&invalid, &mut output), Err(PathError::ComponentTooLong));
        let utf8 = format!("/{}", "é".repeat(32)); // 64 encoded bytes
        assert_eq!(normalize_absolute(&utf8, &mut output), Err(PathError::ComponentTooLong));
    }

    #[test]
    fn rejected_suffix_cannot_modify_buffer() {
        let mut output = [9u8; 70];
        let bad = format!("/ok/{}/end", "a".repeat(64));
        assert_eq!(normalize_absolute(&bad, &mut output), Err(PathError::ComponentTooLong));
        assert_eq!(output, [9; 70]);
    }
}


/// Normalize both operands before checking a namespace boundary. Root denotes
/// the whole tree. This helper only checks spelling; it grants no I/O rights.
pub fn is_within_tree(path: &str, root: &str) -> Result<bool, PathError> {
    let mut candidate = [0u8; 256];
    let mut boundary = [0u8; 256];
    let n = normalize_absolute(path, &mut candidate)?;
    let m = normalize_absolute(root, &mut boundary)?;
    Ok(m == 1 || (n >= m && candidate[..m] == boundary[..m]
        && (n == m || candidate[m] == b'/')))
}

/// Append one safe component without allocating. On any error, output is
/// unchanged, including when a normalized parent would exceed capacity.
pub fn join_child(parent: &str, child: &str, output: &mut [u8])
    -> Result<usize, PathError>
{
    if child.is_empty() { return Err(PathError::Empty); }
    if child.as_bytes().contains(&0) { return Err(PathError::EmbeddedNul); }
    if child == "." || child == ".." { return Err(PathError::ParentTraversal); }
    if child.contains('/') { return Err(PathError::Relative); }
    if child.len() > MAX_COMPONENT_BYTES { return Err(PathError::ComponentTooLong); }
    let mut normalized = [0u8; 256];
    let n = normalize_absolute(parent, &mut normalized)?;
    let separator = usize::from(n != 1);
    let size = n.checked_add(separator)
        .and_then(|x| x.checked_add(child.len()))
        .ok_or(PathError::LengthOverflow)?;
    if size > output.len() { return Err(PathError::OutputTooSmall); }
    output[..n].copy_from_slice(&normalized[..n]);
    if separator != 0 { output[n] = b'/'; }
    output[n + separator..size].copy_from_slice(child.as_bytes());
    Ok(size)
}

/// Copy the normalized parent of a non-root pathname, preserving output on
/// failure. No filesystem lookup is performed.
pub fn copy_parent(path: &str, output: &mut [u8]) -> Result<usize, PathError> {
    let mut normalized = [0u8; 256];
    let len = normalize_absolute(path, &mut normalized)?;
    if len == 1 { return Err(PathError::Empty); }
    let end = normalized[..len].iter().rposition(|&b| b == b'/')
        .ok_or(PathError::Relative)?;
    let parent_len = end.max(1);
    if parent_len > output.len() { return Err(PathError::OutputTooSmall); }
    output[..parent_len].copy_from_slice(&normalized[..parent_len]);
    Ok(parent_len)
}

pub fn canonical_equals(a: &str, b: &str) -> Result<bool, PathError> {
    let mut first = [0u8; 256];
    let mut second = [0u8; 256];
    let left = normalize_absolute(a, &mut first)?;
    let right = normalize_absolute(b, &mut second)?;
    Ok(first[..left] == second[..right])
}

pub fn path_depth(path: &str) -> Result<usize, PathError> {
    let mut normalized = [0u8; 256];
    let len = normalize_absolute(path, &mut normalized)?;
    if len == 1 { return Ok(0); }
    Ok(normalized[..len].iter().filter(|&&b| b == b'/').count())
}

#[cfg(test)]
mod r114_path_boundary_tests {
    use super::*;

    #[test]
    fn prefix_bypass_and_traversal_fail_closed() {
        assert_eq!(is_within_tree("/shared/note", "/shared"), Ok(true));
        assert_eq!(is_within_tree("//shared/./note", "/shared/"), Ok(true));
        assert_eq!(is_within_tree("/sharedness/note", "/shared"), Ok(false));
        assert_eq!(is_within_tree("/shared", "/shared"), Ok(true));
        assert_eq!(is_within_tree("/etc", "/"), Ok(true));
        assert_eq!(is_within_tree("/shared/../etc", "/shared"), Err(PathError::ParentTraversal));
        assert_eq!(is_within_tree("shared/note", "/shared"), Err(PathError::Relative));
    }
    #[test]
    fn joining_and_parent_selection_remain_atomic() {
        let mut out = [0xa5; 32];
        assert_eq!(join_child("/shared//.", "note", &mut out), Ok(12));
        assert_eq!(&out[..12], b"/shared/note");
        let before = out;
        assert_eq!(join_child("/shared", "../keys", &mut out), Err(PathError::Relative));
        assert_eq!(out, before);
        assert_eq!(join_child("/shared", "..", &mut out), Err(PathError::ParentTraversal));
        assert_eq!(out, before);
        assert_eq!(copy_parent("/shared/note", &mut out), Ok(7));
        assert_eq!(&out[..7], b"/shared");
        assert_eq!(copy_parent("/a", &mut out), Ok(1));
        assert_eq!(out[0], b'/');
        let before = out;
        assert_eq!(copy_parent("/", &mut out), Err(PathError::Empty));
        assert_eq!(out, before);
    }
    #[test]
    fn canonical_comparisons_and_depth_work_without_heap() {
        assert_eq!(canonical_equals("//a/./b/", "/a/b"), Ok(true));
        assert_eq!(canonical_equals("/a/b", "/a/bb"), Ok(false));
        assert_eq!(canonical_equals("/a/../b", "/b"), Err(PathError::ParentTraversal));
        assert_eq!(path_depth("/"), Ok(0));
        assert_eq!(path_depth("//a//./b"), Ok(2));
        assert_eq!(path_depth("bad"), Err(PathError::Relative));
    }
    #[test]
    fn exact_capacity_and_non_ascii_components() {
        let mut out = [0x33u8; 4];
        assert_eq!(join_child("/", "abc", &mut out), Ok(4));
        assert_eq!(&out, b"/abc");
        assert_eq!(join_child("/x", "abc", &mut out), Err(PathError::OutputTooSmall));
        assert_eq!(&out, b"/abc");
        let mut utf8 = [0u8; 16];
        assert_eq!(join_child("/", "é", &mut utf8), Ok(3));
        assert_eq!(&utf8[..3], "/é".as_bytes());
    }
}



/// R115: translate a canonical path into a namespace-relative byte slice.
/// This does not grant file access. Both paths are validated before touching
/// output, and a sibling-prefix match is never considered in the namespace.
pub fn copy_relative_under_root(
    path: &str, root: &str, output: &mut [u8],
) -> Result<Option<usize>, PathError> {
    let mut candidate = [0u8; 256];
    let mut boundary = [0u8; 256];
    let n = normalize_absolute(path, &mut candidate)?;
    let m = normalize_absolute(root, &mut boundary)?;
    let inside = m == 1 || (n >= m && candidate[..m] == boundary[..m]
        && (n == m || candidate[m] == b'/'));
    if !inside { return Ok(None); }
    let offset = if m == 1 { 1 } else if n == m { m } else { m + 1 };
    let length = n - offset;
    if output.len() < length { return Err(PathError::OutputTooSmall); }
    output[..length].copy_from_slice(&candidate[offset..n]);
    Ok(Some(length))
}

#[cfg(test)]
mod r115_namespace_relative_tests {
    use super::*;

    #[test]
    fn canonicalize_each_side_and_retain_byte_exact_relative_path() {
        let mut out = [0xa5u8; 32];
        assert_eq!(copy_relative_under_root("//shared/./docs//report", "/shared/",
            &mut out), Ok(Some(11)));
        assert_eq!(&out[..11], b"docs/report");
        assert!(out[11..].iter().all(|b| *b == 0xa5));
        assert_eq!(copy_relative_under_root("/shared", "/shared", &mut out),
            Ok(Some(0)));
        assert_eq!(copy_relative_under_root("/", "/", &mut out),
            Ok(Some(0)));
        assert_eq!(copy_relative_under_root("/a/b", "/", &mut out),
            Ok(Some(3)));
        assert_eq!(&out[..3], b"a/b");
    }

    #[test]
    fn sibling_prefix_and_parent_namespace_denied_without_output_mutation() {
        let mut out = [0x51u8; 20];
        for (path, root) in [
            ("/sharedness/private", "/shared"),
            ("/share", "/shared"),
            ("/sh", "/shared"),
            ("/etc", "/shared"),
            ("/a", "/a/b"),
        ] {
            assert_eq!(copy_relative_under_root(path, root, &mut out), Ok(None));
            assert_eq!(out, [0x51; 20]);
        }
    }

    #[test]
    fn malformed_paths_roots_and_short_buffers_fail_atomically() {
        let mut out = [0x33u8; 4];
        for (path, root, error) in [
            ("/shared/../etc", "/shared", PathError::ParentTraversal),
            ("/shared/item", "/shared/..", PathError::ParentTraversal),
            ("shared/item", "/shared", PathError::Relative),
            ("/shared/item", "shared", PathError::Relative),
            ("/shared/\0secret", "/shared", PathError::EmbeddedNul),
            ("/shared/item", "/shared/\0", PathError::EmbeddedNul),
            ("/shared/12345", "/shared", PathError::OutputTooSmall),
        ] {
            assert_eq!(copy_relative_under_root(path, root, &mut out),
                Err(error));
            assert_eq!(out, [0x33; 4]);
        }
    }

    #[test]
    fn exact_capacity_utf8_and_component_byte_limits() {
        let mut exact = [0u8; 3];
        assert_eq!(copy_relative_under_root("/αβ", "/", &mut exact),
            Err(PathError::OutputTooSmall));
        assert_eq!(exact, [0; 3]);
        let mut utf8 = [0x77u8; 5];
        assert_eq!(copy_relative_under_root("/αβ", "/", &mut utf8),
            Ok(Some(4)));
        assert_eq!(&utf8[..4], "αβ".as_bytes());
        assert_eq!(utf8[4], 0x77);
        let long = format!("/shared/{}", "a".repeat(MAX_COMPONENT_BYTES + 1));
        assert_eq!(copy_relative_under_root(&long, "/shared", &mut utf8),
            Err(PathError::ComponentTooLong));
        assert_eq!(utf8[4], 0x77);
    }
}

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

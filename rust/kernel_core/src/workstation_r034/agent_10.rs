//! R034/10: Workspace file filter by extension.
use super::Error;
pub fn extension(path:&[u8],want:&[u8])->Result<bool,Error>{if want.is_empty()||want.len()>16||!want.starts_with(b"."){return Err(Error::Invalid);}let file=path.rsplit(|b|*b==b'/').next().ok_or(Error::Invalid)?;Ok(file.ends_with(want)&&file.len()>want.len())}
#[cfg(test)] mod tests {use super::*;#[test]fn functional_and_boundary_cases(){assert_eq!(extension(b"/p/main.rs",b".rs"),Ok(true));assert_eq!(extension(b"/p/main.md",b".rs"),Ok(false));}}

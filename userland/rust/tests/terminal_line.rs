#[path="../src/terminal_line.rs"]
mod terminal_line;
use terminal_line::{Action,Line,MAX_NOTE};
fn enter(line:&mut Line,bytes:&[u8])->Action{
 for &byte in bytes{line.feed(byte);}line.feed(b'\n')
}
#[test]fn editing_preserves_literal_payload_and_resets_after_submit(){
 let mut line=Line::new();
 assert_eq!(enter(&mut line,b"write note  "),Action::Write{
  bytes:*b"note  \0\0\0\0\0\0\0\0\0\0\0\0\0\0",len:6});
 assert_eq!(enter(&mut line,b"cat"),Action::Read);
 for &b in b"write NOTX"{line.feed(b);}
 line.feed(8);line.feed(b'E');
 let mut expected=[0;MAX_NOTE];expected[..4].copy_from_slice(b"NOTE");
 assert_eq!(line.feed(b'\n'),Action::Write{bytes:expected,len:4});
}
#[test]fn oversized_input_never_executes_a_truncated_command(){
 let mut line=Line::new();
 for &b in b"write "{line.feed(b);}
 for _ in 0..80{line.feed(b'X');}
 for _ in 0..40{line.feed(8);}
 assert_eq!(line.feed(b'\n'),Action::Invalid);
 assert_eq!(enter(&mut line,b"cat"),Action::Read);
}
#[test]fn unknown_private_paths_empty_and_long_writes_are_rejected(){
 let mut line=Line::new();
 for candidate in [b"".as_slice(),b"write ",
  b"rm /shared/readme",b"cat /etc/secret",b"write 123456789012345678901"]{
  assert_eq!(enter(&mut line,candidate),Action::Invalid);
 }
 assert_eq!(line.feed(b'\t'),Action::Leave);
 assert_eq!(enter(&mut line,b"cat"),Action::Read);
}
#[test]fn display_is_bounded_and_backspace_never_underflows(){
 let mut line=Line::new();line.feed(8);
 let mut out=[0;22];assert_eq!(line.display(&mut out),2);
 for &b in b"write 12345678901234567890"{line.feed(b);}
 assert_eq!(line.display(&mut out),22);
 assert_eq!(&out,b"> 12345678901234567890");
 line.feed(b'\t');assert_eq!(line.display(&mut out),2);
}

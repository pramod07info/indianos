#[path="../src/terminal_fs.rs"]
mod terminal_fs;
#[path="../src/terminal_line.rs"]
mod terminal_line;
use terminal_line::{Action,Line,MAX_NOTE};
#[test]fn integrity_loss_poison_survives_focus_loss_and_rejects_truncated_commands(){
 for front in [1,2]{
  let mut line=Line::new();
  for &byte in b"write secret"{line.feed(byte);}
  // Ordinary keys are focus-gated, but the broker forwards transport loss
  // independently, even while this editor is suspended in Files.
  assert_eq!(terminal_line::accepts_input(front,true,b'x'),front==2);
  assert_eq!(line.feed(0),Action::Invalid);
  for &byte in b"write injected"{line.feed(byte);}
  assert_eq!(line.feed(b'\n'),Action::Invalid);
  for &byte in b"write recovered"{line.feed(byte);}
  let Action::Write{bytes,len}=line.feed(b'\n')else{panic!("failed to recover after loss")};
  assert_eq!(&bytes[..len],b"recovered");
 }
}
fn enter(line:&mut Line,bytes:&[u8])->Action{
 for &byte in bytes{line.feed(byte);}line.feed(b'\n')
}
#[test]fn suspended_editor_cannot_capture_another_windows_physical_keys(){
 let mut line=Line::new();
 assert!(!terminal_line::accepts_input(1,false,b'\n'));
 assert!(terminal_line::accepts_input(2,false,b'\n'));
 assert!(!terminal_line::accepts_input(2,false,b'z'));
 for &byte in b"write kept"{
  assert!(terminal_line::accepts_input(2,true,byte));line.feed(byte);
 }
 // Physical focus switches to Files while the editor remains open. No key,
 // including Enter/Tab/backspace, reaches the suspended Terminal parser.
 for front in [0,1,3,u32::MAX]{
  for byte in [b'z',b'\n',b'\t',8]{
   assert!(!terminal_line::accepts_input(front,true,byte));
  }
 }
 assert!(terminal_line::accepts_input(2,true,b'\n'));
 let Action::Write{bytes,len}=line.feed(b'\n')else{panic!("lost suspended line")};
 assert_eq!(&bytes[..len],b"kept");
}
#[test]fn named_file_commands_roundtrip_without_changing_legacy_commands(){
 use terminal_fs::{Command,Name};
 let name=Name::parse(b"note").unwrap();
 let other=Name::parse(b"saved").unwrap();
 for (text,expected) in [(b"touch note".as_slice(),Command::Touch(name)),
   (b"cat note",Command::Read(name)),(b"rm note",Command::Remove(name)),
   (b"mkdir note",Command::Mkdir(name)),
   (b"mv note saved",Command::Move{from:name,to:other})]{
  let mut line=Line::new();
  assert_eq!(enter(&mut line,text),Action::Filesystem(expected));
  let mut wire=[0xff;64];
  assert_eq!(terminal_fs::encode(expected,&mut wire),Some(64));
  assert_eq!(terminal_fs::decode(&wire),Some(expected));
 }
 let mut line=Line::new();
 assert!(matches!(enter(&mut line,b"write note=unchanged  "),Action::Write{len:16,..}));
 assert_eq!(enter(&mut line,b"cat"),Action::Read);
}
#[test]fn named_put_preserves_equals_and_trailing_spaces_and_rejects_invalid_payload(){
 use terminal_fs::Command;
 let mut line=Line::new();
 let Action::Filesystem(command)=enter(&mut line,b"put note=a=b  ")else{panic!("put rejected")};
 let Command::Put{bytes,len,..}=command else{panic!("wrong action")};
 assert_eq!(&bytes[..len as usize],b"a=b  ");
 let mut wire=[0;64];assert_eq!(terminal_fs::encode(command,&mut wire),Some(64));
 assert_eq!(terminal_fs::decode(&wire),Some(command));
 for text in [b"put note=".as_slice(),b"put note=x\n",b"put note=\x7f",
   b"put note=123456789012345678901",b"put =x",b"put ../key=x"]{
  assert_eq!(terminal_fs::parse(text),None);
 }
}
#[test]fn filename_validation_cannot_escape_the_shared_single_component(){
 use terminal_fs::Name;
 for candidate in [b"".as_slice(),b".",b"..",b"../key",b"a/b",b"/boot",
   b"a b",b"a\0b",b"a;b",b"abcdefghijklmnop"]{
  assert!(Name::parse(candidate).is_none(),"{candidate:?}");
 }
 for byte in 0..=255u8{
  let valid=byte.is_ascii_alphanumeric()||matches!(byte,b'_'|b'-');
  assert_eq!(Name::parse(&[byte]).is_some(),valid,"{byte}");
 }
 let name=Name::parse(b"abcdefghijklmn-").unwrap();
 let mut path=[0xff;24];assert_eq!(name.path(&mut path),23);
 assert_eq!(&path,b"/shared/abcdefghijklmn-\0");
 let name=Name::parse(b"a.b").unwrap();
 assert_eq!(name.path(&mut path),11);assert_eq!(&path[..11],b"/shared/a.b");
 assert_eq!(&path[11..],&[0;13]);
}
#[test]fn broker_rejects_malformed_wire_lengths_padding_and_unknown_operations(){
 let command=terminal_fs::parse(b"mv note saved").unwrap();
 let mut valid=[0;64];terminal_fs::encode(command,&mut valid).unwrap();
 for length in 0..64{assert_eq!(terminal_fs::decode(&valid[..length]),None);}
 let mut oversized=valid.to_vec();oversized.push(0);assert_eq!(terminal_fs::decode(&oversized),None);
 for index in (5..8).chain(12..23).chain(28..64){
  let mut bad=valid;bad[index]=1;
  assert_eq!(terminal_fs::decode(&bad),None,"padding {index}");
 }
 for (index,expected) in [(2,4),(3,5),(4,0)]{
  for value in 0..=255u8{
   let mut bad=valid;bad[index]=value;
   assert_eq!(terminal_fs::decode(&bad).is_some(),value==expected,"field {index}={value}");
  }
 }
 for opcode in [0,1,2,3,4,6,7,255]{
  let mut bad=valid;bad[1]=opcode;assert_eq!(terminal_fs::decode(&bad),None);
 }
 for index in [0,8,9,23,24]{
  let mut bad=valid;bad[index]=b'/';assert_eq!(terminal_fs::decode(&bad),None);
 }
}
#[test]fn broker_rechecks_put_bytes_even_when_a_client_bypasses_editor_parse(){
 use terminal_fs::{Command,Name};
 let command=terminal_fs::parse(b"put note=hello").unwrap();
 let mut wire=[0;64];terminal_fs::encode(command,&mut wire).unwrap();
 for byte in (0..=255u8).filter(|byte|!(0x20..=0x7e).contains(byte)){
  let mut bad=wire;bad[40]=byte;assert_eq!(terminal_fs::decode(&bad),None);
 }
 let name=Name::parse(b"note").unwrap();
 for len in [0,21,255]{
  let invalid=Command::Put{name,bytes:[b'x';20],len};
  let mut output=[0xff;64];assert_eq!(terminal_fs::encode(invalid,&mut output),None);
  assert_eq!(output,[0;64]);
 }
}
#[test]fn named_commands_do_not_recover_from_editor_overflow_or_control_loss(){
 let mut line=Line::new();
 for _ in 0..33{line.feed(b'x');}
 for _ in 0..33{line.feed(8);}
 assert_eq!(enter(&mut line,b"rm note"),Action::Invalid);
 assert!(matches!(enter(&mut line,b"touch note"),Action::Filesystem(_)));
 line.feed(0);assert_eq!(enter(&mut line,b"put note=forged"),Action::Invalid);
 assert!(matches!(enter(&mut line,b"put note=valid"),Action::Filesystem(_)));
 let mut long=b"put abcdefghijklmno=".to_vec();long.extend_from_slice(&[b'x';20]);
 assert_eq!(enter(&mut line,&long),Action::Invalid);
}
#[test]fn invalid_transport_bytes_poison_the_whole_command_until_delimiter(){
 for byte in (0..=255u8).filter(|b|!matches!(b,8|9|10|13|32..=127)){
  let mut line=Line::new();
  for &b in b"write pre"{line.feed(b);}
  assert_eq!(line.feed(byte),Action::Invalid);
  for &b in b"\x08write forged"{line.feed(b);}
  assert_eq!(line.feed(b'\n'),Action::Invalid);
  assert_eq!(enter(&mut line,b"cat"),Action::Read);
 }
 let mut line=Line::new();line.feed(0);
 assert_eq!(line.feed(b'\t'),Action::Leave);
 assert_eq!(enter(&mut line,b"ls"),Action::List);
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
#[test]fn directory_and_stat_commands_have_no_client_selected_paths(){
 let mut line=Line::new();
 assert_eq!(enter(&mut line,b"ls"),Action::List);
 assert_eq!(enter(&mut line,b"stat"),Action::Stat);
 for candidate in [b"ls /".as_slice(),b"ls /etc",b"stat /etc/secret",b"stat ",b"ls;cat"]{
  assert_eq!(enter(&mut line,candidate),Action::Invalid);
 }
}
#[test]fn directory_rendering_rejects_truncation_and_controls_without_partial_output(){
 use terminal_line::directory_text;
 let mut out=[0xff;MAX_NOTE];
 assert_eq!(directory_text(b"f:readme\n",&mut out),Some(8));
 assert_eq!(&out[..8],b"f:readme");
 assert_eq!(directory_text(b"d:a\nf:b\n",&mut out),Some(7));
 assert_eq!(&out[..7],b"d:a f:b");
 assert_eq!(directory_text(b"",&mut out),Some(0));
 for bad in [b"f:readme".as_slice(),b"f:a\x1b\n",b"f:a\r\n",b"f:abcdefghijklmnopqrs\n"]{
  assert_eq!(directory_text(bad,&mut out),None);
  assert_eq!(out,[0;MAX_NOTE]);
 }
}

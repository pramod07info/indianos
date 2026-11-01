//! M1 (first integration slice): secure Terminal/Files operations on a
//! mounted, checksummed copy-on-write *block-backed* hierarchical VFS.
//! Unlike Batch 7's RAM store, files survive a device remount. This layer
//! never formats a device, never probes hardware, and never trusts raw PID
//! arguments from unprivileged callers. Kernel syscall ingress must supply
//! authenticated process identity, user buffers and monotonic tick values.
use crate::block::BlockDevice;
use crate::hier_vfs::{HierVfs,MAX_TRANSFER};
use crate::durablefs::FsError;
use crate::batch5_session::Identity;
use crate::batch6_security::{Access,FILE_READ,FILE_WRITE};
use crate::vfs_path::normalize_absolute;
use crate::fs_write_guard::{WriteGuard,Error as GuardError};

pub const MAX_COMMAND:usize=192;
/// The actual persistent Terminal's supported verbs. Shared with the live
/// Desktop's Tab completion so hidden/unknown commands are never suggested.
pub const COMMAND_VERBS:&[&str]=&[
 "help","pwd","tools","ls","cat","page","write","append","mkdir","mv",
 "cp","rm","mkfile","truncate","head","count","rmdir","mvdir",
 "pread","pwrite","lines","line","hash","stat","tail","find","grep","cmp","diffat",
 "touch","copynew","copyrange","matchpage","textpage","idpage","caswrite","casappend","castruncate","casclone","wordpage","idcount","replace","appendln","health","hex","slice","concat","cmprange","bytefreq","patchhex","patchrange","copylines","wc","headlines","rfind","taillines","linespan","linehits","sourceview","sourcecontext","textprofile","blankrows","indentmap","eolcheck",
];
/// Complete exactly one printable lower-case verb prefix. Ambiguous matches,
/// empty prefixes and user-supplied paths never cause completion or I/O.
pub fn complete_verb(prefix:&[u8])->Option<&'static str>{
 if prefix.is_empty()||prefix.iter().any(|b|!b.is_ascii_lowercase()){
  return None;
 }
 let mut found=None;
 for &word in COMMAND_VERBS{
  if word.as_bytes().starts_with(prefix){
   if found.is_some(){return None;}
   found=Some(word);
  }
 }
 found
}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{Invalid,Denied,Io(FsError),OutputTooSmall,Unknown}
fn path<'a>(input:&str,buffer:&'a mut [u8;64])->Result<&'a str,Error>{
 let n=normalize_absolute(input,buffer).map_err(|_|Error::Invalid)?;
 if n<2{return Err(Error::Invalid);}
 core::str::from_utf8(&buffer[..n]).map_err(|_|Error::Invalid)
}
/// S1.1: protect reserved system namespaces at the M1 Terminal mutation
/// boundary too. This entrypoint bypasses the S0 FileHandles table, so it
/// must independently validate both source and destination before writes.
/// This is not yet a universal VFS/raw-block authorization layer.
fn writable_path(p:&str)->Result<(),Error>{
 match WriteGuard::allow_path(p){
  Ok(())=>Ok(()),
  Err(GuardError::InvalidPath)=>Err(Error::Invalid),
  Err(GuardError::Protected|GuardError::RateLimited)=>Err(Error::Denied),
 }
}
fn publish(data:&[u8],out:&mut[u8])->Result<usize,Error>{
 if data.len()>out.len(){return Err(Error::OutputTooSmall);}
 out[..data.len()].copy_from_slice(data);Ok(data.len())
}
/// Bounded numeric response; no allocator, locale or partial caller writes.
fn publish_number(mut value:u64,out:&mut[u8])->Result<usize,Error>{
 let mut digits=[0u8;20];
 let mut i=digits.len();
 loop{
  i-=1;digits[i]=b'0'+(value%10) as u8;value/=10;
  if value==0{break;}
 }
 publish(&digits[i..],out)
}
/// Deterministic content fingerprint for diagnostics, NOT a signature.
fn publish_hash(bytes:&[u8],out:&mut[u8])->Result<usize,Error>{
 let hash=bytes.iter().fold(0xcbf29ce484222325u64,
  |h,b|(h^(*b as u64)).wrapping_mul(0x100000001b3));
 let mut digits=[0u8;16];
 for (i,place) in digits.iter_mut().enumerate(){
  let nibble=((hash>>((15-i)*4))&0xf) as u8;
  *place=if nibble<10{b'0'+nibble}else{b'a'+nibble-10};
 }
 publish(&digits,out)
}
pub fn command<D:BlockDevice,const N:usize>(
 fs:&mut HierVfs<D>,auth:&Access<N>,caller:Identity,now:u64,
 input:&[u8],out:&mut[u8])->Result<usize,Error>{
 if input.is_empty()||input.len()>MAX_COMMAND||input.iter().any(|c|
  *c!=b' '&&!c.is_ascii_graphic()){return Err(Error::Invalid);}
 // Ignore leading command indentation, but never trim literal payloads:
 // trailing spaces are file/search data, not an implicit edit request.
 let entered=core::str::from_utf8(input).map_err(|_|Error::Invalid)?.trim_start();
 let line=entered.trim_end();
 if line=="help"{return publish(b"pwd ls /dir cat /path write /path text append /path text mkdir /dir mv /old /new cp /src /dst rm /path mkfile /path truncate /path head /path count rmdir /dir mvdir /old /new pread /file offset count pwrite /file@offset text",out);}
 if line=="pwd"{return publish(b"/",out);}
 if line=="tools"{return publish(b"count lines line tail find grep hash cmp diffat stat touch copynew replace appendln health hex slice concat cmprange bytefreq patchhex patchrange copylines wc headlines rfind taillines linespan linehits sourceview sourcecontext textprofile [tools more]",out);}
 if line=="tools more"{return publish(b"blankrows indentmap eolcheck",out);}
 if line=="health"{
  auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
  let health=fs.mounted_health().map_err(Error::Io)?;
  let mut scratch=[0u8;96];
  let status=match health.redundancy{
   crate::durablefs::Redundancy::Healthy=>b"healthy ".as_slice(),
   crate::durablefs::Redundancy::Degraded=>b"degraded ".as_slice(),
  };
  scratch[..status.len()].copy_from_slice(status);
  let mut at=status.len();
  for (prefix,value) in [
   (b"gen:".as_slice(),health.current_generation),
   (b" copies:".as_slice(),health.valid_copies as u64),
  ]{
   scratch[at..at+prefix.len()].copy_from_slice(prefix);at+=prefix.len();
   let n=publish_number(value,&mut scratch[at..])?;at+=n;
  }
  return publish(&scratch[..at],out);
 }
 let (verb,arguments)=entered.split_once(' ').ok_or(Error::Unknown)?;
 let raw=if matches!(verb,"write"|"append"|"appendln"|"pwrite"|
    "replace"|"find"|"grep"|"caswrite"|"casappend"){
  arguments
 }else{arguments.trim_end()};
 let mut name=[0u8;64];
 match verb{
  "count"|"lines"|"hash"|"stat"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   let p=path(raw,&mut name)?;
   let mut bytes=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let n=fs.read(p,&mut bytes).map_err(Error::Io)?;
   match verb{
    "count"=>publish_number(n as u64,out),
    "hash"=>publish_hash(&bytes[..n],out),
    "lines"=>{
     core::str::from_utf8(&bytes[..n]).map_err(|_|Error::Invalid)?;
     let count=bytes[..n].iter().filter(|b|**b==b'\n').count()
      +usize::from(n>0&&bytes[n-1]!=b'\n');
     publish_number(count as u64,out)
    },
    // Expose only volume generation and byte length; no raw metadata.
    "stat"=>{
     let mut buf=[0u8;64];let mut at=0;
     for (prefix,value) in [(b"bytes:".as_slice(),n as u64),
                             (b" generation:".as_slice(),fs.generation())]{
      buf[at..at+prefix.len()].copy_from_slice(prefix);at+=prefix.len();
      let len=publish_number(value,&mut buf[at..])?;at+=len;
     }
     publish(&buf[..at],out)
    },
    _=>Err(Error::Unknown)
   }
  }
  // Return one UTF-8 row from an authenticated persistent source file.
  // Never publish an output prefix if UTF-8, row bounds or destination
  // capacity validation fails. A final newline adds no phantom row.

  // Bounded real-file output page; caller may resume at offset+bytes read.
  // R077: resumable literal match offsets for real UTF-8 source files.
  // Offset pages are staged before publication; no out-of-bounds scans.
  "matchpage"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   let mut args=raw.split_ascii_whitespace();
   let source=args.next().ok_or(Error::Invalid)?;
   let needle=args.next().ok_or(Error::Invalid)?;
   let start=args.next().ok_or(Error::Invalid)?
       .parse::<usize>().map_err(|_|Error::Invalid)?;
   let limit=args.next().ok_or(Error::Invalid)?
       .parse::<usize>().map_err(|_|Error::Invalid)?;
   if needle.is_empty()||limit==0||limit>16||args.next().is_some(){
    return Err(Error::Invalid);
   }
   let end=start.checked_add(limit).ok_or(Error::Invalid)?;
   let p=path(source,&mut name)?;
   let mut bytes=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let n=fs.read(p,&mut bytes).map_err(Error::Io)?;
   let text=core::str::from_utf8(&bytes[..n]).map_err(|_|Error::Invalid)?;
   let mut staged=[0u8;MAX_TRANSFER];
   let mut used=0usize;let mut total=0usize;
   for (at,_) in text.match_indices(needle){
    if total>=start&&total<end{
     used+=publish_number(at as u64,&mut staged[used..])?;
     staged[used]=b'\n';used+=1;
    }
    total+=1;
   }
   if start>total{return Err(Error::Invalid);}
   publish(&staged[..used],out)
  }
  // R077: create an immutable, checksum-protected slice of a source file.
  // The target is new-only; protected paths and missing grants fail closed.
  "copyrange"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   auth.require(caller,FILE_WRITE,now).map_err(|_|Error::Denied)?;
   let mut args=raw.split_ascii_whitespace();
   let source=args.next().ok_or(Error::Invalid)?;
   let target=args.next().ok_or(Error::Invalid)?;
   let offset=args.next().ok_or(Error::Invalid)?
       .parse::<u64>().map_err(|_|Error::Invalid)?;
   let count=args.next().ok_or(Error::Invalid)?
       .parse::<usize>().map_err(|_|Error::Invalid)?;
   if count==0||count>MAX_TRANSFER||args.next().is_some(){
    return Err(Error::Invalid);
   }
   let mut source_buffer=[0u8;64];let mut target_buffer=[0u8;64];
   let src=path(source,&mut source_buffer)?;
   let dst=path(target,&mut target_buffer)?;
   if src==dst{return Err(Error::Invalid);}
   writable_path(dst)?;
   let mut bytes=[0u8;MAX_TRANSFER];
   let n=fs.read_at(src,offset,&mut bytes[..count]).map_err(Error::Io)?;
   if n!=count{return Err(Error::Invalid);}
   fs.create(dst,&bytes[..n]).map_err(Error::Io)?;
   publish_number(n as u64,out)
  }
  // R078-09: UTF-8 boundary-safe source byte windows (no split glyphs).
  "textpage"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   let mut args=raw.split_ascii_whitespace();
   let source=args.next().ok_or(Error::Invalid)?;
   let offset=args.next().ok_or(Error::Invalid)?
       .parse::<usize>().map_err(|_|Error::Invalid)?;
   let count=args.next().ok_or(Error::Invalid)?
       .parse::<usize>().map_err(|_|Error::Invalid)?;
   if count==0||count>MAX_TRANSFER||args.next().is_some(){return Err(Error::Invalid);}
   let p=path(source,&mut name)?;
   let mut bytes=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let n=fs.read(p,&mut bytes).map_err(Error::Io)?;
   let source=core::str::from_utf8(&bytes[..n]).map_err(|_|Error::Invalid)?;
   let end=core::cmp::min(n,offset.checked_add(count).ok_or(Error::Invalid)?);
   let part=source.get(offset..end).ok_or(Error::Invalid)?;
   publish(part.as_bytes(),out)
  }
  // R078-10: whole-identifier matches with a resumable occurrence cursor.
  // Only ASCII identifiers are accepted; nonmatching UTF-8 bytes remain safe.
  "idpage"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   let mut args=raw.split_ascii_whitespace();
   let source=args.next().ok_or(Error::Invalid)?;
   let symbol=args.next().ok_or(Error::Invalid)?;
   let start=args.next().ok_or(Error::Invalid)?
       .parse::<usize>().map_err(|_|Error::Invalid)?;
   let count=args.next().ok_or(Error::Invalid)?
       .parse::<usize>().map_err(|_|Error::Invalid)?;
   if symbol.as_bytes().first().is_none_or(|b|
       !b.is_ascii_alphabetic()&&*b!=b'_')||
       !symbol.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_')||
       count==0||count>8||args.next().is_some(){return Err(Error::Invalid);}
   let end=start.checked_add(count).ok_or(Error::Invalid)?;
   let p=path(source,&mut name)?;
   let mut bytes=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let n=fs.read(p,&mut bytes).map_err(Error::Io)?;
   let source=core::str::from_utf8(&bytes[..n]).map_err(|_|Error::Invalid)?;
   let mut staged=[0u8;MAX_TRANSFER];
   let mut used=0usize;let mut seen=0usize;
   for (at,_) in source.match_indices(symbol){
    let word=|b:u8|b.is_ascii_alphanumeric()||b==b'_';
    if (at>0&&word(source.as_bytes()[at-1]))||
       (at+symbol.len()<n&&word(source.as_bytes()[at+symbol.len()])){continue;}
    if seen>=start&&seen<end {
     used+=publish_number(at as u64,&mut staged[used..])?;
     staged[used]=b'\n';used+=1;
    }
    seen=seen.checked_add(1).ok_or(Error::Invalid)?;
   }
   if start>seen{return Err(Error::Invalid);}
   publish(&staged[..used],out)
  }
  // R078-11: compare-and-swap whole file contents using FNV as an
  // optimistic conflict indicator, not an authentication hash/signature.
  "caswrite"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   auth.require(caller,FILE_WRITE,now).map_err(|_|Error::Denied)?;
   let (src,tail)=raw.split_once(' ').ok_or(Error::Invalid)?;
   let (expected,content)=tail.split_once(' ').ok_or(Error::Invalid)?;
   if expected.len()!=16||content.is_empty()||
       content.len()>crate::durablefs::MAX_FILE_BYTES{return Err(Error::Invalid);}
   let expected=u64::from_str_radix(expected,16).map_err(|_|Error::Invalid)?;
   let p=path(src,&mut name)?;
   writable_path(p)?;
   let mut original=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let n=fs.read(p,&mut original).map_err(Error::Io)?;
   let actual=original[..n].iter().fold(0xcbf29ce484222325u64,
        |h,b|(h^(*b as u64)).wrapping_mul(0x100000001b3));
   if expected!=actual{return Err(Error::Denied);}
   let mut digits=[0u8;20];
   let len=publish_number(content.len() as u64,&mut digits)?;
   if out.len()<len{return Err(Error::OutputTooSmall);}
   fs.overwrite(p,content.as_bytes()).map_err(Error::Io)?;
   publish(&digits[..len],out)
  }
  // R078-12: append only if the source byte length is still what the
  // editor observed; entire new file commits in one COW VFS generation.
  "casappend"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   auth.require(caller,FILE_WRITE,now).map_err(|_|Error::Denied)?;
   let (src,tail)=raw.split_once(' ').ok_or(Error::Invalid)?;
   let (expected,content)=tail.split_once(' ').ok_or(Error::Invalid)?;
   if content.is_empty(){return Err(Error::Invalid);}
   let expected=expected.parse::<usize>().map_err(|_|Error::Invalid)?;
   let p=path(src,&mut name)?;
   writable_path(p)?;
   let mut bytes=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let len=fs.read(p,&mut bytes).map_err(Error::Io)?;
   if len!=expected{return Err(Error::Denied);}
   let next=len.checked_add(content.len()).ok_or(Error::Invalid)?;
   if next>bytes.len(){return Err(Error::Invalid);}
   let mut digits=[0u8;20];
   let count=publish_number(next as u64,&mut digits)?;
   if out.len()<count{return Err(Error::OutputTooSmall);}
   bytes[len..next].copy_from_slice(content.as_bytes());
   fs.overwrite(p,&bytes[..next]).map_err(Error::Io)?;
   publish(&digits[..count],out)
  }

  // R079-09: COW truncate only when the on-disk length still matches.
  "castruncate"=>{
   auth.require(caller,FILE_READ|FILE_WRITE,now).map_err(|_|Error::Denied)?;
   let mut args=raw.split_ascii_whitespace();
   let file=args.next().ok_or(Error::Invalid)?;
   let prior=args.next().ok_or(Error::Invalid)?.parse::<usize>().map_err(|_|Error::Invalid)?;
   let next=args.next().ok_or(Error::Invalid)?.parse::<usize>().map_err(|_|Error::Invalid)?;
   if args.next().is_some()||next>crate::durablefs::MAX_FILE_BYTES{return Err(Error::Invalid);}
   let p=path(file,&mut name)?;writable_path(p)?;
   let observed=fs.file_len(p).map_err(Error::Io)?;
   if observed!=prior{return Err(Error::Denied);}
   let mut digits=[0u8;20];
   let n=publish_number(next as u64,&mut digits)?;
   if out.len()<n{return Err(Error::OutputTooSmall);}
   fs.truncate(p,next as u64).map_err(Error::Io)?;
   publish(&digits[..n],out)
  }
  // R079-10: optimistic, complete source clone to a new destination.
  // FNV is a conflict detector and does not authenticate downloaded code.
  "casclone"=>{
   auth.require(caller,FILE_READ|FILE_WRITE,now).map_err(|_|Error::Denied)?;
   let mut args=raw.split_ascii_whitespace();
   let source=args.next().ok_or(Error::Invalid)?;
   let target=args.next().ok_or(Error::Invalid)?;
   let expected=args.next().ok_or(Error::Invalid)?;
   if args.next().is_some()||expected.len()!=16{return Err(Error::Invalid);}
   let expected=u64::from_str_radix(expected,16).map_err(|_|Error::Invalid)?;
   let mut srcbuf=[0u8;64];let mut dstbuf=[0u8;64];
   let src=path(source,&mut srcbuf)?;
   let dst=path(target,&mut dstbuf)?;
   if src==dst{return Err(Error::Invalid);}
   writable_path(dst)?;
   let mut bytes=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let n=fs.read(src,&mut bytes).map_err(Error::Io)?;
   let digest=bytes[..n].iter().fold(0xcbf29ce484222325u64,
      |h,b|(h^(*b as u64)).wrapping_mul(0x100000001b3));
   if digest!=expected{return Err(Error::Denied);}
   let mut digits=[0u8;20];let count=publish_number(n as u64,&mut digits)?;
   if out.len()<count{return Err(Error::OutputTooSmall);}
   fs.create(dst,&bytes[..n]).map_err(Error::Io)?;
   publish(&digits[..count],out)
  }
  // R079-11: paginated UTF-8 word starts, not substring matches.
  "wordpage"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   let mut args=raw.split_ascii_whitespace();
   let source=args.next().ok_or(Error::Invalid)?;
   let start=args.next().ok_or(Error::Invalid)?.parse::<usize>().map_err(|_|Error::Invalid)?;
   let count=args.next().ok_or(Error::Invalid)?.parse::<usize>().map_err(|_|Error::Invalid)?;
   if count==0||count>16||args.next().is_some(){return Err(Error::Invalid);}
   let end=start.checked_add(count).ok_or(Error::Invalid)?;
   let p=path(source,&mut name)?;
   let mut bytes=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let n=fs.read(p,&mut bytes).map_err(Error::Io)?;
   let text=core::str::from_utf8(&bytes[..n]).map_err(|_|Error::Invalid)?;
   let mut staged=[0u8;MAX_TRANSFER];let mut used=0usize;
   let mut total=0usize;let mut previous_space=true;
   for (at,ch) in text.char_indices(){
    let space=ch.is_whitespace();
    if !space&&previous_space{
     if total>=start&&total<end{
      used+=publish_number(at as u64,&mut staged[used..])?;
      staged[used]=b'\n';used+=1;
     }
     total+=1;
    }
    previous_space=space;
   }
   if start>total{return Err(Error::Invalid);}
   publish(&staged[..used],out)
  }
  // R079-12: count complete ASCII identifiers, never their substrings.
  "idcount"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   let (file,symbol)=raw.split_once(' ').ok_or(Error::Invalid)?;
   if symbol.is_empty()||symbol.bytes().next().is_none_or(|b|
        !b.is_ascii_alphabetic()&&b!=b'_')||
      !symbol.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'){
      return Err(Error::Invalid);
   }
   let p=path(file,&mut name)?;
   let mut bytes=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let n=fs.read(p,&mut bytes).map_err(Error::Io)?;
   let text=core::str::from_utf8(&bytes[..n]).map_err(|_|Error::Invalid)?;
   let mut total=0usize;
   for (at,_) in text.match_indices(symbol){
    let word=|b:u8|b.is_ascii_alphanumeric()||b==b'_';
    if (at>0&&word(text.as_bytes()[at-1]))||
       (at+symbol.len()<n&&word(text.as_bytes()[at+symbol.len()])){continue;}
    total+=1;
   }
   publish_number(total as u64,out)
  }
  "page"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   let mut args=raw.split_ascii_whitespace();
   let source=args.next().ok_or(Error::Invalid)?;
   let offset=args.next().ok_or(Error::Invalid)?
       .parse::<u64>().map_err(|_|Error::Invalid)?;
   let count=args.next().ok_or(Error::Invalid)?
       .parse::<usize>().map_err(|_|Error::Invalid)?;
   if count==0||count>MAX_TRANSFER||args.next().is_some(){
    return Err(Error::Invalid);
   }
   if out.len()<count{return Err(Error::OutputTooSmall);}
   let p=path(source,&mut name)?;
   let mut bytes=[0u8;MAX_TRANSFER];
   let n=fs.read_at(p,offset,&mut bytes[..count]).map_err(Error::Io)?;
   publish(&bytes[..n],out)
  }

  "line"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   let (source,row)=raw.split_once(' ').ok_or(Error::Invalid)?;
   let number=row.parse::<usize>().map_err(|_|Error::Invalid)?;
   if number==0{return Err(Error::Invalid);}
   let p=path(source,&mut name)?;
   let mut bytes=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let count=fs.read(p,&mut bytes).map_err(Error::Io)?;
   let text=core::str::from_utf8(&bytes[..count]).map_err(|_|Error::Invalid)?;
   let selected=text.split_terminator('\n').nth(number-1)
    .ok_or(Error::Invalid)?;
   publish(selected.as_bytes(),out)
  }
  // Inspect a bounded range of real persistent file bytes as lower-case
  // hexadecimal, without shell expansion or untrusted binary terminal data.
  // All output is staged before publication; short buffers stay untouched.
  "hex"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   let mut args=raw.split_ascii_whitespace();
   let source=args.next().ok_or(Error::Invalid)?;
   let offset=args.next().ok_or(Error::Invalid)?
      .parse::<u64>().map_err(|_|Error::Invalid)?;
   let count=args.next().ok_or(Error::Invalid)?
      .parse::<usize>().map_err(|_|Error::Invalid)?;
   if args.next().is_some()||count==0||count>MAX_TRANSFER{
    return Err(Error::Invalid);
   }
   let p=path(source,&mut name)?;
   let mut bytes=[0u8;MAX_TRANSFER];
   let read=fs.read_at(p,offset,&mut bytes[..count]).map_err(Error::Io)?;
   let mut encoded=[0u8;MAX_TRANSFER*2];
   let alphabet=b"0123456789abcdef";
   for (i,&b) in bytes[..read].iter().enumerate(){
    encoded[2*i]=alphabet[(b>>4) as usize];
    encoded[2*i+1]=alphabet[(b&15) as usize];
   }
   publish(&encoded[..read*2],out)
  }
  // Line-oriented source preview, unlike the byte-oriented "head" verb.
  // UTF-8 and the ENTIRE output capacity are validated before publication.
  "headlines"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   let mut args=raw.split_ascii_whitespace();
   let source=args.next().ok_or(Error::Invalid)?;
   let count=args.next().ok_or(Error::Invalid)?
     .parse::<usize>().map_err(|_|Error::Invalid)?;
   if count==0||count>MAX_TRANSFER||args.next().is_some(){return Err(Error::Invalid);}
   let p=path(source,&mut name)?;
   let mut bytes=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let size=fs.read(p,&mut bytes).map_err(Error::Io)?;
   let text=core::str::from_utf8(&bytes[..size]).map_err(|_|Error::Invalid)?;
   let mut end=0usize;
   for line in text.split_inclusive('\n').take(count){end+=line.len();}
   publish(&bytes[..end],out)
  }
  // Return the byte offset of the LAST exact UTF-8 substring in a durable
  // file. Read-only, auth-gated and safe for downstream byte-offset tools.
  "rfind"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   let (source,needle)=raw.split_once(' ').ok_or(Error::Invalid)?;
   if needle.is_empty(){return Err(Error::Invalid);}
   let p=path(source,&mut name)?;
   let mut bytes=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let size=fs.read(p,&mut bytes).map_err(Error::Io)?;
   let text=core::str::from_utf8(&bytes[..size]).map_err(|_|Error::Invalid)?;
   match text.rfind(needle){
    Some(position)=>publish_number(position as u64,out),
    None=>publish(b"not found",out),
   }
  }
  // Preview the last N complete UTF-8 source rows, unlike byte-tail.
  // Validation and output-capacity checks finish before touching the caller.
  "taillines"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   let mut args=raw.split_ascii_whitespace();
   let source=args.next().ok_or(Error::Invalid)?;
   let requested=args.next().ok_or(Error::Invalid)?
      .parse::<usize>().map_err(|_|Error::Invalid)?;
   if requested==0||requested>MAX_TRANSFER||args.next().is_some(){
    return Err(Error::Invalid);
   }
   let p=path(source,&mut name)?;
   let mut bytes=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let count=fs.read(p,&mut bytes).map_err(Error::Io)?;
   let text=core::str::from_utf8(&bytes[..count]).map_err(|_|Error::Invalid)?;
   let rows=text.split_terminator('\n').count();
   let skip=rows.saturating_sub(requested);
   let start=text.split_inclusive('\n').take(skip)
      .map(|row|row.len()).sum::<usize>();
   publish(&bytes[start..count],out)
  }
  // Return a contiguous 1-based range of complete UTF-8 source rows.
  // Validate the entire range and output capacity before publishing bytes.
  "linespan"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   let mut args=raw.split_ascii_whitespace();
   let source=args.next().ok_or(Error::Invalid)?;
   let start=args.next().ok_or(Error::Invalid)?
      .parse::<usize>().map_err(|_|Error::Invalid)?;
   let count=args.next().ok_or(Error::Invalid)?
      .parse::<usize>().map_err(|_|Error::Invalid)?;
   if start==0||count==0||count>MAX_TRANSFER||args.next().is_some(){
    return Err(Error::Invalid);
   }
   let stop=start.checked_add(count).ok_or(Error::Invalid)?;
   let p=path(source,&mut name)?;
   let mut bytes=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let size=fs.read(p,&mut bytes).map_err(Error::Io)?;
   let source=core::str::from_utf8(&bytes[..size]).map_err(|_|Error::Invalid)?;
   let (mut pos,mut first,mut last,mut total)=(0usize,0usize,0usize,0usize);
   for (index,row) in source.split_inclusive('\n').enumerate(){
    let number=index+1;
    if number==start{first=pos;}
    pos+=row.len();
    if number>=start&&number<stop{last=pos;}
    total=number;
   }
   if total<stop-1{return Err(Error::Invalid);}
   publish(&bytes[first..last],out)
  }
  // Return one-based row numbers containing an exact printable substring.
  // Output is assembled fully before publication and never mutates storage.
  "linehits"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   let (source,needle)=raw.split_once(' ').ok_or(Error::Invalid)?;
   if needle.is_empty(){return Err(Error::Invalid);}
   let p=path(source,&mut name)?;
   let mut bytes=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let n=fs.read(p,&mut bytes).map_err(Error::Io)?;
   let text=core::str::from_utf8(&bytes[..n]).map_err(|_|Error::Invalid)?;
   let mut staged=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let mut written=0usize;
   for (index,line) in text.split_terminator('\n').enumerate(){
    if line.contains(needle){
     if written>0{
      if written==staged.len(){return Err(Error::OutputTooSmall);}
      staged[written]=b' ';written+=1;
     }
     let row=(index+1) as u64;
     let len=publish_number(row,&mut staged[written..])?;
     written+=len;
    }
   }
   if written==0{publish(b"not found",out)}
   else{publish(&staged[..written],out)}
  }
  // Present a developer source snippet with stable 1-based line numbers.
  // Every row and number is staged before the caller buffer is touched.
  "sourceview"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   let mut args=raw.split_ascii_whitespace();
   let file=args.next().ok_or(Error::Invalid)?;
   let start=args.next().ok_or(Error::Invalid)?
      .parse::<usize>().map_err(|_|Error::Invalid)?;
   let count=args.next().ok_or(Error::Invalid)?
      .parse::<usize>().map_err(|_|Error::Invalid)?;
   if start==0||count==0||count>MAX_TRANSFER||args.next().is_some(){
    return Err(Error::Invalid);
   }
   let stop=start.checked_add(count).ok_or(Error::Invalid)?;
   let p=path(file,&mut name)?;
   let mut content=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let size=fs.read(p,&mut content).map_err(Error::Io)?;
   let text=core::str::from_utf8(&content[..size]).map_err(|_|Error::Invalid)?;
   let mut staged=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let mut used=0usize;
   let mut selected=0usize;
   for (index,row) in text.split_inclusive('\n').enumerate(){
    let line=index+1;
    if line<start||line>=stop{continue;}
    let digits=publish_number(line as u64,&mut staged[used..])?;
    used+=digits;
    let next=used.checked_add(2).and_then(|n|n.checked_add(row.len()))
      .ok_or(Error::OutputTooSmall)?;
    if next>staged.len(){return Err(Error::OutputTooSmall);}
    staged[used]=b'|';staged[used+1]=b' ';used+=2;
    staged[used..used+row.len()].copy_from_slice(row.as_bytes());
    used+=row.len();selected+=1;
   }
   if selected!=count{return Err(Error::Invalid);}
   publish(&staged[..used],out)
  }
  // Numbered source matches with up to three surrounding rows each.
  // Mark every selected row first, deduplicating overlapping match windows,
  // and never publish a partial caller result.
  "sourcecontext"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   let mut args=raw.split_ascii_whitespace();
   let file=args.next().ok_or(Error::Invalid)?;
   let needle=args.next().ok_or(Error::Invalid)?;
   let radius=args.next().ok_or(Error::Invalid)?
      .parse::<usize>().map_err(|_|Error::Invalid)?;
   if needle.is_empty()||radius>3||args.next().is_some(){return Err(Error::Invalid);}
   let p=path(file,&mut name)?;
   let mut bytes=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let n=fs.read(p,&mut bytes).map_err(Error::Io)?;
   let source=core::str::from_utf8(&bytes[..n]).map_err(|_|Error::Invalid)?;
   let mut selected=[false;crate::durablefs::MAX_FILE_BYTES];
   let mut matched=false;
   for (i,row) in source.split_inclusive('\n').enumerate(){
    if row.contains(needle){
     matched=true;
     let last=(i+radius).min(selected.len()-1);
     for hit in i.saturating_sub(radius)..=last{selected[hit]=true;}
    }
   }
   if !matched{return publish(b"not found",out);}
   let mut staged=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let mut written=0usize;
   for (i,row) in source.split_inclusive('\n').enumerate(){
    if !selected[i]{continue;}
    let count=publish_number((i+1) as u64,&mut staged[written..])?;
    written+=count;
    let next=written.checked_add(2).and_then(|x|x.checked_add(row.len()))
      .ok_or(Error::OutputTooSmall)?;
    if next>staged.len(){return Err(Error::OutputTooSmall);}
    staged[written]=b'|';staged[written+1]=b' ';
    written+=2;
    staged[written..next].copy_from_slice(row.as_bytes());
    written=next;
   }
   publish(&staged[..written],out)
  }
  // UTF-8-aware persisted source statistics. Character count and
  // maxscalars count Unicode scalar values, NOT grapheme clusters or
  // terminal display cells. No partial output or write-side effects.
  "textprofile"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   if raw.is_empty()||raw.contains(' '){return Err(Error::Invalid);}
   let p=path(raw,&mut name)?;
   let mut bytes=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let n=fs.read(p,&mut bytes).map_err(Error::Io)?;
   let source=core::str::from_utf8(&bytes[..n]).map_err(|_|Error::Invalid)?;
   if source.as_bytes().contains(&0){return Err(Error::Invalid);}
   let mut rows=0usize;
   let mut longest=0usize;
   for row in source.split_inclusive('\n'){
    let content=row.strip_suffix('\n').unwrap_or(row);
    rows+=1;
    longest=longest.max(content.chars().count());
   }
   let values=[n as u64,source.chars().count() as u64,
       rows as u64,longest as u64];
   let labels=[b"bytes:".as_slice(),b" chars:".as_slice(),
       b" rows:".as_slice(),b" maxscalars:".as_slice()];
   let mut staged=[0u8;128];
   let mut written=0usize;
   for (label,value) in labels.iter().zip(values.iter()){
    let next=written.checked_add(label.len()).ok_or(Error::OutputTooSmall)?;
    if next>staged.len(){return Err(Error::OutputTooSmall);}
    staged[written..next].copy_from_slice(label);written=next;
    written+=publish_number(*value,&mut staged[written..])?;
   }
   publish(&staged[..written],out)
  }
  // R075: show 1-based blank or indentation-only source row numbers.
  // Never publish partially formatted results into the caller buffer.
  "blankrows"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   if raw.is_empty()||raw.contains(' '){return Err(Error::Invalid);}
   let p=path(raw,&mut name)?;
   let mut bytes=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let size=fs.read(p,&mut bytes).map_err(Error::Io)?;
   let source=core::str::from_utf8(&bytes[..size]).map_err(|_|Error::Invalid)?;
   let mut staged=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let mut written=0usize;
   for (i,row) in source.split_inclusive('\n').enumerate(){
    let body=row.strip_suffix('\n').unwrap_or(row);
    if body.bytes().all(|b|b==b' '||b==b'\t'){
     if written>0 {
      if written==staged.len(){return Err(Error::OutputTooSmall);}
      staged[written]=b' ';written+=1;
     }
     written+=publish_number((i+1) as u64,&mut staged[written..])?;
    }
   }
   if written==0{publish(b"not found",out)}
   else{publish(&staged[..written],out)}
  }
  // R075: numeric leading-space and leading-tab profile per Unicode row.
  "indentmap"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   if raw.is_empty()||raw.contains(' '){return Err(Error::Invalid);}
   let p=path(raw,&mut name)?;
   let mut bytes=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let size=fs.read(p,&mut bytes).map_err(Error::Io)?;
   let source=core::str::from_utf8(&bytes[..size]).map_err(|_|Error::Invalid)?;
   let mut staged=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let mut written=0usize;
   for (i,row) in source.split_inclusive('\n').enumerate(){
    if written>0{
     if written==staged.len(){return Err(Error::OutputTooSmall);}
     staged[written]=b' ';written+=1;
    }
    written+=publish_number((i+1) as u64,&mut staged[written..])?;
    if written==staged.len(){return Err(Error::OutputTooSmall);}
    staged[written]=b':';written+=1;
    let mut spaces=0usize;let mut tabs=0usize;
    for byte in row.bytes(){
     match byte{
      b' '=>spaces+=1,
      b'\t'=>tabs+=1,
      _=>break
     }
    }
    written+=publish_number(spaces as u64,&mut staged[written..])?;
    if written==staged.len(){return Err(Error::OutputTooSmall);}
    staged[written]=b':';written+=1;
    written+=publish_number(tabs as u64,&mut staged[written..])?;
   }
   if written==0{publish(b"empty",out)}
   else{publish(&staged[..written],out)}
  }
  // R075: source line-ending integrity metrics, no implicit conversion.
  "eolcheck"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   if raw.is_empty()||raw.contains(' '){return Err(Error::Invalid);}
   let p=path(raw,&mut name)?;
   let mut bytes=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let n=fs.read(p,&mut bytes).map_err(Error::Io)?;
   core::str::from_utf8(&bytes[..n]).map_err(|_|Error::Invalid)?;
   let mut lf=0usize;let mut crlf=0usize;let mut barecr=0usize;
   for (i,&byte) in bytes[..n].iter().enumerate(){
    if byte==b'\n'{
     lf+=1;
     if i>0&&bytes[i-1]==b'\r'{crlf+=1;}
    }
    if byte==b'\r' && (i+1==n||bytes[i+1]!=b'\n'){barecr+=1;}
   }
   let labels=[b"lf:".as_slice(),b" crlf:".as_slice(),
     b" barecr:".as_slice(),b" final:".as_slice()];
   let values=[lf as u64,crlf as u64,barecr as u64,
     u64::from(n>0&&bytes[n-1]==b'\n')];
   let mut staged=[0u8;128];let mut used=0usize;
   for (label,value) in labels.iter().zip(values.iter()){
    let end=used.checked_add(label.len()).ok_or(Error::OutputTooSmall)?;
    if end>staged.len(){return Err(Error::OutputTooSmall);}
    staged[used..end].copy_from_slice(label);used=end;
    used+=publish_number(*value,&mut staged[used..])?;
   }
   publish(&staged[..used],out)
  }
  "tail"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   let (source,requested)=raw.split_once(' ').ok_or(Error::Invalid)?;
   let count=requested.parse::<usize>().map_err(|_|Error::Invalid)?;
   if count==0||count>MAX_TRANSFER{return Err(Error::Invalid);}
   let p=path(source,&mut name)?;
   let mut bytes=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let n=fs.read(p,&mut bytes).map_err(Error::Io)?;
   publish(&bytes[n.saturating_sub(count)..n],out)
  }
  "find"|"grep"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   let (source,needle)=raw.split_once(' ').ok_or(Error::Invalid)?;
   if needle.is_empty(){return Err(Error::Invalid);}
   let p=path(source,&mut name)?;
   let mut bytes=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let n=fs.read(p,&mut bytes).map_err(Error::Io)?;
   let text=core::str::from_utf8(&bytes[..n]).map_err(|_|Error::Invalid)?;
   if verb=="find"{
    return match text.find(needle){
     Some(offset)=>publish_number(offset as u64,out),
     None=>publish(b"not found",out)
    };
   }
   // Stage ALL matching lines before publishing a single output byte.
   let mut matched=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let mut len=0;
   for line in text.split_inclusive('\n'){
    if line.contains(needle){
     matched[len..len+line.len()].copy_from_slice(line.as_bytes());
     len+=line.len();
    }
   }
   publish(&matched[..len],out)
  }
  // Diagnostic diff: report the first byte offset that differs between
  // two authenticated durable files, or "equal" when identical. The
  // first mismatching offset is the shorter length when one is a prefix.
  // This is a read-only primitive, not an allocator-backed unified diff.
  "diffat"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   let mut args=raw.split_ascii_whitespace();
   let left=args.next().ok_or(Error::Invalid)?;
   let right=args.next().ok_or(Error::Invalid)?;
   if args.next().is_some(){return Err(Error::Invalid);}
   let a=path(left,&mut name)?;
   let mut peer=[0u8;64];
   let b=path(right,&mut peer)?;
   let mut first=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let mut second=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let x=fs.read(a,&mut first).map_err(Error::Io)?;
   let y=fs.read(b,&mut second).map_err(Error::Io)?;
   if x==y&&first[..x]==second[..y]{
    return publish(b"equal",out);
   }
   let first_difference=first[..x].iter().zip(second[..y].iter())
    .position(|(a,b)|a!=b).unwrap_or(x.min(y));
   publish_number(first_difference as u64,out)
  }
  "cmp"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   let (left,right)=raw.split_once(' ').ok_or(Error::Invalid)?;
   let a=path(left,&mut name)?;
   let mut other=[0u8;64];let b=path(right,&mut other)?;
   let mut first=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let mut second=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let x=fs.read(a,&mut first).map_err(Error::Io)?;
   let y=fs.read(b,&mut second).map_err(Error::Io)?;
   publish(if first[..x]==second[..y]{b"equal"}else{b"different"},out)
  }
  "touch"=>{
   auth.require(caller,FILE_READ|FILE_WRITE,now).map_err(|_|Error::Denied)?;
   if out.len()<7{return Err(Error::OutputTooSmall);}
   let p=path(raw,&mut name)?;
   writable_path(p)?;
   let mut buffer=[0u8;crate::durablefs::MAX_FILE_BYTES];
   match fs.read(p,&mut buffer){
    Ok(_)=>publish(b"exists",out),
    Err(FsError::NotFound)=>{
     fs.create(p,b"").map_err(Error::Io)?;
     publish(b"created",out)
    },
    Err(e)=>Err(Error::Io(e))
   }
  }
  // Durable non-clobbering extraction of a real file byte range.
  // This copies binary data (including NULs) through one checksummed
  // destination COW create. A denied, bad or short-output operation can
  // neither modify a source file nor create a partial destination.
  "slice"=>{
   auth.require(caller,FILE_READ|FILE_WRITE,now).map_err(|_|Error::Denied)?;
   if out.len()<6{return Err(Error::OutputTooSmall);}
   let mut args=raw.split_ascii_whitespace();
   let src=args.next().ok_or(Error::Invalid)?;
   let offset=args.next().ok_or(Error::Invalid)?
      .parse::<u64>().map_err(|_|Error::Invalid)?;
   let count=args.next().ok_or(Error::Invalid)?
      .parse::<usize>().map_err(|_|Error::Invalid)?;
   let dst=args.next().ok_or(Error::Invalid)?;
   if args.next().is_some()||count==0||count>MAX_TRANSFER{
    return Err(Error::Invalid);
   }
   let src=path(src,&mut name)?;
   let mut target=[0u8;64];
   let dst=path(dst,&mut target)?;
   if src==dst{return Err(Error::Invalid);}
   writable_path(dst)?;
   let mut bytes=[0u8;MAX_TRANSFER];
   let n=fs.read_at(src,offset,&mut bytes[..count]).map_err(Error::Io)?;
   if n==0{return Err(Error::Invalid);}
   fs.create(dst,&bytes[..n]).map_err(Error::Io)?;
   publish(b"copied",out)
  }
  // Merge two existing binary payloads into a NEW checksummed destination.
  // Both input reads and all bounds checks complete before a COW create.
  // A failing second read, oversized sum or protected target cannot leave
  // a truncated destination. The source files are never modified.
  "concat"=>{
   auth.require(caller,FILE_READ|FILE_WRITE,now).map_err(|_|Error::Denied)?;
   if out.len()<6{return Err(Error::OutputTooSmall);}
   let mut args=raw.split_ascii_whitespace();
   let a=args.next().ok_or(Error::Invalid)?;
   let b=args.next().ok_or(Error::Invalid)?;
   let target=args.next().ok_or(Error::Invalid)?;
   if args.next().is_some(){return Err(Error::Invalid);}
   let first=path(a,&mut name)?;
   let mut second_name=[0u8;64];
   let second=path(b,&mut second_name)?;
   let mut target_name=[0u8;64];
   let destination=path(target,&mut target_name)?;
   if destination==first||destination==second{return Err(Error::Invalid);}
   writable_path(destination)?;
   let mut payload=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let first_size=fs.read(first,&mut payload).map_err(Error::Io)?;
   let mut next=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let second_size=fs.read(second,&mut next).map_err(Error::Io)?;
   let total=first_size.checked_add(second_size).ok_or(Error::Invalid)?;
   if total>payload.len(){return Err(Error::Invalid);}
   payload[first_size..total].copy_from_slice(&next[..second_size]);
   fs.create(destination,&payload[..total]).map_err(Error::Io)?;
   publish(b"joined",out)
  }
  // Compare bounded ranges from two mounted durable files. This read-only
  // diagnostic rejects missing/partial ranges, unauthorized identity,
  // malformed offsets and short outputs before touching caller bytes.
  "cmprange"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   let mut args=raw.split_ascii_whitespace();
   let left=args.next().ok_or(Error::Invalid)?;
   let left_offset=args.next().ok_or(Error::Invalid)?
    .parse::<u64>().map_err(|_|Error::Invalid)?;
   let right=args.next().ok_or(Error::Invalid)?;
   let right_offset=args.next().ok_or(Error::Invalid)?
    .parse::<u64>().map_err(|_|Error::Invalid)?;
   let count=args.next().ok_or(Error::Invalid)?
    .parse::<usize>().map_err(|_|Error::Invalid)?;
   if args.next().is_some()||count==0||count>MAX_TRANSFER{
    return Err(Error::Invalid);
   }
   if out.len()<9{return Err(Error::OutputTooSmall);}
   let left=path(left,&mut name)?;
   let mut peer=[0u8;64];
   let right=path(right,&mut peer)?;
   let mut first=[0u8;MAX_TRANSFER];
   let mut second=[0u8;MAX_TRANSFER];
   let n=fs.read_at(left,left_offset,&mut first[..count]).map_err(Error::Io)?;
   let m=fs.read_at(right,right_offset,&mut second[..count]).map_err(Error::Io)?;
   if n!=count||m!=count{return Err(Error::Invalid);}
   publish(if first[..count]==second[..count]{
    b"equal"
   }else{b"different"},out)
  }
  // Count an exact binary byte in a mounted checksummed COW file.
  // A two-digit hex argument avoids binary control characters in the
  // interactive terminal; caller identity and READ grants are verified.
  // The entire response is preflighted before publishing any output.
  "bytefreq"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   let mut args=raw.split_ascii_whitespace();
   let source=args.next().ok_or(Error::Invalid)?;
   let encoded=args.next().ok_or(Error::Invalid)?;
   if args.next().is_some()||encoded.len()!=2{return Err(Error::Invalid);}
   let parse=|c:u8|->Option<u8>{
    match c{
     b'0'..=b'9'=>Some(c-b'0'),
     b'a'..=b'f'=>Some(c-b'a'+10),
     b'A'..=b'F'=>Some(c-b'A'+10),
     _=>None
    }
   };
   let pair=encoded.as_bytes();
   let first=parse(pair[0]).ok_or(Error::Invalid)?;
   let second=parse(pair[1]).ok_or(Error::Invalid)?;
   let needle=(first<<4)|second;
   let p=path(source,&mut name)?;
   let mut data=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let n=fs.read(p,&mut data).map_err(Error::Io)?;
   let frequency=data[..n].iter().filter(|&&b|b==needle).count();
   publish_number(frequency as u64,out)
  }
  // Authenticated single-byte binary patch through a checksummed COW
  // overwrite. No sparse writes and no mutation on invalid offset,
  // protected namespace, short output, missing file or unchanged byte.
  "patchhex"=>{
   auth.require(caller,FILE_READ|FILE_WRITE,now).map_err(|_|Error::Denied)?;
   if out.len()<9{return Err(Error::OutputTooSmall);}
   let mut args=raw.split_ascii_whitespace();
   let source=args.next().ok_or(Error::Invalid)?;
   let offset=args.next().ok_or(Error::Invalid)?
      .parse::<u64>().map_err(|_|Error::Invalid)?;
   let encoded=args.next().ok_or(Error::Invalid)?;
   if args.next().is_some(){return Err(Error::Invalid);}
   let replacement=r065_hex_byte(encoded)?;
   let p=path(source,&mut name)?;
   writable_path(p)?;
   let mut bytes=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let size=fs.read(p,&mut bytes).map_err(Error::Io)?;
   let offset=usize::try_from(offset).map_err(|_|Error::Invalid)?;
   if offset>=size{return Err(Error::Invalid);}
   if bytes[offset]==replacement{return publish(b"unchanged",out);}
   bytes[offset]=replacement;
   fs.overwrite(p,&bytes[..size]).map_err(Error::Io)?;
   publish(b"patched",out)
  }
  // Bounded multi-byte patch of existing checksummed COW file contents.
  // Command: patchrange /path offset HEXPAIRS. Decode every byte, preflight
  // capacity and validate the entire existing file before ONE overwrite.
  // No sparse writes, partial results or unauthorized reserved-path writes.
  "patchrange"=>{
   auth.require(caller,FILE_READ|FILE_WRITE,now).map_err(|_|Error::Denied)?;
   if out.len()<9{return Err(Error::OutputTooSmall);}
   let mut args=raw.split_ascii_whitespace();
   let source=args.next().ok_or(Error::Invalid)?;
   let offset=args.next().ok_or(Error::Invalid)?
     .parse::<u64>().map_err(|_|Error::Invalid)?;
   let encoded=args.next().ok_or(Error::Invalid)?;
   if args.next().is_some()||encoded.is_empty()||encoded.len()%2!=0||
      encoded.len()>MAX_TRANSFER*2{return Err(Error::Invalid);}
   let mut patch=[0u8;MAX_TRANSFER];
   let count=encoded.len()/2;
   for (i,pair) in encoded.as_bytes().chunks_exact(2).enumerate(){
    let hex=core::str::from_utf8(pair).map_err(|_|Error::Invalid)?;
    patch[i]=r065_hex_byte(hex)?;
   }
   let path=path(source,&mut name)?;
   writable_path(path)?;
   let mut contents=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let size=fs.read(path,&mut contents).map_err(Error::Io)?;
   let at=usize::try_from(offset).map_err(|_|Error::Invalid)?;
   let end=at.checked_add(count).ok_or(Error::Invalid)?;
   if end>size{return Err(Error::Invalid);}
   if contents[at..end]==patch[..count]{return publish(b"unchanged",out);}
   contents[at..end].copy_from_slice(&patch[..count]);
   fs.overwrite(path,&contents[..size]).map_err(Error::Io)?;
   publish(b"patched",out)
  }
  // Durable UTF-8 line snippet extraction into a NEW COW file; source
  // remains unchanged and a pre-existing destination cannot be overwritten.
  "copylines"=>{
   auth.require(caller,FILE_READ|FILE_WRITE,now).map_err(|_|Error::Denied)?;
   if out.len()<6{return Err(Error::OutputTooSmall);}
   let mut args=raw.split_ascii_whitespace();
   let source=args.next().ok_or(Error::Invalid)?;
   let first=args.next().ok_or(Error::Invalid)?
      .parse::<usize>().map_err(|_|Error::Invalid)?;
   let count=args.next().ok_or(Error::Invalid)?
      .parse::<usize>().map_err(|_|Error::Invalid)?;
   let target=args.next().ok_or(Error::Invalid)?;
   if args.next().is_some()||first==0||count==0{return Err(Error::Invalid);}
   let last=first.checked_add(count-1).ok_or(Error::Invalid)?;
   let a=path(source,&mut name)?;
   let mut destination=[0u8;64];
   let b=path(target,&mut destination)?;
   if a==b{return Err(Error::Invalid);}
   writable_path(b)?;
   let mut bytes=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let n=fs.read(a,&mut bytes).map_err(Error::Io)?;
   let text=core::str::from_utf8(&bytes[..n]).map_err(|_|Error::Invalid)?;
   let mut start=None;let mut end=None;let mut cursor=0usize;
   for (i,row) in text.split_inclusive('\n').enumerate(){
    let line=i+1;
    if line==first{start=Some(cursor);}
    cursor+=row.len();
    if line==last{end=Some(cursor);break;}
   }
   let (Some(start),Some(end))=(start,end)else{return Err(Error::Invalid)};
   fs.create(b,&bytes[start..end]).map_err(Error::Io)?;
   publish(b"copied",out)
  }
  // Unicode-aware durable file metrics: lines, words, UTF-8 bytes.
  // Does not mutate mounted storage or publish partial caller output.
  "wc"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   let source=path(raw,&mut name)?;
   let mut bytes=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let n=fs.read(source,&mut bytes).map_err(Error::Io)?;
   let text=core::str::from_utf8(&bytes[..n]).map_err(|_|Error::Invalid)?;
   let rows=text.split_inclusive('\n').count();
   let words=text.split_whitespace().count();
   let mut encoded=[0u8;64];let mut used=0usize;
   for (i,value) in [rows as u64,words as u64,n as u64].iter().enumerate(){
    if i>0{encoded[used]=b' ';used+=1;}
    used+=publish_number(*value,&mut encoded[used..])?;
   }
   publish(&encoded[..used],out)
  }
  "copynew"=>{
   auth.require(caller,FILE_READ|FILE_WRITE,now).map_err(|_|Error::Denied)?;
   if out.len()<6{return Err(Error::OutputTooSmall);}
   let (source,target)=raw.split_once(' ').ok_or(Error::Invalid)?;
   let a=path(source,&mut name)?;
   let mut dest=[0u8;64];let b=path(target,&mut dest)?;
   if a==b{return Err(Error::Invalid);}
   writable_path(b)?;
   let mut bytes=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let count=fs.read(a,&mut bytes).map_err(Error::Io)?;
   fs.create(b,&bytes[..count]).map_err(Error::Io)?;
   publish(b"copied",out)
  }
  "replace"=>{
   // Literal text replacement is a single COW overwrite, never a
   // destructive series of edits; no partial file changes on overflow.
   auth.require(caller,FILE_READ|FILE_WRITE,now).map_err(|_|Error::Denied)?;
   if out.len()<9{return Err(Error::OutputTooSmall);}
   let mut args=raw.splitn(3,' ');
   let source=args.next().ok_or(Error::Invalid)?;
   let needle=args.next().ok_or(Error::Invalid)?;
   let replacement=args.next().ok_or(Error::Invalid)?;
   if needle.is_empty()||replacement.is_empty(){return Err(Error::Invalid);}
   let p=path(source,&mut name)?;
   writable_path(p)?;
   let mut previous=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let n=fs.read(p,&mut previous).map_err(Error::Io)?;
   let text=core::str::from_utf8(&previous[..n]).map_err(|_|Error::Invalid)?;
   let mut updated=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let mut read=0usize;let mut written=0usize;let mut replaced=0usize;
   while read<n{
    if text[read..].starts_with(needle){
     let next=written.checked_add(replacement.len()).ok_or(Error::Invalid)?;
     if next>updated.len(){return Err(Error::Io(FsError::TooLarge));}
     updated[written..next].copy_from_slice(replacement.as_bytes());
     written=next;read+=needle.len();replaced+=1;
    }else{
     let width=text[read..].chars().next().ok_or(Error::Invalid)?.len_utf8();
     if written+width>updated.len(){return Err(Error::Io(FsError::TooLarge));}
     updated[written..written+width].copy_from_slice(&previous[read..read+width]);
     written+=width;read+=width;
    }
   }
   if replaced==0{return publish(b"unchanged",out);}
   if written==n&&updated[..written]==previous[..n]{
    return publish(b"unchanged",out);
   }
   fs.overwrite(p,&updated[..written]).map_err(Error::Io)?;
   publish(b"replaced",out)
  }
  "appendln"=>{
   auth.require(caller,FILE_READ|FILE_WRITE,now).map_err(|_|Error::Denied)?;
   if out.len()<8{return Err(Error::OutputTooSmall);}
   let (source,data)=raw.split_once(' ').ok_or(Error::Invalid)?;
   if data.is_empty()||data.len()>MAX_TRANSFER{return Err(Error::Invalid);}
   let p=path(source,&mut name)?;
   writable_path(p)?;
   let mut existing=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let n=fs.read(p,&mut existing).map_err(Error::Io)?;
   let next=n.checked_add(data.len()+1).ok_or(Error::Invalid)?;
   if next>existing.len(){return Err(Error::Io(FsError::TooLarge));}
   existing[n..n+data.len()].copy_from_slice(data.as_bytes());
   existing[next-1]=b'\n';
   fs.overwrite(p,&existing[..next]).map_err(Error::Io)?;
   publish(b"appended",out)
  }
  "ls"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   let p=if raw=="/"{"/"}else{path(raw,&mut name)?};
   let mut buf=[0u8;MAX_TRANSFER];
   let n=fs.list_dir(p,&mut buf).map_err(Error::Io)?;
   publish(&buf[..n],out)
  }
  "cat"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   let p=path(raw,&mut name)?;
   let mut buf=[0u8;MAX_TRANSFER];
   // Intentionally reject oversized files rather than disclose partial data.
   let n=fs.read(p,&mut buf).map_err(Error::Io)?;
   publish(&buf[..n],out)
  }
  "mkfile"=>{
   // Creation is explicit, never truncates an existing file. Check the
   // output and authorization before changing durable device metadata.
   auth.require(caller,FILE_WRITE,now).map_err(|_|Error::Denied)?;
   if out.len()<7{return Err(Error::OutputTooSmall);}
   let p=path(raw,&mut name)?;
   writable_path(p)?;
   fs.create(p,&[]).map_err(Error::Io)?;
   publish(b"created",out)
  }
  "mkdir"=>{
   auth.require(caller,FILE_WRITE,now).map_err(|_|Error::Denied)?;
   if out.len()<7{return Err(Error::OutputTooSmall);}
   let p=path(raw,&mut name)?;
   writable_path(p)?;
   fs.mkdir(p).map_err(Error::Io)?;
   publish(b"created",out)
  }
  "write"=>{
   auth.require(caller,FILE_WRITE,now).map_err(|_|Error::Denied)?;
   if out.len()<5{return Err(Error::OutputTooSmall);}
   let (raw_path,data)=raw.split_once(' ').ok_or(Error::Invalid)?;
   if data.is_empty()||data.len()>MAX_TRANSFER{return Err(Error::Invalid);}
   let p=path(raw_path,&mut name)?;
   writable_path(p)?;
   // Existing files are COW-overwritten; absent files are created. DurableFs
   // atomically publishes metadata; BlockDevice must flush each write.
   match fs.overwrite(p,data.as_bytes()){
    Ok(())=>{},
    Err(FsError::NotFound)=>fs.create(p,data.as_bytes()).map_err(Error::Io)?,
    Err(e)=>return Err(Error::Io(e)),
   };
   publish(b"saved",out)
  }
  "append"=>{
   // DurableFS commits a whole new checksummed image through write_at;
   // exact prior file content is verified before any new disk mutation.
   auth.require(caller,FILE_READ|FILE_WRITE,now).map_err(|_|Error::Denied)?;
   if out.len()<8{return Err(Error::OutputTooSmall);}
   let (raw_path,data)=raw.split_once(' ').ok_or(Error::Invalid)?;
   if data.is_empty()||data.len()>MAX_TRANSFER{return Err(Error::Invalid);}
   let p=path(raw_path,&mut name)?;
   writable_path(p)?;
   let mut prior=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let len=fs.read(p,&mut prior).map_err(Error::Io)?;
   if len.checked_add(data.len()).is_none_or(|n|
      n>crate::durablefs::MAX_FILE_BYTES){return Err(Error::Io(FsError::TooLarge));}
   fs.write_at(p,len as u64,data.as_bytes()).map_err(Error::Io)?;
   publish(b"appended",out)
  }
  "truncate"=>{
   // Retains the existing file entry, empties only its data, and publishes
   // through the already mounted COW volume. Never auto-creates a path.
   auth.require(caller,FILE_WRITE,now).map_err(|_|Error::Denied)?;
   if out.len()<9{return Err(Error::OutputTooSmall);}
   let p=path(raw,&mut name)?;
   writable_path(p)?;
   fs.overwrite(p,b"").map_err(Error::Io)?;
   publish(b"truncated",out)
  }
  "head"=>{
   // Read-only, bounded prefix output from a persisted file. A user can
   // request no more than the VFS transfer limit; malformed counts fail.
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   let (raw_path,raw_count)=raw.split_once(' ').ok_or(Error::Invalid)?;
   let count=raw_count.parse::<usize>().map_err(|_|Error::Invalid)?;
   if count==0||count>MAX_TRANSFER{return Err(Error::Invalid);}
   let p=path(raw_path,&mut name)?;
   let mut bytes=[0u8;MAX_TRANSFER];
   let size=fs.read(p,&mut bytes).map_err(Error::Io)?;
   publish(&bytes[..core::cmp::min(count,size)],out)
  }
  "rmdir"=>{
   // Delete empty directories only, using the persistent VFS tree guard.
   auth.require(caller,FILE_WRITE,now).map_err(|_|Error::Denied)?;
   if out.len()<7{return Err(Error::OutputTooSmall);}
   let dir=path(raw,&mut name)?;
   writable_path(dir)?;
   fs.rmdir(dir).map_err(Error::Io)?;
   publish(b"removed",out)
  }
  "mvdir"=>{
   // Move an entire directory subtree transactionally: verify both names
   // and deny protected namespaces before any COW metadata modification.
   auth.require(caller,FILE_WRITE,now).map_err(|_|Error::Denied)?;
   if out.len()<5{return Err(Error::OutputTooSmall);}
   let (src,dest)=raw.split_once(' ').ok_or(Error::Invalid)?;
   let src=path(src,&mut name)?;
   let mut target=[0u8;64];
   let dest=path(dest,&mut target)?;
   writable_path(src)?;
   writable_path(dest)?;
   fs.rename_dir(src,dest).map_err(Error::Io)?;
   publish(b"moved",out)
  }
  "pread"=>{
   // Bounded random-access read with an authenticated process grant.
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   let mut items=raw.split(' ');
   let raw_path=items.next().ok_or(Error::Invalid)?;
   let offset=items.next().ok_or(Error::Invalid)?
     .parse::<u64>().map_err(|_|Error::Invalid)?;
   let count=items.next().ok_or(Error::Invalid)?
     .parse::<usize>().map_err(|_|Error::Invalid)?;
   if items.next().is_some()||count==0||count>MAX_TRANSFER{
    return Err(Error::Invalid);
   }
   let p=path(raw_path,&mut name)?;
   let mut data=[0u8;MAX_TRANSFER];
   let n=fs.read_at(p,offset,&mut data[..count]).map_err(Error::Io)?;
   publish(&data[..n],out)
  }
  "pwrite"=>{
   // In-place bounded persistent update; never creates sparse holes or
   // bypasses the protected mount namespace. Output preflight precedes I/O.
   auth.require(caller,FILE_WRITE,now).map_err(|_|Error::Denied)?;
   if out.len()<5{return Err(Error::OutputTooSmall);}
   let (where_at,data)=raw.split_once(' ').ok_or(Error::Invalid)?;
   let (raw_path,offset)=where_at.split_once('@').ok_or(Error::Invalid)?;
   let offset=offset.parse::<u64>().map_err(|_|Error::Invalid)?;
   if data.is_empty()||data.len()>MAX_TRANSFER{return Err(Error::Invalid);}
   let p=path(raw_path,&mut name)?;
   writable_path(p)?;
   fs.write_at(p,offset,data.as_bytes()).map_err(Error::Io)?;
   publish(b"saved",out)
  }
  "cp"=>{
   // A copy reads the source AND mutates the destination. Neither a
   // read-only nor a write-only grant may authorize this operation.
   auth.require(caller,FILE_READ|FILE_WRITE,now).map_err(|_|Error::Denied)?;
   if out.len()<6{return Err(Error::OutputTooSmall);}
   let (left,right)=raw.split_once(' ').ok_or(Error::Invalid)?;
   let from=path(left,&mut name)?;
   let mut target=[0u8;64];
   let to=path(right,&mut target)?;
   if from==to{return Err(Error::Invalid);}
   writable_path(to)?;
   // Read the entire source before any destination mutation. In particular,
   // a missing/oversized source or a protected destination is fail-closed.
   // Files may be larger than a single terminal transfer. Stage the
   // complete, checksummed source (up to the current COW volume limit) before
   // publishing any destination metadata. This avoids partial copies.
   let mut bytes=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let n=fs.read(from,&mut bytes).map_err(Error::Io)?;
   match fs.overwrite(to,&bytes[..n]){
    Ok(())=>{},
    Err(FsError::NotFound)=>fs.create(to,&bytes[..n]).map_err(Error::Io)?,
    Err(e)=>return Err(Error::Io(e)),
   }
   publish(b"copied",out)
  }
  "rm"=>{
   auth.require(caller,FILE_WRITE,now).map_err(|_|Error::Denied)?;
   if out.len()<7{return Err(Error::OutputTooSmall);}
   let p=path(raw,&mut name)?;
   writable_path(p)?;
   fs.unlink(p).map_err(Error::Io)?;
   publish(b"removed",out)
  }
  "mv"=>{
   auth.require(caller,FILE_WRITE,now).map_err(|_|Error::Denied)?;
   if out.len()<5{return Err(Error::OutputTooSmall);}
   let (left,right)=raw.split_once(' ').ok_or(Error::Invalid)?;
   let from=path(left,&mut name)?;
   let mut target=[0u8;64];
   let to=path(right,&mut target)?;
   writable_path(from)?;
   writable_path(to)?;
   fs.rename_file(from,to).map_err(Error::Io)?;
   publish(b"moved",out)
  }
  _=>Err(Error::Unknown)
 }
}


/// Execute a bounded, authenticated Terminal command against an explicitly
/// selected existing on-disk MBR/GPT partition. This is the G1/G2 kernel
/// service bridge, not yet a ring-3 syscall or keyboard driver.
///
/// The partition parser verifies all GPT metadata and rejects hybrid media.
/// BlockWindow confines durable VFS writes to the selected partition; no
/// disk is ever formatted here. A new mount for each command provides a
/// genuine metadata reload, rather than a RAM-only file snapshot.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum PartitionCommandError {
 Mount(crate::m1_partition::MountError),
 Command(Error),
}
/// Do not probe or disclose any partition metadata before checking that
/// the scheduler-derived caller still has an active filesystem capability.
/// Fine-grained per-command permissions are checked again by command().
fn preflight_partition_caller<const N:usize>(
 auth:&Access<N>,caller:Identity,now:u64,
)->Result<(),PartitionCommandError>{
 if caller.pid==0||caller.generation==0{
  return Err(PartitionCommandError::Command(Error::Denied));
 }
 let rights=auth.active_rights(caller,now)
  .map_err(|_|PartitionCommandError::Command(Error::Denied))?;
 if rights&(FILE_READ|FILE_WRITE)==0{
  return Err(PartitionCommandError::Command(Error::Denied));
 }
 Ok(())
}
/// Explicit MBR/GPT geometry-locked, healthy COW mount for an authenticated
/// Terminal command. Caller supplies a previously inspected partition;
/// a changed ordinal, LBA range, missing partition or degraded volume is
/// rejected without formatting or repairing disk sectors.
pub fn command_on_checked_partition<D:BlockDevice,const N:usize>(
 disk:&mut D,selected:crate::m1_partition::Partition,
 auth:&Access<N>,caller:Identity,now:u64,input:&[u8],out:&mut[u8],
)->Result<usize,PartitionCommandError>{
 preflight_partition_caller(auth,caller,now)?;
 let mut volume=crate::m1_partition::mount_checked_healthy(disk,selected)
  .map_err(PartitionCommandError::Mount)?;
 command(&mut volume,auth,caller,now,input,out)
  .map_err(PartitionCommandError::Command)
}
/// The preferred persistent developer workstation command entrypoint.
/// A user explicitly selects a GPT partition and stores its disk GUID and
/// unique partition GUID. Every invocation revalidates both identifiers,
/// complete primary/backup GPT metadata, geometry and COW redundancy before
/// permitting authenticated read/write/mkdir/rename operations.
pub fn command_on_gpt_selection<D:BlockDevice,const N:usize>(
 disk:&mut D,selected:crate::m1_partition::GptSelection,
 auth:&Access<N>,caller:Identity,now:u64,input:&[u8],out:&mut[u8],
)->Result<usize,PartitionCommandError>{
 preflight_partition_caller(auth,caller,now)?;
 let mut volume=crate::m1_partition::mount_gpt_selected_healthy(disk,selected)
  .map_err(PartitionCommandError::Mount)?;
 command(&mut volume,auth,caller,now,input,out)
  .map_err(PartitionCommandError::Command)
}
pub fn command_on_partition<D:BlockDevice,const N:usize>(
 disk:&mut D,selected_index:u32,auth:&Access<N>,caller:Identity,
 now:u64,input:&[u8],out:&mut[u8],
)->Result<usize,PartitionCommandError>{
 // Require an active owner/generation/expiry and filesystem scope before
 // any on-disk discovery. The command parser remains the final rights gate.
 preflight_partition_caller(auth,caller,now)?;
 let mut volume=crate::m1_partition::mount(disk,selected_index)
  .map_err(PartitionCommandError::Mount)?;
 command(&mut volume,auth,caller,now,input,out)
  .map_err(PartitionCommandError::Command)
}

/// Authenticated, bounded, explicitly selected GPT Terminal session.
/// Only scheduler-derived identities may be supplied by the trusted caller.
/// This is a kernel-service API, not an unprivileged ring-3 syscall entry.
/// New connections require a fresh disk/partition GUID selection and a healthy
/// COW volume; old sessions never silently switch to replacement media.
pub struct GptTerminalSession{
 owner:Identity,
 selection:crate::m1_partition::GptSelection,
 expires:u64,
 command_count:u64,
 active:bool,
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct TerminalSessionStatus{
 pub partition_index:u32,pub commands:u64,pub expires:u64,
}
impl GptTerminalSession{
 /// Read-only explicit selection. A missing grant or invalid deadline
 /// cannot even probe the block device. The mount is immediately released.
 pub fn open<D:BlockDevice,const N:usize>(
  disk:&mut D,index:u32,auth:&Access<N>,caller:Identity,now:u64,expires:u64,
 )->Result<Self,PartitionCommandError>{
  preflight_partition_caller(auth,caller,now)?;
  if expires<=now{return Err(PartitionCommandError::Command(Error::Denied));}
  let selection=crate::m1_partition::inspect_gpt_selection(disk,index)
   .map_err(|error|PartitionCommandError::Mount(
     crate::m1_partition::MountError::Partition(error)))?;
  // Reject unformatted or degraded media, and revalidate the GPT identity
  // before accepting a session. No format or automatic repair is allowed.
  drop(crate::m1_partition::mount_gpt_selected_healthy(disk,selection)
   .map_err(PartitionCommandError::Mount)?);
  Ok(Self{owner:caller,selection,expires,command_count:0,active:true})
 }
 /// Explicit operator recovery of the originally selected GPT volume.
 /// Both filesystem rights and a fresh deadline are required before any
 /// device read. A healthy volume is a no-op; a degraded volume is repaired
 /// only by this explicit request. Success returns a NEW healthy session and
 /// whether one metadata head was repaired. No old/retired session is revived.
 /// The trusted block-device owner must serialize/quiesce media access.
 pub fn recover<D:BlockDevice,const N:usize>(
  disk:&mut D,selection:crate::m1_partition::GptSelection,
  auth:&Access<N>,caller:Identity,now:u64,expires:u64,
 )->Result<(Self,bool),PartitionCommandError>{
  preflight_partition_caller(auth,caller,now)?;
  auth.require(caller,FILE_READ|FILE_WRITE,now)
   .map_err(|_|PartitionCommandError::Command(Error::Denied))?;
  if expires<=now{return Err(PartitionCommandError::Command(Error::Denied));}
  let repaired=crate::m1_partition::repair_gpt_selected(disk,selection)
   .map_err(PartitionCommandError::Mount)?;
  Ok((Self{owner:caller,selection,expires,command_count:0,active:true},repaired))
 }
 fn authorize<const N:usize>(&self,auth:&Access<N>,caller:Identity,now:u64)
  ->Result<(),PartitionCommandError>{
  if !self.active||caller!=self.owner||now>=self.expires{
   return Err(PartitionCommandError::Command(Error::Denied));
  }
  preflight_partition_caller(auth,caller,now)
 }
 /// An auth failure or malformed command never mutates session state.
 /// A GPT identity, mount or uncertain filesystem I/O failure permanently
 /// retires the selection; a fresh session requires validated media.
 /// Ordinary NotFound/Exists user errors leave the session active.
 pub fn execute<D:BlockDevice,const N:usize>(
  &mut self,disk:&mut D,auth:&Access<N>,caller:Identity,now:u64,
  input:&[u8],out:&mut[u8],
 )->Result<usize,PartitionCommandError>{
  self.authorize(auth,caller,now)?;
  let result=command_on_gpt_selection(disk,self.selection,auth,caller,now,input,out);
  if matches!(result,Err(PartitionCommandError::Mount(_))|
       Err(PartitionCommandError::Command(Error::Io(FsError::Io|FsError::InvalidVolume)))){
   // A failed or ambiguously acknowledged data/metadata write may have
   // changed the durable head. Never permit a session to issue a second
   // command on its old checkpoint assumptions; require a fresh volume
   // validation and newly authorized session.
   self.active=false;
  }
  if result.is_ok(){self.command_count=self.command_count.saturating_add(1);}
  result
 }
 /// No disk read is needed for owner-only session state inspection.
 pub fn status<const N:usize>(&self,auth:&Access<N>,caller:Identity,now:u64)
  ->Result<TerminalSessionStatus,PartitionCommandError>{
  self.authorize(auth,caller,now)?;
  Ok(TerminalSessionStatus{
   partition_index:self.selection.partition.index,
   commands:self.command_count,expires:self.expires,
  })
 }
 /// A session may only reduce its own lifetime, never renew or expand it.
 pub fn shorten<const N:usize>(&mut self,auth:&Access<N>,caller:Identity,
  now:u64,new_expiry:u64)->Result<(),PartitionCommandError>{
  self.authorize(auth,caller,now)?;
  if new_expiry<=now||new_expiry>self.expires{
   return Err(PartitionCommandError::Command(Error::Denied));
  }
  self.expires=new_expiry;Ok(())
 }
 /// Owner-only close; a recycled PID cannot close a different generation.
 /// Closing invalidates the token but never touches persistent disk sectors.
 pub fn close(&mut self,caller:Identity)->Result<(),PartitionCommandError>{
  if !self.active||self.owner!=caller{
   return Err(PartitionCommandError::Command(Error::Denied));
  }
  self.active=false;Ok(())
 }
 pub fn is_active(&self)->bool{self.active}
}

#[cfg(test)]
mod tests{
 use super::*;
 use crate::block::{BlockError,SECTOR_BYTES};
 const SECTORS:usize=100;
 #[derive(Clone)]
 struct Disk{data:[[u8;SECTOR_BYTES];SECTORS],fail_writes:bool}
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{SECTORS as u64}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
   ->Result<(),BlockError>{
   *out=*self.data.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,lba:u64,input:&[u8;SECTOR_BYTES])
   ->Result<(),BlockError>{
   if self.fail_writes{return Err(BlockError::Io);}
   *self.data.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*input;
   Ok(())
  }
 }
 fn blank()->Disk{Disk{data:[[0;SECTOR_BYTES];SECTORS],fail_writes:false}}
 fn user()->Identity{Identity{pid:3,generation:1}}
 fn grants()->Access<1>{let mut access=Access::new();
  access.grant(user(),FILE_READ|FILE_WRITE,500).unwrap();access}
 fn run(fs:&mut HierVfs<Disk>,access:&Access<1>,who:Identity,
   cmd:&[u8])->Result<(usize,[u8;MAX_TRANSFER]),Error>{
  let mut out=[0xaau8;MAX_TRANSFER];
  let size=command(fs,access,who,1,cmd,&mut out)?;
  Ok((size,out))
 }
 #[test]fn r058_line_inspection_survives_disk_remount_and_unicode(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();let auth=grants();
  fs.create("/source","alpha\nβeta\nthird\n".as_bytes()).unwrap();
  let generation=fs.generation();
  let (n,out)=run(&mut fs,&auth,user(),b"line /source 2").unwrap();
  assert_eq!(&out[..n],"βeta".as_bytes());
  assert_eq!(fs.generation(),generation);
  assert_eq!(run(&mut fs,&auth,user(),b"line /source 4"),Err(Error::Invalid));
  fs=HierVfs::mount(fs.into_device()).unwrap();
  let (n,out)=run(&mut fs,&auth,user(),b"line /source 3").unwrap();
  assert_eq!(&out[..n],b"third");
  assert_eq!(fs.generation(),generation);
 }
 #[test]fn r058_line_selection_rejects_bad_rows_stale_owner_short_buffer(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();let auth=grants();
  fs.create("/source",b"first\nsecond\n").unwrap();
  let generation=fs.generation();
  let mut short=[0xa5;3];
  assert_eq!(command(&mut fs,&auth,user(),1,b"line /source 2",&mut short),
   Err(Error::OutputTooSmall));
  assert_eq!(short,[0xa5;3]);
  let foreign=Identity{generation:2,..user()};
  assert_eq!(run(&mut fs,&auth,foreign,b"line /source 1"),Err(Error::Denied));
  for bad in [b"line /source 0".as_slice(),b"line /source 9",
              b"line /../source 1",b"line /source -1"]{
   assert_eq!(run(&mut fs,&auth,user(),bad),Err(Error::Invalid));
  }
  assert_eq!(fs.generation(),generation);
 }
 #[test]fn r059_durable_diffat_reports_first_mismatch_and_prefix_boundary(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();let auth=grants();
  fs.create("/one",b"hello alpha").unwrap();
  fs.create("/two",b"hello beta").unwrap();
  fs.create("/short",b"hello").unwrap();
  let generation=fs.generation();
  let (n,out)=run(&mut fs,&auth,user(),b"diffat /one /two").unwrap();
  assert_eq!(&out[..n],b"6");
  let (n,out)=run(&mut fs,&auth,user(),b"diffat /one /short").unwrap();
  assert_eq!(&out[..n],b"5");
  let (n,out)=run(&mut fs,&auth,user(),b"diffat /one /one").unwrap();
  assert_eq!(&out[..n],b"equal");
  fs=HierVfs::mount(fs.into_device()).unwrap();
  let (n,out)=run(&mut fs,&auth,user(),b"diffat /one /two").unwrap();
  assert_eq!(&out[..n],b"6");
  assert_eq!(fs.generation(),generation);
 }
 #[test]fn r059_diffat_denied_paths_short_output_and_missing_files_are_atomic(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();let auth=grants();
  fs.create("/one",b"same").unwrap();
  fs.create("/two",b"same").unwrap();
  let generation=fs.generation();
  let mut short=[0x6d;2];
  assert_eq!(command(&mut fs,&auth,user(),1,b"diffat /one /two",&mut short),
    Err(Error::OutputTooSmall));
  assert_eq!(short,[0x6d;2]);
  let old=Identity{generation:4,..user()};
  assert_eq!(run(&mut fs,&auth,old,b"diffat /one /two"),Err(Error::Denied));
  assert_eq!(run(&mut fs,&auth,user(),b"diffat /../one /two"),
    Err(Error::Invalid));
  assert_eq!(run(&mut fs,&auth,user(),b"diffat /one /missing"),
    Err(Error::Io(FsError::NotFound)));
  assert_eq!(fs.generation(),generation);
  assert_eq!(short,[0x6d;2]);
 }
 #[test]fn m1_terminal_typed_literal_spaces_survive_each_disk_remount(){
  use crate::m1_input::{InputLine,Event,MAX_LINE};
  let mut fs=HierVfs::format_blank(blank()).unwrap();let auth=grants();
  for (entered,expected) in [
   (b"  write /note hello  ".as_slice(),b"hello  ".as_slice()),
   (b"append /note   ",b"hello    "),
   (b"pwrite /note@0 X  ",b"X  lo    "),
   (b"appendln /note end  ",b"X  lo    end  \n"),
  ]{
   let mut line=InputLine::new();let mut submitted=[0u8;MAX_LINE];
   for &key in entered{line.feed(key,&mut[]).unwrap();}
   assert_eq!(line.feed(b'\n',&mut submitted),Ok(Event::Submitted(entered.len())));
   assert_eq!(&submitted[..entered.len()],entered);
   run(&mut fs,&auth,user(),&submitted[..entered.len()]).unwrap();
   fs=HierVfs::mount(fs.into_device()).unwrap();
   let (n,stored)=run(&mut fs,&auth,user(),b"cat /note").unwrap();
   assert_eq!(&stored[..n],expected);
  }
  // The maximum supported edited line must not lose its final spaces.
  let mut entered=[b'x';MAX_LINE];let prefix=b"write /full ";
  entered[..prefix.len()].copy_from_slice(prefix);
  entered[MAX_LINE-2..].fill(b' ');
  run(&mut fs,&auth,user(),&entered).unwrap();
  let mut fs=HierVfs::mount(fs.into_device()).unwrap();
  let (n,stored)=run(&mut fs,&auth,user(),b"cat /full").unwrap();
  assert_eq!(&stored[..n],&entered[prefix.len()..]);
 }
 #[test]fn m1_terminal_literal_search_and_replacement_keep_trailing_spaces(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();let auth=grants();
  fs.create("/note",b"cat cat  dog\ncat dog\n").unwrap();
  let (n,out)=run(&mut fs,&auth,user(),b"find /note cat  ").unwrap();
  assert_eq!(&out[..n],b"4");
  let (n,out)=run(&mut fs,&auth,user(),b"grep /note cat  ").unwrap();
  assert_eq!(&out[..n],b"cat cat  dog\n");
  fs.overwrite("/note",b"cat cat").unwrap();
  run(&mut fs,&auth,user(),b"replace /note cat dog  ").unwrap();
  let mut fs=HierVfs::mount(fs.into_device()).unwrap();
  let (n,out)=run(&mut fs,&auth,user(),b"cat /note").unwrap();
  assert_eq!(&out[..n],b"dog   dog  ");
  run(&mut fs,&auth,user(),b"replace /note dog  ").unwrap();
  let (n,out)=run(&mut fs,&auth,user(),b"cat /note").unwrap();
  assert_eq!(&out[..n],b"       "); // preserve old separators plus replacements
 }
 #[test]fn m1_terminal_space_payload_denial_and_failed_write_preserve_disk(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();let auth=grants();
  fs.create("/note",b"keep  ").unwrap();let generation=fs.generation();
  let mut short=[0x77;4];
  assert_eq!(command(&mut fs,&auth,user(),1,b"write /note   ",&mut short),
   Err(Error::OutputTooSmall));
  assert_eq!(short,[0x77;4]);
  assert_eq!(run(&mut fs,&auth,Identity{generation:2,..user()},b"write /note   "),
   Err(Error::Denied));
  assert_eq!(fs.generation(),generation);
  let mut disk=fs.into_device();let original=disk.data;disk.fail_writes=true;
  let mut fs=HierVfs::mount(disk).unwrap();
  assert_eq!(run(&mut fs,&auth,user(),b"write /note   "),Err(Error::Io(FsError::Io)));
  let disk=fs.into_device();assert_eq!(disk.data,original);
 }
 #[test]fn m1_terminal_nonliteral_command_whitespace_remains_compatible(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();let auth=grants();
  for entered in [b"pwd".as_slice(),b"  pwd   "]{
   let (n,out)=run(&mut fs,&auth,user(),entered).unwrap();
   assert_eq!(&out[..n],b"/");
  }
  run(&mut fs,&auth,user(),b"  write /note abc  ").unwrap();
  let (n,out)=run(&mut fs,&auth,user(),b"  head /note 3   ").unwrap();
  assert_eq!(&out[..n],b"abc");
  run(&mut fs,&auth,user(),b"  mv /note /moved   ").unwrap();
  let (n,out)=run(&mut fs,&auth,user(),b"cat /moved   ").unwrap();
  assert_eq!(&out[..n],b"abc  ");
  assert_eq!(run(&mut fs,&auth,user(),b"write /empty "),Err(Error::Invalid));
 }
 #[test]fn m1_r046_readonly_terminal_health_reports_redundancy_and_generation(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let auth=grants();
  let (n,out)=run(&mut fs,&auth,user(),b"health").unwrap();
  assert_eq!(&out[..n],b"healthy gen:1 copies:2");
  fs.create("/note",b"committed").unwrap();
  let before=fs.generation();
  let (n,out)=run(&mut fs,&auth,user(),b"health").unwrap();
  assert_eq!(&out[..n],b"healthy gen:2 copies:2");
  assert_eq!(fs.generation(),before);
  let mut disk=fs.into_device();
  disk.data[0][24]^=1; // older metadata invalid, newest still valid
  let mut fs=HierVfs::mount(disk).unwrap();
  let (n,out)=run(&mut fs,&auth,user(),b"health").unwrap();
  assert_eq!(&out[..n],b"degraded gen:2 copies:1");
  assert_eq!(fs.generation(),before);
 }
 #[test]fn m1_r046_health_refuses_stale_identity_and_short_output_without_writes(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let auth=grants();
  let generation=fs.generation();
  let mut short=[0x5au8;5];
  assert_eq!(command(&mut fs,&auth,user(),1,b"health",&mut short),
   Err(Error::OutputTooSmall));
  assert_eq!(short,[0x5a;5]);
  assert_eq!(fs.generation(),generation);
  let stale=Identity{generation:2,..user()};
  assert_eq!(run(&mut fs,&auth,stale,b"health"),Err(Error::Denied));
  let mut write_only=Access::<1>::new();
  write_only.grant(user(),FILE_WRITE,500).unwrap();
  assert_eq!(run(&mut fs,&write_only,user(),b"health"),Err(Error::Denied));
  assert_eq!(fs.generation(),generation);
  assert_eq!(complete_verb(b"heal"),Some("health"));
 }
 #[test]fn r044_command_registry_drives_unique_safe_completion(){
  // R078: four new developer commands after the R077 total of 56.
  assert_eq!(COMMAND_VERBS.len(),64);
  for (i,verb) in COMMAND_VERBS.iter().enumerate(){
   assert!(verb.as_bytes().iter().all(|b|b.is_ascii_lowercase()));
   assert!(!COMMAND_VERBS[..i].contains(verb));
  }
  assert_eq!(complete_verb(b"mkf"),Some("mkfile"));
  assert_eq!(complete_verb(b"copyne"),Some("copynew"));
  assert_eq!(complete_verb(b"hex"),Some("hex"));
  assert_eq!(complete_verb(b"slic"),Some("slice"));
  assert_eq!(complete_verb(b"conc"),Some("concat"));
  assert_eq!(complete_verb(b"cmpr"),Some("cmprange"));
  assert_eq!(complete_verb(b"headl"),Some("headlines"));
  assert_eq!(complete_verb(b"rfin"),Some("rfind"));
  assert_eq!(complete_verb(b"tailli"),Some("taillines"));
  assert_eq!(complete_verb(b"linesp"),Some("linespan"));
  assert_eq!(complete_verb(b"lineh"),Some("linehits"));
  assert_eq!(complete_verb(b"sourcev"),Some("sourceview"));
  assert_eq!(complete_verb(b"sourcec"),Some("sourcecontext"));
  assert_eq!(complete_verb(b"textp"),None); // Both textpage and textprofile match.
  assert_eq!(complete_verb(b"textpr"),Some("textprofile"));
  assert_eq!(complete_verb(b"textpa"),Some("textpage"));
  assert_eq!(complete_verb(b"blankr"),Some("blankrows"));
  assert_eq!(complete_verb(b"indentm"),Some("indentmap"));
  assert_eq!(complete_verb(b"eolch"),Some("eolcheck"));
  assert_eq!(complete_verb(b"hea"),None);
  assert_eq!(complete_verb(b"rmd"),Some("rmdir"));
  assert_eq!(complete_verb(b"app"),None);
  assert_eq!(complete_verb(b"pw"),None);
  assert_eq!(complete_verb(b""),None);
  assert_eq!(complete_verb(b"rm /"),None);
  assert_eq!(complete_verb(b"PWD"),None);
 }
 #[test]fn r043_terminal_inspection_tools_report_durable_bytes_and_lines(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();let auth=grants();
  fs.create("/source",b"one\nsecond\nthird").unwrap();
  let start=fs.generation();
  let (n,out)=run(&mut fs,&auth,user(),b"count /source").unwrap();
  assert_eq!(&out[..n],b"16");
  let (n,out)=run(&mut fs,&auth,user(),b"lines /source").unwrap();
  assert_eq!(&out[..n],b"3");
  let (n,out)=run(&mut fs,&auth,user(),b"stat /source").unwrap();
  assert!(core::str::from_utf8(&out[..n]).unwrap().starts_with("bytes:16 generation:"));
  let (n,out)=run(&mut fs,&auth,user(),b"hash /source").unwrap();
  assert_eq!(n,16);assert!(out[..n].iter().all(|b|b.is_ascii_hexdigit()));
  assert_eq!(fs.generation(),start);
  let mut remounted=HierVfs::mount(fs.into_device()).unwrap();
  let (n,out)=run(&mut remounted,&auth,user(),b"tail /source 5").unwrap();
  assert_eq!(&out[..n],b"third");
 }
 #[test]fn r043_terminal_search_and_compare_are_bounded_and_read_only(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();let auth=grants();
  fs.create("/a",b"alpha\nbeta alpha\n").unwrap();
  fs.create("/b",b"alpha\nbeta alpha\n").unwrap();
  let rev=fs.generation();
  let (n,out)=run(&mut fs,&auth,user(),b"find /a beta").unwrap();
  assert_eq!(&out[..n],b"6");
  let (n,out)=run(&mut fs,&auth,user(),b"grep /a alpha").unwrap();
  assert_eq!(&out[..n],b"alpha\nbeta alpha\n");
  let (n,out)=run(&mut fs,&auth,user(),b"cmp /a /b").unwrap();
  assert_eq!(&out[..n],b"equal");
  let (n,out)=run(&mut fs,&auth,user(),b"find /a absent").unwrap();
  assert_eq!(&out[..n],b"not found");
  let mut tiny=[0x44;3];
  assert_eq!(command(&mut fs,&auth,user(),1,b"grep /a alpha",&mut tiny),
    Err(Error::OutputTooSmall));
  assert_eq!(tiny,[0x44;3]);assert_eq!(fs.generation(),rev);
 }
 #[test]fn r043_copynew_touch_and_appendln_preserve_cow_after_reboot(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();let auth=grants();
  fs.mkdir("/docs").unwrap();fs.create("/docs/source",b"start").unwrap();
  run(&mut fs,&auth,user(),b"copynew /docs/source /docs/copy").unwrap();
  assert_eq!(run(&mut fs,&auth,user(),b"copynew /docs/source /docs/copy"),
    Err(Error::Io(FsError::Exists)));
  run(&mut fs,&auth,user(),b"appendln /docs/copy line").unwrap();
  let mut fs=HierVfs::mount(fs.into_device()).unwrap();
  let (n,out)=run(&mut fs,&auth,user(),b"cat /docs/copy").unwrap();
  assert_eq!(&out[..n],b"startline\n");
  run(&mut fs,&auth,user(),b"touch /docs/empty").unwrap();
  let rev=fs.generation();
  let (n,out)=run(&mut fs,&auth,user(),b"touch /docs/empty").unwrap();
  assert_eq!(&out[..n],b"exists");assert_eq!(fs.generation(),rev);
  let mut fs=HierVfs::mount(fs.into_device()).unwrap();
  assert_eq!(run(&mut fs,&auth,user(),b"cat /docs/empty").unwrap().0,0);
 }
 #[test]fn r043_replace_is_atomic_authorized_and_persists_after_remount(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();let auth=grants();
  fs.create("/code",b"cat cat cat").unwrap();
  let before=fs.generation();
  let mut tiny=[0x77;2];
  assert_eq!(command(&mut fs,&auth,user(),1,b"replace /code cat dog",&mut tiny),
    Err(Error::OutputTooSmall));
  assert_eq!(tiny,[0x77;2]);assert_eq!(fs.generation(),before);
  assert_eq!(run(&mut fs,&auth,Identity{generation:2,..user()},
    b"replace /code cat dog"),Err(Error::Denied));
  assert_eq!(run(&mut fs,&auth,user(),b"replace /code cat dog").unwrap().0,8);
  let (n,out)=run(&mut fs,&auth,user(),b"replace /code cat dog").unwrap();
  assert_eq!(&out[..n],b"unchanged");
  let mut fs=HierVfs::mount(fs.into_device()).unwrap();
  let (n,out)=run(&mut fs,&auth,user(),b"cat /code").unwrap();
  assert_eq!(&out[..n],b"dog dog dog");
 }
 #[test]fn r043_write_tools_protect_reserved_namespace_and_byte_caps(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();let auth=grants();
  fs.mkdir("/system").unwrap();fs.create("/system/config",b"trusted").unwrap();
  fs.create("/source",b"stable").unwrap();
  let rev=fs.generation();
  for command_line in [
   b"touch /system/new".as_slice(),
   b"copynew /source /system/new".as_slice(),
   b"appendln /system/config bad".as_slice(),
   b"replace /system/config trusted bad".as_slice(),
  ]{
   assert_eq!(run(&mut fs,&auth,user(),command_line),Err(Error::Denied));
  }
  assert_eq!(fs.generation(),rev);
  let mut out=[0u8;MAX_TRANSFER];
  assert_eq!(command(&mut fs,&auth,user(),1,b"tail /source 257",&mut out),
    Err(Error::Invalid));
 }

 #[test]fn actual_directory_file_rename_and_reboot_remount(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();
  assert_eq!(run(&mut fs,&access,user(),b"mkdir /home").unwrap().0,7);
  assert_eq!(run(&mut fs,&access,user(),b"write /home/note hello").unwrap().0,5);
  let (n,b)=run(&mut fs,&access,user(),b"cat /home/note").unwrap();
  assert_eq!(&b[..n],b"hello");
  let (n,b)=run(&mut fs,&access,user(),b"ls /home").unwrap();
  assert_eq!(&b[..n],b"f:note\n");
  run(&mut fs,&access,user(),b"mv /home/note /home/renamed").unwrap();
  let mut fs=HierVfs::mount(fs.into_device()).unwrap();
  let (n,b)=run(&mut fs,&access,user(),b"cat /home/renamed").unwrap();
  assert_eq!(&b[..n],b"hello");
  assert!(matches!(run(&mut fs,&access,user(),b"cat /home/note"),
   Err(Error::Io(FsError::NotFound))));
 }
 #[test]fn denied_mutation_and_failed_device_write_preserve_data(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();run(&mut fs,&access,user(),b"mkdir /home").unwrap();
  run(&mut fs,&access,user(),b"write /home/a original").unwrap();
  let wrong=Identity{generation:2,..user()};
  let before=fs.generation();
  assert_eq!(run(&mut fs,&access,wrong,b"write /home/a evil"),Err(Error::Denied));
  assert_eq!(run(&mut fs,&access,user(),b"write /../bad evil"),
   Err(Error::Invalid));
  assert_eq!(fs.generation(),before);
  let mut disk=fs.into_device();disk.fail_writes=true;
  let mut fs=HierVfs::mount(disk).unwrap();
  assert_eq!(run(&mut fs,&access,user(),b"write /home/a evil"),
   Err(Error::Io(FsError::Io)));
  let mut disk=fs.into_device();disk.fail_writes=false;
  let mut remount=HierVfs::mount(disk).unwrap();
  let (n,b)=run(&mut remount,&access,user(),b"cat /home/a").unwrap();
  assert_eq!(&b[..n],b"original");
 }
 #[test]fn read_output_must_be_large_enough_without_mutation(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();fs.create("/x",b"hello").unwrap();
  let mut small=[0xccu8;2];
  assert_eq!(command(&mut fs,&access,user(),1,b"cat /x",&mut small),
   Err(Error::OutputTooSmall));
  assert_eq!(small,[0xcc;2]);
 }
 #[test]fn security_m1_terminal_protects_namespaces_across_all_mutators(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  fs.mkdir("/system").unwrap();
  fs.create("/system/config",b"trusted").unwrap();
  fs.mkdir("/home").unwrap();
  fs.create("/home/note",b"personal").unwrap();
  let access=grants();
  let before=fs.generation();
  for cmd in [
   b"write /system/config corrupted".as_slice(),
   b"write //system/./config corrupt".as_slice(),
   b"mkdir /system/evil".as_slice(),
   b"mkdir /boot".as_slice(),
   b"mkdir /recovery".as_slice(),
   b"mv /system/config /home/stolen".as_slice(),
   b"mv /home/note /system/config".as_slice(),
   b"mv /home/note /recovery/file".as_slice(),
  ]{
   assert_eq!(run(&mut fs,&access,user(),cmd),Err(Error::Denied),
              "{:?}",cmd);
   assert_eq!(fs.generation(),before,"denied command mutated the volume");
  }
  assert_eq!(run(&mut fs,&access,user(),
              b"write /system/../home/note corrupt"),
             Err(Error::Invalid));
  let (n,buf)=run(&mut fs,&access,user(),b"cat /system/config").unwrap();
  assert_eq!(&buf[..n],b"trusted"); // read-only inspection remains allowed
  let (n,buf)=run(&mut fs,&access,user(),b"cat /home/note").unwrap();
  assert_eq!(&buf[..n],b"personal");
  let mut fs=HierVfs::mount(fs.into_device()).unwrap();
  let (n,buf)=run(&mut fs,&access,user(),b"cat /system/config").unwrap();
  assert_eq!(&buf[..n],b"trusted");
  let (n,buf)=run(&mut fs,&access,user(),b"cat /home/note").unwrap();
  assert_eq!(&buf[..n],b"personal");
 }
 #[test]fn terminal_removal_and_multi_write_survive_separate_remounts(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();
  run(&mut fs,&access,user(),b"mkdir /docs").unwrap();
  run(&mut fs,&access,user(),b"write /docs/note first").unwrap();
  fs=HierVfs::mount(fs.into_device()).unwrap();
  run(&mut fs,&access,user(),b"write /docs/note second").unwrap();
  fs=HierVfs::mount(fs.into_device()).unwrap();
  let (count,bytes)=run(&mut fs,&access,user(),b"cat /docs/note").unwrap();
  assert_eq!(&bytes[..count],b"second");
  let (count,bytes)=run(&mut fs,&access,user(),b"rm /docs/note").unwrap();
  assert_eq!(&bytes[..count],b"removed");
  fs=HierVfs::mount(fs.into_device()).unwrap();
  assert_eq!(run(&mut fs,&access,user(),b"cat /docs/note"),
    Err(Error::Io(FsError::NotFound)));
 }
 #[test]fn terminal_remove_denied_and_short_output_cannot_delete_data(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();
  fs.mkdir("/system").unwrap();
  fs.create("/system/config",b"trusted").unwrap();
  fs.mkdir("/docs").unwrap();fs.create("/docs/file",b"keep").unwrap();
  let generation=fs.generation();
  assert_eq!(run(&mut fs,&access,user(),b"rm /system/config"),Err(Error::Denied));
  let stale=Identity{generation:2,..user()};
  assert_eq!(run(&mut fs,&access,stale,b"rm /docs/file"),Err(Error::Denied));
  let mut short=[0xf0;2];
  assert_eq!(command(&mut fs,&access,user(),1,b"rm /docs/file",&mut short),
    Err(Error::OutputTooSmall));
  assert_eq!(short,[0xf0;2]);
  assert_eq!(fs.generation(),generation);
  let (n,out)=run(&mut fs,&access,user(),b"cat /docs/file").unwrap();
  assert_eq!(&out[..n],b"keep");
 }
 #[test]fn security_m1_namespace_guard_does_not_block_unrelated_names(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();
  assert_eq!(run(&mut fs,&access,user(),b"mkdir /systematic").unwrap().0,7);
  assert_eq!(run(&mut fs,&access,user(),
                b"write /systematic/log okay").unwrap().0,5);
  let (n,b)=run(&mut fs,&access,user(),b"cat /systematic/log").unwrap();
  assert_eq!(&b[..n],b"okay");
 }

 #[test]fn terminal_append_is_committed_across_multiple_remounts(){
  let access=grants();
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  run(&mut fs,&access,user(),b"mkdir /home").unwrap();
  run(&mut fs,&access,user(),b"write /home/log first").unwrap();
  fs=HierVfs::mount(fs.into_device()).unwrap();
  let (n,reply)=run(&mut fs,&access,user(),b"append /home/log +second").unwrap();
  assert_eq!(&reply[..n],b"appended");
  fs=HierVfs::mount(fs.into_device()).unwrap();
  run(&mut fs,&access,user(),b"append /home/log +third").unwrap();
  fs=HierVfs::mount(fs.into_device()).unwrap();
  let (n,contents)=run(&mut fs,&access,user(),b"cat /home/log").unwrap();
  assert_eq!(&contents[..n],b"first+second+third");
 }
 #[test]fn terminal_append_denial_and_torn_write_preserve_last_commit(){
  let access=grants();
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  run(&mut fs,&access,user(),b"mkdir /docs").unwrap();
  run(&mut fs,&access,user(),b"write /docs/notes stable").unwrap();
  let before=fs.generation();
  let stale=Identity{generation:2,..user()};
  assert_eq!(run(&mut fs,&access,stale,b"append /docs/notes evil"),
    Err(Error::Denied));
  assert_eq!(run(&mut fs,&access,user(),b"append /boot/kernel evil"),
    Err(Error::Denied));
  let mut too_small=[0x5a;7];
  assert_eq!(command(&mut fs,&access,user(),1,b"append /docs/notes evil",
    &mut too_small),Err(Error::OutputTooSmall));
  assert_eq!(too_small,[0x5a;7]);
  assert_eq!(fs.generation(),before);
  let mut disk=fs.into_device();disk.fail_writes=true;
  let mut fs=HierVfs::mount(disk).unwrap();
  assert_eq!(run(&mut fs,&access,user(),b"append /docs/notes unsafe"),
    Err(Error::Io(FsError::Io)));
  let mut disk=fs.into_device();disk.fail_writes=false;
  let mut fs=HierVfs::mount(disk).unwrap();
  let (n,content)=run(&mut fs,&access,user(),b"cat /docs/notes").unwrap();
  assert_eq!(&content[..n],b"stable");
 }
 #[test]fn terminal_copy_is_durable_and_requires_both_rights(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();
  fs.mkdir("/home").unwrap();
  fs.create("/home/source",b"durable source").unwrap();
  fs.create("/home/target",b"old target").unwrap();
  let mut read_only=Access::<1>::new();
  read_only.grant(user(),FILE_READ,500).unwrap();
  let mut write_only=Access::<1>::new();
  write_only.grant(user(),FILE_WRITE,500).unwrap();
  let before=fs.generation();
  assert_eq!(run(&mut fs,&read_only,user(),b"cp /home/source /home/target"),
             Err(Error::Denied));
  assert_eq!(run(&mut fs,&write_only,user(),b"cp /home/source /home/target"),
             Err(Error::Denied));
  assert_eq!(fs.generation(),before);
  let (n,result)=run(&mut fs,&access,user(),b"cp /home/source /home/target").unwrap();
  assert_eq!(&result[..n],b"copied");
  fs=HierVfs::mount(fs.into_device()).unwrap();
  let (n,data)=run(&mut fs,&access,user(),b"cat /home/target").unwrap();
  assert_eq!(&data[..n],b"durable source");
  run(&mut fs,&access,user(),b"cp /home/source /home/second").unwrap();
  fs=HierVfs::mount(fs.into_device()).unwrap();
  let (n,data)=run(&mut fs,&access,user(),b"cat /home/second").unwrap();
  assert_eq!(&data[..n],b"durable source");
 }
 #[test]fn terminal_copy_rejected_paths_and_small_output_do_not_mutate(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();
  fs.mkdir("/home").unwrap();fs.mkdir("/system").unwrap();
  fs.create("/home/source",b"good").unwrap();
  fs.create("/home/target",b"keep").unwrap();
  fs.create("/system/config",b"protected").unwrap();
  let before=fs.generation();
  for cmd in [
   b"cp /home/source /system/config".as_slice(),
   b"cp /system/config /boot/config".as_slice(),
  ]{
   assert_eq!(run(&mut fs,&access,user(),cmd),Err(Error::Denied));
  }
  assert_eq!(run(&mut fs,&access,user(),b"cp /home/source /home/source"),
             Err(Error::Invalid));
  assert_eq!(run(&mut fs,&access,user(),b"cp /home/missing /home/target"),
             Err(Error::Io(FsError::NotFound)));
  let mut small=[0xcc;5];
  assert_eq!(command(&mut fs,&access,user(),1,
             b"cp /home/source /home/target",&mut small),
             Err(Error::OutputTooSmall));
  assert_eq!(small,[0xcc;5]);
  assert_eq!(fs.generation(),before);
  fs=HierVfs::mount(fs.into_device()).unwrap();
  let (n,data)=run(&mut fs,&access,user(),b"cat /home/target").unwrap();
  assert_eq!(&data[..n],b"keep");
  let (n,data)=run(&mut fs,&access,user(),b"cat /system/config").unwrap();
  assert_eq!(&data[..n],b"protected");
 }
 #[test]fn terminal_copy_failed_disk_write_keeps_last_committed_target(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();
  fs.mkdir("/home").unwrap();
  fs.create("/home/source",b"replacement").unwrap();
  fs.create("/home/target",b"committed").unwrap();
  let mut disk=fs.into_device();disk.fail_writes=true;
  let mut fs=HierVfs::mount(disk).unwrap();
  assert_eq!(run(&mut fs,&access,user(),b"cp /home/source /home/target"),
             Err(Error::Io(FsError::Io)));
  let mut disk=fs.into_device();disk.fail_writes=false;
  let mut fs=HierVfs::mount(disk).unwrap();
  let (n,data)=run(&mut fs,&access,user(),b"cat /home/target").unwrap();
  assert_eq!(&data[..n],b"committed");
 }


 #[test]fn mkfile_empty_file_is_durable_across_remount(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();
  run(&mut fs,&access,user(),b"mkdir /home").unwrap();
  assert_eq!(run(&mut fs,&access,user(),b"mkfile /home/blank").unwrap().0,7);
  let mut fs=HierVfs::mount(fs.into_device()).unwrap();
  let (n,_)=run(&mut fs,&access,user(),b"cat /home/blank").unwrap();
  assert_eq!(n,0);
 }
 #[test]fn mkfile_denies_reserved_paths_and_duplicate_mutations(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();
  assert_eq!(run(&mut fs,&access,user(),b"mkfile /boot/system"),
             Err(Error::Denied));
  run(&mut fs,&access,user(),b"mkfile /blank").unwrap();
  let generation=fs.generation();
  assert_eq!(run(&mut fs,&access,user(),b"mkfile /blank"),
             Err(Error::Io(FsError::Exists)));
  assert_eq!(fs.generation(),generation);
 }
 #[test]fn mkfile_small_response_and_stale_identity_do_not_write(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();
  let generation=fs.generation();
  let mut out=[0xcc;2];
  assert_eq!(command(&mut fs,&access,user(),1,b"mkfile /blank",&mut out),
             Err(Error::OutputTooSmall));
  assert_eq!(out,[0xcc;2]);
  assert_eq!(run(&mut fs,&access,Identity{pid:3,generation:2},
                 b"mkfile /blank"),Err(Error::Denied));
  assert_eq!(fs.generation(),generation);
 }


 #[test]fn truncate_and_head_survive_a_real_block_device_remount(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();
  run(&mut fs,&access,user(),b"mkdir /home").unwrap();
  run(&mut fs,&access,user(),b"write /home/log abcdef").unwrap();
  let (n,b)=run(&mut fs,&access,user(),b"head /home/log 3").unwrap();
  assert_eq!(&b[..n],b"abc");
  let mut fs=HierVfs::mount(fs.into_device()).unwrap();
  let (n,b)=run(&mut fs,&access,user(),b"head /home/log 99").unwrap();
  assert_eq!(&b[..n],b"abcdef");
  assert_eq!(run(&mut fs,&access,user(),b"truncate /home/log").unwrap().0,9);
  let mut fs=HierVfs::mount(fs.into_device()).unwrap();
  assert_eq!(run(&mut fs,&access,user(),b"cat /home/log").unwrap().0,0);
  run(&mut fs,&access,user(),b"append /home/log xyz").unwrap();
  let (n,b)=run(&mut fs,&access,user(),b"head /home/log 2").unwrap();
  assert_eq!(&b[..n],b"xy");
 }
 #[test]fn truncate_denies_protected_and_missing_paths_without_mutation(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();
  let generation=fs.generation();
  assert_eq!(run(&mut fs,&access,user(),b"truncate /system/settings"),
             Err(Error::Denied));
  assert_eq!(run(&mut fs,&access,user(),b"truncate /absent"),
             Err(Error::Io(FsError::NotFound)));
  assert_eq!(fs.generation(),generation);
 }
 #[test]fn bounded_head_rejects_malformed_counts_and_stale_process_generation(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();
  run(&mut fs,&access,user(),b"mkfile /empty").unwrap();
  for cmd in [b"head /empty 0".as_slice(),b"head /empty x",
              b"head /empty 9999999",b"head /empty -1"]{
   assert_eq!(run(&mut fs,&access,user(),cmd),Err(Error::Invalid));
  }
  assert_eq!(run(&mut fs,&access,
    Identity{generation:2,..user()},b"head /empty 1"),Err(Error::Denied));
 }
 #[test]fn insufficient_terminal_output_never_truncates_file(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();
  run(&mut fs,&access,user(),b"write /note original").unwrap();
  let generation=fs.generation();
  let mut out=[0xaau8;4];
  assert_eq!(command(&mut fs,&access,user(),1,b"truncate /note",&mut out),
             Err(Error::OutputTooSmall));
  assert_eq!(fs.generation(),generation);
  assert_eq!(out,[0xaa;4]);
  let (n,b)=run(&mut fs,&access,user(),b"cat /note").unwrap();
  assert_eq!(&b[..n],b"original");
 }


 #[test]fn recursive_directory_move_and_empty_remove_survive_remount(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();let a=grants();
  run(&mut fs,&a,user(),b"mkdir /old").unwrap();
  run(&mut fs,&a,user(),b"mkdir /old/sub").unwrap();
  run(&mut fs,&a,user(),b"write /old/sub/note forever").unwrap();
  run(&mut fs,&a,user(),b"mvdir /old /new").unwrap();
  let mut fs=HierVfs::mount(fs.into_device()).unwrap();
  let (n,data)=run(&mut fs,&a,user(),b"cat /new/sub/note").unwrap();
  assert_eq!(&data[..n],b"forever");
  assert_eq!(run(&mut fs,&a,user(),b"rmdir /new"),
             Err(Error::Io(FsError::Full)));
  run(&mut fs,&a,user(),b"rm /new/sub/note").unwrap();
  run(&mut fs,&a,user(),b"rmdir /new/sub").unwrap();
  run(&mut fs,&a,user(),b"rmdir /new").unwrap();
  let mut fs=HierVfs::mount(fs.into_device()).unwrap();
  assert!(matches!(run(&mut fs,&a,user(),b"ls /new"),Err(Error::Io(_))));
 }
 #[test]fn directory_cycle_and_protected_destination_never_mutate_disk(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();let a=grants();
  run(&mut fs,&a,user(),b"mkdir /safe").unwrap();
  run(&mut fs,&a,user(),b"mkdir /safe/sub").unwrap();
  let old=fs.generation();
  assert_eq!(run(&mut fs,&a,user(),b"mvdir /safe /safe/sub/cycle"),
             Err(Error::Io(FsError::InvalidName)));
  assert_eq!(run(&mut fs,&a,user(),b"mvdir /safe /system"),
             Err(Error::Denied));
  assert_eq!(fs.generation(),old);
 }
 #[test]fn directory_operation_requires_live_grant_and_output_preflight(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();let a=grants();
  run(&mut fs,&a,user(),b"mkdir /empty").unwrap();
  let old=fs.generation();
  let mut out=[0x5bu8;3];
  assert_eq!(command(&mut fs,&a,user(),1,b"rmdir /empty",&mut out),
             Err(Error::OutputTooSmall));
  assert_eq!(out,[0x5b;3]);
  assert_eq!(run(&mut fs,&a,Identity{generation:2,..user()},b"rmdir /empty"),
             Err(Error::Denied));
  assert_eq!(fs.generation(),old);
  run(&mut fs,&a,user(),b"rmdir /empty").unwrap();
 }


 #[test]fn persistent_random_access_survives_mount_and_keeps_other_bytes(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();let a=grants();
  run(&mut fs,&a,user(),b"write /note abcdefgh").unwrap();
  let (n,out)=run(&mut fs,&a,user(),b"pread /note 2 3").unwrap();
  assert_eq!(&out[..n],b"cde");
  run(&mut fs,&a,user(),b"pwrite /note@3 XYZ").unwrap();
  let mut fs=HierVfs::mount(fs.into_device()).unwrap();
  let (n,out)=run(&mut fs,&a,user(),b"cat /note").unwrap();
  assert_eq!(&out[..n],b"abcXYZgh");
 }
 #[test]fn random_io_rejects_stale_caller_protected_write_and_sparse_holes(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();let a=grants();
  run(&mut fs,&a,user(),b"write /note original").unwrap();
  let generation=fs.generation();
  let stale=Identity{generation:2,..user()};
  assert_eq!(run(&mut fs,&a,stale,b"pread /note 1 2"),Err(Error::Denied));
  assert_eq!(run(&mut fs,&a,stale,b"pwrite /note@0 bad"),Err(Error::Denied));
  assert_eq!(run(&mut fs,&a,user(),b"pwrite /system/key@0 bad"),Err(Error::Denied));
  assert!(matches!(run(&mut fs,&a,user(),b"pwrite /note@1000 bad"),Err(Error::Io(_))));
  assert_eq!(fs.generation(),generation);
  let (n,out)=run(&mut fs,&a,user(),b"cat /note").unwrap();
  assert_eq!(&out[..n],b"original");
 }
 #[test]fn malformed_offsets_and_output_preflight_never_touch_disk(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();let a=grants();
  run(&mut fs,&a,user(),b"write /note hello").unwrap();
  let before=fs.generation();
  for cmd in [b"pread /note x 2".as_slice(),b"pread /note 0 0",
              b"pread /note 0 300",b"pwrite /note@-1 text",
              b"pwrite /note@invalid text"]{
   assert!(run(&mut fs,&a,user(),cmd).is_err());
  }
  let mut out=[0x99u8;2];
  assert_eq!(command(&mut fs,&a,user(),1,b"pwrite /note@0 hi",&mut out),
             Err(Error::OutputTooSmall));
  assert_eq!(out,[0x99;2]);
  assert_eq!(fs.generation(),before);
 }


 #[test]fn copies_multi_transfer_file_and_preserves_bytes_after_reboot(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();
  let mut contents=[0u8;600];
  for (i,c) in contents.iter_mut().enumerate(){*c=b'A'+(i%26) as u8;}
  fs.create("/source",&contents).unwrap();
  let (n,b)=run(&mut fs,&access,user(),b"cp /source /copy").unwrap();
  assert_eq!(&b[..n],b"copied");
  let mut fs=HierVfs::mount(fs.into_device()).unwrap();
  let mut out=[0u8;600];
  assert_eq!(fs.read("/copy",&mut out),Ok(600));
  assert_eq!(out,contents);
  // Copy must be a distinct object; source survives destination overwrite.
  fs.overwrite("/copy",b"independent").unwrap();
  assert_eq!(fs.read("/source",&mut out),Ok(600));
  assert_eq!(out,contents);
 }
 #[test]fn copy_preflight_and_stale_process_do_not_create_destination(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();
  fs.create("/source",b"protected source").unwrap();
  let before=fs.generation();
  let mut output=[0x7bu8;4];
  assert_eq!(command(&mut fs,&access,user(),1,b"cp /source /copy",&mut output),
             Err(Error::OutputTooSmall));
  assert_eq!(output,[0x7b;4]);
  assert_eq!(run(&mut fs,&access,Identity{pid:3,generation:2},
             b"cp /source /copy"),Err(Error::Denied));
  assert_eq!(run(&mut fs,&access,user(),b"cp /source /system"),
             Err(Error::Denied));
  assert_eq!(fs.generation(),before);
  let mut out=[0u8;32];
  assert_eq!(fs.read("/copy",&mut out),Err(FsError::NotFound));
 }

}


// M1 G1/G2: actual block-device partitions, not a simulated RAM namespace.
// All remounts reconstruct VFS metadata from the selected disk sectors.
#[cfg(test)]
mod m1_partition_terminal_integration_tests {
 use super::*;
 use crate::block::{BlockWindow,BlockError,SECTOR_BYTES};
 use crate::m1_partition::{Error as PartError,MountError,Partition,Scheme};

 const SECTORS:usize=256;
 const FIRST_START:u64=10;
 const SECOND_START:u64=120;
 const LENGTH:u64=100;
 struct Disk {
  sectors:[[u8;SECTOR_BYTES];SECTORS],
  writes:usize,
  fail_at:Option<usize>,
 }
 impl BlockDevice for Disk {
  fn sector_count(&self)->u64{SECTORS as u64}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
   ->Result<(),BlockError>{
   *out=*self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?;
   Ok(())
  }
  fn write_sector(&mut self,lba:u64,bytes:&[u8;SECTOR_BYTES])
   ->Result<(),BlockError>{
   if self.fail_at==Some(self.writes){return Err(BlockError::Io);}
   *self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?= *bytes;
   self.writes+=1;
   Ok(())
  }
 }
 fn disk()->Disk{
  let mut d=Disk{sectors:[[0;SECTOR_BYTES];SECTORS],writes:0,fail_at:None};
  d.sectors[0][510]=0x55;
  d.sectors[0][511]=0xaa;
  for (slot,start) in [(0usize,FIRST_START),(1usize,SECOND_START)]{
   let off=446+slot*16;
   d.sectors[0][off+4]=0x83;
   d.sectors[0][off+8..off+12].copy_from_slice(&(start as u32).to_le_bytes());
   d.sectors[0][off+12..off+16].copy_from_slice(&(LENGTH as u32).to_le_bytes());
  }
  // Explicit test provisioning. The production command API never formats.
  for start in [FIRST_START,SECOND_START]{
   let window=BlockWindow::new(&mut d,start,LENGTH).unwrap();
   let mut volume=HierVfs::format_blank(window).unwrap();
   volume.mkdir("/home").unwrap();
  }
  d
 }
 fn owner()->Identity{Identity{pid:3,generation:7}}
 fn access()->Access<1>{
  let mut a=Access::new();
  a.grant(owner(),FILE_READ|FILE_WRITE,100).unwrap();
  a
 }
 fn exec(d:&mut Disk,index:u32,actor:Identity,command_bytes:&[u8])
  ->Result<(usize,[u8;MAX_TRANSFER]),PartitionCommandError>{
  let mut out=[0xa5;MAX_TRANSFER];
  let n=command_on_partition(d,index,&access(),actor,1,command_bytes,&mut out)?;
  Ok((n,out))
 }
 #[test]
 fn m1_selected_partition_terminal_commands_survive_remount_and_are_isolated(){
  let mut d=disk();
  let metadata=d.sectors[0];
  let gap=d.sectors[110];
  let untouched=d.sectors[220];
  let (n,out)=exec(&mut d,0,owner(),b"write /home/note first").unwrap();
  assert_eq!(&out[..n],b"saved");
  exec(&mut d,1,owner(),b"write /home/note second").unwrap();
  let (n,a)=exec(&mut d,0,owner(),b"cat /home/note").unwrap();
  assert_eq!(&a[..n],b"first");
  let (n,b)=exec(&mut d,1,owner(),b"cat /home/note").unwrap();
  assert_eq!(&b[..n],b"second");
  exec(&mut d,0,owner(),b"mv /home/note /home/moved").unwrap();
  let (n,a)=exec(&mut d,0,owner(),b"cat /home/moved").unwrap();
  assert_eq!(&a[..n],b"first");
  let (n,b)=exec(&mut d,1,owner(),b"cat /home/note").unwrap();
  assert_eq!(&b[..n],b"second");
  assert_eq!(d.sectors[0],metadata);
  assert_eq!(d.sectors[110],gap);
  assert_eq!(d.sectors[220],untouched);
 }
 #[test]
 fn m1_selected_partition_denied_identity_corruption_and_bad_index_cannot_write(){
  let mut d=disk();
  let before=d.writes;
  let stale=Identity{generation:8,..owner()};
  let mut output=[0xa5;16];
  assert_eq!(command_on_partition(&mut d,0,&access(),stale,1,
   b"write /home/evil denied",&mut output),
   Err(PartitionCommandError::Command(Error::Denied)));
  assert_eq!(d.writes,before);
  assert_eq!(output,[0xa5;16]);
  assert_eq!(command_on_partition(&mut d,0,&access(),Identity{pid:0,generation:7},
   1,b"cat /home/note",&mut output),
   Err(PartitionCommandError::Command(Error::Denied)));
  assert_eq!(command_on_partition(&mut d,2,&access(),owner(),1,
   b"write /home/evil denied",&mut output),
   Err(PartitionCommandError::Mount(MountError::Partition(PartError::NotFound))));
  assert_eq!(d.writes,before);
  d.sectors[0][510]=0;
  assert_eq!(command_on_partition(&mut d,0,&access(),owner(),1,
   b"write /home/evil denied",&mut output),
   Err(PartitionCommandError::Mount(MountError::Partition(PartError::Invalid))));
  assert_eq!(d.writes,before);
 }
 #[test]
 fn m1_selected_partition_power_cut_keeps_previous_committed_file(){
  let mut d=disk();
  exec(&mut d,0,owner(),b"write /home/note stable").unwrap();
  // Every command remounts its selected window; fail a COW sector write.
  d.fail_at=Some(d.writes+1);
  let mut response=[0x66;16];
  assert_eq!(command_on_partition(&mut d,0,&access(),owner(),1,
   b"write /home/note corrupted",&mut response),
   Err(PartitionCommandError::Command(Error::Io(FsError::Io))));
  assert_eq!(response,[0x66;16]);
  d.fail_at=None;
  let (n,out)=exec(&mut d,0,owner(),b"cat /home/note").unwrap();
  assert_eq!(&out[..n],b"stable");
  assert_eq!(exec(&mut d,1,owner(),b"cat /home/note"),
   Err(PartitionCommandError::Command(Error::Io(FsError::NotFound))));
 }
}


#[cfg(test)]
mod r060_hex_diagnostic_tests{
 use super::*;
 use crate::block::{BlockError,SECTOR_BYTES};
 struct TestDisk{sectors:[[u8;SECTOR_BYTES];100]}
 impl BlockDevice for TestDisk{
  fn sector_count(&self)->u64{100}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *out=*self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,lba:u64,bytes:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*bytes;Ok(())
  }
 }
 fn owner()->Identity{Identity{pid:6,generation:4}}
 fn ready()->(HierVfs<TestDisk>,Access<1>){
  let mut fs=HierVfs::format_blank(TestDisk{
   sectors:[[0;SECTOR_BYTES];100]}).unwrap();
  fs.create("/binary",&[0,0x1f,0xff,b'A']).unwrap();
  let mut auth=Access::new();auth.grant(owner(),FILE_READ,100).unwrap();
  (fs,auth)
 }
 #[test]fn persistent_hex_bytes_survive_fresh_remount(){
  let (mut fs,auth)=ready();let generation=fs.generation();
  let mut out=[0xa5;32];
  assert_eq!(command(&mut fs,&auth,owner(),1,b"hex /binary 1 3",&mut out),Ok(6));
  assert_eq!(&out[..6],b"1fff41");
  fs=HierVfs::mount(fs.into_device()).unwrap();
  assert_eq!(command(&mut fs,&auth,owner(),1,b"hex /binary 0 4",&mut out),Ok(8));
  assert_eq!(&out[..8],b"001fff41");
  assert_eq!(fs.generation(),generation);
 }
 #[test]fn no_partial_hex_output_or_cross_generation_data_leak(){
  let (mut fs,auth)=ready();let mut short=[0xb8;3];
  assert_eq!(command(&mut fs,&auth,owner(),1,b"hex /binary 0 4",&mut short),
    Err(Error::OutputTooSmall));
  assert_eq!(short,[0xb8;3]);
  assert_eq!(command(&mut fs,&auth,Identity{generation:5,..owner()},1,
    b"hex /binary 0 2",&mut short),Err(Error::Denied));
  for invalid in [b"hex /binary 0 0".as_slice(),b"hex /binary -1 2",
    b"hex /binary 0 999999",b"hex /../bad 0 2"]{
   assert_eq!(command(&mut fs,&auth,owner(),1,invalid,&mut short),
       Err(Error::Invalid));
  }
  assert_eq!(short,[0xb8;3]);
 }
}


#[cfg(test)]
mod r061_persistent_slice_tests{
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
 fn actor()->Identity{Identity{pid:4,generation:6}}
 fn ready()->(HierVfs<Disk>,Access<1>){
  let mut fs=HierVfs::format_blank(Disk{data:[[0;SECTOR_BYTES];100]}).unwrap();
  fs.create("/binary",&[0,1,2,3,255]).unwrap();
  let mut access=Access::new();
  access.grant(actor(),FILE_READ|FILE_WRITE,100).unwrap();
  (fs,access)
 }
 #[test]fn binary_slice_is_durable_and_does_not_modify_source(){
  let (mut fs,auth)=ready();let mut output=[0u8;16];
  let before=fs.generation();
  assert_eq!(command(&mut fs,&auth,actor(),1,
    b"slice /binary 1 3 /portion",&mut output),Ok(6));
  assert_eq!(&output[..6],b"copied");
  assert!(fs.generation()>before);
  let mut saved=[0u8;8];
  assert_eq!(fs.read("/binary",&mut saved),Ok(5));
  assert_eq!(&saved[..5],&[0,1,2,3,255]);
  fs=HierVfs::mount(fs.into_device()).unwrap();
  assert_eq!(fs.read("/portion",&mut saved),Ok(3));
  assert_eq!(&saved[..3],&[1,2,3]);
 }
 #[test]fn existing_destination_and_short_output_do_not_publish(){
  let (mut fs,auth)=ready();
  fs.create("/portion",b"untouched").unwrap();
  let version=fs.generation();let mut out=[0xa5;5];
  assert_eq!(command(&mut fs,&auth,actor(),1,
    b"slice /binary 0 3 /new",&mut out),Err(Error::OutputTooSmall));
  assert_eq!(out,[0xa5;5]);assert_eq!(fs.generation(),version);
  let mut long=[0u8;16];
  assert_eq!(command(&mut fs,&auth,actor(),1,
    b"slice /binary 0 3 /portion",&mut long),
    Err(Error::Io(FsError::Exists)));
  assert_eq!(fs.generation(),version);
  let mut verify=[0u8;16];assert_eq!(fs.read("/portion",&mut verify),Ok(9));
  assert_eq!(&verify[..9],b"untouched");
 }
 #[test]fn stale_owner_protected_path_and_bad_offsets_are_fail_closed(){
  let (mut fs,auth)=ready();
  let before=fs.generation();let mut out=[0x22;16];
  let stale=Identity{generation:7,..actor()};
  assert_eq!(command(&mut fs,&auth,stale,1,
     b"slice /binary 0 2 /copy",&mut out),Err(Error::Denied));
  for bad in [b"slice /binary 0 0 /copy".as_slice(),
              b"slice /binary -1 2 /copy",
              b"slice /binary 0 99999 /copy",
              b"slice /binary 0 2 /../bad"]{
   assert_eq!(command(&mut fs,&auth,actor(),1,bad,&mut out),
     Err(Error::Invalid));
  }
  assert_eq!(fs.generation(),before);
  assert_eq!(out,[0x22;16]);
 }
}


#[cfg(test)]
mod r062_durable_concat_tests{
 use super::*;
 use crate::block::{BlockError,SECTOR_BYTES};
 struct TestDisk{sectors:[[u8;SECTOR_BYTES];100]}
 impl BlockDevice for TestDisk{
  fn sector_count(&self)->u64{100}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *out=*self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,lba:u64,bytes:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*bytes;Ok(())
  }
 }
 fn identity()->Identity{Identity{pid:7,generation:3}}
 fn setup()->(HierVfs<TestDisk>,Access<1>){
  let mut fs=HierVfs::format_blank(TestDisk{sectors:[[0;SECTOR_BYTES];100]}).unwrap();
  fs.create("/first",b"alpha").unwrap();
  fs.create("/second",&[0,0xff,b'Z']).unwrap();
  let mut auth=Access::new();
  auth.grant(identity(),FILE_READ|FILE_WRITE,100).unwrap();
  (fs,auth)
 }
 #[test]fn binary_concat_is_one_durable_create_across_reboot(){
  let (mut fs,auth)=setup();let start=fs.generation();
  let mut output=[0;16];
  assert_eq!(command(&mut fs,&auth,identity(),1,
   b"concat /first /second /joined",&mut output),Ok(6));
  assert_eq!(&output[..6],b"joined");
  assert_eq!(fs.generation(),start+1);
  fs=HierVfs::mount(fs.into_device()).unwrap();
  let mut saved=[0;16];
  assert_eq!(fs.read("/joined",&mut saved),Ok(8));
  assert_eq!(&saved[..8],b"alpha\x00\xffZ");
  assert_eq!(fs.read("/first",&mut saved),Ok(5));
  assert_eq!(&saved[..5],b"alpha");
 }
 #[test]fn short_response_existing_destination_and_stale_owner_preserve_disk(){
  let (mut fs,auth)=setup();
  fs.create("/joined",b"keep").unwrap();
  let start=fs.generation();
  let mut out=[0x6au8;5];
  assert_eq!(command(&mut fs,&auth,identity(),1,
   b"concat /first /second /new",&mut out),Err(Error::OutputTooSmall));
  assert_eq!(out,[0x6a;5]);
  let mut long=[0u8;16];
  assert_eq!(command(&mut fs,&auth,identity(),1,
   b"concat /first /second /joined",&mut long),
   Err(Error::Io(FsError::Exists)));
  assert_eq!(command(&mut fs,&auth,
   Identity{generation:4,..identity()},1,
   b"concat /first /second /new",&mut long),Err(Error::Denied));
  assert_eq!(fs.generation(),start);
  let mut kept=[0u8;8];assert_eq!(fs.read("/joined",&mut kept),Ok(4));
  assert_eq!(&kept[..4],b"keep");
 }
 #[test]fn bad_source_missing_input_and_reserved_target_are_fail_closed(){
  let (mut fs,auth)=setup();let start=fs.generation();
  let mut out=[0x33;16];
  for invalid in [
    b"concat /first /second /first".as_slice(),
    b"concat /first /second /../bad",
    b"concat /first /second",
    b"concat /first /second /new extra"
  ]{
   assert_eq!(command(&mut fs,&auth,identity(),1,invalid,&mut out),
     Err(Error::Invalid));
  }
  assert_eq!(command(&mut fs,&auth,identity(),1,
   b"concat /first /missing /new",&mut out),
   Err(Error::Io(FsError::NotFound)));
  assert_eq!(fs.generation(),start);
  assert_eq!(out,[0x33;16]);
 }
}


#[cfg(test)]
mod r063_range_comparison_tests{
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
 fn caller()->Identity{Identity{pid:8,generation:7}}
 fn mounted()->(HierVfs<Disk>,Access<1>){
  let mut fs=HierVfs::format_blank(Disk{sectors:[[0;SECTOR_BYTES];100]}).unwrap();
  fs.create("/left",b"hello-world").unwrap();
  fs.create("/right",b"XXhello-YY").unwrap();
  let mut auth=Access::new();
  auth.grant(caller(),FILE_READ,100).unwrap();
  (fs,auth)
 }
 #[test]fn real_durable_byte_ranges_compare_identical_and_different_contents(){
  let (mut fs,auth)=mounted();let generation=fs.generation();
  let mut out=[0u8;24];
  assert_eq!(command(&mut fs,&auth,caller(),1,
    b"cmprange /left 0 /right 2 6",&mut out),Ok(5));
  assert_eq!(&out[..5],b"equal");
  assert_eq!(command(&mut fs,&auth,caller(),1,
    b"cmprange /left 5 /right 2 4",&mut out),Ok(9));
  assert_eq!(&out[..9],b"different");
  fs=HierVfs::mount(fs.into_device()).unwrap();
  assert_eq!(command(&mut fs,&auth,caller(),1,
    b"cmprange /left 0 /right 2 6",&mut out),Ok(5));
  assert_eq!(fs.generation(),generation);
 }
 #[test]fn short_response_and_stale_owner_never_publish_partial_comparison(){
  let (mut fs,auth)=mounted();let generation=fs.generation();
  let mut short=[0xa5;4];
  assert_eq!(command(&mut fs,&auth,caller(),1,
    b"cmprange /left 0 /right 2 6",&mut short),
    Err(Error::OutputTooSmall));
  assert_eq!(short,[0xa5;4]);
  let stale=Identity{generation:9,..caller()};
  let mut long=[0u8;24];
  assert_eq!(command(&mut fs,&auth,stale,1,
    b"cmprange /left 0 /right 2 6",&mut long),Err(Error::Denied));
  assert_eq!(fs.generation(),generation);
 }
 #[test]fn invalid_missing_overrun_and_traversal_ranges_are_rejected(){
  let (mut fs,auth)=mounted();let generation=fs.generation();
  let mut out=[0xa5;24];
  for bad in [
    b"cmprange /left 0 /right 2 0".as_slice(),
    b"cmprange /left 0 /right 2 999999",
    b"cmprange /left -1 /right 2 4",
    b"cmprange /left 0 /../right 2 4",
    b"cmprange /left 0 /right 2 4 extra",
  ]{
   assert_eq!(command(&mut fs,&auth,caller(),1,bad,&mut out),Err(Error::Invalid));
  }
  assert_eq!(command(&mut fs,&auth,caller(),1,
    b"cmprange /left 10 /right 2 6",&mut out),Err(Error::Invalid));
  assert_eq!(command(&mut fs,&auth,caller(),1,
    b"cmprange /left 0 /missing 0 3",&mut out),
    Err(Error::Io(FsError::NotFound)));
  assert_eq!(out,[0xa5;24]);
  assert_eq!(fs.generation(),generation);
 }
}


#[cfg(test)]
mod r064_byte_frequency_tests{
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
 fn actor()->Identity{Identity{pid:19,generation:3}}
 fn setup()->(HierVfs<Disk>,Access<1>){
  let mut fs=HierVfs::format_blank(Disk{sectors:[[0;SECTOR_BYTES];100]}).unwrap();
  fs.create("/binary",&[0,65,0,65,255]).unwrap();
  let mut access=Access::new();
  access.grant(actor(),FILE_READ,100).unwrap();
  (fs,access)
 }
 #[test]fn count_binary_bytes_from_actual_durable_file_after_remount(){
  let (mut fs,auth)=setup();let generation=fs.generation();
  let mut out=[0u8;8];
  assert_eq!(command(&mut fs,&auth,actor(),1,
   b"bytefreq /binary 00",&mut out),Ok(1));
  assert_eq!(&out[..1],b"2");
  fs=HierVfs::mount(fs.into_device()).unwrap();
  assert_eq!(command(&mut fs,&auth,actor(),1,
   b"bytefreq /binary ff",&mut out),Ok(1));
  assert_eq!(&out[..1],b"1");
  assert_eq!(command(&mut fs,&auth,actor(),1,
   b"bytefreq /binary 41",&mut out),Ok(1));
  assert_eq!(&out[..1],b"2");
  assert_eq!(fs.generation(),generation);
 }
 #[test]fn invalid_hex_bad_paths_and_stale_owner_never_publish(){
  let (mut fs,auth)=setup();let before=fs.generation();
  let mut out=[0xaa;8];
  for invalid in [
   b"bytefreq /binary 0".as_slice(),
   b"bytefreq /binary gg",
   b"bytefreq /binary 000",
   b"bytefreq /../escape 00",
   b"bytefreq /binary 00 extra"
  ]{
   assert_eq!(command(&mut fs,&auth,actor(),1,invalid,&mut out),
    Err(Error::Invalid));
  }
  assert_eq!(command(&mut fs,&auth,actor(),1,
   b"bytefreq /missing 00",&mut out),Err(Error::Io(FsError::NotFound)));
  assert_eq!(command(&mut fs,&auth,
   Identity{generation:4,..actor()},1,
   b"bytefreq /binary 00",&mut out),Err(Error::Denied));
  assert_eq!(out,[0xaa;8]);
  assert_eq!(fs.generation(),before);
 }
 #[test]fn short_output_buffer_does_not_receive_partial_number(){
  let (mut fs,auth)=setup();let mut out=[0x42;0];
  assert_eq!(command(&mut fs,&auth,actor(),1,
   b"bytefreq /binary 00",&mut out),Err(Error::OutputTooSmall));
 }
}


/// R065: strictly decode a single hex byte for binary file diagnostics.
/// Reject signs, whitespace, partial digits and non-ASCII characters.
fn r065_hex_byte(encoded:&str)->Result<u8,Error>{
 let bytes=encoded.as_bytes();
 if bytes.len()!=2{return Err(Error::Invalid);}
 let nibble=|b:u8|->Result<u8,Error>{
  match b{
   b'0'..=b'9'=>Ok(b-b'0'),
   b'a'..=b'f'=>Ok(b-b'a'+10),
   b'A'..=b'F'=>Ok(b-b'A'+10),
   _=>Err(Error::Invalid),
  }
 };
 Ok((nibble(bytes[0])?<<4)|nibble(bytes[1])?)
}
#[cfg(test)]
mod r065_durable_hex_patch_tests{
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
 fn actor()->Identity{Identity{pid:18,generation:4}}
 fn setup()->(HierVfs<Disk>,Access<1>){
  let mut fs=HierVfs::format_blank(Disk{data:[[0;SECTOR_BYTES];100]}).unwrap();
  fs.create("/binary",&[0,65,255]).unwrap();
  let mut auth=Access::new();
  auth.grant(actor(),FILE_READ|FILE_WRITE,100).unwrap();
  (fs,auth)
 }
 #[test]fn binary_edit_is_one_cow_overwrite_and_survives_mount(){
  let (mut fs,auth)=setup();let before=fs.generation();
  let mut out=[0u8;16];
  assert_eq!(command(&mut fs,&auth,actor(),1,
   b"patchhex /binary 1 7f",&mut out),Ok(7));
  assert_eq!(&out[..7],b"patched");
  assert_eq!(fs.generation(),before+1);
  fs=HierVfs::mount(fs.into_device()).unwrap();
  let mut bytes=[0u8;8];
  assert_eq!(fs.read("/binary",&mut bytes),Ok(3));
  assert_eq!(&bytes[..3],&[0,127,255]);
  assert_eq!(command(&mut fs,&auth,actor(),1,
   b"patchhex /binary 1 7F",&mut out),Ok(9));
  assert_eq!(&out[..9],b"unchanged");
  assert_eq!(fs.generation(),before+1);
 }
 #[test]fn short_output_stale_identity_and_reserved_namespace_fail_closed(){
  let (mut fs,auth)=setup();let generation=fs.generation();
  let mut short=[0xab;6];
  assert_eq!(command(&mut fs,&auth,actor(),1,
   b"patchhex /binary 1 09",&mut short),Err(Error::OutputTooSmall));
  assert_eq!(short,[0xab;6]);
  let mut out=[0xab;16];
  assert_eq!(command(&mut fs,&auth,
   Identity{generation:5,..actor()},1,
   b"patchhex /binary 1 09",&mut out),Err(Error::Denied));
  assert_eq!(command(&mut fs,&auth,actor(),1,
   b"patchhex /boot/kernel 0 09",&mut out),Err(Error::Denied));
  assert_eq!(fs.generation(),generation);
  assert_eq!(out,[0xab;16]);
 }
 #[test]fn invalid_offsets_hex_paths_and_extra_args_preserve_disk(){
  let (mut fs,auth)=setup();let before=fs.generation();
  let mut out=[0xac;16];
  for invalid in [
   b"patchhex /binary 3 09".as_slice(),
   b"patchhex /binary -1 09",
   b"patchhex /binary 0 gg",
   b"patchhex /binary 0 0",
   b"patchhex /binary 0 123",
   b"patchhex /binary 1 09 extra",
   b"patchhex /../bad 0 09",
  ]{
   assert_eq!(command(&mut fs,&auth,actor(),1,invalid,&mut out),
    Err(Error::Invalid));
  }
  assert_eq!(command(&mut fs,&auth,actor(),1,
   b"patchhex /missing 0 ff",&mut out),Err(Error::Io(FsError::NotFound)));
  assert_eq!(fs.generation(),before);
  assert_eq!(out,[0xac;16]);
 }
}


#[cfg(test)]
mod r066_durable_multibyte_patch_tests{
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
 fn actor()->Identity{Identity{pid:17,generation:6}}
 fn setup()->(HierVfs<Disk>,Access<1>){
  let mut fs=HierVfs::format_blank(Disk{sectors:[[0;SECTOR_BYTES];100]}).unwrap();
  fs.create("/binary",&[0,1,2,3,4,255]).unwrap();
  let mut auth=Access::new();
  auth.grant(actor(),FILE_READ|FILE_WRITE,100).unwrap();
  (fs,auth)
 }
 #[test]fn multi_byte_patch_commits_once_and_survives_block_remount(){
  let (mut fs,auth)=setup();let initial=fs.generation();
  let mut out=[0u8;16];
  assert_eq!(command(&mut fs,&auth,actor(),1,
   b"patchrange /binary 1 414200",&mut out),Ok(7));
  assert_eq!(&out[..7],b"patched");
  assert_eq!(fs.generation(),initial+1);
  fs=HierVfs::mount(fs.into_device()).unwrap();
  let mut bytes=[0u8;16];assert_eq!(fs.read("/binary",&mut bytes),Ok(6));
  assert_eq!(&bytes[..6],&[0,65,66,0,4,255]);
  assert_eq!(command(&mut fs,&auth,actor(),1,
   b"patchrange /binary 1 414200",&mut out),Ok(9));
  assert_eq!(&out[..9],b"unchanged");
  assert_eq!(fs.generation(),initial+1);
 }
 #[test]fn malformed_hex_and_overrun_reject_without_mutating_any_bytes(){
  let (mut fs,auth)=setup();let generation=fs.generation();
  let mut out=[0xa5;16];
  for bad in [
   b"patchrange /binary 0 0".as_slice(),
   b"patchrange /binary 0 zz",
   b"patchrange /binary 1 010",
   b"patchrange /binary 4 010203",
   b"patchrange /binary -1 01",
   b"patchrange /binary 0 0102 extra",
   b"patchrange /../bad 0 01",
  ]{
   assert_eq!(command(&mut fs,&auth,actor(),1,bad,&mut out),
    Err(Error::Invalid));
  }
  assert_eq!(command(&mut fs,&auth,actor(),1,
   b"patchrange /missing 0 0102",&mut out),Err(Error::Io(FsError::NotFound)));
  assert_eq!(fs.generation(),generation);assert_eq!(out,[0xa5;16]);
 }
 #[test]fn authorization_protected_path_and_short_output_fail_closed(){
  let (mut fs,auth)=setup();let generation=fs.generation();
  let mut short=[0xa7;4];
  assert_eq!(command(&mut fs,&auth,actor(),1,
    b"patchrange /binary 1 4142",&mut short),Err(Error::OutputTooSmall));
  assert_eq!(short,[0xa7;4]);
  let mut out=[0xa7;16];
  assert_eq!(command(&mut fs,&auth,
   Identity{generation:7,..actor()},1,
    b"patchrange /binary 1 4142",&mut out),Err(Error::Denied));
  assert_eq!(command(&mut fs,&auth,actor(),1,
   b"patchrange /system/boot 0 4142",&mut out),Err(Error::Denied));
  assert_eq!(fs.generation(),generation);assert_eq!(out,[0xa7;16]);
 }
}


#[cfg(test)]
mod r067_terminal_copylines_tests{
 use super::*;
 use crate::block::{BlockError,SECTOR_BYTES};
 struct Disk{data:[[u8;SECTOR_BYTES];100]}
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{100}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *out=*self.data.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *self.data.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*data;Ok(())
  }
 }
 fn id()->Identity{Identity{pid:4,generation:9}}
 fn mounted()->(HierVfs<Disk>,Access<1>){
  let mut fs=HierVfs::format_blank(Disk{data:[[0;SECTOR_BYTES];100]}).unwrap();
  fs.create("/source","α\nβ\nend".as_bytes()).unwrap();
  let mut access=Access::new();
  access.grant(id(),FILE_READ|FILE_WRITE,100).unwrap();
  (fs,access)
 }
 #[test]fn copy_complete_source_lines_to_real_durable_file(){
  let (mut fs,auth)=mounted();let before=fs.generation();
  let mut out=[0u8;20];
  assert_eq!(command(&mut fs,&auth,id(),1,
    b"copylines /source 2 2 /sample",&mut out),Ok(6));
  assert_eq!(&out[..6],b"copied");
  assert_eq!(fs.generation(),before+1);
  let mut fs=HierVfs::mount(fs.into_device()).unwrap();
  let mut bytes=[0;20];
  assert_eq!(fs.read("/sample",&mut bytes),Ok(6));
  assert_eq!(&bytes[..6],"β\nend".as_bytes());
  assert_eq!(fs.read("/source",&mut bytes),Ok(9));
 }
 #[test]fn invalid_rows_existing_destination_and_small_output_never_write(){
  let (mut fs,auth)=mounted();fs.create("/old",b"safe").unwrap();
  let gen=fs.generation();let mut short=[0xa5;5];
  assert_eq!(command(&mut fs,&auth,id(),1,
    b"copylines /source 1 1 /dest",&mut short),
    Err(Error::OutputTooSmall));
  assert_eq!(short,[0xa5;5]);
  let mut out=[0u8;16];
  for input in [b"copylines /source 0 1 /new".as_slice(),
      b"copylines /source 2 8 /new",
      b"copylines /source 1 1 /../bad",
      b"copylines /source 1 1 /source"]{
   assert_eq!(command(&mut fs,&auth,id(),1,input,&mut out),
     Err(Error::Invalid));
  }
  assert_eq!(command(&mut fs,&auth,id(),1,
    b"copylines /source 1 1 /old",&mut out),
    Err(Error::Io(FsError::Exists)));
  assert_eq!(fs.generation(),gen);
 }
 #[test]fn stale_identity_cannot_extract_source_to_new_file(){
  let (mut fs,auth)=mounted();let gen=fs.generation();
  let other=Identity{generation:10,..id()};
  let mut out=[0xa7;16];
  assert_eq!(command(&mut fs,&auth,other,1,
    b"copylines /source 1 1 /dest",&mut out),Err(Error::Denied));
  assert_eq!(out,[0xa7;16]);assert_eq!(fs.generation(),gen);
 }
}


#[cfg(test)]
mod r068_durable_word_count_tests{
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
 fn identity()->Identity{Identity{pid:9,generation:6}}
 fn setup()->(HierVfs<Disk>,Access<1>){
  let mut fs=HierVfs::format_blank(Disk{sectors:[[0;SECTOR_BYTES];100]}).unwrap();
  fs.create("/text",b"one two\nthree\n").unwrap();
  fs.create("/unicode","α b\n".as_bytes()).unwrap();
  fs.create("/binary",&[0xff,0xfe]).unwrap();
  let mut auth=Access::new();auth.grant(identity(),FILE_READ,100).unwrap();
  (fs,auth)
 }
 #[test]fn counts_real_durable_unicode_words_lines_bytes_across_remount(){
  let (mut fs,auth)=setup();let gen=fs.generation();let mut out=[0u8;32];
  assert_eq!(command(&mut fs,&auth,identity(),1,b"wc /text",&mut out),Ok(6));
  assert_eq!(&out[..6],b"2 3 14");
  assert_eq!(command(&mut fs,&auth,identity(),1,b"wc /unicode",&mut out),Ok(5));
  assert_eq!(&out[..5],b"1 2 5");
  fs=HierVfs::mount(fs.into_device()).unwrap();
  assert_eq!(command(&mut fs,&auth,identity(),1,b"wc /text",&mut out),Ok(6));
  assert_eq!(fs.generation(),gen);
 }
 #[test]fn invalid_utf8_missing_source_and_bad_path_never_write_output(){
  let (mut fs,auth)=setup();let gen=fs.generation();let mut out=[0xa5;32];
  assert_eq!(command(&mut fs,&auth,identity(),1,b"wc /binary",&mut out),
   Err(Error::Invalid));
  assert_eq!(command(&mut fs,&auth,identity(),1,b"wc /missing",&mut out),
   Err(Error::Io(FsError::NotFound)));
  assert_eq!(command(&mut fs,&auth,identity(),1,b"wc /../escape",&mut out),
   Err(Error::Invalid));
  assert_eq!(fs.generation(),gen);assert_eq!(out,[0xa5;32]);
 }
 #[test]fn short_output_and_foreign_generation_are_fail_closed(){
  let (mut fs,auth)=setup();let before=fs.generation();
  let mut short=[0x33;3];
  assert_eq!(command(&mut fs,&auth,identity(),1,b"wc /text",&mut short),
   Err(Error::OutputTooSmall));
  assert_eq!(short,[0x33;3]);
  let mut out=[0x33;32];
  assert_eq!(command(&mut fs,&auth,
   Identity{generation:7,..identity()},1,b"wc /text",&mut out),
   Err(Error::Denied));
  assert_eq!(out,[0x33;32]);assert_eq!(fs.generation(),before);
 }
}


#[cfg(test)]
mod r069_source_inspection_commands_tests {
 use super::*;
 use crate::block::{BlockError,SECTOR_BYTES};
 struct Disk{sectors:[[u8;SECTOR_BYTES];100]}
 impl BlockDevice for Disk {
  fn sector_count(&self)->u64{100}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *out=*self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,lba:u64,bytes:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*bytes;Ok(())
  }
 }
 fn id()->Identity{Identity{pid:3,generation:1}}
 fn setup()->(HierVfs<Disk>,Access<1>){
  let mut fs=HierVfs::format_blank(Disk{sectors:[[0;SECTOR_BYTES];100]}).unwrap();
  fs.create("/src","α one\nsecond\nα one".as_bytes()).unwrap();
  fs.create("/binary",&[0xff,0x80]).unwrap();
  let mut auth=Access::new();auth.grant(id(),FILE_READ,100).unwrap();
  (fs,auth)
 }
 #[test]fn preview_complete_rows_and_find_last_unicode_match_after_remount(){
  let (mut fs,auth)=setup();let before=fs.generation();let mut out=[0u8;64];
  let preview="α one\nsecond\n".as_bytes();
  assert_eq!(command(&mut fs,&auth,id(),1,b"headlines /src 2",&mut out),Ok(preview.len()));
  assert_eq!(&out[..preview.len()],preview);
  assert_eq!(command(&mut fs,&auth,id(),1,b"rfind /src one",&mut out),Ok(2));
  assert_eq!(&out[..2],b"17");
  fs=HierVfs::mount(fs.into_device()).unwrap();
  assert_eq!(command(&mut fs,&auth,id(),1,b"headlines /src 1",&mut out),Ok(7));
  assert_eq!(&out[..7],"α one\n".as_bytes());
  assert_eq!(fs.generation(),before);
 }
 #[test]fn invalid_source_utf8_and_small_output_have_no_side_effects(){
  let (mut fs,auth)=setup();let gen=fs.generation();let mut out=[0xa5;4];
  assert_eq!(command(&mut fs,&auth,id(),1,b"headlines /src 2",&mut out),Err(Error::OutputTooSmall));
  assert_eq!(out,[0xa5;4]);
  assert_eq!(command(&mut fs,&auth,id(),1,b"headlines /src 0",&mut out),Err(Error::Invalid));
  assert_eq!(command(&mut fs,&auth,id(),1,b"headlines /binary 1",&mut out),Err(Error::Invalid));
  assert_eq!(command(&mut fs,&auth,id(),1,b"rfind /binary a",&mut out),Err(Error::Invalid));
  assert_eq!(fs.generation(),gen);assert_eq!(out,[0xa5;4]);
 }
 #[test]fn stale_identity_cannot_read_source_or_publish_bytes(){
  let (mut fs,auth)=setup();let mut out=[0x55;32];
  let other=Identity{generation:2,..id()};
  assert_eq!(command(&mut fs,&auth,other,1,b"headlines /src 1",&mut out),Err(Error::Denied));
  assert_eq!(command(&mut fs,&auth,other,1,b"rfind /src one",&mut out),Err(Error::Denied));
  assert_eq!(out,[0x55;32]);
 }
}


#[cfg(test)]
mod r070_terminal_tail_lines_tests {
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
    fn setup() -> (HierVfs<Disk>, Access<1>) {
        let mut fs = HierVfs::format_blank(Disk { sectors: [[0; SECTOR_BYTES]; 100] }).unwrap();
        fs.create("/src", "α\nβ\nγ\n".as_bytes()).unwrap();
        fs.create("/bad", &[0xff, 0xfe]).unwrap();
        let mut auth = Access::new();
        auth.grant(id(), FILE_READ, 100).unwrap();
        (fs, auth)
    }
    #[test]
    fn complete_unicode_tail_rows_survive_real_remount() {
        let (mut fs, auth) = setup();
        let before = fs.generation();
        let mut out = [0u8; 32];
        let expected = "β\nγ\n".as_bytes();
        assert_eq!(command(&mut fs, &auth, id(), 1, b"taillines /src 2", &mut out),
            Ok(expected.len()));
        assert_eq!(&out[..expected.len()], expected);
        fs = HierVfs::mount(fs.into_device()).unwrap();
        assert_eq!(command(&mut fs, &auth, id(), 1, b"taillines /src 1", &mut out),
            Ok("γ\n".len()));
        assert_eq!(&out[..3], "γ\n".as_bytes());
        assert_eq!(fs.generation(), before);
    }
    #[test]
    fn undersized_output_invalid_utf8_and_expired_identity_are_atomic() {
        let (mut fs, auth) = setup();
        let before = fs.generation();
        let mut out = [0x52u8; 2];
        assert_eq!(command(&mut fs, &auth, id(), 1, b"taillines /src 2", &mut out),
            Err(Error::OutputTooSmall));
        assert_eq!(command(&mut fs, &auth, id(), 1, b"taillines /bad 1", &mut out),
            Err(Error::Invalid));
        assert_eq!(command(&mut fs, &auth, id(), 1, b"taillines /src 0", &mut out),
            Err(Error::Invalid));
        assert_eq!(command(&mut fs, &auth, id(), 101, b"taillines /src 2", &mut out),
            Err(Error::Denied));
        assert_eq!(out, [0x52; 2]);
        assert_eq!(fs.generation(), before);
    }
}


#[cfg(test)]
mod r071_terminal_linespan_tests {
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
    fn id() -> Identity { Identity { pid: 8, generation: 4 } }
    fn setup() -> (HierVfs<Disk>, Access<1>) {
        let mut fs = HierVfs::format_blank(Disk { sectors: [[0; SECTOR_BYTES]; 100] }).unwrap();
        fs.create("/source", "one\nα\nlast".as_bytes()).unwrap();
        fs.create("/binary", &[0xff, 0xfe]).unwrap();
        let mut auth = Access::new();
        auth.grant(id(), FILE_READ, 30).unwrap();
        (fs, auth)
    }
    #[test]
    fn exact_unicode_source_rows_are_durable_across_remount() {
        let (mut fs, auth) = setup();
        let previous = fs.generation();
        let mut out = [0u8; 32];
        let expected = "α\nlast";
        assert_eq!(command(&mut fs, &auth, id(), 1, b"linespan /source 2 2", &mut out),
            Ok(expected.len()));
        assert_eq!(&out[..expected.len()], expected.as_bytes());
        fs = HierVfs::mount(fs.into_device()).unwrap();
        assert_eq!(command(&mut fs, &auth, id(), 1, b"linespan /source 1 1", &mut out), Ok(4));
        assert_eq!(&out[..4], b"one\n");
        assert_eq!(fs.generation(), previous);
    }
    #[test]
    fn invalid_range_binary_expired_grant_and_small_out_are_atomic() {
        let (mut fs, auth) = setup();
        let previous = fs.generation();
        let mut out = [0x41u8; 4];
        assert_eq!(command(&mut fs, &auth, id(), 1,
            b"linespan /source 2 2", &mut out), Err(Error::OutputTooSmall));
        assert_eq!(command(&mut fs, &auth, id(), 1,
            b"linespan /source 3 2", &mut out), Err(Error::Invalid));
        assert_eq!(command(&mut fs, &auth, id(), 1,
            b"linespan /source 0 1", &mut out), Err(Error::Invalid));
        assert_eq!(command(&mut fs, &auth, id(), 1,
            b"linespan /binary 1 1", &mut out), Err(Error::Invalid));
        assert_eq!(command(&mut fs, &auth, id(), 31,
            b"linespan /source 1 1", &mut out), Err(Error::Denied));
        assert_eq!(out, [0x41; 4]);
        assert_eq!(fs.generation(), previous);
    }
}


#[cfg(test)]
mod r072_terminal_matching_lines_tests {
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
    fn id() -> Identity { Identity { pid: 6, generation: 4 } }
    fn setup() -> (HierVfs<Disk>, Access<1>) {
        let mut fs = HierVfs::format_blank(Disk { sectors: [[0; SECTOR_BYTES]; 100] }).unwrap();
        fs.create("/src", "α foo\nbar\nfoo α\n".as_bytes()).unwrap();
        fs.create("/bad", &[0xff, 0xfe]).unwrap();
        let mut access = Access::new();
        access.grant(id(), FILE_READ, 20).unwrap();
        (fs, access)
    }
    #[test]
    fn reports_one_based_matching_unicode_source_rows_after_remount() {
        let (mut fs, access) = setup();
        let baseline = fs.generation();
        let mut out = [0u8; 24];
        assert_eq!(command(&mut fs, &access, id(), 1,
            b"linehits /src foo", &mut out), Ok(3));
        assert_eq!(&out[..3], b"1 3");
        fs = HierVfs::mount(fs.into_device()).unwrap();
        assert_eq!(command(&mut fs, &access, id(), 1,
            b"linehits /src missing", &mut out), Ok(9));
        assert_eq!(&out[..9], b"not found");
        assert_eq!(fs.generation(), baseline);
    }
    #[test]
    fn output_short_corruption_and_expired_identity_do_not_publish() {
        let (mut fs, access) = setup();
        let baseline = fs.generation();
        let mut output = [0x61u8; 2];
        assert_eq!(command(&mut fs, &access, id(), 1,
            b"linehits /src foo", &mut output), Err(Error::OutputTooSmall));
        assert_eq!(command(&mut fs, &access, id(), 1,
            b"linehits /bad foo", &mut output), Err(Error::Invalid));
        assert_eq!(command(&mut fs, &access, id(), 21,
            b"linehits /src foo", &mut output), Err(Error::Denied));
        assert_eq!(output, [0x61; 2]);
        assert_eq!(fs.generation(), baseline);
    }
}


#[cfg(test)]
mod r073_terminal_numbered_source_tests {
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
    fn id() -> Identity { Identity { pid: 6, generation: 7 } }
    fn ready() -> (HierVfs<Disk>, Access<1>) {
        let mut fs = HierVfs::format_blank(Disk { sectors: [[0; SECTOR_BYTES]; 100] }).unwrap();
        fs.create("/source", "α\nβ\nγ".as_bytes()).unwrap();
        fs.create("/binary", &[0xff, 0xfe]).unwrap();
        let mut auth = Access::new();
        auth.grant(id(), FILE_READ, 60).unwrap();
        (fs, auth)
    }
    #[test]
    fn renders_numbered_unicode_lines_and_survives_real_reboot() {
        let (mut fs, auth) = ready();
        let generation = fs.generation();
        let mut out = [0u8; 48];
        let rows = "2| β\n3| γ";
        assert_eq!(command(&mut fs, &auth, id(), 1,
            b"sourceview /source 2 2", &mut out), Ok(rows.len()));
        assert_eq!(&out[..rows.len()], rows.as_bytes());
        fs = HierVfs::mount(fs.into_device()).unwrap();
        assert_eq!(command(&mut fs, &auth, id(), 1,
            b"sourceview /source 1 1", &mut out), Ok("1| α\n".len()));
        assert_eq!(&out[.."1| α\n".len()], "1| α\n".as_bytes());
        assert_eq!(fs.generation(), generation);
    }
    #[test]
    fn short_output_invalid_utf8_and_expired_owner_publish_nothing() {
        let (mut fs, auth) = ready();
        let generation = fs.generation();
        let mut out = [0x45u8; 3];
        assert_eq!(command(&mut fs, &auth, id(), 1,
            b"sourceview /source 2 2", &mut out), Err(Error::OutputTooSmall));
        assert_eq!(command(&mut fs, &auth, id(), 1,
            b"sourceview /source 3 2", &mut out), Err(Error::Invalid));
        assert_eq!(command(&mut fs, &auth, id(), 1,
            b"sourceview /binary 1 1", &mut out), Err(Error::Invalid));
        assert_eq!(command(&mut fs, &auth, id(), 61,
            b"sourceview /source 1 1", &mut out), Err(Error::Denied));
        assert_eq!(out, [0x45; 3]);
        assert_eq!(fs.generation(), generation);
    }
}


#[cfg(test)]
mod r074_sourcecontext_tests {
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
    fn ready() -> (HierVfs<Disk>, Access<1>) {
        let mut fs = HierVfs::format_blank(Disk { sectors: [[0; SECTOR_BYTES]; 100] }).unwrap();
        fs.create("/source", "α before\nfoo\nγ after\nother".as_bytes()).unwrap();
        fs.create("/binary", &[0xff, 0xfe]).unwrap();
        let mut auth = Access::new();
        auth.grant(id(), FILE_READ, 80).unwrap();
        (fs, auth)
    }
    #[test]
    fn contextual_numbered_unicode_source_is_durable_and_deduplicated() {
        let (mut fs, auth) = ready();
        let generation = fs.generation();
        let expected = "1| α before\n2| foo\n3| γ after\n";
        let mut out = [0u8; 80];
        assert_eq!(command(&mut fs, &auth, id(), 1,
            b"sourcecontext /source foo 1", &mut out), Ok(expected.len()));
        assert_eq!(&out[..expected.len()], expected.as_bytes());
        fs = HierVfs::mount(fs.into_device()).unwrap();
        assert_eq!(command(&mut fs, &auth, id(), 1,
            b"sourcecontext /source missing 1", &mut out), Ok(9));
        assert_eq!(&out[..9], b"not found");
        assert_eq!(fs.generation(), generation);
    }
    #[test]
    fn malformed_radius_denied_access_invalid_utf8_and_short_out_are_atomic() {
        let (mut fs, auth) = ready();
        let generation = fs.generation();
        let mut out = [0x6au8; 3];
        assert_eq!(command(&mut fs, &auth, id(), 1,
            b"sourcecontext /source foo 1", &mut out), Err(Error::OutputTooSmall));
        assert_eq!(command(&mut fs, &auth, id(), 1,
            b"sourcecontext /source foo 4", &mut out), Err(Error::Invalid));
        assert_eq!(command(&mut fs, &auth, id(), 1,
            b"sourcecontext /binary foo 1", &mut out), Err(Error::Invalid));
        assert_eq!(command(&mut fs, &auth, id(), 81,
            b"sourcecontext /source foo 1", &mut out), Err(Error::Denied));
        assert_eq!(out, [0x6a; 3]);
        assert_eq!(fs.generation(), generation);
    }
}


#[cfg(test)]
mod r075_utf8_text_profile_tests {
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
    fn id() -> Identity { Identity { pid: 8, generation: 4 } }
    fn ready() -> (HierVfs<Disk>, Access<1>) {
        let mut fs = HierVfs::format_blank(Disk { sectors: [[0; SECTOR_BYTES]; 100] }).unwrap();
        fs.create("/src", "α🙂\nβ\n".as_bytes()).unwrap();
        fs.create("/empty", b"").unwrap();
        fs.create("/binary", &[0xff, 0xfe]).unwrap();
        let mut grants = Access::new();
        grants.grant(id(), FILE_READ, 30).unwrap();
        (fs, grants)
    }
    #[test]
    fn unicode_scalar_profile_survives_real_filesystem_remount() {
        let (mut fs, grants) = ready();
        let before = fs.generation();
        let expected = b"bytes:10 chars:5 rows:2 maxscalars:2";
        let mut out = [0u8; 80];
        assert_eq!(command(&mut fs, &grants, id(), 1, b"textprofile /src", &mut out),
            Ok(expected.len()));
        assert_eq!(&out[..expected.len()], expected);
        fs = HierVfs::mount(fs.into_device()).unwrap();
        let empty = b"bytes:0 chars:0 rows:0 maxscalars:0";
        assert_eq!(command(&mut fs, &grants, id(), 1, b"textprofile /empty", &mut out),
            Ok(empty.len()));
        assert_eq!(&out[..empty.len()], empty);
        assert_eq!(fs.generation(), before);
    }
    #[test]
    fn invalid_utf8_short_output_and_expired_identity_are_fail_closed() {
        let (mut fs, grants) = ready();
        let before = fs.generation();
        let mut out = [0x44u8; 3];
        assert_eq!(command(&mut fs, &grants, id(), 1, b"textprofile /src", &mut out),
            Err(Error::OutputTooSmall));
        assert_eq!(command(&mut fs, &grants, id(), 1, b"textprofile /binary", &mut out),
            Err(Error::Invalid));
        assert_eq!(command(&mut fs, &grants, id(), 31, b"textprofile /src", &mut out),
            Err(Error::Denied));
        assert_eq!(out, [0x44; 3]);
        assert_eq!(fs.generation(), before);
    }
}


#[cfg(test)]
mod r075_expanded_source_diagnostics_tests {
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
    fn id() -> Identity { Identity { pid: 7, generation: 4 } }
    fn setup() -> (HierVfs<Disk>, Access<1>) {
        let mut fs = HierVfs::format_blank(Disk { sectors: [[0; SECTOR_BYTES]; 100] }).unwrap();
        fs.create("/src", " \tα \n\nβ\r\nlast\r".as_bytes()).unwrap();
        fs.create("/bad", &[0xff, 0xfe]).unwrap();
        let mut auth = Access::new();
        auth.grant(id(), FILE_READ, 20).unwrap();
        (fs, auth)
    }
    #[test]
    fn three_source_analysis_commands_use_real_remountable_utf8_files() {
        let (mut fs, auth) = setup();
        let generation = fs.generation();
        let mut out = [0u8; 96];
        for (command_line, expected) in [
            (b"blankrows /src".as_slice(), b"2".as_slice()),
            (b"indentmap /src".as_slice(), b"1:1:1 2:0:0 3:0:0 4:0:0".as_slice()),
            (b"eolcheck /src".as_slice(), b"lf:3 crlf:1 barecr:1 final:0".as_slice())
        ]{
            assert_eq!(command(&mut fs, &auth, id(), 1, command_line, &mut out),
                Ok(expected.len()));
            assert_eq!(&out[..expected.len()], expected);
        }
        fs = HierVfs::mount(fs.into_device()).unwrap();
        assert_eq!(command(&mut fs, &auth, id(), 1, b"blankrows /src", &mut out), Ok(1));
        assert_eq!(fs.generation(), generation);
    }
    #[test]
    fn short_output_invalid_utf8_and_expired_grant_do_not_publish_bytes() {
        let (mut fs, auth) = setup();
        let previous = fs.generation();
        let mut out = [0x5cu8; 2];
        assert_eq!(command(&mut fs, &auth, id(), 1, b"indentmap /src", &mut out),
            Err(Error::OutputTooSmall));
        assert_eq!(command(&mut fs, &auth, id(), 1, b"eolcheck /bad", &mut out),
            Err(Error::Invalid));
        assert_eq!(command(&mut fs, &auth, id(), 21, b"blankrows /src", &mut out),
            Err(Error::Denied));
        assert_eq!(out, [0x5c; 2]);
        assert_eq!(fs.generation(), previous);
    }
}


#[cfg(test)]
mod r075_paged_tool_catalog_tests {
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
    #[test]
    fn tool_discovery_remains_bounded_for_existing_desktop_output() {
        let mut fs = HierVfs::format_blank(Disk {
            sectors: [[0; SECTOR_BYTES]; 100]
        }).unwrap();
        let auth = Access::<1>::new();
        let actor = Identity { pid: 1, generation: 1 };
        let mut output = [0u8; 256];
        let n = command(&mut fs, &auth, actor, 1, b"tools", &mut output).unwrap();
        assert!(n <= 256);
        assert!(core::str::from_utf8(&output[..n]).unwrap().contains("tools more"));
        let n = command(&mut fs, &auth, actor, 1, b"tools more", &mut output).unwrap();
        assert_eq!(&output[..n], b"blankrows indentmap eolcheck");
    }
}

#[cfg(test)]
mod r076_terminal_page_tests{
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
 fn id()->Identity{Identity{pid:28,generation:2}}
 #[test]fn paged_output_checks_grants_capacity_and_survives_remount(){
  let mut fs=HierVfs::format_blank(Disk{sectors:[[0;SECTOR_BYTES];100]}).unwrap();
  fs.create("/note",b"abcdefghij").unwrap();
  let gen=fs.generation();
  let mut auth=Access::<1>::new();auth.grant(id(),FILE_READ,10).unwrap();
  let mut out=[0xadu8;5];
  assert_eq!(command(&mut fs,&auth,id(),1,b"page /note 3 4",&mut out),Ok(4));
  assert_eq!(&out[..4],b"defg");
  out.fill(0xad);
  assert_eq!(command(&mut fs,&auth,id(),1,b"page /note 3 6",&mut out),
    Err(Error::OutputTooSmall));
  assert_eq!(out,[0xad;5]);
  assert_eq!(command(&mut fs,&auth,id(),11,b"page /note 0 3",&mut out),
    Err(Error::Denied));
  assert_eq!(command(&mut fs,&auth,id(),1,b"page /note 0 0",&mut out),
    Err(Error::Invalid));
  assert_eq!(fs.generation(),gen);
  fs=HierVfs::mount(fs.into_device()).unwrap();
  assert_eq!(command(&mut fs,&auth,id(),1,b"page /note 8 2",&mut out),Ok(2));
  assert_eq!(&out[..2],b"ij");
 }
}


#[cfg(test)]
mod r077_terminal_range_and_search_tests {
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
    fn id() -> Identity { Identity { pid: 32, generation: 5 } }
    fn ready() -> (HierVfs<Disk>, Access<2>) {
        let mut fs = HierVfs::format_blank(Disk { sectors: [[0; SECTOR_BYTES]; 100] }).unwrap();
        fs.create("/src", "α foo foo\nfoo".as_bytes()).unwrap();
        let mut access = Access::new();
        access.grant(id(), FILE_READ | FILE_WRITE, 10).unwrap();
        (fs, access)
    }
    #[test]
    fn paged_occurrences_and_range_clone_survive_real_remount() {
        let (mut fs, access) = ready();
        let mut out = [0u8; 24];
        let n = command(&mut fs, &access, id(), 1,
            b"matchpage /src foo 1 2", &mut out).unwrap();
        assert_eq!(&out[..n], b"7\n11\n");
        assert_eq!(command(&mut fs, &access, id(), 1,
            b"copyrange /src /excerpt 3 7", &mut out), Ok(1));
        assert_eq!(out[0], b'7');
        let mut mounted = HierVfs::mount(fs.into_device()).unwrap();
        let mut data = [0u8; 16];
        assert_eq!(mounted.read("/excerpt", &mut data), Ok(7));
        assert_eq!(&data[..7], b"foo foo");
    }
    #[test]
    fn protected_targets_short_output_and_expired_grants_are_rejected() {
        let (mut fs, access) = ready();
        let generation = fs.generation();
        let mut out = [0xabu8; 2];
        assert_eq!(command(&mut fs, &access, id(), 1,
            b"matchpage /src foo 0 3", &mut out), Err(Error::OutputTooSmall));
        assert_eq!(out, [0xab; 2]);
        assert_eq!(command(&mut fs, &access, id(), 1,
            b"copyrange /src /system/protected 0 3", &mut out), Err(Error::Denied));
        assert_eq!(command(&mut fs, &access, id(), 11,
            b"copyrange /src /new 0 3", &mut out), Err(Error::Denied));
        assert_eq!(command(&mut fs, &access, id(), 1,
            b"copyrange /src /new 0 999", &mut out), Err(Error::Invalid));
        assert_eq!(fs.generation(), generation);
    }
}


#[cfg(test)]
mod r078_terminal_source_and_cas_tests {
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
    fn owner()->Identity{Identity{pid:44,generation:2}}
    fn ready()->(HierVfs<Disk>,Access<2>){
        let mut fs=HierVfs::format_blank(Disk{sectors:[[0;SECTOR_BYTES];100]}).unwrap();
        fs.create("/src","α foo foo_bar\nfoo\n".as_bytes()).unwrap();
        let mut rights=Access::new();
        rights.grant(owner(),FILE_READ|FILE_WRITE,10).unwrap();
        (fs,rights)
    }
    #[test]
    fn real_unicode_textpage_and_identifier_page_preserve_file_bytes(){
        let (mut fs,rights)=ready();
        let before=fs.generation();
        let mut out=[0u8;32];
        assert_eq!(command(&mut fs,&rights,owner(),1,b"textpage /src 3 3",&mut out),Ok(3));
        assert_eq!(&out[..3],b"foo");
        assert_eq!(command(&mut fs,&rights,owner(),1,b"idpage /src foo 0 2",&mut out),Ok(5));
        assert_eq!(&out[..5],b"3\n15\n");
        assert_eq!(command(&mut fs,&rights,owner(),1,b"textpage /src 1 2",&mut out),
            Err(Error::Invalid));
        let mut short=[0xaau8;1];
        assert_eq!(command(&mut fs,&rights,owner(),1,b"idpage /src foo 0 2",&mut short),
            Err(Error::OutputTooSmall));
        assert_eq!(short,[0xaa]);
        assert_eq!(fs.generation(),before);
    }
    #[test]
    fn conditional_write_append_and_reboot_are_durable(){
        let (mut fs,rights)=ready();
        let mut out=[0u8;24];
        let original="α foo foo_bar\nfoo\n";
        let digest=original.as_bytes().iter().fold(0xcbf29ce484222325u64,
            |h,b|(h^(*b as u64)).wrapping_mul(0x100000001b3));
        let update=format!("caswrite /src {:016x} changed",digest);
        assert_eq!(command(&mut fs,&rights,owner(),1,update.as_bytes(),&mut out),Ok(1));
        assert_eq!(out[0],b'7');
        assert_eq!(command(&mut fs,&rights,owner(),1,b"casappend /src 7 !",&mut out),Ok(1));
        assert_eq!(out[0],b'8');
        let mut fs=HierVfs::mount(fs.into_device()).unwrap();
        let mut bytes=[0u8;24];
        assert_eq!(fs.read("/src",&mut bytes),Ok(8));
        assert_eq!(&bytes[..8],b"changed!");
    }
    #[test]
    fn stale_hash_length_reserved_destination_and_expired_grants_are_denied(){
        let (mut fs,rights)=ready();
        let generation=fs.generation();
        let mut out=[0x5eu8;3];
        assert_eq!(command(&mut fs,&rights,owner(),1,
            b"caswrite /src 0000000000000000 wrong",&mut out),Err(Error::Denied));
        assert_eq!(command(&mut fs,&rights,owner(),1,
            b"casappend /src 999 wrong",&mut out),Err(Error::Denied));
        assert_eq!(command(&mut fs,&rights,owner(),1,
            b"caswrite /system/secret 0000000000000000 wrong",&mut out),Err(Error::Denied));
        assert_eq!(command(&mut fs,&rights,owner(),11,
            b"textpage /src 0 3",&mut out),Err(Error::Denied));
        assert_eq!(out,[0x5e;3]);
        assert_eq!(fs.generation(),generation);
    }
}


#[cfg(test)]
mod r079_terminal_revision_tests{
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
    fn user()->Identity{Identity{pid:62,generation:1}}
    fn ready()->(HierVfs<Disk>,Access<2>){
        let mut fs=HierVfs::format_blank(Disk{sectors:[[0;SECTOR_BYTES];100]}).unwrap();
        fs.create("/src","α foo foo_bar\nfoo\n".as_bytes()).unwrap();
        let mut access=Access::new();
        access.grant(user(),FILE_READ|FILE_WRITE,10).unwrap();
        (fs,access)
    }
    #[test]
    fn word_offsets_idcount_and_conditional_clone_survive_remount(){
        let (mut fs,auth)=ready();
        let original="α foo foo_bar\nfoo\n";
        let mut out=[0u8;24];
        assert_eq!(command(&mut fs,&auth,user(),1,b"wordpage /src 1 2",&mut out),Ok(4));
        assert_eq!(&out[..4],b"3\n7\n");
        assert_eq!(command(&mut fs,&auth,user(),1,b"idcount /src foo",&mut out),Ok(1));
        assert_eq!(out[0],b'2');
        let digest=original.as_bytes().iter().fold(0xcbf29ce484222325u64,
          |h,b|(h^(*b as u64)).wrapping_mul(0x100000001b3));
        let clone=format!("casclone /src /clone {:016x}",digest);
        assert_eq!(command(&mut fs,&auth,user(),1,clone.as_bytes(),&mut out),Ok(2));
        assert_eq!(&out[..2],b"19");
        assert_eq!(command(&mut fs,&auth,user(),1,b"castruncate /clone 19 4",&mut out),Ok(1));
        assert_eq!(out[0],b'4');
        let mut fs=HierVfs::mount(fs.into_device()).unwrap();
        let mut bytes=[0u8;32];
        assert_eq!(fs.read("/clone",&mut bytes),Ok(4));
        assert_eq!(&bytes[..4],"α f".as_bytes());
        assert_eq!(fs.read("/src",&mut bytes),Ok(19));
        assert_eq!(&bytes[..19],original.as_bytes());
    }
    #[test]
    fn stale_length_hash_short_output_and_authority_do_not_mutate(){
        let (mut fs,auth)=ready();let before=fs.generation();
        let mut out=[0xabu8;1];
        assert_eq!(command(&mut fs,&auth,user(),1,b"castruncate /src 3 0",&mut out),
            Err(Error::Denied));
        assert_eq!(command(&mut fs,&auth,user(),1,
            b"casclone /src /no 0000000000000000",&mut out),Err(Error::Denied));
        assert_eq!(command(&mut fs,&auth,user(),11,b"wordpage /src 0 1",&mut out),
            Err(Error::Denied));
        assert_eq!(command(&mut fs,&auth,user(),1,b"wordpage /src 1 2",&mut out),
            Err(Error::OutputTooSmall));
        assert_eq!(fs.generation(),before);
        assert_eq!(out,[0xab]);
    }
}

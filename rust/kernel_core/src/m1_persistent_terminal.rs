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
 "help","pwd","tools","ls","cat","write","append","mkdir","mv",
 "cp","rm","mkfile","truncate","head","count","rmdir","mvdir",
 "pread","pwrite","lines","hash","stat","tail","find","grep","cmp",
 "touch","copynew","replace","appendln","health",
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
 if line=="tools"{return publish(b"count lines tail find grep hash cmp stat touch copynew replace appendln health",out);}
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
    "replace"|"find"|"grep"){
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
 /// A GPT identity, I/O or metadata mount failure permanently retires the
 /// selection, preventing a later media swap from reviving it.
 pub fn execute<D:BlockDevice,const N:usize>(
  &mut self,disk:&mut D,auth:&Access<N>,caller:Identity,now:u64,
  input:&[u8],out:&mut[u8],
 )->Result<usize,PartitionCommandError>{
  self.authorize(auth,caller,now)?;
  let result=command_on_gpt_selection(disk,self.selection,auth,caller,now,input,out);
  if matches!(result,Err(PartitionCommandError::Mount(_))){self.active=false;}
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
  assert_eq!(COMMAND_VERBS.len(),31);
  for (i,verb) in COMMAND_VERBS.iter().enumerate(){
   assert!(verb.as_bytes().iter().all(|b|b.is_ascii_lowercase()));
   assert!(!COMMAND_VERBS[..i].contains(verb));
  }
  assert_eq!(complete_verb(b"mkf"),Some("mkfile"));
  assert_eq!(complete_verb(b"copyne"),Some("copynew"));
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

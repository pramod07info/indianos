//! Deterministic IndianOS host-side build tools, written in Rust.
//! No network, subprocesses, privileged operations, or external crates.

use std::{collections::BTreeSet, env, fs, path::Path};

mod smoke;
mod kernel_audit;

const MAX_FILES:usize=16;
const MAX_NAME:usize=128;
const MAX_ARCHIVE:usize=32*1024*1024;
const MAGIC:&[u8;8]=b"INDRAM01";
const GREETING:&[u8]=b"Namaste from IndianOS user mode!";

fn valid_name(path:&str)->Result<&[u8],String>{
    let raw=path.as_bytes();
    if !(2..=MAX_NAME).contains(&raw.len()) || raw[0]!=b'/' ||
       !raw.is_ascii() {
        return Err("invalid archive absolute path".into())
    }
    for part in raw[1..].split(|x| *x==b'/') {
        if part.is_empty() || part==b"." || part==b".." ||
           part.iter().any(|&c|
                !(c.is_ascii_alphanumeric() || matches!(c,b'_'|b'-'|b'.'))) {
            return Err("unsafe archive path".into())
        }
    }
    Ok(raw)
}
fn pack(entries:&[(&str,&[u8])])->Result<Vec<u8>,String>{
    if !(1..=MAX_FILES).contains(&entries.len()){
        return Err("invalid initramfs file count".into())
    }
    let mut sorted=entries.to_vec();
    sorted.sort_by(|a,b| a.0.cmp(b.0));
    let mut seen=BTreeSet::new();
    let mut out=Vec::with_capacity(16);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for (path,content) in sorted {
        let name=valid_name(path)?;
        if !seen.insert(name.to_vec()){return Err("duplicate filename".into())}
        if content.len()>u32::MAX as usize{return Err("archive entry too large".into())}
        let next=out.len().checked_add(8).and_then(|n|n.checked_add(name.len()))
            .and_then(|n|n.checked_add(content.len()))
            .ok_or("archive size overflow")?;
        if next>MAX_ARCHIVE{return Err("archive exceeds maximum size".into())}
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(content.len() as u32).to_le_bytes());
        out.extend_from_slice(name);
        out.extend_from_slice(content);
    }
    let total=out.len() as u32;
    out[12..16].copy_from_slice(&total.to_le_bytes());
    Ok(out)
}
fn number(bytes:&[u8],off:usize,size:usize)->Result<u64,String>{
    let raw=bytes.get(off..off.checked_add(size).ok_or("integer overflow")?)
        .ok_or("truncated ELF")?;
    let mut value=0u64;
    for (i,&b) in raw.iter().enumerate(){value|=(b as u64)<<(i*8)}
    Ok(value)
}
fn check_elf(bytes:&[u8])->Result<(),String>{
    if bytes.len()<64 || bytes.len()>MAX_ARCHIVE ||
       bytes.get(..4)!=Some(&b"\x7fELF"[..]) ||
       bytes.get(4..7)!=Some(&[2,1,1][..]) ||
       number(bytes,16,2)?!=2 || number(bytes,18,2)?!=62 ||
       number(bytes,52,2)?!=64 {
        return Err("test program must be an ELF64 x86-64 ET_EXEC".into())
    }
    let entry=number(bytes,24,8)?;
    let phoff=number(bytes,32,8)? as usize;
    let entsize=number(bytes,54,2)? as usize;
    let count=number(bytes,56,2)? as usize;
    if entsize!=56 || !(1..=32).contains(&count) ||
       phoff>bytes.len() || count*entsize>bytes.len()-phoff {
        return Err("malformed program header table".into())
    }
    let mut executable_entry=false;
    let mut interrupts=0;
    let mut message_found=false;
    for i in 0..count{
        let at=phoff+i*entsize;
        let kind=number(bytes,at,4)?;
        if kind!=1{continue}
        let flags=number(bytes,at+4,4)?;
        let offset=number(bytes,at+8,8)?;
        let va=number(bytes,at+16,8)?;
        let file_size=number(bytes,at+32,8)?;
        let mem_size=number(bytes,at+40,8)?;
        let align=number(bytes,at+48,8)?;
        if flags&4==0 || flags&3==3 || file_size>mem_size ||
           offset>bytes.len() as u64 || file_size>bytes.len() as u64-offset ||
           offset%4096!=va%4096 ||
           (align>1 && !align.is_power_of_two()){
            return Err("invalid or writable-and-executable load segment".into())
        }
        let region=&bytes[offset as usize..(offset+file_size) as usize];
        message_found |= region.windows(GREETING.len()).any(|x|x==GREETING);
        if flags&1!=0 {
            executable_entry |= va<=entry && entry-va<file_size;
            interrupts+=region.windows(2).filter(|x|*x==b"\xcd\x80").count();
        }
    }
    if !executable_entry{return Err("entry is outside initialized executable segment".into())}
    if interrupts<2{return Err("test ELF lacks expected write/exit INT 0x80 instructions".into())}
    if !message_found{return Err("test ELF lacks expected greeting in a load segment".into())}
    Ok(())
}
fn option(args:&[String],name:&str)->Option<String>{
    args.windows(2).find(|pair|pair[0]==name).map(|pair|pair[1].clone())
}
fn command(args:&[String])->Result<(),String>{
    let Some(cmd)=args.first().map(String::as_str) else{
        return Err("usage: indianos-tools pack|check-elf|smoke|audit-kernel ...".into())
    };
    match cmd{
        "pack"=>{
            let elf=option(args,"--elf").ok_or("missing --elf")?;
            let output=option(args,"--output").ok_or("missing --output")?;
            let name=option(args,"--name").unwrap_or_else(||"/bin/init".into());
            let data=fs::read(elf).map_err(|e|e.to_string())?;
            let extra=option(args,"--extra-elf").map(|path|
                fs::read(path).map_err(|e|e.to_string())).transpose()?;
            let third=option(args,"--extra2-elf").map(|p|
                fs::read(p).map_err(|e|e.to_string())).transpose()?;
            let n2=option(args,"--extra-name").unwrap_or_else(||"/bin/worker".into());
            let n3=option(args,"--extra2-name").unwrap_or_else(||"/bin/terminal".into());
            let archive=match(extra.as_ref(),third.as_ref()){
                (Some(a),Some(b))=>pack(&[(&name,&data),(&n2,a),(&n3,b)])?,
                (Some(a),None)=>pack(&[(&name,&data),(&n2,a)])?,
                (None,None)=>pack(&[(&name,&data)])?,
                (None,Some(_))=>return Err("third ELF requires second ELF".into())
            };
            let dest=Path::new(&output);
            if let Some(dir)=dest.parent(){fs::create_dir_all(dir).map_err(|e|e.to_string())?}
            fs::write(dest,&archive).map_err(|e|e.to_string())?;
            println!("IndianOS read-only initramfs: {} bytes",archive.len());
            Ok(())
        }
        "smoke"=>{
            let log=smoke::run(&args[1..])?;
            println!("{log}");
            println!("IndianOS QEMU boot smoke test PASSED");
            Ok(())
        }
        "audit-kernel"=>{
            println!("{}",kernel_audit::run(&args[1..])?);
            Ok(())
        }
        "check-elf"=>{
            if args.len()!=2{return Err("usage: indianos-tools check-elf ELF".into())}
            let bytes=fs::read(&args[1]).map_err(|e|e.to_string())?;
            check_elf(&bytes)?;
            println!("Experimental ring-3 demo ELF layout and syscall markers verified (not executed).");
            Ok(())
        }
        _=>Err("unknown tool command".into())
    }
}
fn main(){
    if let Err(e)=command(&env::args().skip(1).collect::<Vec<_>>()){
        eprintln!("IndianOS Rust tools: {e}");
        std::process::exit(1)
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn archive_sorting_and_binary_parity(){
        let a=pack(&[("/bin/z",b"Z"),("/bin/a",b"A")]).unwrap();
        let b=pack(&[("/bin/a",b"A"),("/bin/z",b"Z")]).unwrap();
        assert_eq!(a,b);
        assert_eq!(&a[..8],MAGIC);
        assert_eq!(u32::from_le_bytes(a[12..16].try_into().unwrap()) as usize,a.len());
    }
    #[test]
    fn archive_rejects_duplicates_and_traversal(){
        assert!(pack(&[]).is_err());
        assert!(pack(&[("/bin/a",b"A"),("/bin/a",b"B")]).is_err());
        for path in ["bin/a","/../a","/a/./b","/a//b","/a/","/a\\b","/a b","/"] {
            assert!(pack(&[(path,b"")]).is_err(),"invalid: {}",path);
        }
    }
    #[test]
    fn elf_rejects_empty_files(){
        assert!(check_elf(&[]).is_err());
        assert!(check_elf(&[0u8;64]).is_err());
    }
}

//! R010: audit actual ld.lld map input objects for the freestanding kernel.
//! No network, parser crates, compiler invocation or shell dependencies.
use std::{collections::BTreeSet, fs::{self, File}, io::Read};

#[derive(Debug,Default)]
struct Provenance {
     rust_members: BTreeSet<String>,
}

// Each ld.lld input record has the object in column 5, e.g.
// 200100 200100 30 16 build/obj/user_entry.o:(.text)
// 200100 200100 30 16 build/libindianos_kernel_core.a(kernel.rcgu.o):(.text)
fn verify_map(source:&str, archive:&str)->Result<Provenance,String>{
    let prefix=format!("{archive}(");
    let mut found=Provenance::default();
    for (index,line) in source.lines().enumerate(){
        let Some(input)=line.split_whitespace().nth(4) else {continue};
        let Some(end)=input.find(":(") else {continue};
        let path=&input[..end];
        if let Some(member)=path.strip_prefix(&prefix){
            if !member.ends_with(".o)") || member.len()<4 {
                return Err(format!("line {}: invalid Rust member {path}",index+1));
            }
            found.rust_members.insert(member.to_string());
        }else if path.ends_with(".o"){
            return Err(format!("line {}: forbidden standalone object {path}",index+1));
        }else if path.contains(".a(")||path.ends_with(".a")||path.ends_with(".rlib"){
            return Err(format!("line {}: unexpected linked archive {path}",index+1))
        }
    }
     if found.rust_members.is_empty(){return Err("Rust staticlib has no linked member".into())}
    Ok(found)
}
fn header(path:&str, n:usize)->Result<Vec<u8>,String>{
    let mut f=File::open(path).map_err(|e|format!("{path}: {e}"))?;
    let mut bytes=vec![0u8;n];
    f.read_exact(&mut bytes).map_err(|e|format!("{path}: {e}"))?;
    Ok(bytes)
}
fn elf_type(path:&str, expected:u16)->Result<(),String>{
    let h=header(path,64)?;
    if h.get(..7)!=Some(&b"\x7fELF\x02\x01\x01"[..]) ||
       u16::from_le_bytes([h[16],h[17]])!=expected ||
       u16::from_le_bytes([h[18],h[19]])!=62 {
       return Err(format!("{path}: expected x86-64 ELF type {expected}"));
    }
    Ok(())
}
// MAP ELF RUST_ARCHIVE. No standalone object may enter the default kernel.
pub fn run(args:&[String])->Result<String,String>{
    if args.len()!=3 {return Err(
        "usage: indianos-tools audit-kernel MAP ELF RUST_ARCHIVE".into())}
    let (map,elf,archive)=(&args[0],&args[1],&args[2]);
    elf_type(elf,2)?;
    if header(archive,8)?.as_slice()!=b"!<arch>\n" {
        return Err(format!("{archive}: invalid static archive signature"));
    }
    let map_contents=fs::read_to_string(map).map_err(|e|format!("{map}: {e}"))?;
    let p=verify_map(&map_contents,archive)?;
    Ok(format!("R011 PASS: Rust archive ({} members); no standalone C/ASM objects",
               p.rust_members.len()))
}
#[cfg(test)]
mod tests{
    use super::*;
    const LIB:&str="build/libindianos_kernel_core.a";
    const A:&str="build/obj/interrupt_entry.o";
    const B:&str="build/obj/user_entry.o";
    fn map(extra:&str)->String{
        format!("VMA LMA Size Align Out In Symbol\n\
                 0 0 20 16 {LIB}(kernel.rcgu.o):(.text)\n{extra}\n")
    }
    #[test]fn accepts_rust_archive_as_only_link_input(){
        let p=verify_map(&map("0 0 4 1 <internal>:(.symtab)"),LIB).unwrap();
        assert_eq!(p.rust_members.len(),1);
    }
    #[test]fn rejects_injected_assembly_c_objects_and_foreign_archives(){
        assert!(verify_map(&map(&format!("0 0 12 4 {A}:(.text)")),LIB).is_err());
        assert!(verify_map(&map(&format!("0 0 12 4 {B}:(.text)")),LIB).is_err());
        assert!(verify_map(&map("0 0 12 4 build/obj/main.o:(.text)"),LIB).is_err());
        assert!(verify_map(&map("0 0 12 4 build/obj/user_mode.o:(.text)"),LIB).is_err());
        assert!(verify_map(&map("0 0 12 4 build/libc.a(memcpy.o):(.text)"),LIB).is_err());
        assert!(verify_map(&map(""),"build/other.a").is_err());
    }
    #[test]fn rejects_missing_rust_members_and_disguised_archive(){
        assert!(verify_map("VMA LMA Size Align Out In Symbol",LIB).is_err());
        assert!(verify_map(&map("0 0 8 8 build/obj/interrupt_entry.o:(.text)"),LIB).is_err());
    }
}
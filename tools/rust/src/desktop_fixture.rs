//! Explicit host-only provisioning of disposable QEMU GPT storage.
//! Uses the kernel's actual COW/VFS implementation, never a second encoder.
use indianos_kernel_core::{block::{BlockDevice,BlockError,BlockWindow,SECTOR_BYTES},
    durablefs::DurableFs,hier_vfs::HierVfs,
    m1_partition::{inspect_partition_receipt,mount_gpt_selected_healthy}};
use std::{fs::OpenOptions,io::Write,path::Path};

const SECTORS:usize=256;
const START:u64=40;
const COUNT:u64=100;
const DISK_GUID:[u8;16]=[0x49,0x4e,0x44,0x49,0x41,0x4e,0x4f,0x53,1,0,0,0,0,0,0,1];
const PARTITION_GUID:[u8;16]=[0x49,0x4e,0x44,0x49,0x41,0x4e,0x4f,0x53,2,0,0,0,0,0,0,1];
const NOTE:&[u8]=b"HELLO INDIANOS";

struct Image{sectors:Vec<[u8;SECTOR_BYTES]>}
impl BlockDevice for Image{
    fn sector_count(&self)->u64{self.sectors.len() as u64}
    fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
        *out=*self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
    }
    fn write_sector(&mut self,lba:u64,bytes:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
        *self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*bytes;Ok(())
    }
    // The fixture serializes this complete construction to a new host file
    // only after all production consistency checks pass.
    fn sync(&mut self)->Result<(),BlockError>{Ok(())}
}
fn crc(bytes:&[u8])->u32{
    let mut value=!0u32;
    for byte in bytes{
        value^=*byte as u32;
        for _ in 0..8{value=(value>>1)^if value&1!=0{0xedb8_8320}else{0};}
    }
    !value
}
fn header(current:u64,alternate:u64,table:u64,checksum:u32)->[u8;SECTOR_BYTES]{
    let mut h=[0u8;SECTOR_BYTES];h[..8].copy_from_slice(b"EFI PART");
    h[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes());
    h[12..16].copy_from_slice(&92u32.to_le_bytes());
    h[24..32].copy_from_slice(&current.to_le_bytes());
    h[32..40].copy_from_slice(&alternate.to_le_bytes());
    h[40..48].copy_from_slice(&34u64.to_le_bytes());
    h[48..56].copy_from_slice(&222u64.to_le_bytes());
    h[56..72].copy_from_slice(&DISK_GUID);
    h[72..80].copy_from_slice(&table.to_le_bytes());
    h[80..84].copy_from_slice(&4u32.to_le_bytes());
    h[84..88].copy_from_slice(&128u32.to_le_bytes());
    h[88..92].copy_from_slice(&checksum.to_le_bytes());
    let sum=crc(&h[..92]);h[16..20].copy_from_slice(&sum.to_le_bytes());h
}
fn image()->Result<Image,String>{
    let mut disk=Image{sectors:vec![[0u8;SECTOR_BYTES];SECTORS]};
    let mbr=&mut disk.sectors[0];mbr[450]=0xee;
    mbr[454..458].copy_from_slice(&1u32.to_le_bytes());
    mbr[458..462].copy_from_slice(&255u32.to_le_bytes());mbr[510]=0x55;mbr[511]=0xaa;
    let mut table=[0u8;SECTOR_BYTES];table[..16].copy_from_slice(b"IndianOS-COW-VFS");
    table[16..32].copy_from_slice(&PARTITION_GUID);
    table[32..40].copy_from_slice(&START.to_le_bytes());
    table[40..48].copy_from_slice(&(START+COUNT-1).to_le_bytes());
    disk.sectors[2]=table;disk.sectors[254]=table;
    disk.sectors[1]=header(1,255,2,crc(&table));
    disk.sectors[255]=header(255,1,254,crc(&table));
    let receipt=inspect_partition_receipt(&mut disk,0).map_err(|e|format!("GPT: {e:?}"))?;
    {
        let window=BlockWindow::new(&mut disk,START,COUNT).map_err(|e|format!("window: {e:?}"))?;
        let mut fs=HierVfs::format_blank(window).map_err(|e|format!("format: {e:?}"))?;
        fs.mkdir("/shared").map_err(|e|format!("mkdir: {e:?}"))?;
        fs.create("/shared/readme",NOTE).map_err(|e|format!("create: {e:?}"))?;
        fs.sync().map_err(|e|format!("sync: {e:?}"))?;
    }
    {
        let mut fs=mount_gpt_selected_healthy(&mut disk,receipt.gpt.ok_or("missing GPT identity")?)
            .map_err(|e|format!("remount: {e:?}"))?;
        let mut note=[0u8;32];let n=fs.read("/shared/readme",&mut note)
            .map_err(|e|format!("readback: {e:?}"))?;
        if &note[..n]!=NOTE{return Err("readback mismatch".into());}
    }
    Ok(disk)
}
fn provision(path:&Path)->Result<(),String>{
    write_image(path,image()?)
}
fn write_image(path:&Path,disk:Image)->Result<(),String>{
    if path.extension().and_then(|s|s.to_str())!=Some("raw"){
        return Err("fixture output must be a new .raw file".into());
    }
    // create_new refuses existing regular files, symlinks and block devices.
    let mut file=OpenOptions::new().write(true).create_new(true).open(path)
        .map_err(|e|format!("refusing existing/unwritable fixture destination: {e}"))?;
    for sector in &disk.sectors{file.write_all(sector).map_err(|e|e.to_string())?;}
    file.sync_all().map_err(|e|e.to_string())?;
    println!("Created disposable GPT fixture: 256 sectors, entry=0 start=40 count=100, /shared/readme");
    Ok(())
}
// Deliberately malformed namespace with valid GPT and COW checksums. This is
// host-only disposable fault media, never a guest provisioning/repair path.
fn namespace_fault(kind:&str)->Result<Image,String>{
    let (path,payload):(&str,&[u8])=match kind{
        "orphan"=>("/~fmissing~leaf",b"orphan"),
        "alias"=>("/~dshared~readme",b"INDDIR31"),
        "marker"=>("/~dbad",b"BAD DIR!"),
        "link"=>("/~lshared~link",b"../readme"),
        _=>return Err("unknown namespace fault (orphan, alias, marker, link)".into()),
    };
    let mut disk=image()?;
    {
        let window=BlockWindow::new(&mut disk,START,COUNT).map_err(|e|format!("window: {e:?}"))?;
        let mut flat=DurableFs::mount(window).map_err(|e|format!("raw mount: {e:?}"))?;
        flat.create_path(path,payload).map_err(|e|format!("fault entry: {e:?}"))?;
        if flat.mounted_health().map_err(|e|format!("COW health: {e:?}"))?.redundancy
            !=indianos_kernel_core::durablefs::Redundancy::Healthy{
            return Err("fault fixture lost COW redundancy".into());
        }
    }
    Ok(disk)
}
fn refresh_partition_table(disk:&mut Image){
    let table=disk.sectors[2];
    disk.sectors[254]=table;
    disk.sectors[1]=header(1,255,2,crc(&table));
    disk.sectors[255]=header(255,1,254,crc(&table));
}
// Each case preserves the selected partition's filesystem and valid GPT
// header/array checksums. The rejection must come from the named layout fault.
fn partition_fault(kind:&str)->Result<Image,String>{
    if !matches!(kind,"overlap"|"duplicate-id"|"protective-overflow"){
        return Err("unknown partition fault (overlap, duplicate-id, protective-overflow)".into());
    }
    let mut disk=image()?;
    if kind=="protective-overflow"{
        disk.sectors[0][458..462].copy_from_slice(&(SECTORS as u32).to_le_bytes());
    }else{
        let entry=&mut disk.sectors[2][128..256];
        entry[..16].copy_from_slice(b"IndianOS-COW-VFS");
        entry[16..32].copy_from_slice(if kind=="duplicate-id"{&PARTITION_GUID}else{&[0x7d;16]});
        let (begin,end)=if kind=="overlap"{(START+10,START+20)}else{(150,199)};
        entry[32..40].copy_from_slice(&begin.to_le_bytes());
        entry[40..48].copy_from_slice(&end.to_le_bytes());
        refresh_partition_table(&mut disk);
    }
    Ok(disk)
}
fn selection()->[u8;64]{
    let mut bytes=[0u8;64];bytes[..8].copy_from_slice(b"INDVOL01");
    bytes[16..32].copy_from_slice(&DISK_GUID);
    bytes[32..48].copy_from_slice(&PARTITION_GUID);
    bytes[48..56].copy_from_slice(&START.to_le_bytes());
    bytes[56..64].copy_from_slice(&COUNT.to_le_bytes());bytes
}
fn provision_selected(path:&Path,config:&Path)->Result<(),String>{
    let mut output=OpenOptions::new().write(true).create_new(true).open(config)
        .map_err(|e|format!("refusing existing/unwritable selection destination: {e}"))?;
    if let Err(error)=provision(path){let _=std::fs::remove_file(config);return Err(error);}
    output.write_all(&selection()).and_then(|_|output.sync_all()).map_err(|e|e.to_string())
}
fn main(){
    let args:Vec<_>=std::env::args_os().collect();
    let result=if args.len()==5&&args[1]=="--output"&&args[3]=="--selection-output"{
        provision_selected(Path::new(&args[2]),Path::new(&args[4]))
    }else if args.len()==5&&args[1]=="--namespace-fault"&&args[3]=="--output"{
        args[2].to_str().ok_or_else(||"invalid fault name".to_string())
            .and_then(namespace_fault).and_then(|disk|write_image(Path::new(&args[4]),disk))
    }else if args.len()==5&&args[1]=="--partition-fault"&&args[3]=="--output"{
        args[2].to_str().ok_or_else(||"invalid fault name".to_string())
            .and_then(partition_fault).and_then(|disk|write_image(Path::new(&args[4]),disk))
    }else{Err("usage: indianos-desktop-fixture --output NEW.raw --selection-output NEW.bin; or --namespace-fault KIND --output NEW.raw; or --partition-fault KIND --output NEW.raw".into())};
    if let Err(error)=result{eprintln!("desktop fixture: {error}");std::process::exit(1);}
}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn partition_faults_are_crc_valid_isolated_and_rejected_for_the_named_reason(){
        use indianos_kernel_core::m1_partition::{discover,Error};
        let original=image().unwrap();
        for (kind,error) in [("overlap",Error::Overlap),("duplicate-id",Error::Invalid),
            ("protective-overflow",Error::Invalid)]{
            let mut disk=partition_fault(kind).unwrap();
            for lba in 0..SECTORS{
                if ![0,1,2,254,255].contains(&lba){assert_eq!(disk.sectors[lba],original.sectors[lba]);}
            }
            assert_eq!(disk.sectors[2],disk.sectors[254]);
            let table_crc=crc(&disk.sectors[2]);
            for lba in [1,255]{
                let mut h=disk.sectors[lba];
                let header_crc=u32::from_le_bytes(h[16..20].try_into().unwrap());
                h[16..20].fill(0);assert_eq!(crc(&h[..92]),header_crc);
                assert_eq!(u32::from_le_bytes(h[88..92].try_into().unwrap()),table_crc);
            }
            let mut bounded=BlockWindow::new(&mut disk,START,COUNT).unwrap();
            assert_eq!(DurableFs::inspect_health(&mut bounded).unwrap().redundancy,
                indianos_kernel_core::durablefs::Redundancy::Healthy);
            drop(bounded);
            let before=disk.sectors.clone();
            assert_eq!(discover(&mut disk),Err(error),"{kind}");
            assert_eq!(disk.sectors,before);
            // Repair only this fixture's named fault as a positive control.
            match kind{
                "overlap"=>{
                    disk.sectors[2][160..168].copy_from_slice(&150u64.to_le_bytes());
                    disk.sectors[2][168..176].copy_from_slice(&199u64.to_le_bytes());
                    refresh_partition_table(&mut disk);
                },
                "duplicate-id"=>{
                    disk.sectors[2][144..160].fill(0x7d);
                    refresh_partition_table(&mut disk);
                },
                _=>disk.sectors[0][458..462].copy_from_slice(&255u32.to_le_bytes()),
            }
            assert_eq!(discover(&mut disk).unwrap().len(),if kind=="protective-overflow"{1}else{2});
        }
        assert!(partition_fault("unknown").is_err());
    }
    #[test]fn namespace_faults_keep_partition_and_cow_health_but_fail_strict_admission(){
        let original=image().unwrap();
        for kind in ["orphan","alias","marker","link"]{
            let mut disk=namespace_fault(kind).unwrap();
            assert_eq!(&disk.sectors[..START as usize],&original.sectors[..START as usize]);
            assert_eq!(&disk.sectors[(START+COUNT) as usize..],&original.sectors[(START+COUNT) as usize..]);
            let receipt=inspect_partition_receipt(&mut disk,0).unwrap();
            let mut window=BlockWindow::new(&mut disk,START,COUNT).unwrap();
            assert_eq!(DurableFs::inspect_health(&mut window).unwrap().redundancy,
                indianos_kernel_core::durablefs::Redundancy::Healthy);
            drop(window);
            assert!(matches!(mount_gpt_selected_healthy(&mut disk,receipt.gpt.unwrap()),
                Err(indianos_kernel_core::m1_partition::MountError::Filesystem(
                    indianos_kernel_core::durablefs::FsError::InvalidVolume))),"{kind}");
        }
        assert!(namespace_fault("unknown").is_err());
    }
    #[test]fn actual_gpt_vfs_roundtrip_preserves_all_guard_sectors(){
        let disk=image().unwrap();
        for lba in (3..START as usize).chain((START+COUNT) as usize..254){
            assert_eq!(disk.sectors[lba],[0;SECTOR_BYTES]);
        }
        assert_eq!(disk.sectors[2],disk.sectors[254]);
        assert_eq!(&disk.sectors[START as usize][..8],b"INDR027!");
    }
    #[test]fn existing_destination_and_device_like_names_are_never_overwritten(){
        let path=std::env::temp_dir().join(format!("indianos-fixture-{}.raw",std::process::id()));
        let _=std::fs::remove_file(&path);
        std::fs::write(&path,b"sentinel").unwrap();
        assert!(provision(&path).is_err());assert_eq!(std::fs::read(&path).unwrap(),b"sentinel");
        assert!(provision(Path::new("/dev/sda")).is_err());
        std::fs::remove_file(path).unwrap();
    }
    #[test]fn boot_selection_binds_exact_fixture_guids_and_geometry(){
        let bytes=selection();assert_eq!(&bytes[..8],b"INDVOL01");
        assert_eq!(&bytes[8..16],&[0;8]);
        assert_eq!(&bytes[16..32],&DISK_GUID);assert_eq!(&bytes[32..48],&PARTITION_GUID);
        assert_eq!(u64::from_le_bytes(bytes[48..56].try_into().unwrap()),START);
        assert_eq!(u64::from_le_bytes(bytes[56..64].try_into().unwrap()),COUNT);
    }
}

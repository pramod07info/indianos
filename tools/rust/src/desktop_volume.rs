//! Read-only admission of an existing image; never formats or repairs storage.
use indianos_kernel_core::{
    block::{BlockDevice, BlockError, SECTOR_BYTES},
    durablefs::MIN_SECTORS,
    m1_partition::{
        inspect_partition_receipt, mount_gpt_selected_healthy, GptSelection, MAX_GPT_ENTRIES,
    },
};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};

struct ExistingImage {
    file: File,
    sectors: u64,
}
fn open_regular(path: &Path) -> Result<File, String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.file_type().is_file() {
        return Err("input must be a regular file, not a device or symlink".into());
    }
    let file = File::open(path).map_err(|e| e.to_string())?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("opened input is not a regular file".into());
    }
    Ok(file)
}
impl ExistingImage {
    fn open(path: &Path) -> Result<Self, String> {
        // Refuse devices, directories and symlinks before opening. Recheck the
        // opened descriptor so a replaced regular file cannot become a device.
        let file = open_regular(path)?;
        let metadata = file.metadata().map_err(|e| e.to_string())?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() % SECTOR_BYTES as u64 != 0 {
            return Err("input must contain complete 512-byte sectors".into());
        }
        Ok(Self {
            file,
            sectors: metadata.len() / SECTOR_BYTES as u64,
        })
    }
}
impl BlockDevice for ExistingImage {
    fn sector_count(&self) -> u64 {
        self.sectors
    }
    fn read_sector(&mut self, lba: u64, out: &mut [u8; SECTOR_BYTES]) -> Result<(), BlockError> {
        if lba >= self.sectors {
            return Err(BlockError::OutOfRange);
        }
        if self.file.metadata().map_err(|_| BlockError::Io)?.len()
            != self.sectors * SECTOR_BYTES as u64
        {
            return Err(BlockError::Io);
        }
        let mut staged = [0; SECTOR_BYTES];
        self.file
            .seek(SeekFrom::Start(lba * SECTOR_BYTES as u64))
            .map_err(|_| BlockError::Io)?;
        self.file
            .read_exact(&mut staged)
            .map_err(|_| BlockError::Io)?;
        *out = staged;
        Ok(())
    }
    fn write_sector(&mut self, _: u64, _: &[u8; SECTOR_BYTES]) -> Result<(), BlockError> {
        Err(BlockError::ReadOnly)
    }
    fn sync(&mut self) -> Result<(), BlockError> {
        Err(BlockError::ReadOnly)
    }
}

fn encode(selection: GptSelection) -> [u8; 64] {
    let mut bytes = [0; 64];
    bytes[..8].copy_from_slice(b"INDVOL01");
    bytes[8..12].copy_from_slice(&selection.partition.index.to_le_bytes());
    bytes[16..32].copy_from_slice(&selection.disk_guid);
    bytes[32..48].copy_from_slice(&selection.unique_partition_guid);
    bytes[48..56].copy_from_slice(&selection.partition.start.to_le_bytes());
    bytes[56..64].copy_from_slice(&selection.partition.sectors.to_le_bytes());
    bytes
}

fn validate(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() != 64 || &bytes[..8] != b"INDVOL01" || bytes[12..16] != [0; 4] {
        return Err("selection must be a canonical 64-byte INDVOL01 record".into());
    }
    let index = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    let start = u64::from_le_bytes(bytes[48..56].try_into().unwrap());
    let sectors = u64::from_le_bytes(bytes[56..64].try_into().unwrap());
    if index >= MAX_GPT_ENTRIES as u32
        || bytes[16..32] == [0; 16]
        || bytes[32..48] == [0; 16]
        || start == 0
        || sectors < MIN_SECTORS
        || start.checked_add(sectors).is_none()
    {
        return Err("selection contains an invalid ordinal, GUID or sector range".into());
    }
    Ok(())
}

fn check_selection(path: &Path) -> Result<(), String> {
    let mut bytes = Vec::with_capacity(65);
    open_regular(path)?
        .take(65)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    validate(&bytes)
}

fn pin(image: &Path, index: u32, output: &Path) -> Result<(), String> {
    let mut disk = ExistingImage::open(image)?;
    let receipt =
        inspect_partition_receipt(&mut disk, index).map_err(|e| format!("partition: {e:?}"))?;
    let selection = receipt
        .gpt
        .ok_or("Desktop requires explicit GPT identities")?;
    // Use the actual kernel parser and healthy-only filesystem admission, not
    // a parallel host implementation or a signature-only check.
    {
        let _mounted = mount_gpt_selected_healthy(&mut disk, selection)
            .map_err(|e| format!("healthy filesystem: {e:?}"))?;
    }
    let after =
        inspect_partition_receipt(&mut disk, index).map_err(|e| format!("recheck: {e:?}"))?;
    if receipt != after {
        return Err("partition identity changed during admission".into());
    }
    let bytes = encode(selection);
    validate(&bytes)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|e| format!("refusing existing/unwritable selection destination: {e}"))?;
    if let Err(error) = file.write_all(&bytes).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(output);
        return Err(error.to_string());
    }
    Ok(())
}

fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    let result = if args.len() == 3 && args[1] == "--check-selection" {
        check_selection(Path::new(&args[2]))
    } else if args.len() == 7
        && args[1] == "--image"
        && args[3] == "--partition"
        && args[5] == "--output"
    {
        args[4]
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
            .ok_or("invalid partition ordinal".into())
            .and_then(|index| pin(Path::new(&args[2]), index, Path::new(&args[6])))
    } else {
        Err("usage: indianos-desktop-volume --image EXISTING.raw --partition ORDINAL --output NEW.bin | --check-selection SELECTION.bin".into())
    };
    if let Err(error) = result {
        eprintln!("desktop volume: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indianos_kernel_core::m1_partition::{Partition, Scheme};
    fn selection() -> [u8; 64] {
        encode(GptSelection {
            partition: Partition {
                scheme: Scheme::Gpt,
                index: 0,
                start: 40,
                sectors: 100,
            },
            disk_guid: [1; 16],
            unique_partition_guid: [2; 16],
        })
    }
    #[test]
    fn canonical_selection_and_all_invalid_fields() {
        let valid = selection();
        assert!(validate(&valid).is_ok());
        for size in [0, 63, 65] {
            assert!(validate(&vec![0; size]).is_err());
        }
        for (offset, bytes) in [
            (0, vec![0; 8]),
            (8, 128u32.to_le_bytes().to_vec()),
            (12, vec![1]),
            (16, vec![0; 16]),
            (32, vec![0; 16]),
            (48, vec![0; 8]),
            (56, vec![0; 8]),
            (48, u64::MAX.to_le_bytes().to_vec()),
        ] {
            let mut bad = valid;
            bad[offset..offset + bytes.len()].copy_from_slice(&bytes);
            assert!(validate(&bad).is_err(), "offset {offset}");
        }
    }
    #[test]
    fn devices_symlinks_and_partial_sectors_are_refused() {
        assert!(ExistingImage::open(Path::new("/dev/null")).is_err());
        assert!(check_selection(Path::new("/dev/zero")).is_err());
        let path = std::env::temp_dir().join(format!("indianos-volume-{}.raw", std::process::id()));
        std::fs::write(&path, [0; 513]).unwrap();
        assert!(ExistingImage::open(&path).is_err());
        std::fs::write(&path, [0; 512]).unwrap();
        let mut disk = ExistingImage::open(&path).unwrap();
        let mut out = [7; 512];
        assert_eq!(disk.read_sector(1, &mut out), Err(BlockError::OutOfRange));
        assert_eq!(out, [7; 512]);
        assert_eq!(disk.write_sector(0, &[1; 512]), Err(BlockError::ReadOnly));
        assert_eq!(disk.sync(), Err(BlockError::ReadOnly));
        assert_eq!(std::fs::read(&path).unwrap(), vec![0; 512]);
        #[cfg(unix)]
        {
            let link = path.with_extension("link");
            std::os::unix::fs::symlink(&path, &link).unwrap();
            assert!(ExistingImage::open(&link).is_err());
            std::fs::remove_file(link).unwrap();
        }
        std::fs::remove_file(path).unwrap();
    }
}

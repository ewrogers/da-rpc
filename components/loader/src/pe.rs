use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

const PE_MACHINE_I386: u16 = 0x014C;
const PE_CHARACTERISTIC_DLL: u16 = 0x2000;

pub(crate) fn validate_x86_dll(path: &Path) -> Result<(), String> {
    let mut file = File::open(path)
        .map_err(|error| format!("failed to open `{}`: {error}", path.display()))?;

    let mut dos_header = [0_u8; 64];
    file.read_exact(&mut dos_header)
        .map_err(|error| format!("failed to read DOS header: {error}"))?;

    if &dos_header[..2] != b"MZ" {
        return Err("file does not have an MZ header".to_owned());
    }

    let pe_offset = u32::from_le_bytes([
        dos_header[0x3C],
        dos_header[0x3D],
        dos_header[0x3E],
        dos_header[0x3F],
    ]);

    file.seek(SeekFrom::Start(u64::from(pe_offset)))
        .map_err(|error| format!("failed to seek to PE header: {error}"))?;

    let mut pe_header = [0_u8; 24];
    file.read_exact(&mut pe_header)
        .map_err(|error| format!("failed to read PE header: {error}"))?;

    if &pe_header[..4] != b"PE\0\0" {
        return Err("file does not have a PE signature".to_owned());
    }

    let machine = u16::from_le_bytes([pe_header[4], pe_header[5]]);
    let characteristics = u16::from_le_bytes([pe_header[22], pe_header[23]]);

    if machine != PE_MACHINE_I386 {
        return Err(format!("DLL is not x86: machine=0x{machine:04X}"));
    }

    if characteristics & PE_CHARACTERISTIC_DLL == 0 {
        return Err("file is not marked as a DLL".to_owned());
    }

    Ok(())
}

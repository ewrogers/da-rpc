use darpc_win32::lifecycle::{INITIALIZE_EXPORT, SHUTDOWN_EXPORT};
use std::{
    fs,
    path::{Path, PathBuf},
};

const MAX_DLL_SIZE: u64 = 64 * 1024 * 1024;
const MAX_EXPORT_NAME_SIZE: u32 = 256;

const PE_MACHINE_I386: u16 = 0x014C;
const PE_CHARACTERISTIC_DLL: u16 = 0x2000;
const PE32_MAGIC: u16 = 0x010B;
const COFF_HEADER_SIZE: usize = 20;
const MIN_PE32_OPTIONAL_HEADER_SIZE: usize = 104;
const NUMBER_OF_DATA_DIRECTORIES_OFFSET: usize = 92;
const EXPORT_DIRECTORY_RVA_OFFSET: usize = 96;
const EXPORT_DIRECTORY_SIZE_OFFSET: usize = 100;
const EXPORT_DIRECTORY_HEADER_SIZE: u32 = 40;
const SECTION_HEADER_SIZE: usize = 40;
const SECTION_VIRTUAL_SIZE_OFFSET: usize = 8;
const SECTION_VIRTUAL_ADDRESS_OFFSET: usize = 12;
const SECTION_RAW_SIZE_OFFSET: usize = 16;
const SECTION_RAW_POINTER_OFFSET: usize = 20;
const EXPORT_FUNCTION_COUNT_OFFSET: usize = 20;
const EXPORT_NAME_COUNT_OFFSET: usize = 24;
const EXPORT_FUNCTION_TABLE_RVA_OFFSET: usize = 28;
const EXPORT_NAME_TABLE_RVA_OFFSET: usize = 32;
const EXPORT_ORDINAL_TABLE_RVA_OFFSET: usize = 36;

pub(crate) struct DarpcDll {
    pub(crate) path: PathBuf,
    pub(crate) initialize_rva: u32,
    pub(crate) shutdown_rva: u32,
}

struct LifecycleExports {
    initialize_rva: u32,
    shutdown_rva: u32,
}

struct PeHeaders<'a> {
    section_table: &'a [u8],
    export_directory_rva: u32,
    export_directory_size: u32,
}

fn checked_offset(base: usize, delta: usize, field: &str) -> Result<usize, String> {
    base.checked_add(delta)
        .ok_or_else(|| format!("{field} offset overflow"))
}

fn slice_at<'a>(
    image: &'a [u8],
    offset: usize,
    length: usize,
    field: &str,
) -> Result<&'a [u8], String> {
    let end = checked_offset(offset, length, field)?;
    image
        .get(offset..end)
        .ok_or_else(|| format!("{field} is outside the file"))
}

fn bytes_at<const N: usize>(image: &[u8], offset: usize, field: &str) -> Result<[u8; N], String> {
    slice_at(image, offset, N, field)?
        .try_into()
        .map_err(|_| format!("invalid {field}"))
}

fn u16_at(image: &[u8], offset: usize, field: &str) -> Result<u16, String> {
    Ok(u16::from_le_bytes(bytes_at(image, offset, field)?))
}

fn u32_at(image: &[u8], offset: usize, field: &str) -> Result<u32, String> {
    Ok(u32::from_le_bytes(bytes_at(image, offset, field)?))
}

fn rva_range_to_file_offset(
    image: &[u8],
    section_table: &[u8],
    rva: u32,
    size: u32,
) -> Result<usize, String> {
    let rva_end = rva
        .checked_add(size)
        .ok_or_else(|| "RVA range overflow".to_owned())?;

    for section in section_table.chunks_exact(SECTION_HEADER_SIZE) {
        let virtual_size = u32_at(section, SECTION_VIRTUAL_SIZE_OFFSET, "section virtual size")?;
        let virtual_address = u32_at(
            section,
            SECTION_VIRTUAL_ADDRESS_OFFSET,
            "section virtual address",
        )?;
        let raw_size = u32_at(section, SECTION_RAW_SIZE_OFFSET, "section raw size")?;
        let raw_pointer = u32_at(section, SECTION_RAW_POINTER_OFFSET, "section raw pointer")?;

        let mapped_size = virtual_size.max(raw_size);
        let section_end = virtual_address
            .checked_add(mapped_size)
            .ok_or_else(|| "section RVA range overflow".to_owned())?;

        if rva < virtual_address || rva_end > section_end {
            continue;
        }

        let section_offset = rva - virtual_address;
        let raw_end = section_offset
            .checked_add(size)
            .ok_or_else(|| "section raw range overflow".to_owned())?;

        if raw_end > raw_size {
            return Err("RVA range is not backed by section file data".to_owned());
        }

        let file_offset = raw_pointer
            .checked_add(section_offset)
            .ok_or_else(|| "file offset overflow".to_owned())?;

        let file_offset = usize::try_from(file_offset)
            .map_err(|_| "file offset does not fit usize".to_owned())?;
        let size = usize::try_from(size).map_err(|_| "RVA size does not fit usize".to_owned())?;

        slice_at(image, file_offset, size, "RVA range")?;

        return Ok(file_offset);
    }

    Err(format!(
        "RVA range 0x{rva:08X}..0x{rva_end:08X} is not mapped by a section"
    ))
}

fn export_name_at_rva(
    image: &[u8],
    section_table: &[u8],
    name_rva: u32,
) -> Result<Vec<u8>, String> {
    let mut name = Vec::new();

    for offset in 0..MAX_EXPORT_NAME_SIZE {
        let byte_rva = name_rva
            .checked_add(offset)
            .ok_or_else(|| "export name RVA overflow".to_owned())?;

        let file_offset = rva_range_to_file_offset(image, section_table, byte_rva, 1)?;

        let byte = bytes_at::<1>(image, file_offset, "export name byte")?[0];
        name.push(byte);

        if byte == 0 {
            return Ok(name);
        }
    }

    Err(format!("export name exceeds {MAX_EXPORT_NAME_SIZE} bytes"))
}

fn read_dll(path: &Path) -> Result<Vec<u8>, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("failed to inspect `{}`: {error}", path.display()))?;

    if !metadata.is_file() {
        return Err(format!("DLL path is not a file: `{}`", path.display()));
    }

    if metadata.len() > MAX_DLL_SIZE {
        return Err(format!("DLL exceeds {MAX_DLL_SIZE} bytes"));
    }

    fs::read(path).map_err(|error| format!("failed to read `{}`: {error}", path.display()))
}

fn parse_headers(image: &[u8]) -> Result<PeHeaders<'_>, String> {
    if bytes_at::<2>(image, 0, "DOS signature")? != *b"MZ" {
        return Err("file does not have an MZ header".to_owned());
    }

    let pe_offset = usize::try_from(u32_at(image, 0x3C, "PE offset")?)
        .map_err(|_| "PE offset does not fit usize".to_owned())?;

    if bytes_at::<4>(image, pe_offset, "PE signature")? != *b"PE\0\0" {
        return Err("file does not have a PE signature".to_owned());
    }

    let machine = u16_at(image, checked_offset(pe_offset, 4, "machine")?, "machine")?;
    let characteristics = u16_at(
        image,
        checked_offset(pe_offset, 22, "characteristics")?,
        "characteristics",
    )?;

    if machine != PE_MACHINE_I386 {
        return Err(format!("DLL is not x86: machine=0x{machine:04X}"));
    }

    if characteristics & PE_CHARACTERISTIC_DLL == 0 {
        return Err("file is not marked as a DLL".to_owned());
    }

    let coff_offset = checked_offset(pe_offset, 4, "COFF header")?;

    let number_of_sections = usize::from(u16_at(
        image,
        checked_offset(coff_offset, 2, "section count")?,
        "section_count",
    )?);

    if number_of_sections == 0 {
        return Err("DLL has no sections".to_owned());
    }

    let optional_header_size = usize::from(u16_at(
        image,
        checked_offset(coff_offset, 16, "optional header size")?,
        "optional header size",
    )?);

    if optional_header_size < MIN_PE32_OPTIONAL_HEADER_SIZE {
        return Err(format!(
            "PE32 optional header is too small: {optional_header_size}"
        ));
    }

    let optional_header_offset = checked_offset(coff_offset, COFF_HEADER_SIZE, "optional header")?;

    let optional_header = slice_at(
        image,
        optional_header_offset,
        optional_header_size,
        "optional header",
    )?;

    let magic = u16_at(optional_header, 0, "optional header magic")?;

    if magic != PE32_MAGIC {
        return Err(format!("DLL is not PE32: magic=0x{magic:04X}"));
    }

    let number_of_data_directories = u32_at(
        optional_header,
        NUMBER_OF_DATA_DIRECTORIES_OFFSET,
        "number of data directories",
    )?;

    if number_of_data_directories == 0 {
        return Err("DLL has no export directory entry".to_owned());
    }

    let export_directory_rva = u32_at(
        optional_header,
        EXPORT_DIRECTORY_RVA_OFFSET,
        "export directory RVA",
    )?;

    let export_directory_size = u32_at(
        optional_header,
        EXPORT_DIRECTORY_SIZE_OFFSET,
        "export directory size",
    )?;

    if export_directory_rva == 0 {
        return Err("DLL has no export directory".to_owned());
    }

    if export_directory_size < EXPORT_DIRECTORY_HEADER_SIZE {
        return Err(format!(
            "export directory is too small: {export_directory_size}"
        ));
    }

    let section_table_offset = checked_offset(
        optional_header_offset,
        optional_header_size,
        "section table",
    )?;

    let section_table_size = number_of_sections
        .checked_mul(SECTION_HEADER_SIZE)
        .ok_or_else(|| "section table size overflow".to_owned())?;

    let section_table = slice_at(
        image,
        section_table_offset,
        section_table_size,
        "section table",
    )?;

    Ok(PeHeaders {
        section_table,
        export_directory_rva,
        export_directory_size,
    })
}

fn find_lifecycle_exports(
    image: &[u8],
    headers: &PeHeaders<'_>,
) -> Result<LifecycleExports, String> {
    let section_table = headers.section_table;
    let export_directory_rva = headers.export_directory_rva;
    let export_directory_size = headers.export_directory_size;

    let export_directory_end_rva = export_directory_rva
        .checked_add(export_directory_size)
        .ok_or_else(|| "export directory RVA range overflow".to_owned())?;

    let export_directory_offset = rva_range_to_file_offset(
        image,
        section_table,
        export_directory_rva,
        EXPORT_DIRECTORY_HEADER_SIZE,
    )?;

    let export_directory = bytes_at::<40>(image, export_directory_offset, "export directory")?;

    let function_count = u32_at(
        &export_directory,
        EXPORT_FUNCTION_COUNT_OFFSET,
        "export function count",
    )?;
    let name_count = u32_at(
        &export_directory,
        EXPORT_NAME_COUNT_OFFSET,
        "export name count",
    )?;

    if function_count == 0 || name_count == 0 {
        return Err("DLL has no named exports".to_owned());
    }

    let function_table_rva = u32_at(
        &export_directory,
        EXPORT_FUNCTION_TABLE_RVA_OFFSET,
        "export function table RVA",
    )?;
    let name_table_rva = u32_at(
        &export_directory,
        EXPORT_NAME_TABLE_RVA_OFFSET,
        "export name table RVA",
    )?;
    let ordinal_table_rva = u32_at(
        &export_directory,
        EXPORT_ORDINAL_TABLE_RVA_OFFSET,
        "export ordinal table RVA",
    )?;

    let function_table_size = function_count
        .checked_mul(4)
        .ok_or_else(|| "export function table size overflow".to_owned())?;
    let name_table_size = name_count
        .checked_mul(4)
        .ok_or_else(|| "export name table size overflow".to_owned())?;
    let ordinal_table_size = name_count
        .checked_mul(2)
        .ok_or_else(|| "export ordinal table size overflow".to_owned())?;

    let name_table_offset =
        rva_range_to_file_offset(image, section_table, name_table_rva, name_table_size)?;
    let name_table_length = usize::try_from(name_table_size)
        .map_err(|_| "export name table size does not fit usize".to_owned())?;

    let name_table = slice_at(
        image,
        name_table_offset,
        name_table_length,
        "export name table",
    )?;

    let name_count = usize::try_from(name_count)
        .map_err(|_| "export name count does not fit usize".to_owned())?;

    let function_table_offset = rva_range_to_file_offset(
        image,
        section_table,
        function_table_rva,
        function_table_size,
    )?;
    let ordinal_table_offset =
        rva_range_to_file_offset(image, section_table, ordinal_table_rva, ordinal_table_size)?;

    let function_table = slice_at(
        image,
        function_table_offset,
        usize::try_from(function_table_size)
            .map_err(|_| "function table size does not fit usize".to_owned())?,
        "export function table",
    )?;

    let ordinal_table = slice_at(
        image,
        ordinal_table_offset,
        usize::try_from(ordinal_table_size)
            .map_err(|_| "ordinal table size does not fit usize".to_owned())?,
        "export ordinal table",
    )?;

    let function_count = usize::try_from(function_count)
        .map_err(|_| "export function count does not fit usize".to_owned())?;

    let mut initialize_rva = None;
    let mut shutdown_rva = None;

    for index in 0..name_count {
        let entry_offset = index
            .checked_mul(4)
            .ok_or_else(|| "export name entry offset overflow".to_owned())?;

        let name_rva = u32_at(name_table, entry_offset, "export name RVA")?;

        let name = export_name_at_rva(image, section_table, name_rva)?;

        let destination = if name.as_slice() == INITIALIZE_EXPORT {
            &mut initialize_rva
        } else if name.as_slice() == SHUTDOWN_EXPORT {
            &mut shutdown_rva
        } else {
            continue;
        };

        let ordinal_offset = index
            .checked_mul(2)
            .ok_or_else(|| "export ordinal offset overflow".to_owned())?;
        let ordinal = usize::from(u16_at(ordinal_table, ordinal_offset, "export ordinal")?);

        if ordinal >= function_count {
            return Err("export ordinal exceeds function table".to_owned());
        }

        let function_offset = ordinal
            .checked_mul(4)
            .ok_or_else(|| "export function offset overflow".to_owned())?;
        let function_rva = u32_at(function_table, function_offset, "export function RVA")?;

        if function_rva == 0 {
            return Err("lifecycle export has a null RVA".to_owned());
        }

        if (export_directory_rva..export_directory_end_rva).contains(&function_rva) {
            return Err("forwarded lifecycle exports are unsupported".to_owned());
        }

        *destination = Some(function_rva);
    }

    Ok(LifecycleExports {
        initialize_rva: initialize_rva
            .ok_or_else(|| "DLL does not export darpc_initialize".to_owned())?,
        shutdown_rva: shutdown_rva
            .ok_or_else(|| "DLL does not export darpc_shutdown".to_owned())?,
    })
}

impl DarpcDll {
    pub(crate) fn validate(path: PathBuf) -> Result<Self, String> {
        let path = fs::canonicalize(&path)
            .map_err(|error| format!("failed to resolve DLL path `{}`: {error}", path.display()))?;

        let image = read_dll(&path)?;
        let headers = parse_headers(&image)?;
        let exports = find_lifecycle_exports(&image, &headers)?;

        Ok(Self {
            path,
            initialize_rva: exports.initialize_rva,
            shutdown_rva: exports.shutdown_rva,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::DarpcDll;
    use std::{env, fs, process};

    #[test]
    fn validation_rejects_directory() {
        let path = env::temp_dir().join(format!("darpc-loader-{}-directory", process::id()));
        let _ = fs::remove_dir(&path);
        fs::create_dir(&path).expect("create test directory");

        let result = DarpcDll::validate(path.clone());

        fs::remove_dir(&path).expect("remove test directory");
        let error = result.err().expect("directory must be rejected");
        assert!(error.contains("DLL path is not a file"));
    }

    #[test]
    fn validation_rejects_non_pe_file() {
        let path = env::temp_dir().join(format!("darpc-loader-{}-invalid.dll", process::id()));
        let _ = fs::remove_file(&path);
        fs::write(&path, b"NO").expect("write invalid test DLL");

        let result = DarpcDll::validate(path.clone());

        fs::remove_file(&path).expect("remove invalid test DLL");
        let error = result.err().expect("non-PE file must be rejected");
        assert_eq!(error, "file does not have an MZ header");
    }
}

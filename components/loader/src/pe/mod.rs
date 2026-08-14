mod disk;
mod mapped;

const MAX_DLL_SIZE: u64 = 64 * 1024 * 1024;
const MAX_EXPORT_NAME_SIZE: u32 = 256;
#[cfg_attr(not(windows), allow(dead_code))]
const MAX_PE_HEADER_OFFSET: u32 = 1024 * 1024;
#[cfg_attr(not(windows), allow(dead_code))]
const MAX_OPTIONAL_HEADER_SIZE: usize = 4096;
#[cfg_attr(not(windows), allow(dead_code))]
const MAX_EXPORT_ENTRIES: u32 = 4_096;

const PE_MACHINE_I386: u16 = 0x014C;
const PE_CHARACTERISTIC_DLL: u16 = 0x2000;
const PE32_MAGIC: u16 = 0x010B;
const COFF_HEADER_SIZE: usize = 20;
const COFF_TIMESTAMP_OFFSET: usize = 4;
const MIN_PE32_OPTIONAL_HEADER_SIZE: usize = 104;
const SIZE_OF_IMAGE_OFFSET: usize = 56;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LifecycleImageIdentity {
    pub(crate) initialize_rva: u32,
    pub(crate) shutdown_rva: u32,
    pub(crate) timestamp: u32,
    pub(crate) size_of_image: u32,
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

pub(crate) use disk::DarpcDll;
#[cfg(windows)]
pub(crate) use mapped::lifecycle_identity_from_mapped_image;

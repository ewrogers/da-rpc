use super::*;
use darpc_win32::lifecycle::{INITIALIZE_EXPORT, SHUTDOWN_EXPORT};

#[cfg_attr(not(windows), allow(dead_code))]
fn mapped_read<F>(
    read: &mut F,
    rva: u32,
    size: usize,
    size_of_image: Option<u32>,
    field: &str,
) -> Result<Vec<u8>, String>
where
    F: FnMut(u32, usize) -> Result<Vec<u8>, String>,
{
    let size_u32 = u32::try_from(size).map_err(|_| format!("{field} size does not fit u32"))?;
    let end = rva
        .checked_add(size_u32)
        .ok_or_else(|| format!("{field} RVA range overflow"))?;
    if let Some(size_of_image) = size_of_image
        && end > size_of_image
    {
        return Err(format!("{field} is outside the mapped image"));
    }
    let bytes = read(rva, size)?;
    if bytes.len() != size {
        return Err(format!(
            "incomplete {field} read: expected={size} actual={}",
            bytes.len()
        ));
    }
    Ok(bytes)
}

#[cfg_attr(not(windows), allow(dead_code))]
fn mapped_export_name<F>(read: &mut F, name_rva: u32, size_of_image: u32) -> Result<Vec<u8>, String>
where
    F: FnMut(u32, usize) -> Result<Vec<u8>, String>,
{
    let mut name = Vec::new();
    for offset in 0..MAX_EXPORT_NAME_SIZE {
        let rva = name_rva
            .checked_add(offset)
            .ok_or_else(|| "export name RVA overflow".to_owned())?;
        let byte = mapped_read(read, rva, 1, Some(size_of_image), "export name byte")?[0];
        name.push(byte);
        if byte == 0 {
            return Ok(name);
        }
    }
    Err(format!("export name exceeds {MAX_EXPORT_NAME_SIZE} bytes"))
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn lifecycle_identity_from_mapped_image<F>(
    mut read: F,
) -> Result<LifecycleImageIdentity, String>
where
    F: FnMut(u32, usize) -> Result<Vec<u8>, String>,
{
    let dos = mapped_read(&mut read, 0, 0x40, None, "DOS header")?;
    if bytes_at::<2>(&dos, 0, "DOS signature")? != *b"MZ" {
        return Err("mapped image does not have an MZ header".to_owned());
    }

    let pe_rva = u32_at(&dos, 0x3C, "PE offset")?;
    if pe_rva > MAX_PE_HEADER_OFFSET {
        return Err(format!("mapped PE offset is too large: {pe_rva}"));
    }
    let pe = mapped_read(
        &mut read,
        pe_rva,
        4 + COFF_HEADER_SIZE,
        None,
        "PE and COFF headers",
    )?;
    if bytes_at::<4>(&pe, 0, "PE signature")? != *b"PE\0\0" {
        return Err("mapped image does not have a PE signature".to_owned());
    }

    let coff = &pe[4..];
    let machine = u16_at(coff, 0, "machine")?;
    if machine != PE_MACHINE_I386 {
        return Err(format!("mapped DLL is not x86: machine=0x{machine:04X}"));
    }
    let characteristics = u16_at(coff, 18, "characteristics")?;
    if characteristics & PE_CHARACTERISTIC_DLL == 0 {
        return Err("mapped image is not marked as a DLL".to_owned());
    }
    let timestamp = u32_at(coff, COFF_TIMESTAMP_OFFSET, "COFF timestamp")?;
    let optional_header_size = usize::from(u16_at(coff, 16, "optional header size")?);
    if !(MIN_PE32_OPTIONAL_HEADER_SIZE..=MAX_OPTIONAL_HEADER_SIZE).contains(&optional_header_size) {
        return Err(format!(
            "invalid mapped PE32 optional header size: {optional_header_size}"
        ));
    }

    let optional_header_rva = pe_rva
        .checked_add(4 + COFF_HEADER_SIZE as u32)
        .ok_or_else(|| "optional header RVA overflow".to_owned())?;
    let optional = mapped_read(
        &mut read,
        optional_header_rva,
        optional_header_size,
        None,
        "optional header",
    )?;
    let magic = u16_at(&optional, 0, "optional header magic")?;
    if magic != PE32_MAGIC {
        return Err(format!("mapped DLL is not PE32: magic=0x{magic:04X}"));
    }
    let size_of_image = u32_at(&optional, SIZE_OF_IMAGE_OFFSET, "image size")?;
    if size_of_image == 0 || u64::from(size_of_image) > MAX_DLL_SIZE {
        return Err(format!("invalid mapped image size: {size_of_image}"));
    }
    let number_of_data_directories = u32_at(
        &optional,
        NUMBER_OF_DATA_DIRECTORIES_OFFSET,
        "number of data directories",
    )?;
    if number_of_data_directories == 0 {
        return Err("mapped DLL has no export directory entry".to_owned());
    }
    let export_directory_rva = u32_at(
        &optional,
        EXPORT_DIRECTORY_RVA_OFFSET,
        "export directory RVA",
    )?;
    let export_directory_size = u32_at(
        &optional,
        EXPORT_DIRECTORY_SIZE_OFFSET,
        "export directory size",
    )?;
    if export_directory_rva == 0 || export_directory_size < EXPORT_DIRECTORY_HEADER_SIZE {
        return Err("mapped DLL has no valid export directory".to_owned());
    }
    let export_directory_end_rva = export_directory_rva
        .checked_add(export_directory_size)
        .ok_or_else(|| "export directory RVA range overflow".to_owned())?;
    if export_directory_end_rva > size_of_image {
        return Err("export directory is outside the mapped image".to_owned());
    }

    let export_directory = mapped_read(
        &mut read,
        export_directory_rva,
        EXPORT_DIRECTORY_HEADER_SIZE as usize,
        Some(size_of_image),
        "export directory",
    )?;
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
        return Err("mapped DLL has no named exports".to_owned());
    }
    if function_count > MAX_EXPORT_ENTRIES || name_count > MAX_EXPORT_ENTRIES {
        return Err("mapped DLL export table exceeds its safety limit".to_owned());
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
    let function_table_size = usize::try_from(
        function_count
            .checked_mul(4)
            .ok_or_else(|| "export function table size overflow".to_owned())?,
    )
    .map_err(|_| "export function table size does not fit usize".to_owned())?;
    let name_table_size = usize::try_from(
        name_count
            .checked_mul(4)
            .ok_or_else(|| "export name table size overflow".to_owned())?,
    )
    .map_err(|_| "export name table size does not fit usize".to_owned())?;
    let ordinal_table_size = usize::try_from(
        name_count
            .checked_mul(2)
            .ok_or_else(|| "export ordinal table size overflow".to_owned())?,
    )
    .map_err(|_| "export ordinal table size does not fit usize".to_owned())?;
    let function_table = mapped_read(
        &mut read,
        function_table_rva,
        function_table_size,
        Some(size_of_image),
        "export function table",
    )?;
    let name_table = mapped_read(
        &mut read,
        name_table_rva,
        name_table_size,
        Some(size_of_image),
        "export name table",
    )?;
    let ordinal_table = mapped_read(
        &mut read,
        ordinal_table_rva,
        ordinal_table_size,
        Some(size_of_image),
        "export ordinal table",
    )?;

    let mut initialize_rva = None;
    let mut shutdown_rva = None;
    for index in 0..usize::try_from(name_count)
        .map_err(|_| "export name count does not fit usize".to_owned())?
    {
        let name_rva = u32_at(&name_table, index * 4, "export name RVA")?;
        let name = mapped_export_name(&mut read, name_rva, size_of_image)?;
        let destination = if name.as_slice() == INITIALIZE_EXPORT {
            &mut initialize_rva
        } else if name.as_slice() == SHUTDOWN_EXPORT {
            &mut shutdown_rva
        } else {
            continue;
        };
        let ordinal = usize::from(u16_at(&ordinal_table, index * 2, "export ordinal")?);
        if ordinal
            >= usize::try_from(function_count)
                .map_err(|_| "export function count does not fit usize".to_owned())?
        {
            return Err("export ordinal exceeds function table".to_owned());
        }
        let function_rva = u32_at(&function_table, ordinal * 4, "export function RVA")?;
        if function_rva == 0 || function_rva >= size_of_image {
            return Err("lifecycle export has an invalid RVA".to_owned());
        }
        if (export_directory_rva..export_directory_end_rva).contains(&function_rva) {
            return Err("forwarded lifecycle exports are unsupported".to_owned());
        }
        *destination = Some(function_rva);

        if initialize_rva.is_some() && shutdown_rva.is_some() {
            break;
        }
    }

    Ok(LifecycleImageIdentity {
        initialize_rva: initialize_rva
            .ok_or_else(|| "mapped DLL does not export darpc_initialize".to_owned())?,
        shutdown_rva: shutdown_rva
            .ok_or_else(|| "mapped DLL does not export darpc_shutdown".to_owned())?,
        timestamp,
        size_of_image,
    })
}

#[cfg(test)]
mod tests {
    use super::{LifecycleImageIdentity, lifecycle_identity_from_mapped_image};

    fn write_u16(image: &mut [u8], offset: usize, value: u16) {
        image[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn write_u32(image: &mut [u8], offset: usize, value: u32) {
        image[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn mapped_dll() -> Vec<u8> {
        let mut image = vec![0; 0x1000];
        image[..2].copy_from_slice(b"MZ");
        write_u32(&mut image, 0x3C, 0x80);
        image[0x80..0x84].copy_from_slice(b"PE\0\0");
        write_u16(&mut image, 0x84, super::PE_MACHINE_I386);
        write_u32(&mut image, 0x88, 0x6A7F_4A10);
        write_u16(
            &mut image,
            0x94,
            super::MIN_PE32_OPTIONAL_HEADER_SIZE as u16,
        );
        write_u16(&mut image, 0x96, super::PE_CHARACTERISTIC_DLL);
        let optional = 0x98;
        write_u16(&mut image, optional, super::PE32_MAGIC);
        write_u32(&mut image, optional + super::SIZE_OF_IMAGE_OFFSET, 0x1000);
        write_u32(
            &mut image,
            optional + super::NUMBER_OF_DATA_DIRECTORIES_OFFSET,
            1,
        );
        write_u32(
            &mut image,
            optional + super::EXPORT_DIRECTORY_RVA_OFFSET,
            0x200,
        );
        write_u32(
            &mut image,
            optional + super::EXPORT_DIRECTORY_SIZE_OFFSET,
            0x100,
        );
        write_u32(&mut image, 0x200 + super::EXPORT_FUNCTION_COUNT_OFFSET, 2);
        write_u32(&mut image, 0x200 + super::EXPORT_NAME_COUNT_OFFSET, 2);
        write_u32(
            &mut image,
            0x200 + super::EXPORT_FUNCTION_TABLE_RVA_OFFSET,
            0x240,
        );
        write_u32(
            &mut image,
            0x200 + super::EXPORT_NAME_TABLE_RVA_OFFSET,
            0x248,
        );
        write_u32(
            &mut image,
            0x200 + super::EXPORT_ORDINAL_TABLE_RVA_OFFSET,
            0x250,
        );
        write_u32(&mut image, 0x240, 0x500);
        write_u32(&mut image, 0x244, 0x600);
        write_u32(&mut image, 0x248, 0x260);
        write_u32(&mut image, 0x24C, 0x280);
        write_u16(&mut image, 0x250, 0);
        write_u16(&mut image, 0x252, 1);
        image[0x260..0x260 + 17].copy_from_slice(b"darpc_initialize\0");
        image[0x280..0x280 + 15].copy_from_slice(b"darpc_shutdown\0");
        image
    }

    #[test]
    fn reads_lifecycle_identity_from_a_mapped_image() {
        let image = mapped_dll();
        let identity = lifecycle_identity_from_mapped_image(|rva, size| {
            let start = usize::try_from(rva).map_err(|_| "RVA does not fit usize".to_owned())?;
            let end = start
                .checked_add(size)
                .ok_or_else(|| "mapped read overflow".to_owned())?;
            image
                .get(start..end)
                .map(<[u8]>::to_vec)
                .ok_or_else(|| "mapped read is outside image".to_owned())
        })
        .expect("mapped image is valid");

        assert_eq!(
            identity,
            LifecycleImageIdentity {
                initialize_rva: 0x500,
                shutdown_rva: 0x600,
                timestamp: 0x6A7F_4A10,
                size_of_image: 0x1000,
            }
        );
    }
}

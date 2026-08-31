use std::collections::BTreeMap;

use flate2::{Decompress, FlushDecompress, Status};

use super::{
    AasxExportError, AasxImportError, MAX_AASX_PACKAGE_SIZE, MAX_AASX_PART_SIZE, PACKAGE_PARTS,
};

pub(super) struct ZipEntry {
    name: &'static str,
    contents: Vec<u8>,
}

impl ZipEntry {
    pub(super) fn new(name: &'static str, contents: Vec<u8>) -> Self {
        Self { name, contents }
    }
}

pub(super) fn read_zip_parts(package: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, AasxImportError> {
    if package.len() > MAX_AASX_PACKAGE_SIZE {
        return Err(unsupported("AASX package exceeds the supported size"));
    }
    let eocd = find_eocd(package)?;
    let disk = read_u16(package, eocd + 4)?;
    let central_disk = read_u16(package, eocd + 6)?;
    let disk_entries = read_u16(package, eocd + 8)?;
    let entry_count = read_u16(package, eocd + 10)?;
    let central_size = to_usize(read_u32(package, eocd + 12)?)?;
    let central_offset = to_usize(read_u32(package, eocd + 16)?)?;
    if disk != 0
        || central_disk != 0
        || disk_entries != entry_count
        || usize::from(entry_count) > PACKAGE_PARTS.len()
    {
        return Err(unsupported(
            "multi-disk or ZIP64 archives are not supported",
        ));
    }
    let central_end = central_offset
        .checked_add(central_size)
        .ok_or_else(|| unsupported("ZIP central directory range overflows"))?;
    if central_end != eocd || central_end > package.len() {
        return Err(unsupported("ZIP central directory range is invalid"));
    }

    let mut parts = BTreeMap::new();
    let mut local_ranges = Vec::with_capacity(usize::from(entry_count));
    let mut cursor = central_offset;
    for _ in 0..usize::from(entry_count) {
        let record = read_central_record(package, cursor, central_end)?;
        cursor = record.next_offset;
        if !PACKAGE_PARTS.contains(&record.name.as_str()) {
            return Err(unsupported("unsupported AASX package structure"));
        }
        if parts.contains_key(&record.name) {
            return Err(AasxImportError::MalformedPackage("duplicate ZIP entry"));
        }
        let (range, compressed) = read_local_record(package, central_offset, &record)?;
        if local_ranges
            .iter()
            .any(|(start, end)| range.0 < *end && *start < range.1)
        {
            return Err(unsupported("ZIP local entries overlap"));
        }
        local_ranges.push(range);
        let contents = match record.compression {
            0 => compressed.to_vec(),
            8 => inflate(compressed, record.uncompressed_size)?,
            _ => return Err(unsupported("ZIP compression method is not supported")),
        };
        if contents.len() != record.uncompressed_size || crc32(&contents) != record.crc {
            return Err(unsupported(
                "ZIP entry checksum or size does not match its contents",
            ));
        }
        parts.insert(record.name, contents);
    }
    if cursor != central_end {
        return Err(unsupported("ZIP central directory contains trailing data"));
    }
    Ok(parts)
}

struct CentralRecord {
    next_offset: usize,
    local_offset: usize,
    compression: u16,
    crc: u32,
    compressed_size: usize,
    uncompressed_size: usize,
    name: String,
}

fn read_central_record(
    package: &[u8],
    offset: usize,
    central_end: usize,
) -> Result<CentralRecord, AasxImportError> {
    if read_u32(package, offset)? != 0x0201_4b50 {
        return Err(unsupported("invalid ZIP central directory entry"));
    }
    let flags = read_u16(package, offset + 8)?;
    reject_flags(flags)?;
    let compression = read_u16(package, offset + 10)?;
    if !matches!(compression, 0 | 8) {
        return Err(unsupported("ZIP compression method is not supported"));
    }
    let crc = read_u32(package, offset + 16)?;
    let compressed_size = to_usize(read_u32(package, offset + 20)?)?;
    let uncompressed_size = to_usize(read_u32(package, offset + 24)?)?;
    if uncompressed_size > MAX_AASX_PART_SIZE {
        return Err(unsupported("AASX part exceeds the supported size"));
    }
    let name_len = usize::from(read_u16(package, offset + 28)?);
    let extra_len = usize::from(read_u16(package, offset + 30)?);
    let comment_len = usize::from(read_u16(package, offset + 32)?);
    if read_u16(package, offset + 34)? != 0 || extra_len != 0 || comment_len != 0 {
        return Err(unsupported("ZIP entry metadata is not canonical"));
    }
    let next_offset = offset
        .checked_add(46)
        .and_then(|value| value.checked_add(name_len))
        .ok_or_else(|| unsupported("ZIP central entry range overflows"))?;
    if next_offset > central_end {
        return Err(unsupported("ZIP central entry range is invalid"));
    }
    let name = std::str::from_utf8(slice(package, offset + 46, next_offset)?)
        .map_err(|_| unsupported("ZIP entry name is not UTF-8"))?
        .to_string();
    if name.ends_with('/') {
        return Err(unsupported("directory entries are not supported"));
    }
    Ok(CentralRecord {
        next_offset,
        local_offset: to_usize(read_u32(package, offset + 42)?)?,
        compression,
        crc,
        compressed_size,
        uncompressed_size,
        name,
    })
}

fn read_local_record<'a>(
    package: &'a [u8],
    central_offset: usize,
    record: &CentralRecord,
) -> Result<((usize, usize), &'a [u8]), AasxImportError> {
    let offset = record.local_offset;
    if offset.checked_add(30).is_none() || offset >= central_offset {
        return Err(unsupported("ZIP local entry offset is invalid"));
    }
    if read_u32(package, offset)? != 0x0403_4b50 {
        return Err(unsupported("invalid ZIP local entry"));
    }
    let flags = read_u16(package, offset + 6)?;
    reject_flags(flags)?;
    let compression = read_u16(package, offset + 8)?;
    let crc = read_u32(package, offset + 14)?;
    let compressed_size = to_usize(read_u32(package, offset + 18)?)?;
    let uncompressed_size = to_usize(read_u32(package, offset + 22)?)?;
    let name_len = usize::from(read_u16(package, offset + 26)?);
    let extra_len = usize::from(read_u16(package, offset + 28)?);
    if extra_len != 0 {
        return Err(unsupported("ZIP local entry metadata is not canonical"));
    }
    let name_start = offset + 30;
    let data_start = name_start
        .checked_add(name_len)
        .ok_or_else(|| unsupported("ZIP local entry range overflows"))?;
    let data_end = data_start
        .checked_add(compressed_size)
        .ok_or_else(|| unsupported("ZIP local entry range overflows"))?;
    if data_end > central_offset
        || flags != 0
        || compression != record.compression
        || crc != record.crc
        || compressed_size != record.compressed_size
        || uncompressed_size != record.uncompressed_size
        || slice(package, name_start, data_start)? != record.name.as_bytes()
    {
        return Err(unsupported(
            "ZIP local entry does not match its central directory record",
        ));
    }
    Ok(((offset, data_end), slice(package, data_start, data_end)?))
}

fn reject_flags(flags: u16) -> Result<(), AasxImportError> {
    if flags != 0 {
        return Err(unsupported(
            "encrypted, descriptor, or non-canonical ZIP flags are not supported",
        ));
    }
    Ok(())
}

fn inflate(compressed: &[u8], expected_size: usize) -> Result<Vec<u8>, AasxImportError> {
    let mut decompressor = Decompress::new(false);
    let mut output = Vec::with_capacity(expected_size);
    let mut input_offset = 0usize;
    let mut buffer = [0u8; 8 * 1024];
    loop {
        let input_before = decompressor.total_in();
        let output_before = decompressor.total_out();
        let status = decompressor
            .decompress(
                &compressed[input_offset..],
                &mut buffer,
                FlushDecompress::None,
            )
            .map_err(|_| unsupported("could not deflate ZIP entry"))?;
        let consumed = usize::try_from(decompressor.total_in() - input_before)
            .map_err(|_| unsupported("deflated ZIP entry is too large"))?;
        let produced = usize::try_from(decompressor.total_out() - output_before)
            .map_err(|_| unsupported("deflated ZIP entry is too large"))?;
        input_offset = input_offset
            .checked_add(consumed)
            .ok_or_else(|| unsupported("deflated ZIP input range overflows"))?;
        if output.len().saturating_add(produced) > expected_size {
            return Err(unsupported("deflated ZIP entry exceeds its declared size"));
        }
        output.extend_from_slice(&buffer[..produced]);
        match status {
            Status::StreamEnd => break,
            Status::Ok if consumed > 0 || produced > 0 => {}
            _ => return Err(unsupported("deflated ZIP entry cannot make progress")),
        }
    }
    if input_offset != compressed.len() || output.len() != expected_size {
        return Err(unsupported("deflated ZIP entry size is invalid"));
    }
    Ok(output)
}

pub(super) fn write_zip(entries: &[ZipEntry]) -> Result<Vec<u8>, AasxExportError> {
    if entries.len() > u16::MAX as usize {
        return Err(AasxExportError::PackageTooLarge);
    }
    let mut package = Vec::new();
    let mut central = Vec::new();
    for entry in entries {
        if entry.contents.len() > MAX_AASX_PART_SIZE {
            return Err(AasxExportError::PackageTooLarge);
        }
        let name = entry.name.as_bytes();
        let size =
            u32::try_from(entry.contents.len()).map_err(|_| AasxExportError::PackageTooLarge)?;
        let offset = u32::try_from(package.len()).map_err(|_| AasxExportError::PackageTooLarge)?;
        let name_len = u16::try_from(name.len()).map_err(|_| AasxExportError::PackageTooLarge)?;
        let crc = crc32(&entry.contents);
        write_local_header(&mut package, name_len, size, crc);
        package.extend_from_slice(name);
        package.extend_from_slice(&entry.contents);
        write_central_header(&mut central, name_len, size, crc, offset);
        central.extend_from_slice(name);
    }
    let central_offset =
        u32::try_from(package.len()).map_err(|_| AasxExportError::PackageTooLarge)?;
    let central_size =
        u32::try_from(central.len()).map_err(|_| AasxExportError::PackageTooLarge)?;
    package.extend_from_slice(&central);
    push_u32(&mut package, 0x0605_4b50);
    push_u16(&mut package, 0);
    push_u16(&mut package, 0);
    push_u16(&mut package, entries.len() as u16);
    push_u16(&mut package, entries.len() as u16);
    push_u32(&mut package, central_size);
    push_u32(&mut package, central_offset);
    push_u16(&mut package, 0);
    if package.len() > MAX_AASX_PACKAGE_SIZE {
        return Err(AasxExportError::PackageTooLarge);
    }
    Ok(package)
}

fn write_local_header(target: &mut Vec<u8>, name_len: u16, size: u32, crc: u32) {
    push_u32(target, 0x0403_4b50);
    push_u16(target, 20);
    push_u16(target, 0);
    push_u16(target, 0);
    push_u16(target, 0);
    push_u16(target, 0);
    push_u32(target, crc);
    push_u32(target, size);
    push_u32(target, size);
    push_u16(target, name_len);
    push_u16(target, 0);
}

fn write_central_header(target: &mut Vec<u8>, name_len: u16, size: u32, crc: u32, offset: u32) {
    push_u32(target, 0x0201_4b50);
    push_u16(target, 20);
    push_u16(target, 20);
    push_u16(target, 0);
    push_u16(target, 0);
    push_u16(target, 0);
    push_u16(target, 0);
    push_u32(target, crc);
    push_u32(target, size);
    push_u32(target, size);
    push_u16(target, name_len);
    push_u16(target, 0);
    push_u16(target, 0);
    push_u16(target, 0);
    push_u16(target, 0);
    push_u32(target, 0);
    push_u32(target, offset);
}

fn find_eocd(package: &[u8]) -> Result<usize, AasxImportError> {
    if package.len() < 22 {
        return Err(unsupported("ZIP end record is missing"));
    }
    let start = package.len().saturating_sub(22 + usize::from(u16::MAX));
    for offset in (start..=package.len() - 22).rev() {
        if read_u32(package, offset)? == 0x0605_4b50
            && offset + 22 + usize::from(read_u16(package, offset + 20)?) == package.len()
        {
            return Ok(offset);
        }
    }
    Err(unsupported("ZIP end record is invalid"))
}

fn slice(bytes: &[u8], start: usize, end: usize) -> Result<&[u8], AasxImportError> {
    bytes
        .get(start..end)
        .ok_or_else(|| unsupported("truncated ZIP record"))
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, AasxImportError> {
    let value = slice(bytes, offset, offset.saturating_add(2))?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, AasxImportError> {
    let value = slice(bytes, offset, offset.saturating_add(4))?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

fn to_usize(value: u32) -> Result<usize, AasxImportError> {
    usize::try_from(value).map_err(|_| unsupported("ZIP range is too large"))
}

fn push_u16(target: &mut Vec<u8>, value: u16) {
    target.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(target: &mut Vec<u8>, value: u32) {
    target.extend_from_slice(&value.to_le_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn unsupported(message: &'static str) -> AasxImportError {
    AasxImportError::UnsupportedZip(message)
}

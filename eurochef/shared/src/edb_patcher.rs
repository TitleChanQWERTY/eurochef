use std::collections::HashMap;
use std::io::{Seek, SeekFrom};
use eurochef_edb::binrw::{BinReaderExt, Endian};
use eurochef_edb::edb::EdbFile;
use eurochef_edb::text::{EXGeoSpreadSheet, EXGeoTextItem};

pub fn patch_text_in_edb(
    file_data: &mut Vec<u8>,
    edb: &mut EdbFile,
    translations: &HashMap<(usize, u32), String>,
    target_set: Option<u32>,
    _spreadsheet_hash: Option<u32>,
) -> anyhow::Result<usize> {
    let endian = edb.endian;
    let mut patched_count = 0;

    let spreadsheet_list = edb.header.spreadsheet_list.clone();
    let refpointer_list = edb.header.refpointer_list.clone();

    for s in &spreadsheet_list {
        if s.stype != 1 { continue; }

        edb.seek(SeekFrom::Start(s.common.address as u64))?;
        let sheader = edb.read_type::<EXGeoSpreadSheet>(edb.endian)?;

        for (s_section_idx, s_section) in sheader.sections.iter().enumerate() {
            let should_patch_set = if let Some(ts) = target_set {
                (s_section.hashcode & 0xffff0000) == ts
            } else {
                true
            };

            if !should_patch_set { continue; }

            let refpointer = &refpointer_list[s_section.refpointer_index as usize];
            edb.seek(SeekFrom::Start(refpointer.address as u64 + 4))?;
            let text_count = edb.read_type::<u32>(edb.endian)?;

            for _ in 0..text_count {
                let item = edb.read_type::<EXGeoTextItem>(edb.endian)?;

                if let Some(new_text) = translations.get(&(s_section_idx, item.hashcode)) {
                    let is_null = item.string.offset_relative() == 0;
                    if is_null { continue; }

                    let original_addr = item.string.offset_absolute() as usize;
                    let original_text = item.string.to_string();
                    
                    // Available space in bytes (including null terminator)
                    let available_space = (original_text.encode_utf16().count() + 1) * 2;

                    // Prepare new UTF-16 bytes
                    let mut utf16_bytes = vec![];
                    for wchar in new_text.encode_utf16() {
                        match endian {
                            Endian::Little => utf16_bytes.extend_from_slice(&wchar.to_le_bytes()),
                            Endian::Big => utf16_bytes.extend_from_slice(&wchar.to_be_bytes()),
                        }
                    }
                    // Add null terminator
                    match endian {
                        Endian::Little => utf16_bytes.extend_from_slice(&0u16.to_le_bytes()),
                        Endian::Big => utf16_bytes.extend_from_slice(&0u16.to_be_bytes()),
                    }

                    // TRUNCATE if it doesn't fit
                    let write_len = utf16_bytes.len().min(available_space);
                    
                    // Write directly into the original file data
                    file_data[original_addr..original_addr + write_len].copy_from_slice(&utf16_bytes[..write_len]);
                    
                    // Ensure the last two bytes of available space are null if we truncated or reached exactly the end
                    if write_len >= available_space {
                        file_data[original_addr + available_space - 2] = 0;
                        file_data[original_addr + available_space - 1] = 0;
                    }

                    patched_count += 1;
                }
            }
        }
    }

    Ok(patched_count)
}

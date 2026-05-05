use std::io::{Cursor, Seek, SeekFrom};
use anyhow::Context;

use eurochef_edb::binrw::{BinReaderExt, Endian};
use eurochef_edb::{edb::EdbFile, versions::Platform};
use eurochef_edb::text::{EXGeoSpreadSheet, EXGeoTextItem};

pub fn execute_patch_text(
    filename: String,
    csv_file: String,
    output_filename: Option<String>,
) -> anyhow::Result<()> {
    let mut file_data = std::fs::read(&filename).context("Failed to read EDB file")?;
    
    let mut translations = std::collections::HashMap::new();
    let csv_content = std::fs::read_to_string(csv_file).context("Failed to read CSV file")?;

    for line in csv_content.lines().skip(1) {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.splitn(4, ',').collect();
        if parts.len() < 4 {
            continue;
        }

        let section_hash = u32::from_str_radix(parts[0], 16).unwrap_or(0);
        let item_hash = u32::from_str_radix(parts[1], 16).unwrap_or(0);
        let mut text = parts[3].trim();
        
        if text.starts_with('"') && text.ends_with('"') {
            text = &text[1..text.len() - 1];
        }

        let processed_text = text.replace("\"\"", "\"").replace("\\n", "\n");
        translations.insert((section_hash, item_hash), processed_text);
    }

    info!("Loaded {} translations from CSV", translations.len());

    let mut patch_actions = vec![];
    let endian;
    let original_base_size;
    let original_size;
    {
        // Use a clone for reading the structure to avoid lifetime issues with Box<dyn Trait + 'static>
        let reader = Cursor::new(file_data.clone());
        let mut edb = EdbFile::new(Box::new(reader), Platform::Pc)?;
        endian = edb.endian;
        original_size = edb.header.file_size;
        original_base_size = edb.header.base_file_size;

        let header = edb.header.clone();
        for s in &header.spreadsheet_list {
            if s.stype != 1 { continue; }

            edb.seek(SeekFrom::Start(s.common.address as u64))?;
            let sheader = edb.read_type::<EXGeoSpreadSheet>(edb.endian)?;

            for s_section in sheader.sections {
                let refpointer = &edb.header.refpointer_list[s_section.refpointer_index as usize];
                edb.seek(SeekFrom::Start(refpointer.address as u64 + 4))?;
                let text_count = edb.read_type::<u32>(edb.endian)?;

                for _i in 0..text_count {
                    let item_pos = edb.stream_position()?;
                    let item = edb.read_type::<EXGeoTextItem>(edb.endian)?;

                    if let Some(new_text) = translations.get(&(s_section.hashcode, item.hashcode)) {
                        let current_text = item.string.to_string();
                        if &current_text.trim() != new_text {
                            let addr = item.string.offset_absolute();
                            let original_len = current_text.encode_utf16().count();
                            let mut extra_space = 0;
                            let mut check_pos = (addr + (original_len + 1) as u64 * 2) as usize;
                            // Scan for trailing nulls to see if we can fit a longer string in-place
                            while check_pos + 1 < file_data.len() && file_data[check_pos] == 0 && file_data[check_pos+1] == 0 {
                                extra_space += 1;
                                check_pos += 2;
                                if extra_space > 256 { break; } 
                            }
                            patch_actions.push((item_pos + 4, new_text.clone(), addr, original_len + extra_space));
                        }
                    }
                }
            }
        }
    }

    // Align to 4 bytes before appending
    while file_data.len() % 4 != 0 {
        file_data.push(0);
    }

    let mut string_offsets = std::collections::HashMap::new();
    let mut strings_patched = 0;
    for (ptr_pos, new_text, original_addr, original_space) in patch_actions {
        let new_len = new_text.encode_utf16().count();
        let string_address = if let Some(&addr) = string_offsets.get(&new_text) {
            addr
        } else if new_len <= original_space {
            // In-place replacement
            let mut utf16_text: Vec<u16> = new_text.encode_utf16().collect();
            while utf16_text.len() <= original_space {
                utf16_text.push(0);
            }
            for (i, &wchar) in utf16_text.iter().enumerate() {
                let bytes = match endian {
                    Endian::Little => wchar.to_le_bytes(),
                    Endian::Big => wchar.to_be_bytes(),
                };
                let start = (original_addr + i as u64 * 2) as usize;
                file_data[start..start + 2].copy_from_slice(&bytes);
            }
            string_offsets.insert(new_text.clone(), original_addr);
            original_addr
        } else {
            // Append to end
            while file_data.len() % 16 != 0 {
                file_data.push(0);
            }
            let addr = file_data.len() as u64;
            let utf16_text: Vec<u16> = new_text.encode_utf16().chain(std::iter::once(0)).collect();
            for &wchar in &utf16_text {
                match endian {
                    Endian::Little => file_data.extend_from_slice(&wchar.to_le_bytes()),
                    Endian::Big => file_data.extend_from_slice(&wchar.to_be_bytes()),
                }
            }
            string_offsets.insert(new_text.clone(), addr);
            addr
        };

        let relative_offset = (string_address as i64 - ptr_pos as i64) as i32;
        let offset_bytes = match endian {
            Endian::Little => relative_offset.to_le_bytes(),
            Endian::Big => relative_offset.to_be_bytes(),
        };

        file_data[ptr_pos as usize..ptr_pos as usize + 4].copy_from_slice(&offset_bytes);
        strings_patched += 1;
    }

    // Align the whole file to 2048 bytes (sector size)
    while file_data.len() % 2048 != 0 {
        file_data.push(0);
    }

    let final_size = file_data.len() as u32;
    let size_bytes = match endian {
        Endian::Little => final_size.to_le_bytes(),
        Endian::Big => final_size.to_be_bytes(),
    };

    file_data[0x14..0x18].copy_from_slice(&size_bytes);
    file_data[0x18..0x1c].copy_from_slice(&size_bytes);

    let output_path = output_filename.unwrap_or(filename);
    std::fs::write(&output_path, file_data).context("Failed to write patched EDB file")?;

    info!("Successfully patched {} strings. New file size: {} bytes", strings_patched, final_size);
    info!("Patched file saved to {}", output_path);

    Ok(())
}

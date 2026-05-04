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
    {
        // Use a clone for reading the structure to avoid lifetime issues with Box<dyn Trait + 'static>
        let reader = Cursor::new(file_data.clone());
        let mut edb = EdbFile::new(Box::new(reader), Platform::Pc)?;
        endian = edb.endian;

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
                        patch_actions.push((item_pos + 4, new_text.clone()));
                    }
                }
            }
        }
    }

    let mut strings_patched = 0;
    for (ptr_pos, new_text) in patch_actions {
        let utf16_text: Vec<u16> = new_text.encode_utf16().chain(std::iter::once(0)).collect();
        let string_address = file_data.len() as u64;

        for &wchar in &utf16_text {
            match endian {
                Endian::Little => file_data.extend_from_slice(&wchar.to_le_bytes()),
                Endian::Big => file_data.extend_from_slice(&wchar.to_be_bytes()),
            }
        }

        let relative_offset = (string_address as i64 - ptr_pos as i64) as i32;
        let offset_bytes = match endian {
            Endian::Little => relative_offset.to_le_bytes(),
            Endian::Big => relative_offset.to_be_bytes(),
        };

        file_data[ptr_pos as usize..ptr_pos as usize + 4].copy_from_slice(&offset_bytes);
        strings_patched += 1;
    }

    let final_size = file_data.len() as u32;
    let size_bytes = match endian {
        Endian::Little => final_size.to_le_bytes(),
        Endian::Big => final_size.to_be_bytes(),
    };

    file_data[0x10..0x14].copy_from_slice(&size_bytes);
    file_data[0x14..0x18].copy_from_slice(&size_bytes);

    let output_path = output_filename.unwrap_or(filename);
    std::fs::write(&output_path, file_data).context("Failed to write patched EDB file")?;

    info!("Successfully patched {} strings. New file size: {} bytes", strings_patched, final_size);
    info!("Patched file saved to {}", output_path);

    Ok(())
}

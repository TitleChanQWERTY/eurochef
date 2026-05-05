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

    struct PatchAction {
        string_ptr_pos: u64,
        new_text: String,
        string_addr: u64,
        original_char_count: usize,
    }

    let mut patch_actions: Vec<PatchAction> = vec![];
    let endian;
    {
        let reader = Cursor::new(file_data.clone());
        let mut edb = EdbFile::new(Box::new(reader), Platform::Pc)?;
        endian = edb.endian;

        let header = edb.header.clone();
        for s in &header.spreadsheet_list {
            if s.stype != 1 {
                continue;
            }

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
                        if current_text.trim() != new_text.as_str() {
                            patch_actions.push(PatchAction {
                                string_ptr_pos: item_pos + 4,
                                new_text: new_text.clone(),
                                string_addr: item.string.offset_absolute(),
                                original_char_count: current_text.encode_utf16().count(),
                            });
                        }
                    }
                }
            }
        }
    }

    let mut strings_patched = 0;
    let mut strings_truncated = 0;

    for action in patch_actions {
        let new_chars: Vec<u16> = action.new_text.encode_utf16().collect();
        let new_char_count = new_chars.len();

        let (chars_to_write, was_truncated) = if new_char_count <= action.original_char_count {
            (new_chars, false)
        } else {
            warn!(
                "String at 0x{:x} too long ({} > {}), truncating",
                action.string_addr, new_char_count, action.original_char_count
            );
            (new_chars[..action.original_char_count].to_vec(), true)
        };

        let total_slots = action.original_char_count + 1;
        let mut utf16_bytes: Vec<u8> = Vec::with_capacity(total_slots * 2);

        for wchar in &chars_to_write {
            match endian {
                Endian::Little => utf16_bytes.extend_from_slice(&wchar.to_le_bytes()),
                Endian::Big => utf16_bytes.extend_from_slice(&wchar.to_be_bytes()),
            }
        }
        for _ in chars_to_write.len()..total_slots {
            utf16_bytes.push(0);
            utf16_bytes.push(0);
        }

        let start = action.string_addr as usize;
        let end = start + utf16_bytes.len();
        if end > file_data.len() {
            warn!("String at 0x{:x} out of bounds, skipping", action.string_addr);
            continue;
        }
        file_data[start..end].copy_from_slice(&utf16_bytes);

        let relative_offset = (action.string_addr as i64 - action.string_ptr_pos as i64) as i32;
        let offset_bytes = match endian {
            Endian::Little => relative_offset.to_le_bytes(),
            Endian::Big => relative_offset.to_be_bytes(),
        };
        let ptr_start = action.string_ptr_pos as usize;
        file_data[ptr_start..ptr_start + 4].copy_from_slice(&offset_bytes);

        strings_patched += 1;
        if was_truncated {
            strings_truncated += 1;
        }
    }

    let output_path = output_filename.unwrap_or(filename);
    std::fs::write(&output_path, &file_data).context("Failed to write patched EDB file")?;

    info!(
        "Patched {} strings ({} truncated). File saved to {}",
        strings_patched, strings_truncated, output_path
    );

    Ok(())
}

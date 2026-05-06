use std::io::{Cursor, Seek, SeekFrom};
use anyhow::Context;

use eurochef_edb::binrw::{BinReaderExt, Endian};
use eurochef_edb::{edb::EdbFile, versions::Platform};
use eurochef_edb::text::{EXGeoSpreadSheet, EXGeoTextItem};

pub fn execute_patch_text(
    filename: String,
    csv_file: String,
    output_filename: Option<String>,
    target_set: Option<u32>,
) -> anyhow::Result<()> {
    let mut file_data = std::fs::read(&filename).context("Failed to read EDB file")?;

    let mut translations = std::collections::HashMap::new();
    let csv_content = std::fs::read_to_string(csv_file).context("Failed to read CSV file")?;

    for (i, line) in csv_content.lines().skip(1).enumerate() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.splitn(4, ',').collect();
        if parts.len() < 4 {
            continue;
        }

        let section_index = parts[0].parse::<usize>().unwrap_or(i);
        let item_hash = u32::from_str_radix(parts[1], 16).unwrap_or(0);
        let mut text = parts[3].trim();

        if text.starts_with('"') && text.ends_with('"') {
            text = &text[1..text.len() - 1];
        }

        let processed_text = text.replace("\"\"", "\"").replace("\\n", "\n");
        translations.insert((section_index, item_hash), processed_text);
    }

    info!("Loaded {} translations from CSV", translations.len());

    #[derive(Debug)]
    struct TextItemInfo {
        ptr_pos: u64,
        original_addr: u64,
        original_bytes: usize,
        final_text: String,
        is_null: bool,
        was_patched: bool,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    struct Region {
        start: u64,
        end: u64,
    }

    let mut all_text_items: Vec<TextItemInfo> = vec![];
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

            for (s_idx, s_section) in sheader.sections.iter().enumerate() {
                let refpointer = &edb.header.refpointer_list[s_section.refpointer_index as usize];
                edb.seek(SeekFrom::Start(refpointer.address as u64 + 4))?;
                let text_count = edb.read_type::<u32>(edb.endian)?;

                for _i in 0..text_count {
                    let item_pos = edb.stream_position()?;
                    let item = edb.read_type::<EXGeoTextItem>(edb.endian)?;

                    let is_null = item.string.offset_relative() == 0;
                    let current_text = if is_null { String::new() } else { item.string.to_string() };
                    let original_bytes = if is_null { 0 } else { (current_text.encode_utf16().count() + 1) * 2 };
                    
                    let mut was_patched = false;
                    let final_text = if let Some(target_set) = target_set {
                        if (s_section.hashcode & 0xffff0000) == target_set {
                            match translations.get(&(s_idx, item.hashcode)) {
                                Some(new_text) => {
                                    if current_text.trim() != new_text.as_str() {
                                        was_patched = true;
                                        new_text.clone()
                                    } else {
                                        current_text
                                    }
                                },
                                None => current_text,
                            }
                        } else {
                            if !is_null {
                                was_patched = true;
                                String::new()
                            } else {
                                String::new()
                            }
                        }
                    } else {
                        match translations.get(&(s_idx, item.hashcode)) {
                            Some(new_text) => {
                                if current_text.trim() != new_text.as_str() {
                                    was_patched = true;
                                    new_text.clone()
                                } else {
                                    current_text
                                }
                            },
                            None => current_text,
                        }
                    };

                    all_text_items.push(TextItemInfo {
                        ptr_pos: item_pos + 4,
                        original_addr: if is_null { 0 } else { item.string.offset_absolute() },
                        original_bytes,
                        final_text,
                        is_null,
                        was_patched,
                    });
                }
            }
        }
    }

    let mut regions: Vec<Region> = all_text_items.iter()
        .filter(|item| !item.is_null)
        .map(|item| Region {
            start: item.original_addr,
            end: item.original_addr + item.original_bytes as u64,
        }).collect();

    regions.sort_by_key(|r| r.start);
    let mut merged_regions: Vec<Region> = vec![];
    for r in regions {
        if let Some(last) = merged_regions.last_mut() {
            if r.start <= last.end {
                if r.end > last.end {
                    last.end = r.end;
                }
                continue;
            }
        }
        merged_regions.push(r);
    }

    let mut final_merged_regions = merged_regions;

    let total_original_space: u64 = final_merged_regions.iter().map(|r| r.end - r.start).sum();
    info!("Total available string space reclaimed: {} bytes", total_original_space);

    let mut string_pool = std::collections::HashMap::new();

    for region in &final_merged_regions {
        let start = region.start as usize;
        let end = region.end as usize;
        if end <= file_data.len() {
            for b in file_data[start..end].iter_mut() {
                *b = 0;
            }
        }
    }

    let mut unique_strings: Vec<String> = vec![];
    let mut seen = std::collections::HashSet::new();
    for item in &all_text_items {
        if !item.is_null {
            if seen.insert(item.final_text.clone()) {
                unique_strings.push(item.final_text.clone());
            }
        }
    }

    for text_to_write in unique_strings {
        let new_chars: Vec<u16> = text_to_write.encode_utf16().collect();
        let mut needed_bytes = (new_chars.len() + 1) * 2;
        if needed_bytes % 4 != 0 {
            needed_bytes += 2;
        }
        
        let mut allocated = None;
        for region in &mut final_merged_regions {
            let start_aligned = (region.start + 3) & !3;
            if region.end >= start_aligned + needed_bytes as u64 {
                allocated = Some(start_aligned);
                region.start = start_aligned + needed_bytes as u64; 
                break;
            }
        }
        
        if let Some(alloc_addr) = allocated {
            string_pool.insert(text_to_write.clone(), alloc_addr);
            
            let mut utf16_bytes = Vec::with_capacity(needed_bytes);
            for wchar in &new_chars {
                match endian {
                    Endian::Little => utf16_bytes.extend_from_slice(&wchar.to_le_bytes()),
                    Endian::Big => utf16_bytes.extend_from_slice(&wchar.to_be_bytes()),
                }
            }
            utf16_bytes.push(0);
            utf16_bytes.push(0);
            while utf16_bytes.len() < needed_bytes {
                utf16_bytes.push(0);
            }
            
            let start = alloc_addr as usize;
            let end = start + needed_bytes;
            if end <= file_data.len() {
                file_data[start..end].copy_from_slice(&utf16_bytes);
            }
        } else {
            let alloc_addr = (file_data.len() as u64 + 3) & !3;
            let padding_needed = (alloc_addr as usize).saturating_sub(file_data.len());
            if padding_needed > 0 {
                file_data.extend(std::iter::repeat(0).take(padding_needed));
            }

            string_pool.insert(text_to_write.clone(), alloc_addr);
            
            let mut utf16_bytes = Vec::with_capacity(needed_bytes);
            for wchar in &new_chars {
                match endian {
                    Endian::Little => utf16_bytes.extend_from_slice(&wchar.to_le_bytes()),
                    Endian::Big => utf16_bytes.extend_from_slice(&wchar.to_be_bytes()),
                }
            }
            utf16_bytes.push(0);
            utf16_bytes.push(0);
            while utf16_bytes.len() < needed_bytes {
                utf16_bytes.push(0);
            }
            
            file_data.extend_from_slice(&utf16_bytes);
        }
    }

    let new_size = file_data.len() as u32;
    let size_bytes = match endian {
        Endian::Little => new_size.to_le_bytes(),
        Endian::Big => new_size.to_be_bytes(),
    };
    file_data[20..24].copy_from_slice(&size_bytes);
    file_data[24..28].copy_from_slice(&size_bytes); // Also update base_file_size

    for item in &all_text_items {
        if !item.is_null {
            if let Some(&addr) = string_pool.get(&item.final_text) {
                let relative_offset = (addr as i64 - item.ptr_pos as i64) as i32;
                let offset_bytes = match endian {
                    Endian::Little => relative_offset.to_le_bytes(),
                    Endian::Big => relative_offset.to_be_bytes(),
                };
                let ptr_start = item.ptr_pos as usize;
                file_data[ptr_start..ptr_start + 4].copy_from_slice(&offset_bytes);
            }
        }
    }

    let output_path = output_filename.unwrap_or(filename);
    std::fs::write(&output_path, &file_data).context("Failed to write patched EDB file")?;

    info!(
        "Patched {} unique strings. New file size: {} bytes. File saved to {}",
        string_pool.len(), new_size, output_path
    );

    Ok(())
}

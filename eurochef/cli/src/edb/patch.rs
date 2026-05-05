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

    #[derive(Debug)]
    struct TextItemInfo {
        ptr_pos: u64,
        original_addr: u64,
        original_bytes: usize,
        final_text: String,
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

            for s_section in sheader.sections {
                let refpointer = &edb.header.refpointer_list[s_section.refpointer_index as usize];
                edb.seek(SeekFrom::Start(refpointer.address as u64 + 4))?;
                let text_count = edb.read_type::<u32>(edb.endian)?;

                for _i in 0..text_count {
                    let item_pos = edb.stream_position()?;
                    let item = edb.read_type::<EXGeoTextItem>(edb.endian)?;

                    let current_text = item.string.to_string();
                    let original_bytes = (current_text.encode_utf16().count() + 1) * 2;
                    
                    let mut was_patched = false;
                    let final_text = match translations.get(&(s_section.hashcode, item.hashcode)) {
                        Some(new_text) => {
                            if current_text.trim() != new_text.as_str() {
                                was_patched = true;
                                new_text.clone()
                            } else {
                                current_text
                            }
                        },
                        None => current_text,
                    };

                    all_text_items.push(TextItemInfo {
                        ptr_pos: item_pos + 4,
                        original_addr: item.string.offset_absolute(),
                        original_bytes,
                        final_text,
                        was_patched,
                    });
                }
            }
        }
    }

    // Gather all memory regions occupied by strings
    let mut regions: Vec<Region> = all_text_items.iter().map(|item| Region {
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

    let total_original_space: u64 = merged_regions.iter().map(|r| r.end - r.start).sum();
    info!("Total available string space reclaimed: {} bytes", total_original_space);

    let mut string_pool = std::collections::HashMap::new();
    let mut strings_patched = 0;
    let mut strings_truncated = 0;

    // Clear all merged regions with zero to avoid garbage data
    for region in &merged_regions {
        let start = region.start as usize;
        let end = region.end as usize;
        if end <= file_data.len() {
            for b in file_data[start..end].iter_mut() {
                *b = 0;
            }
        }
    }

    // To maximize bin packing efficiency, we can sort the text items we need to allocate by length descending
    // However, since we just iterate all_text_items, we can collect unique strings first.
    let mut unique_strings: Vec<String> = vec![];
    let mut seen = std::collections::HashSet::new();
    for item in &all_text_items {
        if seen.insert(item.final_text.clone()) {
            unique_strings.push(item.final_text.clone());
        }
    }

    // Pre-allocate strings
    for text_to_write in unique_strings {
        let new_chars: Vec<u16> = text_to_write.encode_utf16().collect();
        let needed_bytes = (new_chars.len() + 1) * 2;
        
        let mut allocated = None;
        for region in &mut merged_regions {
            if region.end >= region.start + needed_bytes as u64 {
                allocated = Some(region.start);
                region.start += needed_bytes as u64; // shrink the available free space
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
            
            let start = alloc_addr as usize;
            let end = start + needed_bytes;
            if end <= file_data.len() {
                file_data[start..end].copy_from_slice(&utf16_bytes);
            }
        } else {
            // Find largest available region for fallback
            let mut largest_region_idx = 0;
            let mut max_space = 0;
            for (i, r) in merged_regions.iter().enumerate() {
                let space = r.end - r.start;
                if space > max_space {
                    max_space = space;
                    largest_region_idx = i;
                }
            }
            
            if max_space >= 2 {
                let region = &mut merged_regions[largest_region_idx];
                let alloc_addr = region.start;
                let max_chars = (max_space as usize / 2) - 1;
                
                warn!("String too long to fit anywhere! Truncating '{}' to {} chars", text_to_write, max_chars);
                strings_truncated += 1;
                
                let truncated_chars: Vec<u16> = new_chars.into_iter().take(max_chars).collect();
                let needed_bytes = (truncated_chars.len() + 1) * 2;
                
                region.start += needed_bytes as u64;
                string_pool.insert(text_to_write.clone(), alloc_addr);
                
                let mut utf16_bytes = Vec::with_capacity(needed_bytes);
                for wchar in &truncated_chars {
                    match endian {
                        Endian::Little => utf16_bytes.extend_from_slice(&wchar.to_le_bytes()),
                        Endian::Big => utf16_bytes.extend_from_slice(&wchar.to_be_bytes()),
                    }
                }
                utf16_bytes.push(0);
                utf16_bytes.push(0);
                
                let start = alloc_addr as usize;
                let end = start + needed_bytes;
                if end <= file_data.len() {
                    file_data[start..end].copy_from_slice(&utf16_bytes);
                }
            } else {
                warn!("CRITICAL: Out of string memory! Cannot fit '{}'", text_to_write);
                // Can't even fit a null char, very bad.
            }
        }
    }

    // Now update all pointers
    for item in &all_text_items {
        if let Some(&addr) = string_pool.get(&item.final_text) {
            let relative_offset = (addr as i64 - item.ptr_pos as i64) as i32;
            let offset_bytes = match endian {
                Endian::Little => relative_offset.to_le_bytes(),
                Endian::Big => relative_offset.to_be_bytes(),
            };
            let ptr_start = item.ptr_pos as usize;
            file_data[ptr_start..ptr_start + 4].copy_from_slice(&offset_bytes);
        }
        
        if item.was_patched {
            strings_patched += 1;
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

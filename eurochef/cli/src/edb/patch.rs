use std::io::{Cursor, Seek, SeekFrom};
use anyhow::Context;
use eurochef_edb::binrw::{BinReaderExt, Endian};
use eurochef_edb::{edb::EdbFile, versions::Platform};
use eurochef_edb::text::{EXGeoSpreadSheet, EXGeoTextItem};
use std::collections::{BTreeMap, HashMap};
use tracing::info;

pub fn execute_patch_text(
    filename: String,
    csv_file: String,
    output_filename: Option<String>,
    target_set: Option<u32>,
) -> anyhow::Result<()> {
    let mut file_data = std::fs::read(&filename).context("Failed to read EDB file")?;

    let mut translations = HashMap::new();
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

    struct TextItemRecord {
        ptr_pos: u64,
        original_addr: u64,
        final_text: String,
        is_null: bool,
    }

    let mut all_items = vec![];
    let endian;
    {
        let reader = Cursor::new(std::fs::read(&filename)?);
        let mut edb = EdbFile::new(Box::new(reader), Platform::Pc)?;
        endian = edb.endian;

        let spreadsheet_list = edb.header.spreadsheet_list.clone();
        let refpointer_list = edb.header.refpointer_list.clone();
        for s in &spreadsheet_list {
            if s.stype != 1 {
                continue;
            }

            edb.seek(SeekFrom::Start(s.common.address as u64))?;
            let sheader = edb.read_type::<EXGeoSpreadSheet>(edb.endian)?;

            let sections = sheader.sections.clone();
            for (s_section_idx, s_section) in sections.iter().enumerate() {
                let refpointer = &refpointer_list[s_section.refpointer_index as usize];
                edb.seek(SeekFrom::Start(refpointer.address as u64 + 4))?;
                let text_count = edb.read_type::<u32>(edb.endian)?;

                for _ in 0..text_count {
                    let item_pos = edb.stream_position()?;
                    let item = edb.read_type::<EXGeoTextItem>(edb.endian)?;

                    let is_null = item.string.offset_relative() == 0;
                    let current_text = if is_null { String::new() } else { item.string.to_string() };
                    
                    let mut final_text = current_text.clone();
                    if let Some(ts) = target_set {
                        if (s_section.hashcode & 0xffff0000) == ts {
                            if let Some(new_text) = translations.get(&(s_section_idx, item.hashcode)) {
                                final_text = new_text.clone();
                            }
                        }
                    } else {
                        if let Some(new_text) = translations.get(&(s_section_idx, item.hashcode)) {
                            final_text = new_text.clone();
                        }
                    }

                    all_items.push(TextItemRecord {
                        ptr_pos: item_pos + 4,
                        original_addr: if is_null { 0 } else { item.string.offset_absolute() },
                        final_text,
                        is_null,
                    });
                }
            }
        }
    }

    let mut string_info: BTreeMap<u64, (String, Vec<u64>)> = BTreeMap::new();
    for item in &all_items {
        if item.is_null { continue; }
        let entry = string_info.entry(item.original_addr).or_insert_with(|| (item.final_text.clone(), vec![]));
        entry.1.push(item.ptr_pos);
    }

    let mut regions = vec![];
    let addrs: Vec<u64> = string_info.keys().cloned().collect();
    for &addr in &addrs {
        let (text, _) = &string_info[&addr];
        let size = (text.encode_utf16().count() + 1) * 2;
        regions.push((addr, addr + size as u64));
    }
    regions.sort_by_key(|r| r.0);

    let mut merged: Vec<(u64, u64)> = vec![];
    for r in regions {
        if let Some(last) = merged.last_mut() {
            if r.0 <= last.1 {
                if r.1 > last.1 { last.1 = r.1; }
                continue;
            }
        }
        merged.push(r);
    }

    let mut final_string_map = HashMap::new();
    let mut relocated_strings = vec![];

    for (r_start, r_end) in merged {
        let mut strings_in_region: Vec<u64> = addrs.iter().cloned().filter(|&a| a >= r_start && a < r_end).collect();
        strings_in_region.sort();
        strings_in_region.dedup();

        for i in 0..strings_in_region.len() {
            let addr = strings_in_region[i];
            let next_addr = if i + 1 < strings_in_region.len() { strings_in_region[i+1] } else { r_end };
            let available = (next_addr - addr) as usize;
            
            let (new_text, _) = &string_info[&addr];
            let needed = (new_text.encode_utf16().count() + 1) * 2;

            if needed <= available {
                final_string_map.insert(addr, addr);
                let start = addr as usize;
                let mut utf16_bytes = vec![];
                for wchar in new_text.encode_utf16() {
                    match endian {
                        Endian::Little => utf16_bytes.extend_from_slice(&wchar.to_le_bytes()),
                        Endian::Big => utf16_bytes.extend_from_slice(&wchar.to_be_bytes()),
                    }
                }
                utf16_bytes.push(0);
                utf16_bytes.push(0);
                file_data[start..start + utf16_bytes.len()].copy_from_slice(&utf16_bytes);
                for b in &mut file_data[start + utf16_bytes.len()..next_addr as usize] {
                    *b = 0;
                }
            } else {
                relocated_strings.push(addr);
            }
        }
    }

    for addr in relocated_strings {
        let (new_text, _) = &string_info[&addr];
        let mut needed = (new_text.encode_utf16().count() + 1) * 2;
        if needed % 4 != 0 { needed += 2; }

        let alloc_addr = (file_data.len() as u64 + 3) & !3;
        let padding = (alloc_addr as usize).saturating_sub(file_data.len());
        file_data.extend(std::iter::repeat(0).take(padding));
        
        final_string_map.insert(addr, alloc_addr);
        
        let mut utf16_bytes = vec![];
        for wchar in new_text.encode_utf16() {
            match endian {
                Endian::Little => utf16_bytes.extend_from_slice(&wchar.to_le_bytes()),
                Endian::Big => utf16_bytes.extend_from_slice(&wchar.to_be_bytes()),
            }
        }
        utf16_bytes.push(0);
        utf16_bytes.push(0);
        while utf16_bytes.len() < needed { utf16_bytes.push(0); }
        file_data.extend_from_slice(&utf16_bytes);
    }

    let new_size = file_data.len() as u32;
    let sz_bytes = match endian {
        Endian::Little => new_size.to_le_bytes(),
        Endian::Big => new_size.to_be_bytes(),
    };
    file_data[20..24].copy_from_slice(&sz_bytes);
    file_data[24..28].copy_from_slice(&sz_bytes);

    for (orig_addr, (_, ptr_positions)) in &string_info {
        if let Some(&final_addr) = final_string_map.get(&orig_addr) {
            for &ptr_pos in ptr_positions {
                let rel = (final_addr as i64 - ptr_pos as i64) as i32;
                let rel_bytes = match endian {
                    Endian::Little => rel.to_le_bytes(),
                    Endian::Big => rel.to_be_bytes(),
                };
                let p = ptr_pos as usize;
                file_data[p..p+4].copy_from_slice(&rel_bytes);
            }
        }
    }

    let output_path = output_filename.unwrap_or(filename);
    std::fs::write(&output_path, &file_data)?;

    Ok(())
}


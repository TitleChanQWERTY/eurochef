use std::io::Cursor;
use anyhow::Context;
use eurochef_edb::{edb::EdbFile, versions::Platform};
use std::collections::HashMap;
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

    let mut edb = {
        let reader = Cursor::new(file_data.clone());
        EdbFile::new(Box::new(reader), Platform::Pc)?
    };

    let patched_count = eurochef_shared::edb_patcher::patch_text_in_edb(
        &mut file_data,
        &mut edb,
        &translations,
        target_set,
        None,
    )?;

    info!("Patched {} unique strings", patched_count);

    let output_path = output_filename.unwrap_or(filename);
    std::fs::write(&output_path, &file_data)?;

    Ok(())
}


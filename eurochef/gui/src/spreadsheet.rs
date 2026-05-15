use egui::FontSelection;
use eurochef_edb::Hashcode;
use eurochef_shared::spreadsheets::{UXGeoSpreadsheet, UXGeoTextItem};

#[derive(Clone)]
pub struct TextItemList {
    /// Search query for a specific hashcode
    search_hashcode: String,
    /// Search query for text item contents
    search_text: String,

    pub spreadsheets: Vec<(Hashcode, UXGeoSpreadsheet)>,
    selected_section: usize,
    filtered_indices: Option<Vec<Vec<usize>>>,
    last_edited_item: Option<(usize, usize)>,
    should_scroll: bool,
    modified_items: std::collections::HashSet<(usize, usize)>,
    search_only_modified: bool,
    search_section: String,
    reference_text: std::collections::HashMap<u32, String>,
}

impl TextItemList {
    pub fn new(spreadsheets: Vec<(Hashcode, UXGeoSpreadsheet)>) -> Self {
        Self {
            search_hashcode: String::new(),
            search_text: String::new(),
            spreadsheets,
            selected_section: 0,
            filtered_indices: None,
            last_edited_item: None,
            should_scroll: false,
            modified_items: std::collections::HashSet::new(),
            search_only_modified: false,
            search_section: String::new(),
            reference_text: std::collections::HashMap::new(),
        }
    }

    // TODO: Display separate spreadsheets
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let mut update_filter = self.filtered_indices.is_none();

        ui.horizontal(|ui| {
            ui.label("Search: ");
            if ui.text_edit_singleline(&mut self.search_text).changed() {
                update_filter = true;
                if self.search_text.is_empty() && self.search_hashcode.is_empty() {
                    self.should_scroll = true;
                }
            }
            ui.label("Hashcode: ");
            if egui::TextEdit::singleline(&mut self.search_hashcode)
                .font(FontSelection::Style(egui::TextStyle::Monospace))
                .desired_width(76.0)
                .show(ui)
                .response
                .changed()
            {
                update_filter = true;
            }

            ui.label("Section: ");
            if egui::TextEdit::singleline(&mut self.search_section)
                .font(FontSelection::Style(egui::TextStyle::Monospace))
                .desired_width(76.0)
                .show(ui)
                .response
                .changed()
            {
                update_filter = true;
            }

            if ui.checkbox(&mut self.search_only_modified, "Modified only").changed() {
                update_filter = true;
            }

            if ui.button("X").clicked() {
                self.search_text.clear();
                self.search_hashcode.clear();
                self.search_section.clear();
                self.search_only_modified = false;
                update_filter = true;
                self.should_scroll = true;
            }
            ui.separator();
            if ui.button("Export CSV").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("CSV", &["csv"])
                    .save_file()
                {
                    self.export_csv(path);
                }
            }
            if ui.button("Import CSV").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("CSV", &["csv"])
                    .pick_file()
                {
                    self.import_csv(path);
                    update_filter = true;
                }
            }
            if ui.button("Load Reference EDB").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("EngineX Database", &["edb"])
                    .pick_file()
                {
                    // Guess platform from current file or just use PC/PS2 as common ones
                    // For now, let's just try to read it with a few common platforms or the current one
                    // Actually, let's just use a simple guesser or ask? No, let's just use a helper.
                    self.load_reference(path);
                }
            }
            ui.separator();
            if ui.button("UA -> Victim").clicked() {
                self.convert_to_victim();
                update_filter = true;
            }
            if ui.button("Victim -> UA").clicked() {
                self.convert_from_victim();
                update_filter = true;
            }
        });

        ui.separator();

        let spreadsheet = self.spreadsheets.iter_mut().find(|(_, v)| match v {
            UXGeoSpreadsheet::Data(_) => false,
            UXGeoSpreadsheet::Text(_) => true,
        });

        if spreadsheet.is_none() {
            ui.heading("No text spreadsheets found");
            return;
        }

        let (_, spreadsheet) = spreadsheet.unwrap();

        let sections = match spreadsheet {
            UXGeoSpreadsheet::Text(v) => v,
            _ => unreachable!(),
        };

        if update_filter {
            self.filtered_indices = Some(
                sections
                    .iter()
                    .enumerate()
                    .map(|(si, s)| {
                        if !self.search_section.is_empty() && !format!("{:08x}", s.hashcode).contains(&self.search_section.to_lowercase()) {
                            return vec![];
                        }

                        s.entries
                            .iter()
                            .enumerate()
                            .filter(|(i, v)| {
                                if self.search_only_modified {
                                    self.modified_items.contains(&(si, *i))
                                } else {
                                    true
                                }
                            })
                            .filter(|(_, v)| {
                                if self.search_hashcode.is_empty() {
                                    true
                                } else {
                                    format!("{:08x}", v.hashcode)
                                        .contains(&self.search_hashcode.to_lowercase())
                                }
                            })
                            .filter(|(_, v)| {
                                v.text
                                    .to_lowercase()
                                    .contains(&self.search_text.to_lowercase())
                            })
                            .map(|(i, _)| i)
                            .collect()
                    })
                    .collect(),
            );
        }

        let section_hashes: Vec<u32> = sections.iter().map(|s| s.hashcode).collect();
        let filtered_indices = self.filtered_indices.as_ref().unwrap();

        let mut edited_item = None;
        let current_section = self.selected_section;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                egui::ScrollArea::vertical()
                    .id_source("section_scroll_area")
                    .show(ui, |ui| {
                        let mut current_set = 0;
                        for (i, hashcode) in section_hashes.iter().enumerate() {
                            if filtered_indices[i].is_empty() {
                                continue;
                            }

                            if (hashcode & 0xffff0000) != current_set && *hashcode != u32::MAX {
                                ui.label(format!("Set {:08x}", hashcode & 0xffff0000));
                            }

                            if *hashcode != u32::MAX {
                                current_set = hashcode & 0xffff0000;
                            }

                            ui.selectable_value(
                                &mut self.selected_section,
                                i,
                                format!("  Section {:08x}", hashcode),
                            );
                        }
                    });
            });

            ui.vertical(|ui| {
                let text_height = egui::TextStyle::Body.resolve(ui.style()).size * 1.25;
                
                egui::ScrollArea::horizontal().show(ui, |ui| {
                    let mut table = egui_extras::TableBuilder::new(ui)
                        .striped(true)
                        .column(egui_extras::Column::initial(90.0).resizable(true).clip(true))
                        .column(egui_extras::Column::initial(90.0).resizable(true).clip(true));
                    
                    if !self.reference_text.is_empty() {
                        table = table.column(egui_extras::Column::initial(300.0).resizable(true).clip(true));
                    }

                    table = table.column(egui_extras::Column::remainder().at_least(300.0).resizable(true).clip(true));

                    if self.should_scroll {
                        if let Some((section_idx, item_idx)) = self.last_edited_item {
                            self.selected_section = section_idx;
                            table = table.scroll_to_row(item_idx, Some(egui::Align::Center));
                        }
                        self.should_scroll = false;
                    }

                    table
                        .header(20., |mut header| {
                        header.col(|ui| {
                            ui.strong("Hashcode");
                        });
                        header.col(|ui| {
                            ui.strong("Sound");
                        });
                        if !self.reference_text.is_empty() {
                            header.col(|ui| {
                                ui.strong("Reference");
                            });
                        }
                        header.col(|ui| {
                            ui.strong("Text");
                        });
                    })
                    .body(|body| {
                        let section_indices = &filtered_indices[self.selected_section];
                        let section_items = &mut sections[self.selected_section].entries;
                        let num_rows = section_indices.len();
                        body.rows(text_height, num_rows, |row_index, mut row| {
                            let item_index = section_indices[row_index];
                            let item = &mut section_items[item_index];
                            let item_hashcode = item.hashcode;
                            let item_text = item.text.clone();
                            let context_menu = move |ui: &mut egui::Ui| {
                                if ui.button("Copy hashcode").clicked() {
                                    ui.output_mut(|o| o.copied_text = format!("{:08x}", item_hashcode));
                                    ui.close_menu()
                                }
                                if ui.button("Copy text").clicked() {
                                    ui.output_mut(|o| o.copied_text = item_text);
                                    ui.close_menu()
                                }
                            };

                            row.col(|ui| {
                                ui.label(format!("0x{:x}", item.hashcode));
                            })
                            .1
                            .context_menu(context_menu.clone());

                            row.col(|ui| {
                                if item.sound_hashcode == u32::MAX {
                                    ui.label("none");
                                } else {
                                    ui.label(format!("0x{:x}", item.sound_hashcode));
                                }
                            })
                            .1
                            .context_menu(context_menu.clone());

                            if !self.reference_text.is_empty() {
                                row.col(|ui| {
                                    let ref_text = self.reference_text.get(&item.hashcode).cloned().unwrap_or_default();
                                    ui.label(egui::RichText::new(ref_text).italics().color(egui::Color32::GRAY));
                                });
                            }

                            row.col(|ui| {
                                let is_modified = self.modified_items.contains(&(current_section, item_index));
                                let mut text_color = None;
                                if is_modified {
                                    text_color = Some(egui::Color32::from_rgb(255, 255, 100));
                                }

                                let text_edit = egui::TextEdit::multiline(&mut item.text)
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(1)
                                    .text_color_opt(text_color);

                                if ui.add(text_edit).changed() {
                                    edited_item = Some((current_section, item_index));
                                    self.modified_items.insert((current_section, item_index));
                                }
                            })
                            .1
                            .context_menu(context_menu);
                        });
                    });
                });
            });

            if let Some(item) = edited_item {
                self.last_edited_item = Some(item);
            }
        });
    }

    pub fn export_csv(&self, path: std::path::PathBuf) {
        let spreadsheet = self.spreadsheets.iter().find(|(_, v)| match v {
            UXGeoSpreadsheet::Data(_) => false,
            UXGeoSpreadsheet::Text(_) => true,
        });
        if let Some((_, UXGeoSpreadsheet::Text(sections))) = spreadsheet {
            if let Ok(mut w) = std::fs::File::create(path) {
                use std::io::Write;
                writeln!(w, "Section,Hashcode,Sound,Text").ok();
                for (section_idx, section) in sections.iter().enumerate() {
                    for item in &section.entries {
                        let escaped_text = item.text.replace('"', "\"\"").replace('\n', "\\n");
                        writeln!(w, "{},{:08x},{:08x},\"{}\"", section_idx, item.hashcode, item.sound_hashcode, escaped_text).ok();
                    }
                }
                tracing::info!("Exported texts to CSV");
            }
        }
    }

    pub fn import_csv(&mut self, path: std::path::PathBuf) {
        let spreadsheet = self.spreadsheets.iter_mut().find(|(_, v)| match v {
            UXGeoSpreadsheet::Data(_) => false,
            UXGeoSpreadsheet::Text(_) => true,
        });
        if let Some((_, UXGeoSpreadsheet::Text(sections))) = spreadsheet {
            if let Ok(content) = std::fs::read_to_string(path) {
                let mut lines = content.lines();
                lines.next();
                let mut imported = 0;
                for line in lines {
                    if line.is_empty() { continue; }
                    let mut parts = line.splitn(4, ',');
                    if let (Some(section_idx), Some(hashcode_str), Some(_sound), Some(text_part)) = (parts.next(), parts.next(), parts.next(), parts.next()) {
                        if let Ok(section_idx) = section_idx.parse::<usize>() {
                            if let Ok(hashcode) = u32::from_str_radix(hashcode_str, 16) {
                                let unescaped = text_part.trim_matches('"').replace("\"\"", "\"").replace("\\n", "\n");
                                if let Some(section) = sections.get_mut(section_idx) {
                                    if let Some((i, item)) = section.entries.iter_mut().enumerate().find(|(_, e)| e.hashcode == hashcode) {
                                        if item.text != unescaped {
                                            item.text = unescaped;
                                            self.modified_items.insert((section_idx, i));
                                            imported += 1;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                tracing::info!("Imported {} updated texts from CSV", imported);
            }
        }
    }

    pub fn load_reference(&mut self, path: std::path::PathBuf) {
        use eurochef_edb::edb::EdbFile;
        use eurochef_edb::versions::Platform;
        use std::io::BufReader;

        let platform = Platform::from_path(&path).unwrap_or(Platform::Pc);
        if let Ok(f) = std::fs::File::open(path) {
            let reader = BufReader::new(f);
            if let Ok(mut edb) = EdbFile::new(Box::new(reader), platform) {
                if let Ok(spreadsheets) = UXGeoSpreadsheet::read_all(&mut edb) {
                    self.reference_text.clear();
                    for (_, s) in spreadsheets {
                        if let UXGeoSpreadsheet::Text(sections) = s {
                            for section in sections {
                                for entry in section.entries {
                                    self.reference_text.insert(entry.hashcode, entry.text);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn convert_to_victim(&mut self) {
        let spreadsheet = self.spreadsheets.iter_mut().find(|(_, v)| match v {
            UXGeoSpreadsheet::Data(_) => false,
            UXGeoSpreadsheet::Text(_) => true,
        });
        if let Some((_, UXGeoSpreadsheet::Text(sections))) = spreadsheet {
            for (si, section) in sections.iter_mut().enumerate() {
                for (ii, item) in section.entries.iter_mut().enumerate() {
                    let mut new_text = item.text.clone();
                    new_text = new_text.replace('є', "ъ");
                    new_text = new_text.replace('Є', "Ъ");
                    new_text = new_text.replace('ї', "э");
                    new_text = new_text.replace('Ї', "Э");
                    new_text = new_text.replace('і', "i");
                    new_text = new_text.replace('І', "I");
                    new_text = new_text.replace('ґ', "ы");
                    new_text = new_text.replace('Ґ', "Ы");
                    if new_text != item.text {
                        item.text = new_text;
                        self.modified_items.insert((si, ii));
                    }
                }
            }
        }
    }

    pub fn convert_from_victim(&mut self) {
        let spreadsheet = self.spreadsheets.iter_mut().find(|(_, v)| match v {
            UXGeoSpreadsheet::Data(_) => false,
            UXGeoSpreadsheet::Text(_) => true,
        });
        if let Some((_, UXGeoSpreadsheet::Text(sections))) = spreadsheet {
            for (si, section) in sections.iter_mut().enumerate() {
                for (ii, item) in section.entries.iter_mut().enumerate() {
                    let mut new_text = item.text.clone();
                    new_text = new_text.replace('ъ', "є");
                    new_text = new_text.replace('Ъ', "Є");
                    new_text = new_text.replace('э', "ї");
                    new_text = new_text.replace('Э', "Ї");
                    new_text = new_text.replace('i', "і");
                    new_text = new_text.replace('I', "І");
                    new_text = new_text.replace('ы', "ґ");
                    new_text = new_text.replace('Ы', "Ґ");
                    if new_text != item.text {
                        item.text = new_text;
                        self.modified_items.insert((si, ii));
                    }
                }
            }
        }
    }
}

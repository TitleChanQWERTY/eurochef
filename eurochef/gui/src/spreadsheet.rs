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
                if self.search_text.is_empty() && self.search_hashcode.is_empty() {
                    self.should_scroll = true;
                }
            }
            if ui.button("X").clicked() {
                self.search_text.clear();
                self.search_hashcode.clear();
                update_filter = true;
                self.should_scroll = true;
            }
            ui.separator();
            if ui.button("Export CSV").clicked() {
                let this = self.clone();
                std::thread::spawn(move || {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("CSV", &["csv"])
                        .save_file()
                    {
                        this.export_csv(path);
                    }
                });
            }
            if ui.button("Import CSV").clicked() {
                // For import we might need a channel or state update, but for now 
                // let's at least prevent the crash. Note: UI won't update immediately.
                let mut this = self.clone();
                std::thread::spawn(move || {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("CSV", &["csv"])
                        .pick_file()
                    {
                        this.import_csv(path);
                    }
                });
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
                    .map(|s| {
                        s.entries
                            .iter()
                            .enumerate()
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
                        .column(egui_extras::Column::initial(90.0).resizable(true).clip(true))
                        .column(egui_extras::Column::remainder().at_least(300.0).resizable(true).clip(true));

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

                            row.col(|ui| {
                                let text_edit = egui::TextEdit::singleline(&mut item.text)
                                    .desired_width(f32::INFINITY);
                                if ui.add(text_edit).changed() {
                                    edited_item = Some((current_section, item_index));
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
                        let escaped_text = item.text.replace('"', "\"\"");
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
                                let unescaped = text_part.trim_matches('"').replace("\"\"", "\"");
                                if let Some(section) = sections.get_mut(section_idx) {
                                    if let Some(item) = section.entries.iter_mut().find(|e| e.hashcode == hashcode) {
                                        if item.text != unescaped {
                                            item.text = unescaped;
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
}

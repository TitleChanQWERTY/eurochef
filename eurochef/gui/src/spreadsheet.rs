use egui::FontSelection;
use eurochef_edb::Hashcode;
use eurochef_shared::spreadsheets::{UXGeoSpreadsheet, UXGeoTextItem};

pub struct TextItemList {
    /// Search query for a specific hashcode
    search_hashcode: String,
    /// Search query for text item contents
    search_text: String,

    pub spreadsheets: Vec<(Hashcode, UXGeoSpreadsheet)>,
    selected_section: usize,
}

impl TextItemList {
    pub fn new(spreadsheets: Vec<(Hashcode, UXGeoSpreadsheet)>) -> Self {
        Self {
            search_hashcode: String::new(),
            search_text: String::new(),
            spreadsheets,
            selected_section: 0,
        }
    }

    // TODO: Display separate spreadsheets
    pub fn show(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Search: ");
            ui.text_edit_singleline(&mut self.search_text);
            ui.label("Hashcode: ");
            egui::TextEdit::singleline(&mut self.search_hashcode)
                .font(FontSelection::Style(egui::TextStyle::Monospace))
                .desired_width(76.0)
                .show(ui);
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

        let section_hashes: Vec<u32> = sections.iter().map(|s| s.hashcode).collect();

        let mut filtered_items: Vec<Vec<&mut UXGeoTextItem>> = sections
            .iter_mut()
            .map(|s| {
                s.entries
                    .iter_mut()
                    .filter(|v| {
                        if self.search_hashcode.is_empty() {
                            true
                        } else {
                            format!("{:08x}", v.hashcode)
                                .contains(&self.search_hashcode.to_lowercase())
                        }
                    })
                    .filter(|v| {
                        v.text
                            .to_lowercase()
                            .contains(&self.search_text.to_lowercase())
                    })
                    .collect()
            })
            .collect();

        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                egui::ScrollArea::vertical()
                    .id_source("section_scroll_area")
                    .show(ui, |ui| {
                        let mut current_set = 0;
                        for (i, hashcode) in section_hashes.iter().enumerate() {
                            if filtered_items[i].is_empty() {
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
                let table = egui_extras::TableBuilder::new(ui)
                    .striped(true)
                    .column(egui_extras::Column::exact(76.0))
                    .column(egui_extras::Column::exact(76.0))
                    .column(egui_extras::Column::remainder().resizable(true).clip(true));

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
                        let section_items = &mut filtered_items[self.selected_section];
                        let num_rows = section_items.len();
                        body.rows(text_height, num_rows, |row_index, mut row| {
                            let item = &mut section_items[row_index];
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
                                if ui.add(egui::TextEdit::singleline(&mut item.text)).changed() {
                                    // The item.text is already updated because we passed a mutable reference to TextEdit
                                }
                            })
                            .1
                            .context_menu(context_menu);
                        })
                    });
            });
        });
    }
}

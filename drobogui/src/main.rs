use eframe::egui;
use droboapi::{CapacityInfo, Drobo, DroboOptions, SlotInfo};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([600.0, 550.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Drobo Dashboard (Rust)",
        options,
        Box::new(|_cc| Box::new(DroboApp::new())),
    )
}

struct DroboApp {
    drobo: Drobo,
    selected_tab: usize,
    capacity: Option<CapacityInfo>,
    slots: Vec<SlotInfo>,
    options: DroboOptions,
    selected_fs: usize,
    rename_text: String,
    diag_filepath: String,
    picker_dir: String,
    status_msg: String,
    blink_msg: String,
    diag_window_open: bool,
    file_picker_open: bool,
    diag_decoded_text: String,
}

impl DroboApp {
    fn new() -> Self {
        let (drobo_res, is_sim) = match droboapi::discover_luns(None) {
            luns if !luns.is_empty() => (Drobo::new(luns[0].clone(), false), false),
            _ => (Drobo::new(vec!["/dev/simulated_drobo".to_string()], true), true),
        };

        let drobo = drobo_res.unwrap();
        let capacity = drobo.get_subpage_capacity().ok();
        let slots = drobo.get_subpage_slot_info().unwrap_or_default();
        let opts = drobo.get_options().unwrap_or(DroboOptions {
            yellow_threshold: 85,
            red_threshold: 95,
            dual_disk_redundancy: false,
            spin_down_delay_minutes: 15,
            spin_down_delay: true,
            use_manual_volume_management: false,
            use_static_ip_address: false,
            ip_address: "192.168.1.100".to_string(),
            netmask: "255.255.255.0".to_string(),
        });
        let unit_name = drobo.get_subpage_settings().map(|s| s.2).unwrap_or_else(|_| "Drobo01".to_string());
        let status_msg = if is_sim {
            "Ready (Simulation Mode - No physical Drobo found)".to_string()
        } else {
            format!("Connected to {}", drobo.char_dev_file)
        };

        Self {
            drobo,
            selected_tab: 0,
            capacity,
            slots,
            options: opts,
            selected_fs: 0,
            rename_text: unit_name,
            diag_filepath: "/tmp".to_string(),
            picker_dir: "/tmp".to_string(),
            status_msg,
            blink_msg: String::new(),
            diag_window_open: false,
            file_picker_open: false,
            diag_decoded_text: String::new(),
        }
    }

    fn refresh_data(&mut self) {
        if self.drobo.simulation {
            self.capacity = self.drobo.get_subpage_capacity().ok();
            self.slots = self.drobo.get_subpage_slot_info().unwrap_or_default();
            return;
        }

        // 1. Try querying with the EXISTING active device handle first (no udev open event triggered)
        let cap_res = self.drobo.get_subpage_capacity();
        let slot_res = self.drobo.get_subpage_slot_info();

        if cap_res.is_ok() && slot_res.is_ok() {
            self.capacity = cap_res.ok();
            self.slots = slot_res.unwrap_or_default();
            self.status_msg = format!("Connected to {}", self.drobo.char_dev_file);
            return;
        }

        // 2. Existing handle failed (device disconnected / in standby).
        // Only attempt re-discovery when connection is lost!
        let discovered = droboapi::discover_luns(None);
        if !discovered.is_empty() {
            if let Ok(new_drobo) = Drobo::new(discovered[0].clone(), false) {
                if new_drobo.get_subpage_capacity().is_ok() {
                    self.drobo = new_drobo;
                    self.capacity = self.drobo.get_subpage_capacity().ok();
                    self.slots = self.drobo.get_subpage_slot_info().unwrap_or_default();
                    self.status_msg = format!("Reconnected to {}", self.drobo.char_dev_file);
                    return;
                }
            }
        }

        self.status_msg = "Device disconnected / Standby. Click Refresh Data to scan.".to_string();
        self.capacity = None;
        self.slots.clear();
    }
}

impl eframe::App for DroboApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Request gentle repaint every 5 seconds to query status via existing handle
        ctx.request_repaint_after(std::time::Duration::from_secs(5));
        self.refresh_data();

        // 1. Render Custom Directory Browser Modal Window OUTSIDE panels
        if self.file_picker_open {
            let mut close_picker = false;
            let mut selected_file = None;
            let mut new_dir = None;

            egui::Window::new("Select Diagnostic Log File")
                .open(&mut self.file_picker_open)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .default_size([520.0, 380.0])
                .resizable(true)
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Folder:");
                        ui.text_edit_singleline(&mut self.picker_dir);

                        if ui.button("⬆ Up").clicked() {
                            if let Some(parent) = std::path::Path::new(&self.picker_dir).parent() {
                                new_dir = Some(parent.to_string_lossy().to_string());
                            }
                        }
                    });
                    ui.separator();

                    egui::ScrollArea::vertical().show(ui, |ui| {
                        let cur_path = std::path::Path::new(&self.picker_dir);
                        if let Ok(entries) = std::fs::read_dir(cur_path) {
                            let mut dirs = Vec::new();
                            let mut files = Vec::new();

                            for entry in entries.flatten() {
                                let path = entry.path();
                                if path.is_dir() {
                                    dirs.push(path);
                                } else if path.is_file() {
                                    files.push(path);
                                }
                            }

                            dirs.sort();
                            files.sort();

                            // Render Sub-directories
                            for d in &dirs {
                                let dname = d.file_name().unwrap_or_default().to_string_lossy();
                                if ui.button(format!("📁 {}", dname)).clicked() {
                                    new_dir = Some(d.to_string_lossy().to_string());
                                }
                            }

                            if !dirs.is_empty() && !files.is_empty() {
                                ui.separator();
                            }

                            // Render Files
                            if files.is_empty() && dirs.is_empty() {
                                ui.label("Directory is empty.");
                            } else {
                                for f in &files {
                                    let fname = f.file_name().unwrap_or_default().to_string_lossy();
                                    if ui.button(format!("📄 {}", fname)).clicked() {
                                        selected_file = Some(f.clone());
                                        close_picker = true;
                                    }
                                }
                            }
                        } else {
                            ui.colored_label(egui::Color32::RED, "Cannot read directory.");
                        }
                    });
                });

            if let Some(nd) = new_dir {
                self.picker_dir = nd;
            }

            if let Some(path) = selected_file {
                if let Ok(data) = std::fs::read(&path) {
                    self.diag_decoded_text = droboapi::decode_diagnostics(&data);
                    self.diag_filepath = path.to_string_lossy().to_string();
                    self.diag_window_open = true;
                    self.status_msg = format!("Loaded {}", path.display());
                }
            }

            if close_picker {
                self.file_picker_open = false;
            }
        }

        // 2. Render Decoded Diagnostics Log Window OUTSIDE panels
        if self.diag_window_open {
            egui::Window::new("Decoded Diagnostics Log")
                .open(&mut self.diag_window_open)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .default_size([550.0, 420.0])
                .resizable(true)
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.push_id("decoded_diag_scroll_area", |ui| {
                        egui::ScrollArea::vertical()
                            .id_source("diag_log_scroll")
                            .show(ui, |ui| {
                                ui.add(
                                    egui::TextEdit::multiline(&mut self.diag_decoded_text.as_str())
                                        .font(egui::TextStyle::Monospace)
                                        .id_source("diag_log_multiline_text")
                                        .desired_width(f32::INFINITY),
                                );
                            });
                    });
                });
        }

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.selected_tab, 0, "Device");
                ui.selectable_value(&mut self.selected_tab, 1, "Tools");
                ui.selectable_value(&mut self.selected_tab, 2, "Format");
                ui.selectable_value(&mut self.selected_tab, 3, "Options");
            });
        });

        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            ui.label(&self.status_msg);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            match self.selected_tab {

                0 => {
                    ui.heading("Drobo Status & Slots");
                    ui.separator();

                    let unit_name = self.drobo.get_subpage_settings().map(|s| s.2).unwrap_or_else(|_| "Drobo".to_string());
                    let fw_str = self.drobo.firmware.as_ref().map_or_else(
                        || "".to_string(),
                        |fw| format!(" firmware: {}", fw.version),
                    );

                    ui.label(format!("Device: {} {} {}", self.drobo.char_dev_file, unit_name, fw_str));

                    ui.add_space(10.0);
                    ui.label("Drive Bays:");

                    for slot in &self.slots {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                let label_text = if slot.physical_capacity > 0 {
                                    format!(
                                        "Slot {}: {} ({:.1} GB)",
                                        slot.slot_id,
                                        slot.model,
                                        slot.physical_capacity as f64 / 1e9
                                    )
                                } else {
                                    format!("Slot {}: Empty", slot.slot_id)
                                };

                                // Text info on the left
                                ui.add_sized([340.0, 24.0], egui::Label::new(label_text));

                                // LED Color determination
                                let fill_color = match slot.leds.as_str() {
                                    "green" => egui::Color32::from_rgb(0, 220, 0),
                                    "red" => egui::Color32::from_rgb(220, 0, 0),
                                    "yellow" => egui::Color32::from_rgb(230, 190, 0),
                                    "red-green" => egui::Color32::from_rgb(200, 150, 0),
                                    _ => egui::Color32::from_rgb(90, 90, 90),
                                };

                                // Filled Rectangular LED indicator on the right
                                let (rect, _response) = ui.allocate_exact_size(
                                    egui::vec2(28.0, 18.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().rect_filled(rect, 4.0, fill_color);
                                ui.painter().rect_stroke(
                                    rect,
                                    4.0,
                                    egui::Stroke::new(1.0_f32, egui::Color32::BLACK),
                                );
                            });
                        });
                    }

                    ui.add_space(15.0);
                    if let Some(cap) = &self.capacity {
                        let used_gb = cap.used_bytes as f32 / 1e9;
                        let virt_gb = cap.virtual_bytes as f32 / 1e9;
                        let pct = if virt_gb > 0.0 { used_gb / virt_gb } else { 0.0 };

                        ui.label(format!("Capacity Used: {:.1} GB / {:.1} GB", used_gb, virt_gb));
                        ui.add(egui::ProgressBar::new(pct).show_percentage());
                    }
                }

                1 => {
                    ui.heading("Drobo Management Tools");
                    ui.separator();

                    if ui.button("Blink Lights").clicked() {
                        if self.drobo.blink().is_ok() {
                            self.blink_msg = "Blinked lights successfully!".to_string();
                        }
                    }

                    if !self.blink_msg.is_empty() {
                        ui.label(&self.blink_msg);
                    }

                    ui.add_space(10.0);

                    ui.label("Rename Drobo Unit:");
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut self.rename_text);
                        if ui.button("Rename").clicked() {
                            if self.drobo.rename(&self.rename_text).is_ok() {
                                self.status_msg = format!("Renamed unit to '{}'", self.rename_text);
                            } else {
                                self.status_msg = "Rename command failed.".to_string();
                            }
                        }
                    });

                    ui.add_space(10.0);

                    ui.horizontal(|ui| {
                        if ui.button("Dump Diagnostics").clicked() {
                            match self.drobo.dump_diagnostics() {
                                Ok(path) => {
                                    self.diag_filepath = path.clone();
                                    self.status_msg = format!("Diagnostics dumped to {}", path);
                                    println!("Diagnostics dumped to {}", path);
                                }
                                Err(e) => {
                                    self.status_msg = format!("Diagnostics dump failed: {}", e);
                                    println!("Diagnostics dump failed: {}", e);
                                }
                            }
                        }
                    });

                    ui.add_space(5.0);
                    ui.label("Diagnostics File Path:");
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut self.diag_filepath);

                        if ui.button("Show Diag Log").clicked() {
                            println!("Show Diag Log button clicked! Opening file_picker_open...");
                            self.file_picker_open = true;
                        }
                    });

                    ui.add_space(10.0);

                    if ui.button("Shutdown / Standby").clicked() {
                        if self.drobo.standby().is_ok() {
                            self.status_msg = "Standby requested...".to_string();
                        }
                    }

                    ui.add_space(10.0);
                    if ui.button("Refresh Data").clicked() {
                        self.refresh_data();
                        self.status_msg = "Refreshed device status.".to_string();
                    }
                }

                2 => {
                    ui.heading("Format LUNs");
                    ui.separator();
                    ui.colored_label(egui::Color32::RED, "WARNING: Formatting erases all data!");
                    ui.add_space(10.0);
                    ui.label("Select File System:");
                    ui.radio_value(&mut self.selected_fs, 0, "Ext3");
                    ui.radio_value(&mut self.selected_fs, 1, "FAT32");
                    ui.radio_value(&mut self.selected_fs, 2, "NTFS");

                    ui.add_space(10.0);
                    if ui.button("Generate Format Script").clicked() {
                        let fs_type = match self.selected_fs {
                            0 => "ext3",
                            1 => "FAT32",
                            2 => "ntfs",
                            _ => "ext3",
                        };
                        let script_path = std::path::Path::new("/tmp/fmtscript");
                        match self.drobo.format_script(fs_type, script_path) {
                            Ok(path) => {
                                self.status_msg = format!("Format script created at {}", path);
                            }
                            Err(e) => {
                                self.status_msg = format!("Failed to create script: {}", e);
                            }
                        }
                    }
                }

                3 => {
                    ui.heading("Device Options");
                    ui.separator();

                    ui.checkbox(&mut self.options.dual_disk_redundancy, "Dual Disk Redundancy");
                    ui.checkbox(&mut self.options.spin_down_delay, "Enable Spin Down Delay");
                    ui.horizontal(|ui| {
                        ui.label("Spin Down Delay (Minutes):");
                        ui.add(egui::DragValue::new(&mut self.options.spin_down_delay_minutes).clamp_range(5..=120));
                    });

                    ui.add_space(5.0);
                    ui.checkbox(&mut self.options.use_manual_volume_management, "Manual Volume Management");

                    ui.add_space(10.0);
                    ui.heading("Network Settings (iSCSI / DroboShare)");
                    ui.checkbox(&mut self.options.use_static_ip_address, "Use Static IP Address");
                    ui.horizontal(|ui| {
                        ui.label("IP Address:");
                        ui.text_edit_singleline(&mut self.options.ip_address);
                    });
                    ui.horizontal(|ui| {
                        ui.label("NetMask:   ");
                        ui.text_edit_singleline(&mut self.options.netmask);
                    });

                    ui.add_space(15.0);
                    ui.label("Alerting Thresholds:");
                    ui.add(egui::Slider::new(&mut self.options.yellow_threshold, 0..=100).text("Yellow Alert %"));
                    ui.add(egui::Slider::new(&mut self.options.red_threshold, 0..=100).text("Red Alert %"));

                    ui.add_space(15.0);
                    if ui.button("Save Options to Device").clicked() {
                        if self.drobo.set_options(&self.options).is_ok() {
                            self.status_msg = "Saved options to Drobo hardware.".to_string();
                        } else {
                            self.status_msg = "Failed to save options.".to_string();
                        }
                    }
                }
                _ => {}
            }
        });
    }
}

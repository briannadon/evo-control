//! egui GUI for evo-control.
//!
//! Layout: top bar (status + preset controls), input strips (4),
//! output strips (2), collapsible mixer panel.

mod widgets;

use std::collections::HashMap;
use std::time::Instant;

use eframe::egui::{self, Color32, CornerRadius};
use evo_driver::{DriverHandle, DeviceStatus};

const POLL_INTERVAL_MS: u64 = 200;
const WRITE_DEBOUNCE_MS: u64 = 50;

pub struct App {
    handle: Option<DriverHandle>,
    status: DeviceStatus,
    mixer_shadow: [[f32; 4]; 10],

    connected: bool,
    last_poll: Instant,
    last_write: Instant,

    // Presets
    presets: Vec<String>,
    selected_preset: String,
    new_preset_name: String,

}

impl Default for App {
    fn default() -> Self {
        Self {
            handle: None,
            status: DeviceStatus::default(),
            mixer_shadow: [[-128.0_f32; 4]; 10],
            connected: false,
            last_poll: Instant::now(),
            last_write: Instant::now(),
            presets: Vec::new(),
            selected_preset: String::new(),
            new_preset_name: String::new(),
        }
    }
}

impl App {
    fn try_connect(&mut self) {
        if self.handle.is_some() {
            return;
        }
        match evo_driver::worker::spawn() {
            Ok((handle, _disc)) => {
                self.handle = Some(handle);
                self.connected = true;
            }
            Err(_) => {
                self.connected = false;
            }
        }
    }

    fn poll_status(&mut self) {
        let now = Instant::now();
        if now.duration_since(self.last_poll).as_millis() < POLL_INTERVAL_MS as u128 {
            return;
        }
        self.last_poll = now;

        let handle = match &self.handle {
            Some(h) => h,
            None => return,
        };

        match handle.status() {
            Ok(s) => {
                self.status = s;
                self.connected = true;
            }
            Err(_) => {
                self.connected = false;
                self.handle = None;
            }
        }
    }

    fn load_presets(&mut self) {
        let dir = evo_config::presets_dir();
        let names = evo_config::store::list_presets(&dir).unwrap_or_default();
        self.presets = names;
    }

    fn save_preset(&mut self, name: &str) {
        let state = self.build_state();
        let mut config = evo_config::Config {
            state,
            presets: HashMap::new(),
            state_path: evo_config::state_file(),
        };
        if let Err(e) = config.save_preset(name) {
            eprintln!("failed to save preset '{name}': {e}");
        }
        self.load_presets();
    }

    fn load_preset(&mut self, name: &str) {
        let handle = match &self.handle {
            Some(h) => h,
            None => return,
        };
        let file = evo_config::preset_file(name);
        match evo_config::store::load_state(&file) {
            Ok(Some(preset)) => {
                self.mixer_shadow = preset.mixer;
                apply_to_device(handle, &preset);
                let config = evo_config::Config {
                    state: preset,
                    presets: HashMap::new(),
                    state_path: evo_config::state_file(),
                };
                let _ = config.save_state();
            }
            Ok(None) => eprintln!("preset '{name}' not found"),
            Err(e) => eprintln!("failed to load preset '{name}': {e}"),
        }
    }

    fn save_current_state(&self) {
        let state = self.build_state();
        let config = evo_config::Config {
            state,
            presets: HashMap::new(),
            state_path: evo_config::state_file(),
        };
        let _ = config.save_state();
    }

    fn build_state(&self) -> evo_config::DeviceState {
        evo_config::DeviceState {
            schema: 1,
            output_volume_db: self.status.volume_db,
            input_gain_db: self.status.gain_db,
            phantom: self.status.phantom,
            input_mute: self.status.input_mute,
            output_mute: self.status.output_mute,
            mixer: self.mixer_shadow,
        }
    }

    fn write_mixer(&self, in_idx: u8, out_idx: u8, db: f32) {
        let now = Instant::now();
        if now.duration_since(self.last_write).as_millis() < WRITE_DEBOUNCE_MS as u128 {
            return;
        }
        if let Some(handle) = &self.handle {
            let _ = handle.mixer_set(in_idx, out_idx, db);
        }
    }
}

fn apply_to_device(handle: &DriverHandle, preset: &evo_config::DeviceState) {
    let _ = handle.volume_set(0, preset.output_volume_db[0]);
    let _ = handle.volume_set(1, preset.output_volume_db[1]);
    for i in 0..4 {
        let _ = handle.gain_set(i as u8, preset.input_gain_db[i]);
        let _ = handle.phantom_set(i as u8, preset.phantom[i]);
        let _ = handle.input_mute_set(i as u8, preset.input_mute[i]);
    }
    let _ = handle.output_mute_set(preset.output_mute);
    for in_idx in 0..10 {
        for out_idx in 0..4 {
            let db = preset.mixer[in_idx][out_idx];
            let _ = handle.mixer_set(in_idx as u8, out_idx as u8, db);
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.try_connect();
        self.poll_status();

        // Clone the handle once so closures don't fight the borrow checker.
        let handle = self.handle.clone();

        // ── Top bar ──────────────────────────────────────────────────────
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Connection indicator
                let (color, text) = if self.connected {
                    (Color32::GREEN, "Connected")
                } else {
                    (Color32::RED, "Disconnected")
                };
                let dot = egui::Frame::default()
                    .fill(color)
                    .corner_radius(CornerRadius::same(4));
                dot.show(ui, |ui| {
                    ui.add_sized([8.0, 8.0], egui::Label::new(""));
                });
                ui.label(egui::RichText::new(text).size(14.0).color(color));
                ui.separator();

                // Save new preset
                ui.text_edit_singleline(&mut self.new_preset_name);
                let preset_name = self.new_preset_name.clone();
                if ui.button("Save preset").clicked() && !preset_name.is_empty() {
                    self.save_preset(&preset_name);
                }

                ui.separator();

                // Load preset combo
                egui::ComboBox::from_label("Load")
                    .selected_text(&self.selected_preset)
                    .show_ui(ui, |ui| {
                        for name in &self.presets.clone() {
                            let selected = name == &self.selected_preset;
                            if ui.selectable_label(selected, name).clicked() {
                                self.selected_preset = name.clone();
                                self.load_preset(name);
                            }
                        }
                    });

                if ui.button("Refresh").clicked() {
                    self.load_presets();
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Save state").clicked() {
                        self.save_current_state();
                    }
                });
            });
        });

        // ── Main content ─────────────────────────────────────────────────
        egui::CentralPanel::default().show(ctx, |ui| {
            if !self.connected {
                ui.heading("Device Disconnected");
                ui.label("Plug in the Audient EVO 8 and ensure the kernel module is loaded.");
                if ui.button("Retry").clicked() {
                    self.last_poll = Instant::now()
                        - std::time::Duration::from_millis(POLL_INTERVAL_MS + 1);
                }
                return;
            }

            // Input strips
            ui.heading("Inputs");
            ui.horizontal_wrapped(|ui| {
                for i in 0..4 {
                    ui.vertical(|ui| {
                        let frame = egui::Frame::default()
                            .fill(Color32::from_rgb(30, 30, 30))
                            .corner_radius(CornerRadius::same(6))
                            .inner_margin(egui::Margin::symmetric(8, 4));
                        frame.show(ui, |ui| {
                            ui.set_min_width(90.0);

                            ui.label(egui::RichText::new(format!("Input {}", i + 1)).size(12.0).strong());
                            ui.add_space(4.0);

                            // Gain knob (audio-taper curve spreads perceived volume evenly)
                            let mut gain = self.status.gain_db[i];
                            if widgets::rotary_knob(ui, &mut gain, -8.0, 50.0, 48.0, 0.5) {
                                self.status.gain_db[i] = gain;
                                if let Some(handle) = handle.as_ref() {
                                    let _ = handle.gain_set(i as u8, gain);
                                }
                            }
                            ui.label(format!("{gain:.0} dB"));
                            ui.add_space(4.0);

                            // Phantom toggle
                            let mut phantom = self.status.phantom[i];
                            if widgets::toggle_lamp(ui, "48V", phantom, Color32::RED) {
                                phantom = !phantom;
                                self.status.phantom[i] = phantom;
                                if let Some(handle) = handle.as_ref() {
                                    let _ = handle.phantom_set(i as u8, phantom);
                                }
                            }

                            // Mute toggle
                            let mut muted = self.status.input_mute[i];
                            if widgets::toggle_lamp(ui, "Mute", muted, Color32::YELLOW) {
                                muted = !muted;
                                self.status.input_mute[i] = muted;
                                if let Some(handle) = handle.as_ref() {
                                    let _ = handle.input_mute_set(i as u8, muted);
                                }
                            }

                            ui.add_space(4.0);

                            // Send-to-mixer level (set all outputs for this input)
                            let mut send_db = self.mixer_shadow[i][0];
                            if widgets::level_slider(ui, "Send", &mut send_db, -128.0, 6.0) {
                                for out in 0..4 {
                                    self.mixer_shadow[i][out] = send_db;
                                    self.write_mixer(i as u8, out as u8, send_db);
                                }
                            }
                        });
                    });
                }
            });

            ui.add_space(8.0);

            // Output strips
            ui.heading("Outputs");
            ui.horizontal_wrapped(|ui| {
                for pair in 0..2 {
                    ui.vertical(|ui| {
                        let frame = egui::Frame::default()
                            .fill(Color32::from_rgb(30, 30, 30))
                            .corner_radius(CornerRadius::same(6))
                            .inner_margin(egui::Margin::symmetric(8, 4));
                        frame.show(ui, |ui| {
                            ui.set_min_width(120.0);

                            let label = ["Main", "Headphones"][pair];
                            ui.label(egui::RichText::new(label).size(12.0).strong());
                            ui.add_space(4.0);

                            // Volume knob (large, linear — dB range is already perceptual)
                            let mut vol = self.status.volume_db[pair];
                            if widgets::rotary_knob(ui, &mut vol, -96.0, 0.0, 64.0, 0.5) {
                                self.status.volume_db[pair] = vol;
                                if let Some(handle) = handle.as_ref() {
                                    let _ = handle.volume_set(pair as u8, vol);
                                }
                            }
                            ui.label(format!("{vol:.1} dB"));
                            ui.add_space(4.0);

                            // Mute toggle
                            let mut muted = self.status.output_mute;
                            if widgets::toggle_lamp(ui, "Mute", muted, Color32::YELLOW) {
                                muted = !muted;
                                self.status.output_mute = muted;
                                if let Some(handle) = handle.as_ref() {
                                    let _ = handle.output_mute_set(muted);
                                }
                            }
                        });
                    });
                }
            });

            // ── Mixer panel (collapsible) ────────────────────────────────
            ui.add_space(8.0);
            ui.collapsing("Mixer Matrix (10×4)", |ui| {
                let labels_in = [
                    "IN1", "IN2", "IN3", "IN4",
                    "DAW1", "DAW2", "DAW3", "DAW4",
                    "LOOP1", "LOOP2",
                ];
                let labels_out = ["Loopback L1", "Loopback R1", "Loopback L2", "Loopback R2"];

                egui::Grid::new("mixer_grid")
                    .striped(true)
                    .min_col_width(60.0)
                    .show(ui, |ui| {
                        ui.label("");
                        for out_label in &labels_out {
                            ui.label(egui::RichText::new(*out_label).size(10.0).strong());
                        }
                        ui.end_row();

                        for in_idx in 0..10 {
                            ui.label(egui::RichText::new(labels_in[in_idx]).size(10.0).strong());
                            for out_idx in 0..4 {
                                let db = &mut self.mixer_shadow[in_idx][out_idx];
                                let old = *db;
                                ui.horizontal(|ui| {
                                    let slider = egui::Slider::new(db, -128.0..=6.0)
                                        .text("")
                                        .show_value(false)
                                        .clamping(egui::SliderClamping::Always);
                                    let changed = ui.add(slider).changed() && *db != old;
                                    if changed {
                                        let db_val = *db;
                                        if let Some(ref h) = handle {
                                            let _ = h.mixer_set(in_idx as u8, out_idx as u8, db_val);
                                        }
                                    }
                                    let val = if *db <= -128.0 {
                                        "—".to_string()
                                    } else {
                                        format!("{:.0}", db)
                                    };
                                    ui.label(val);
                                });
                            }
                            ui.end_row();
                        }
                    });
            });
        });
    }
}

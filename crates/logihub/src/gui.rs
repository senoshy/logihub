//! Native Linux desktop GUI for LogiHub built with `eframe` / `egui`.

use eframe::egui;
use hidpp::{ChargingStatus, LogitechMouse, MAX_DPI_STAGES, MouseStatus, OnboardMode};

const ACCENT: egui::Color32 = egui::Color32::from_rgb(0x00, 0xB8, 0xFC); // Logitech G Cyan
const CARD_BG: egui::Color32 = egui::Color32::from_rgb(0x18, 0x1A, 0x20);
const PANEL_BG: egui::Color32 = egui::Color32::from_rgb(0x11, 0x13, 0x18);
const BORDER: egui::Color32 = egui::Color32::from_rgb(0x2A, 0x2E, 0x39);
const TEXT_MUTED: egui::Color32 = egui::Color32::from_rgb(0x9A, 0xA2, 0xB1);
const OK_GREEN: egui::Color32 = egui::Color32::from_rgb(0x34, 0xD3, 0x99);
const WARN_AMBER: egui::Color32 = egui::Color32::from_rgb(0xFB, 0xBF, 0x24);
const ERR_RED: egui::Color32 = egui::Color32::from_rgb(0xF8, 0x71, 0x71);

const DPI_PRESETS: [u16; 5] = [400, 800, 1200, 1600, 3200];

pub fn run_gui() -> Result<(), String> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("LogiHub — Linux")
            .with_inner_size([680.0, 580.0])
            .with_min_inner_size([560.0, 480.0]),
        ..Default::default()
    };

    eframe::run_native(
        "LogiHub",
        options,
        Box::new(|cc| {
            configure_theme(&cc.egui_ctx);
            Ok(Box::new(LogiHubApp::new()))
        }),
    )
    .map_err(|e| e.to_string())
}

fn configure_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = PANEL_BG;
    visuals.window_fill = PANEL_BG;
    visuals.widgets.noninteractive.bg_fill = CARD_BG;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, BORDER);
    visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(0x22, 0x25, 0x2F);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(0x2B, 0x30, 0x3E);
    visuals.widgets.active.bg_fill = ACCENT;
    visuals.selection.bg_fill = ACCENT.linear_multiply(0.35);
    visuals.selection.stroke = egui::Stroke::new(1.5_f32, ACCENT);
    ctx.set_visuals(visuals);
}

struct LogiHubApp {
    mouse: Option<LogitechMouse>,
    status: Option<MouseStatus>,
    stages: [u16; MAX_DPI_STAGES],
    active_stage: usize,
    selected_rate_hz: u16,
    persist_onboard: bool,
    banner: Option<(egui::Color32, String)>,
}

impl LogiHubApp {
    fn new() -> Self {
        let mut app = Self {
            mouse: None,
            status: None,
            stages: [400, 800, 1600, 3200, 0],
            active_stage: 1,
            selected_rate_hz: 1000,
            persist_onboard: true,
            banner: None,
        };
        app.reconnect();
        app
    }

    fn reconnect(&mut self) {
        match LogitechMouse::connect_first() {
            Ok(mouse) => {
                self.mouse = Some(mouse);
                self.refresh();
            }
            Err(err) => {
                self.mouse = None;
                self.status = None;
                self.banner = Some((ERR_RED, err.to_string()));
            }
        }
    }

    fn refresh(&mut self) {
        let Some(ref mut mouse) = self.mouse else {
            self.reconnect();
            return;
        };

        match mouse.status() {
            Ok(status) => {
                if let Some(stages) = status.onboard_dpi_stages {
                    self.stages = stages;
                } else if let Some(dpi) = status.dpi {
                    self.stages = [dpi.current_x, 0, 0, 0, 0];
                }
                if let Some(idx) = status.onboard_dpi_index {
                    self.active_stage = idx.min(MAX_DPI_STAGES - 1);
                } else {
                    self.active_stage = 0;
                }
                if let Some(rate) = status.report_rate_hz.or(status.onboard_report_rate_hz) {
                    self.selected_rate_hz = rate;
                }
                self.status = Some(status);
                self.banner = None;
            }
            Err(err) => {
                self.banner = Some((ERR_RED, format!("Failed to read mouse status: {err}")));
            }
        }
    }

    fn apply_dpi_stages(&mut self) {
        let Some(ref mut mouse) = self.mouse else {
            return;
        };
        let active_dpi = self.stages[self.active_stage];
        if active_dpi == 0 {
            self.banner = Some((
                WARN_AMBER,
                "Active DPI stage cannot be disabled (0).".to_string(),
            ));
            return;
        }

        let res = if self.persist_onboard
            && self
                .status
                .as_ref()
                .and_then(|s| s.onboard_dpi_stages)
                .is_some()
        {
            mouse
                .set_dpi_stages(self.stages, self.active_stage)
                .map(|applied| {
                    self.stages = applied;
                    applied[self.active_stage]
                })
        } else {
            mouse.set_dpi(active_dpi, self.persist_onboard)
        };

        match res {
            Ok(applied_dpi) => {
                self.refresh();
                self.banner = Some((
                    OK_GREEN,
                    format!("Applied {applied_dpi} DPI (saved to onboard memory)."),
                ));
            }
            Err(err) => {
                self.banner = Some((ERR_RED, format!("Failed to apply DPI: {err}")));
            }
        }
    }

    fn apply_report_rate(&mut self, hz: u16) {
        let Some(ref mut mouse) = self.mouse else {
            return;
        };
        match mouse.set_report_rate(hz, self.persist_onboard) {
            Ok(()) => {
                self.selected_rate_hz = hz;
                self.refresh();
                self.banner = Some((
                    OK_GREEN,
                    format!("Polling rate set to {hz} Hz (saved to onboard memory)."),
                ));
            }
            Err(err) => {
                self.banner = Some((ERR_RED, format!("Failed to set polling rate: {err}")));
            }
        }
    }

    fn apply_mode(&mut self, mode: OnboardMode) {
        let Some(ref mut mouse) = self.mouse else {
            return;
        };
        match mouse.set_onboard_mode(mode) {
            Ok(()) => {
                self.refresh();
                self.banner = Some((OK_GREEN, format!("Switched to {mode}.")));
            }
            Err(err) => {
                self.banner = Some((ERR_RED, format!("Failed to switch mode: {err}")));
            }
        }
    }
}

impl eframe::App for LogiHubApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(PANEL_BG).inner_margin(20.0))
            .show(ctx, |ui| {
                self.render_header(ui);
                ui.add_space(12.0);

                if let Some((color, ref msg)) = self.banner {
                    egui::Frame::new()
                        .fill(color.linear_multiply(0.14))
                        .stroke(egui::Stroke::new(1.0_f32, color.linear_multiply(0.6)))
                        .corner_radius(6.0)
                        .inner_margin(egui::Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.colored_label(color, msg);
                        });
                    ui.add_space(12.0);
                }

                if self.status.is_none() {
                    self.render_disconnected(ui);
                    return;
                }

                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.render_dpi_card(ui);
                    ui.add_space(12.0);
                    self.render_rate_card(ui);
                    ui.add_space(12.0);
                    self.render_memory_card(ui);
                });
            });
    }
}

impl LogiHubApp {
    fn render_header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                let title = self
                    .status
                    .as_ref()
                    .map(|s| s.name.as_str())
                    .unwrap_or("LogiHub");
                ui.label(
                    egui::RichText::new(title)
                        .size(22.0)
                        .strong()
                        .color(egui::Color32::WHITE),
                );
                if let Some(ref status) = self.status {
                    ui.label(
                        egui::RichText::new(format!(
                            "{}  •  {:04X}:{:04X}  •  HID++ {}.{}",
                            status.hidraw_path.display(),
                            status.vendor_id,
                            status.product_id,
                            status.protocol_version.0,
                            status.protocol_version.1
                        ))
                        .size(12.0)
                        .color(TEXT_MUTED),
                    );
                }
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Refresh").clicked() {
                    self.refresh();
                }

                if let Some(batt) = self.status.as_ref().and_then(|s| s.battery.as_ref()) {
                    let color = match batt.charging {
                        ChargingStatus::Recharging | ChargingStatus::AlmostFull | ChargingStatus::Full => {
                            ACCENT
                        }
                        _ if batt.percentage <= 15 => ERR_RED,
                        _ if batt.percentage <= 30 => WARN_AMBER,
                        _ => OK_GREEN,
                    };
                    egui::Frame::new()
                        .fill(CARD_BG)
                        .stroke(egui::Stroke::new(1.0_f32, BORDER))
                        .corner_radius(6.0)
                        .inner_margin(egui::Margin::symmetric(10, 5))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(format!("{}%  {}", batt.percentage, batt.charging))
                                    .strong()
                                    .color(color),
                            );
                        });
                }
            });
        });
    }

    fn render_disconnected(&mut self, ui: &mut egui::Ui) {
        card(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(24.0);
                ui.label(
                    egui::RichText::new("No Logitech HID++ 2.0 Mouse Connected")
                        .size(16.0)
                        .strong(),
                );
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(
                        "Wake your mouse or check USB / udev permissions (99-logitech-hidpp.rules).",
                    )
                    .color(TEXT_MUTED),
                );
                ui.add_space(16.0);
                if ui.button("Reconnect").clicked() {
                    self.reconnect();
                }
                ui.add_space(24.0);
            });
        });
    }

    fn render_dpi_card(&mut self, ui: &mut egui::Ui) {
        let (min_dpi, max_dpi, step) = self
            .status
            .as_ref()
            .and_then(|s| s.dpi_spec.as_ref())
            .map(|spec| (spec.min(), spec.max(), spec.step()))
            .unwrap_or((100, 25600, 50));

        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("SENSITIVITY (DPI)").size(13.0).strong().color(ACCENT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(dpi) = self.status.as_ref().and_then(|s| s.dpi) {
                        ui.label(
                            egui::RichText::new(format!("Live: {} DPI", dpi.current_x))
                                .color(TEXT_MUTED),
                        );
                    }
                });
            });
            ui.add_space(10.0);

            // Stage selector pills (5 onboard stages)
            ui.horizontal_wrapped(|ui| {
                for i in 0..MAX_DPI_STAGES {
                    let val = self.stages[i];
                    let is_active = self.active_stage == i;
                    let label = if val == 0 {
                        format!("Stage {}: Off", i + 1)
                    } else {
                        format!("Stage {}: {}", i + 1, val)
                    };
                    let btn = egui::Button::new(
                        egui::RichText::new(label).color(if is_active {
                            egui::Color32::BLACK
                        } else {
                            egui::Color32::WHITE
                        }),
                    )
                    .fill(if is_active {
                        ACCENT
                    } else {
                        egui::Color32::from_rgb(0x22, 0x25, 0x2F)
                    })
                    .corner_radius(6.0);

                    if ui.add(btn).clicked() {
                        self.active_stage = i;
                        if self.stages[i] == 0 {
                            self.stages[i] = 800;
                        }
                    }
                }
            });

            ui.add_space(12.0);

            // Slider for the selected stage
            let current = &mut self.stages[self.active_stage];
            let mut val_u16 = (*current).clamp(min_dpi, max_dpi);
            ui.horizontal(|ui| {
                ui.spacing_mut().slider_width = 360.0;
                let slider = egui::Slider::new(&mut val_u16, min_dpi..=max_dpi)
                    .step_by(f64::from(step))
                    .suffix(" DPI");
                if ui.add(slider).changed() {
                    *current = val_u16;
                }
            });

            ui.add_space(8.0);

            // Quick presets + Apply button
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Presets:").color(TEXT_MUTED));
                for preset in DPI_PRESETS {
                    if ui.small_button(format!("{preset}")).clicked() {
                        self.stages[self.active_stage] = preset.clamp(min_dpi, max_dpi);
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let apply_btn = egui::Button::new(
                        egui::RichText::new("Apply DPI").strong().color(egui::Color32::BLACK),
                    )
                    .fill(ACCENT)
                    .corner_radius(6.0);
                    if ui.add(apply_btn).clicked() {
                        self.apply_dpi_stages();
                    }
                });
            });
        });
    }

    fn render_rate_card(&mut self, ui: &mut egui::Ui) {
        let rates = self
            .status
            .as_ref()
            .map(|s| s.supported_rates_hz.clone())
            .filter(|r| !r.is_empty())
            .unwrap_or_else(|| vec![125, 250, 500, 1000]);

        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("POLLING RATE (REPORT RATE)")
                        .size(13.0)
                        .strong()
                        .color(ACCENT),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(rate) = self.status.as_ref().and_then(|s| s.report_rate_hz) {
                        ui.label(egui::RichText::new(format!("Active: {rate} Hz")).color(TEXT_MUTED));
                    }
                });
            });
            ui.add_space(10.0);

            let mut clicked_rate = None;
            ui.horizontal_wrapped(|ui| {
                for hz in rates {
                    let selected = self.selected_rate_hz == hz;
                    let btn = egui::Button::new(
                        egui::RichText::new(format!("{hz} Hz")).strong().color(if selected {
                            egui::Color32::BLACK
                        } else {
                            egui::Color32::WHITE
                        }),
                    )
                    .min_size(egui::vec2(88.0, 32.0))
                    .fill(if selected {
                        ACCENT
                    } else {
                        egui::Color32::from_rgb(0x22, 0x25, 0x2F)
                    })
                    .corner_radius(6.0);

                    if ui.add(btn).clicked() {
                        clicked_rate = Some(hz);
                    }
                }
            });

            if let Some(hz) = clicked_rate {
                self.apply_report_rate(hz);
            }
        });
    }

    fn render_memory_card(&mut self, ui: &mut egui::Ui) {
        let current_mode = self.status.as_ref().and_then(|s| s.onboard_mode);

        card(ui, |ui| {
            ui.label(
                egui::RichText::new("ONBOARD MEMORY")
                    .size(13.0)
                    .strong()
                    .color(ACCENT),
            );
            ui.add_space(8.0);

            ui.checkbox(
                &mut self.persist_onboard,
                "Persist DPI & polling rate directly to onboard flash (survives sleep & reboot)",
            );

            if let Some(mode) = current_mode {
                ui.add_space(8.0);
                let mut switch_to = None;
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Active Mode:").color(TEXT_MUTED));
                    for candidate in [OnboardMode::Onboard, OnboardMode::Host] {
                        let selected = mode == candidate;
                        let btn = egui::Button::new(
                            egui::RichText::new(candidate.to_string()).color(if selected {
                                egui::Color32::BLACK
                            } else {
                                egui::Color32::WHITE
                            }),
                        )
                        .fill(if selected {
                            ACCENT
                        } else {
                            egui::Color32::from_rgb(0x22, 0x25, 0x2F)
                        })
                        .corner_radius(6.0);

                        if ui.add(btn).clicked() && !selected {
                            switch_to = Some(candidate);
                        }
                    }
                });
                if let Some(target) = switch_to {
                    self.apply_mode(target);
                }
            }
        });
    }
}

fn card(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(CARD_BG)
        .stroke(egui::Stroke::new(1.0_f32, BORDER))
        .corner_radius(10.0)
        .inner_margin(16.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add_contents(ui);
        });
}

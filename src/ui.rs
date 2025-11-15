
use crate::ShaderApp;
use crate::data::*;
use crate::FILE_CHECK_TIMEOUT_MS;
use std::time::Duration;
use std::path::Path;

impl eframe::App for ShaderApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Check for hot-reload messages with minimal blocking
        if self.shader_update_receiver
            .recv_timeout(Duration::from_millis(FILE_CHECK_TIMEOUT_MS))
            .is_ok()
        {
            self.try_reload_shader();
        }

        // --- UI controls now go in a SidePanel on the right ---
        egui::SidePanel::right("controls_panel")
            .default_width(250.0) 
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("Controls");
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    ui.label("Shader:");
                    ui.label(
                        egui::RichText::new(
                            self.current_shader_path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("unknown"),
                        )
                        .family(egui::FontFamily::Monospace),
                    );

                    if ui.button("Open").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("GLSL Fragment Shader", &["frag", "glsl"])
                            .set_directory(
                                self.current_shader_path.parent().unwrap_or(Path::new(".")),
                            )
                            .pick_file()
                        {
                            self.load_shader_file(path);
                        }
                    }
                });

                ui.separator();

                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.auto_time, "Auto Time");
                    if !self.auto_time {
                        ui.add(egui::Slider::new(&mut self.time, 0.0..=100.0).text("Time"));
                    }
                    if ui.button("Reset").clicked() {
                        self.time = 0.0;
                    }
                });

                ui.separator();

                // Export section
                ui.label(egui::RichText::new("Export:").strong());
                ui.horizontal(|ui| {
                    ui.label("Width:");
                    ui.add(egui::DragValue::new(&mut self.export_resolution[0]).speed(10).clamp_range(1..=8192));
                });
                ui.horizontal(|ui| {
                    ui.label("Height:");
                    ui.add(egui::DragValue::new(&mut self.export_resolution[1]).speed(10).clamp_range(1..=8192));
                });
                ui.add_space(4.0);
                if ui.button("Export Image").clicked() {
                    self.export_image();
                }

                ui.add_space(8.0);

                // Video export section
                ui.label(egui::RichText::new("Video Export:").strong());
                if !self.ffmpeg_available {
                    ui.label(egui::RichText::new("⚠ FFmpeg not found").color(egui::Color32::YELLOW).small());
                    ui.label(egui::RichText::new("(frames will be saved)").small());
                }
                
                ui.horizontal(|ui| {
                    ui.label("Frames:");
                    ui.add(egui::DragValue::new(&mut self.video_duration_frames).speed(10).clamp_range(1..=10000));
                });
                ui.horizontal(|ui| {
                    ui.label("FPS:");
                    ui.add(egui::DragValue::new(&mut self.video_fps).speed(1).clamp_range(1..=120));
                });
                ui.label(egui::RichText::new(
                    format!("Duration: {:.2}s", self.video_duration_frames as f32 / self.video_fps as f32)
                ).small());
                
                // Show progress or export button
                if let Some(progress) = self.export_progress.lock().clone() {
                    ui.add_space(4.0);
                    let progress_fraction = progress.current_frame as f32 / progress.total_frames as f32;
                    ui.add(egui::ProgressBar::new(progress_fraction)
                        .text(&progress.status));
                } else {
                    ui.add_space(4.0);
                    if ui.button("Export Video").clicked() {
                        self.export_video();
                    }
                }

                ui.separator();

                // Display detected uniforms with controls
                if !self.uniforms.is_empty() {
                    ui.label(egui::RichText::new("Uniforms:").strong());

                    let mut uniform_names: Vec<_> = self.uniforms.keys().cloned().collect();
                    uniform_names.sort();

                    for name in uniform_names {
                        if let Some(uniform) = self.uniforms.get_mut(&name) {
                            if name == "u_resolution" || name == "u_time" {
                                continue;
                            }

                            ui.vertical(|ui| {
                                ui.label(&name);
                                match &mut uniform.value {
                                    UniformValue::Float(val) => {
                                        ui.add(egui::Slider::new(val, 0.0..=1.0));
                                    }
                                    UniformValue::Vec2(vals) => {
                                        ui.add(egui::Slider::new(&mut vals[0], 0.0..=1.0).text("x"));
                                        ui.add(egui::Slider::new(&mut vals[1], 0.0..=1.0).text("y"));
                                    }
                                    UniformValue::Vec3(vals) => {
                                        ui.add(egui::Slider::new(&mut vals[0], 0.0..=1.0).text("r"));
                                        ui.add(egui::Slider::new(&mut vals[1], 0.0..=1.0).text("g"));
                                        ui.add(egui::Slider::new(&mut vals[2], 0.0..=1.0).text("b"));
                                    }
                                    UniformValue::Vec4(vals) => {
                                        ui.add(egui::Slider::new(&mut vals[0], 0.0..=1.0).text("r"));
                                        ui.add(egui::Slider::new(&mut vals[1], 0.0..=1.0).text("g"));
                                        ui.add(egui::Slider::new(&mut vals[2], 0.0..=1.0).text("b"));
                                        ui.add(egui::Slider::new(&mut vals[3], 0.0..=1.0).text("a"));
                                    }
                                }
                            });
                        }
                    }
                    ui.separator();
                }

                // Display compilation errors with proper formatting
                let error_text = self.shader_error.lock().clone();
                if let Some(error) = error_text {
                    egui::ScrollArea::vertical()
                        .max_height(150.0)
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new("Shader Compilation Error:")
                                    .color(egui::Color32::RED)
                                    .strong(),
                            );
                            ui.label(
                                egui::RichText::new(error)
                                    .color(egui::Color32::LIGHT_RED)
                                    .family(egui::FontFamily::Monospace),
                            );
                        });
                    ui.separator();
                }
            });

        // --- The CentralPanel is now just for the shader view ---
        // It will automatically fill the space left by the SidePanel.
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::Frame::canvas(ui.style()).show(ui, |ui| {
                self.custom_painting(ui);
            });
        });

        ctx.request_repaint();
    }
    
    // on_exit remains the same
    fn on_exit(&mut self, gl: Option<&glow::Context>) {
        if let Some(gl) = gl {
            self.shader_renderer.lock().destroy(gl);
        }
    }
}

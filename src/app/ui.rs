use crate::app::ShaderApp;
use super::data::*;
use super::file_io;
use crate::FILE_CHECK_TIMEOUT_MS;

use std::time::Duration;
use std::path::Path;

impl eframe::App for ShaderApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Check for hot-reload messages
        if self.shader_update_receiver
            .recv_timeout(Duration::from_millis(FILE_CHECK_TIMEOUT_MS))
            .is_ok()
        {
            self.try_reload_shader();
        }
        
        // Check for post-process hot-reload
        if let Some(receiver) = &self.post_process_update_receiver {
            if receiver.recv_timeout(Duration::from_millis(FILE_CHECK_TIMEOUT_MS)).is_ok() {
                self.try_reload_post_process();
            }
        }

        egui::SidePanel::right("controls_panel")
            .default_width(250.0) 
            .resizable(true)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("Controls");
                    ui.add_space(8.0);

                    // Main shader section
                    ui.label(egui::RichText::new("Main Shader:").strong());
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(
                                self.current_shader_path
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("unknown"),
                            )
                            .family(egui::FontFamily::Monospace)
                            .small(),
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

                    // Time controls
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

                    // Post-processing section
                    ui.label(egui::RichText::new("Post-Processing:").strong());
                    
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut self.post_process_enabled, "Enable").changed() {
                            log::info!("Post-process: {}", self.post_process_enabled);
                        }
                        
                        if let Some(path) = &self.post_process_shader_path {
                            ui.label(egui::RichText::new(
                                path.file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("unknown")
                            ).small().family(egui::FontFamily::Monospace));
                        }
                    });

                    if ui.button("Load Post-Process Shader...").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("GLSL Fragment Shader", &["frag", "glsl"])
                            .pick_file()
                        {
                            self.load_post_process_shader(path);
                        }
                    }

                    // Show post-process error if any
                    if let Some(error) = self.post_process_error.lock().clone() {
                        ui.label(
                            egui::RichText::new("Post-process error:")
                                .color(egui::Color32::RED)
                                .small()
                        );
                        ui.label(
                            egui::RichText::new(&error)
                                .color(egui::Color32::LIGHT_RED)
                                .family(egui::FontFamily::Monospace)
                                .small()
                        );
                    }

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

                    // Main shader uniforms
                    if !self.uniforms.is_empty() {
                        ui.label(egui::RichText::new("Main Shader Uniforms:").strong());
                        render_uniform_controls(ui, &mut self.uniforms, &self.gl);
                        ui.separator();
                        }

                    // Post-process shader uniforms
                    if self.post_process_enabled && !self.post_process_uniforms.is_empty() {
                        ui.label(egui::RichText::new("Post-Process Uniforms:").strong());
                        render_uniform_controls(ui, &mut self.post_process_uniforms, &self.gl);
                        ui.separator();
                    }

                    // Display main shader compilation errors
                    let error_text = self.shader_error.lock().clone();
                    if let Some(error) = error_text {
                        egui::ScrollArea::vertical()
                            .max_height(150.0)
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new("Main Shader Error:")
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
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::Frame::canvas(ui.style()).show(ui, |ui| {
                self.custom_painting(ui);
            });
        });

        ctx.request_repaint();
    }
    
    fn on_exit(&mut self, gl: Option<&glow::Context>) {
        if let Some(gl) = gl {
            self.shader_renderer.lock().destroy(gl);
            
            if let Some(post_renderer) = &self.post_process_renderer {
                post_renderer.lock().destroy(gl);
            }
            
            // Clean up intermediate framebuffer
            unsafe {
                use glow::HasContext as _;
                if let Some(fbo) = self.intermediate_fbo {
                    gl.delete_framebuffer(fbo);
                }
                if let Some(tex) = self.intermediate_texture {
                    gl.delete_texture(tex);
                }
            }
        }
    }
}

// Helper function to render uniform controls (DRY principle)
fn render_uniform_controls(
    ui: &mut egui::Ui, 
    uniforms: &mut std::collections::HashMap<String, UniformInfo>,
    gl: &glow::Context,
) {
    let mut uniform_names: Vec<_> = uniforms.keys().cloned().collect();
    uniform_names.sort();

    for name in uniform_names {
        // Skip built-in and auto-injected uniforms
        if name == "u_resolution" || name == "u_time" || name == "u_mainPass" {
            continue;
        }
        
        if let Some(uniform) = uniforms.get_mut(&name) {
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
                    UniformValue::Sampler2D(texture_handle) => {
                        if let Some(handle) = texture_handle {
                            ui.label(format!("📷 {}", 
                                handle.path.file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("texture")));
                            ui.label(egui::RichText::new(
                                format!("{}x{}", handle.width, handle.height)
                            ).small());
                        } else {
                            ui.label(egui::RichText::new("No texture loaded").small());
                        }
                        
                        if ui.button("Load Texture...").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("Image", &["png", "jpg", "jpeg", "bmp"])
                                .pick_file()
                            {
                                match file_io::load_texture_from_file(gl, &path) {
                                    Ok(new_texture) => {
                                        // Delete old texture if exists
                                        if let Some(old_handle) = texture_handle.as_ref() {
                                            if let Some(old_tex) = old_handle.texture_id {
                                                file_io::delete_texture(gl, old_tex);
                                            }
                                        }

                                        *texture_handle = Some(new_texture.clone());
                                        log::info!("Texture loaded: {}x{}",
                                            new_texture.width, new_texture.height);
                                    }
                                    Err(e) => {
                                        log::error!("Failed to load texture: {}", e);
                                    }
                                }
                            }
                        }
                    }
                }
            });
        }
    }
}
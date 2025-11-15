use crate::app::ShaderApp;
use super::data::{ExportProgress, TextureHandle};

use std::path::Path;
use std::process::Command;
use std::io::Write;
use std::time::Duration;
use glow::HasContext;

// ==========================================
// TEXTURE LOADING
// ==========================================

pub fn load_texture_from_file(
    gl: &glow::Context,
    path: &Path,
) -> Result<TextureHandle, String> {
    // Load image
    let img = image::open(path)
        .map_err(|e| format!("Failed to load image: {}", e))?
        .to_rgba8();
    
    let (width, height) = img.dimensions();
    
    // ← FIX: Flip image vertically (OpenGL expects bottom-left origin)
    let flipped = flip_image_vertically(&img, width, height);
    
    unsafe {
        let texture = gl.create_texture()
            .map_err(|e| format!("Failed to create texture: {}", e))?;
        
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        
        // Upload flipped texture data
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::RGBA as i32,
            width as i32,
            height as i32,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            Some(&flipped),  // ← Use flipped data
        );
        
        // Set texture parameters
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MIN_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MAG_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_S,
            glow::REPEAT as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_T,
            glow::REPEAT as i32,
        );
        
        gl.bind_texture(glow::TEXTURE_2D, None);
        
        Ok(TextureHandle {
            path: path.to_path_buf(),
            texture_id: Some(texture),
            width,
            height,
        })
    }
}

/// Flip image vertically (OpenGL expects bottom-left origin, images are top-left)
fn flip_image_vertically(img: &image::RgbaImage, width: u32, height: u32) -> Vec<u8> {
    let data = img.as_raw();
    let mut flipped = vec![0u8; data.len()];
    let row_size = (width * 4) as usize;  // 4 bytes per pixel (RGBA)
    
    for y in 0..height {
        let src_row_start = (y * width * 4) as usize;
        let src_row_end = src_row_start + row_size;
        let src_row = &data[src_row_start..src_row_end];
        
        // Flip: bottom row becomes top row
        let dst_y = height - 1 - y;
        let dst_row_start = (dst_y * width * 4) as usize;
        let dst_row_end = dst_row_start + row_size;
        let dst_row = &mut flipped[dst_row_start..dst_row_end];
        
        dst_row.copy_from_slice(src_row);
    }
    
    flipped
}

pub fn delete_texture(gl: &glow::Context, texture: glow::Texture) {
    unsafe {
        gl.delete_texture(texture);
    }
}

impl ShaderApp {
    pub fn export_image(&self) {
        let width = self.export_resolution[0];
        let height = self.export_resolution[1];
        
        log::info!("Exporting image at {}x{}", width, height);
        
        let pixels = if self.post_process_enabled && self.post_process_renderer.is_some() {
            // Two-pass export
            self.render_two_pass_to_buffer(self.time, width, height)
        } else {
            // Single-pass export
            self.render_frame_to_buffer(self.time, width, height)
        };
        
        if let Some(pixels) = pixels {
            std::thread::spawn(move || {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("PNG Image", &["png"])
                    .set_file_name("shader_export.png")
                    .save_file()
                {
                    match image::save_buffer(
                        &path,
                        &pixels,
                        width,
                        height,
                        image::ColorType::Rgba8,
                    ) {
                        Ok(_) => log::info!("Image exported successfully to {:?}", path),
                        Err(e) => log::error!("Failed to save image: {}", e),
                    }
                }
            });
        }
    }

    pub fn render_frame_to_buffer(&self, time: f32, width: u32, height: u32) -> Option<Vec<u8>> {
        unsafe {
            use glow::HasContext as _;
            let gl = &*self.gl;
            
            let fbo = gl.create_framebuffer().ok()?;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            
            let texture = gl.create_texture().ok()?;
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.tex_image_2d(
                glow::TEXTURE_2D, 0, glow::RGBA as i32,
                width as i32, height as i32, 0,
                glow::RGBA, glow::UNSIGNED_BYTE, None,
            );
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
            
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D, Some(texture), 0,
            );
            
            if gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
                log::error!("Framebuffer is not complete");
                gl.delete_texture(texture);
                gl.delete_framebuffer(fbo);
                return None;
            }
            
            gl.viewport(0, 0, width as i32, height as i32);
            gl.clear_color(0.0, 0.0, 0.0, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
            
            let size = egui::Vec2::new(width as f32, height as f32);
            self.shader_renderer.lock().paint(gl, time, size, &self.uniforms);
            
            let mut pixels = vec![0u8; (width * height * 4) as usize];
            gl.read_pixels(
                0, 0, width as i32, height as i32,
                glow::RGBA, glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(&mut pixels),
            );
            
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            gl.delete_texture(texture);
            gl.delete_framebuffer(fbo);
            
            // Flip vertically
            let mut flipped = vec![0u8; pixels.len()];
            for y in 0..height {
                let src_row = &pixels[(y * width * 4) as usize..((y + 1) * width * 4) as usize];
                let dst_y = height - 1 - y;
                let dst_row = &mut flipped[(dst_y * width * 4) as usize..((dst_y + 1) * width * 4) as usize];
                dst_row.copy_from_slice(src_row);
            }
            
            Some(flipped)
        }
    }

    // NEW: Two-pass rendering for export
    pub fn render_two_pass_to_buffer(&self, time: f32, width: u32, height: u32) -> Option<Vec<u8>> {
        use glow::HasContext as _;
        
        unsafe {
            let gl = &*self.gl;
            
            // === PASS 1: Render main shader to intermediate texture ===
            let fbo1 = gl.create_framebuffer().ok()?;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo1));
            
            let tex1 = gl.create_texture().ok()?;
            gl.bind_texture(glow::TEXTURE_2D, Some(tex1));
            gl.tex_image_2d(
                glow::TEXTURE_2D, 0, glow::RGBA as i32,
                width as i32, height as i32, 0,
                glow::RGBA, glow::UNSIGNED_BYTE, None,
            );
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
            
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D, Some(tex1), 0,
            );
            
            if gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
                log::error!("Pass 1 framebuffer incomplete");
                gl.delete_texture(tex1);
                gl.delete_framebuffer(fbo1);
                return None;
            }
            
            gl.viewport(0, 0, width as i32, height as i32);
            gl.clear_color(0.0, 0.0, 0.0, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
            
            let size = egui::Vec2::new(width as f32, height as f32);
            self.shader_renderer.lock().paint(gl, time, size, &self.uniforms);
            
            // === PASS 2: Render post-process to final texture ===
            let fbo2 = gl.create_framebuffer().ok()?;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo2));
            
            let tex2 = gl.create_texture().ok()?;
            gl.bind_texture(glow::TEXTURE_2D, Some(tex2));
            gl.tex_image_2d(
                glow::TEXTURE_2D, 0, glow::RGBA as i32,
                width as i32, height as i32, 0,
                glow::RGBA, glow::UNSIGNED_BYTE, None,
            );
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
            
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D, Some(tex2), 0,
            );
            
            if gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
                log::error!("Pass 2 framebuffer incomplete");
                gl.delete_texture(tex1);
                gl.delete_framebuffer(fbo1);
                gl.delete_texture(tex2);
                gl.delete_framebuffer(fbo2);
                return None;
            }
            
            gl.viewport(0, 0, width as i32, height as i32);
            gl.clear_color(0.0, 0.0, 0.0, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
            
            // Create post-process uniforms with main pass texture
            let mut post_uniforms = self.post_process_uniforms.clone();
            post_uniforms.insert(
                "u_mainPass".to_string(),
                super::data::UniformInfo {
                    uniform_type: super::data::UniformType::Sampler2D,
                    value: super::data::UniformValue::Sampler2D(Some(
                        super::data::TextureHandle {
                            path: std::path::PathBuf::from("[export_pass1]"),
                            texture_id: Some(tex1),
                            width,
                            height,
                        }
                    )),
                },
            );
            
            if let Some(post_renderer) = &self.post_process_renderer {
                post_renderer.lock().paint(gl, time, size, &post_uniforms);
            }
            
            // Read pixels from final framebuffer
            let mut pixels = vec![0u8; (width * height * 4) as usize];
            gl.read_pixels(
                0, 0, width as i32, height as i32,
                glow::RGBA, glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(&mut pixels),
            );
            
            // Cleanup
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            gl.delete_texture(tex1);
            gl.delete_framebuffer(fbo1);
            gl.delete_texture(tex2);
            gl.delete_framebuffer(fbo2);
            
            // Flip vertically
            let mut flipped = vec![0u8; pixels.len()];
            for y in 0..height {
                let src_row = &pixels[(y * width * 4) as usize..((y + 1) * width * 4) as usize];
                let dst_y = height - 1 - y;
                let dst_row = &mut flipped[(dst_y * width * 4) as usize..((dst_y + 1) * width * 4) as usize];
                dst_row.copy_from_slice(src_row);
            }
            
            Some(flipped)
        }
    }

    pub fn export_video(&mut self) {
        let width = self.export_resolution[0];
        let height = self.export_resolution[1];
        let total_frames = self.video_duration_frames;
        let fps = self.video_fps;
        
        log::info!("Starting video export: {}x{} @ {}fps, {} frames", 
                   width, height, fps, total_frames);
        
        let output_path = match rfd::FileDialog::new()
            .add_filter("MP4 Video", &["mp4"])
            .set_file_name("shader_export.mp4")
            .save_file()
        {
            Some(path) => path,
            None => {
                log::info!("Export cancelled");
                return;
            }
        };
        
        if !self.ffmpeg_available {
            log::error!("FFmpeg not available");
            *self.export_progress.lock() = Some(ExportProgress {
                current_frame: 0,
                total_frames,
                status: "FFmpeg not available!".to_string(),
            });
            return;
        }
        
        let mut ffmpeg_child = match Command::new("ffmpeg")
            .args([
                "-y",
                "-f", "rawvideo",
                "-pixel_format", "rgba",
                "-video_size", &format!("{}x{}", width, height),
                "-framerate", &fps.to_string(),
                "-i", "pipe:0",
                "-c:v", "libx264",
                "-preset", "veryfast",
                "-crf", "18",
                "-pix_fmt", "yuv420p",
                output_path.to_str().unwrap(),
            ])
            .stdin(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(e) => {
                log::error!("Failed to start FFmpeg: {}", e);
                *self.export_progress.lock() = Some(ExportProgress {
                    current_frame: 0,
                    total_frames,
                    status: format!("Failed to start FFmpeg: {}", e),
                });
                return;
            }
        };
        
        let mut stdin = ffmpeg_child.stdin.take().unwrap();
        
        *self.export_progress.lock() = Some(ExportProgress {
            current_frame: 0,
            total_frames,
            status: "Rendering and encoding...".to_string(),
        });
        
        // Determine which rendering method to use
        let use_post_process = self.post_process_enabled && self.post_process_renderer.is_some();
        
        for frame in 0..total_frames {
            let time = frame as f32 / fps as f32;
            
            if frame % 10 == 0 {
                *self.export_progress.lock() = Some(ExportProgress {
                    current_frame: frame,
                    total_frames,
                    status: format!("Encoding frame {}/{}", frame + 1, total_frames),
                });
            }
            
            // Render with or without post-processing
            let pixels = if use_post_process {
                self.render_two_pass_to_buffer(time, width, height)
            } else {
                self.render_frame_to_buffer(time, width, height)
            };
            
            if let Some(pixels) = pixels {
                if let Err(e) = stdin.write_all(&pixels) {
                    log::error!("Failed to write frame {}: {}", frame, e);
                    *self.export_progress.lock() = None;
                    return;
                }
            } else {
                log::error!("Failed to render frame {}", frame);
                *self.export_progress.lock() = None;
                return;
            }
        }
        
        drop(stdin);
        
        match ffmpeg_child.wait() {
            Ok(status) if status.success() => {
                log::info!("Video exported successfully to {:?}", output_path);
                *self.export_progress.lock() = Some(ExportProgress {
                    current_frame: total_frames,
                    total_frames,
                    status: "Export complete!".to_string(),
                });
            }
            Ok(status) => {
                log::error!("FFmpeg failed with status: {}", status);
                *self.export_progress.lock() = Some(ExportProgress {
                    current_frame: total_frames,
                    total_frames,
                    status: "FFmpeg encoding failed!".to_string(),
                });
            }
            Err(e) => {
                log::error!("Failed to wait for FFmpeg: {}", e);
                *self.export_progress.lock() = Some(ExportProgress {
                    current_frame: total_frames,
                    total_frames,
                    status: format!("FFmpeg error: {}", e),
                });
            }
        }
        
        let progress = self.export_progress.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(3));
            *progress.lock() = None;
        });
    }
}

/// Helper for flipping raw pixel data (used by export functions)
fn flip_image_vertically_raw(data: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut flipped = vec![0u8; data.len()];
    let row_size = (width * 4) as usize;
    
    for y in 0..height {
        let src_row = &data[(y * width * 4) as usize..((y + 1) * width * 4) as usize];
        let dst_y = height - 1 - y;
        let dst_row = &mut flipped[(dst_y * width * 4) as usize..((dst_y + 1) * width * 4) as usize];
        dst_row.copy_from_slice(src_row);
    }
    
    flipped
}

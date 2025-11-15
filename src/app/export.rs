use crate::app::ShaderApp;
use super::data::ExportProgress;

use std::process::Command;
use std::io::Write;
use std::time::Duration;

impl ShaderApp {
    pub fn export_image(&self) {
        let width = self.export_resolution[0];
        let height = self.export_resolution[1];
        
        log::info!("Exporting image at {}x{}", width, height);
        
        // SAFETY: Creating framebuffer and texture for offscreen rendering
        // with valid OpenGL context
        unsafe {
            use glow::HasContext as _;
            let gl = &*self.gl;
            
            // Create framebuffer
            let fbo = match gl.create_framebuffer() {
                Ok(fbo) => fbo,
                Err(e) => {
                    log::error!("Failed to create framebuffer: {}", e);
                    return;
                }
            };
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            
            // Create texture
            let texture = match gl.create_texture() {
                Ok(tex) => tex,
                Err(e) => {
                    log::error!("Failed to create texture: {}", e);
                    gl.delete_framebuffer(fbo);
                    return;
                }
            };
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA as i32,
                width as i32,
                height as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                None,
            );
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
            
            // Attach texture to framebuffer
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(texture),
                0,
            );
            
            // Check framebuffer status
            if gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
                log::error!("Framebuffer is not complete");
                gl.delete_texture(texture);
                gl.delete_framebuffer(fbo);
                return;
            }
            
            // Set viewport and clear
            gl.viewport(0, 0, width as i32, height as i32);
            gl.clear_color(0.0, 0.0, 0.0, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
            
            // Render shader
            let size = egui::Vec2::new(width as f32, height as f32);
            self.shader_renderer.lock().paint(gl, self.time, size, &self.uniforms);
            
            // Read pixels
            let mut pixels = vec![0u8; (width * height * 4) as usize];
            gl.read_pixels(
                0,
                0,
                width as i32,
                height as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(&mut pixels),
            );
            
            // Cleanup
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            gl.delete_texture(texture);
            gl.delete_framebuffer(fbo);
            
            // Save image in a separate thread to avoid blocking
            std::thread::spawn(move || {
                // Flip image vertically (OpenGL reads bottom-to-top)
                let mut flipped = vec![0u8; pixels.len()];
                for y in 0..height {
                    let src_row = &pixels[(y * width * 4) as usize..((y + 1) * width * 4) as usize];
                    let dst_y = height - 1 - y;
                    let dst_row = &mut flipped[(dst_y * width * 4) as usize..((dst_y + 1) * width * 4) as usize];
                    dst_row.copy_from_slice(src_row);
                }
                
                // Save with file dialog
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("PNG Image", &["png"])
                    .set_file_name("shader_export.png")
                    .save_file()
                {
                    match image::save_buffer(
                        &path,
                        &flipped,
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
        // SAFETY: Creating framebuffer and texture for offscreen rendering
        unsafe {
            use glow::HasContext as _;
            let gl = &*self.gl;
            
            // Create framebuffer
            let fbo = gl.create_framebuffer().ok()?;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            
            // Create texture
            let texture = gl.create_texture().ok()?;
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA as i32,
                width as i32,
                height as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                None,
            );
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
            
            // Attach texture to framebuffer
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(texture),
                0,
            );
            
            // Check framebuffer status
            if gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
                log::error!("Framebuffer is not complete");
                gl.delete_texture(texture);
                gl.delete_framebuffer(fbo);
                return None;
            }
            
            // Set viewport and clear
            gl.viewport(0, 0, width as i32, height as i32);
            gl.clear_color(0.0, 0.0, 0.0, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
            
            // Render shader
            let size = egui::Vec2::new(width as f32, height as f32);
            self.shader_renderer.lock().paint(gl, time, size, &self.uniforms);
            
            // Read pixels
            let mut pixels = vec![0u8; (width * height * 4) as usize];
            gl.read_pixels(
                0,
                0,
                width as i32,
                height as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(&mut pixels),
            );
            
            // Cleanup
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            gl.delete_texture(texture);
            gl.delete_framebuffer(fbo);
            
            // Flip image vertically (OpenGL reads bottom-to-top)
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
        
        // Ask user for output file first
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
        
        // Start FFmpeg process with stdin pipe
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
        
        // Update progress
        *self.export_progress.lock() = Some(ExportProgress {
            current_frame: 0,
            total_frames,
            status: "Rendering and encoding...".to_string(),
        });
        
        // Render frames and stream directly to FFmpeg
        for frame in 0..total_frames {
            let time = frame as f32 / fps as f32;
            
            // Update progress every 10 frames to reduce overhead
            if frame % 10 == 0 {
                *self.export_progress.lock() = Some(ExportProgress {
                    current_frame: frame,
                    total_frames,
                    status: format!("Encoding frame {}/{}", frame + 1, total_frames),
                });
            }
            
            // Render frame
            if let Some(pixels) = self.render_frame_to_buffer(time, width, height) {
                // Write raw RGBA data directly to FFmpeg stdin
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
        
        // Close stdin and wait for FFmpeg to finish
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
        
        // Clear progress after a delay
        let progress = self.export_progress.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(3));
            *progress.lock() = None;
        });
    }
}

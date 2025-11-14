// Cargo.toml
/*
[package]
name = "shader-app"
version = "0.1.0"
edition = "2021"

[dependencies]
eframe = "0.23.0"
egui = "0.23.0"
egui_glow = "0.23.0"
env_logger = "0.11.8"
glow = "0.12.0"
log = "0.4.17"
notify = "6.1.1"
regex = "1.10"
rfd = "0.12"
image = "0.24"
*/

use std::collections::HashMap;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::io::Write;

use eframe::egui_glow;
use egui::mutex::Mutex;
use egui_glow::glow;
use notify::{RecommendedWatcher, Watcher, RecursiveMode};

// Configuration constants
const DEFAULT_SHADER_PATH: &str = "shaders/shader.frag";
const RELOAD_DEBOUNCE_MS: u64 = 100;
const FILE_CHECK_TIMEOUT_MS: u64 = 1;

/// Get the default shader path relative to the executable
fn get_default_shader_path() -> PathBuf {
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let shader_path = exe_dir.join(DEFAULT_SHADER_PATH);
            if shader_path.exists() {
                return shader_path;
            }
        }
    }
    
    // Fallback to current working directory
    PathBuf::from(DEFAULT_SHADER_PATH)
}

/// Check if FFmpeg is available on the system
fn is_ffmpeg_available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Represents a detected uniform and its metadata
#[derive(Debug, Clone)]
pub struct UniformInfo {
    _name: String,
    uniform_type: UniformType,
    value: UniformValue,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UniformType {
    Float,
    Vec2,
    Vec3,
    Vec4,
}

#[derive(Debug, Clone)]
pub enum UniformValue {
    Float(f32),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
}

impl UniformValue {
    fn default_for_type(uniform_type: &UniformType) -> Self {
        match uniform_type {
            UniformType::Float => UniformValue::Float(1.0),
            UniformType::Vec2 => UniformValue::Vec2([0.5, 0.5]),
            UniformType::Vec3 => UniformValue::Vec3([0.5, 0.5, 0.5]),
            UniformType::Vec4 => UniformValue::Vec4([1.0, 1.0, 1.0, 1.0]),
        }
    }
}

pub struct ShaderApp {
    gl: Arc<glow::Context>,
    shader_renderer: Arc<Mutex<ShaderRenderer>>,
    time: f32,
    auto_time: bool,
    shader_error: Arc<Mutex<Option<String>>>,
    watcher: Option<RecommendedWatcher>,
    shader_update_receiver: mpsc::Receiver<()>,
    last_reload: Instant,
    uniforms: HashMap<String, UniformInfo>,
    current_shader_path: PathBuf,
    export_resolution: [u32; 2],
    video_duration_frames: u32,
    video_fps: u32,
    ffmpeg_available: bool,
    export_progress: Arc<Mutex<Option<ExportProgress>>>,
}

#[derive(Debug, Clone)]
struct ExportProgress {
    current_frame: u32,
    total_frames: u32,
    status: String,
}

impl ShaderApp {
    pub fn new<'a>(cc: &'a eframe::CreationContext<'a>) -> Option<Self> {
        let gl = cc.gl.as_ref()?.clone();

        let shader_path = get_default_shader_path();

        // SAFETY: Reading shader file during initialization.
        // If the file is missing, we fail fast with a clear error message.
        let initial_shader_source = std::fs::read_to_string(&shader_path)
            .expect("Failed to read fragment shader on startup");
        
        // Detect uniforms from the shader source
        let detected_uniforms = parse_uniforms(&initial_shader_source);
        
        // SAFETY: Compiling initial shader with OpenGL context.
        // Context is guaranteed to be valid during CreationContext.
        let shader_renderer = ShaderRenderer::new(&gl, &initial_shader_source)
            .expect("Failed to compile initial shader");

        // Set up file watcher with debouncing
        let (tx, rx) = mpsc::channel();
        let watcher = Self::create_watcher(&shader_path, tx);

        cc.egui_ctx.set_visuals(egui::Visuals::dark());

        let ffmpeg_available = is_ffmpeg_available();
        if ffmpeg_available {
            log::info!("FFmpeg detected and available for video export");
        } else {
            log::warn!("FFmpeg not found - video export will save PNG sequence only");
        }

        Some(Self {
            gl,
            shader_renderer: Arc::new(Mutex::new(shader_renderer)),
            time: 0.0,
            auto_time: true,
            shader_error: Arc::new(Mutex::new(None)),
            watcher,
            shader_update_receiver: rx,
            last_reload: Instant::now(),
            uniforms: detected_uniforms,
            current_shader_path: shader_path,
            export_resolution: [1920, 1080],
            video_duration_frames: 300,
            video_fps: 60,
            ffmpeg_available,
            export_progress: Arc::new(Mutex::new(None)),
        })
    }

    fn create_watcher(path: &Path, tx: mpsc::Sender<()>) -> Option<RecommendedWatcher> {
        let mut watcher = notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                if event.kind.is_modify() || event.kind.is_create() {
                    let _ = tx.send(());
                }
            }
        }).ok()?;
        
        // SAFETY: Watching a single file path that exists.
        // Errors are handled gracefully by returning None.
        watcher.watch(path, RecursiveMode::NonRecursive).ok()?;
        Some(watcher)
    }

    fn load_shader_file(&mut self, path: PathBuf) {
        // Stop watching the old file
        if let Some(watcher) = &mut self.watcher {
            let _ = watcher.unwatch(&self.current_shader_path);
        }

        // Update current path
        self.current_shader_path = path;

        // Create new watcher for the new file
        let (tx, rx) = mpsc::channel();
        self.watcher = Self::create_watcher(&self.current_shader_path, tx);
        self.shader_update_receiver = rx;

        // Load and compile the new shader
        match std::fs::read_to_string(&self.current_shader_path) {
            Ok(shader_source) => {
                let new_uniforms = parse_uniforms(&shader_source);
                
                match ShaderRenderer::new(&self.gl, &shader_source) {
                    Ok(new_renderer) => {
                        {
                            let mut renderer_guard = self.shader_renderer.lock();
                            renderer_guard.destroy(&self.gl);
                            *renderer_guard = new_renderer;
                        }
                        
                        *self.shader_error.lock() = None;
                        self.uniforms = new_uniforms;
                        log::info!("Shader loaded successfully: {:?}", self.current_shader_path);
                    }
                    Err(e) => {
                        *self.shader_error.lock() = Some(e.clone());
                        log::error!("Shader compilation failed: {}", e);
                    }
                }
            }
            Err(e) => {
                let error_message = format!("Failed to read shader file: {}", e);
                *self.shader_error.lock() = Some(error_message);
            }
        }
    }

    fn try_reload_shader(&mut self) {
        // Debounce: ignore rapid successive file changes
        if self.last_reload.elapsed() < Duration::from_millis(RELOAD_DEBOUNCE_MS) {
            return;
        }
        
        log::info!("Shader file changed, attempting to reload...");
        
        match std::fs::read_to_string(&self.current_shader_path) {
            Ok(new_source) => {
                // Detect uniforms before compilation
                let new_uniforms = parse_uniforms(&new_source);
                
                // SAFETY: Compiling shader with valid OpenGL context.
                // The context is owned by the app and guaranteed to be valid.
                match ShaderRenderer::new(&self.gl, &new_source) {
                    Ok(new_renderer) => {
                        {
                            let mut renderer_guard = self.shader_renderer.lock();
                            // SAFETY: Destroying old shader resources with valid context.
                            // Resources were created with the same context.
                            renderer_guard.destroy(&self.gl);
                            *renderer_guard = new_renderer;
                        } // Drop the lock before calling merge_uniforms
                        
                        *self.shader_error.lock() = None;
                        
                        // Merge new uniforms with existing ones, preserving values where possible
                        self.merge_uniforms(new_uniforms);
                        
                        self.last_reload = Instant::now();
                        log::info!("Shader reloaded successfully!");
                    }
                    Err(e) => {
                        *self.shader_error.lock() = Some(e.clone());
                        log::error!("Shader compilation failed: {}", e);
                    }
                }
            }
            Err(e) => {
                let error_message = format!("Failed to read shader file: {}", e);
                *self.shader_error.lock() = Some(error_message);
            }
        }
    }

    fn merge_uniforms(&mut self, new_uniforms: HashMap<String, UniformInfo>) {
        let mut merged = HashMap::new();
        
        for (name, new_info) in new_uniforms {
            // If uniform existed before and types match, keep the old value
            if let Some(old_info) = self.uniforms.get(&name) {
                if old_info.uniform_type == new_info.uniform_type {
                    merged.insert(name, old_info.clone());
                    continue;
                }
            }
            // Otherwise use the new uniform with default value
            merged.insert(name, new_info);
        }
        
        self.uniforms = merged;
    }

    fn export_image(&self) {
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

    fn render_frame_to_buffer(&self, time: f32, width: u32, height: u32) -> Option<Vec<u8>> {
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

    fn export_video(&mut self) {
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

impl ShaderApp {
    fn custom_painting(&mut self, ui: &mut egui::Ui) {
        let (rect, _response) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());

        if self.auto_time {
            self.time += ui.input(|i| i.stable_dt);
        }
        let time = self.time;
        
        let shader_renderer = self.shader_renderer.clone();
        let uniforms = self.uniforms.clone();

        let cb = egui_glow::CallbackFn::new(move |_info, painter| {
            shader_renderer.lock().paint(painter.gl(), time, rect.size(), &uniforms);
        });

        let callback = egui::PaintCallback {
            rect,
            callback: Arc::new(cb),
        };
        ui.painter().add(callback);
    }
}

struct ShaderRenderer {
    program: glow::Program,
    vertex_array: glow::VertexArray,
}

impl ShaderRenderer {
    fn new(gl: &glow::Context, fragment_shader_source: &str) -> Result<Self, String> {
        use glow::HasContext as _;

        let shader_version = egui_glow::ShaderVersion::get(gl);

        // SAFETY: All OpenGL calls are made with a valid context.
        // Error handling ensures resources are cleaned up on failure.
        unsafe {
            let program = gl.create_program().map_err(|e| e.to_string())?;

            let vertex_shader_source = r#"
                out vec2 v_uv;
                
                const vec2 verts[4] = vec2[4](
                    vec2(-1.0, -1.0), vec2(1.0, -1.0),
                    vec2(-1.0, 1.0),  vec2(1.0, 1.0)
                );
                
                const vec2 uvs[4] = vec2[4](
                    vec2(0.0, 0.0), vec2(1.0, 0.0),
                    vec2(0.0, 1.0), vec2(1.0, 1.0)
                );
                
                void main() {
                    v_uv = uvs[gl_VertexID];
                    gl_Position = vec4(verts[gl_VertexID], 0.0, 1.0);
                }
            "#;

            let shader_sources = [
                (glow::VERTEX_SHADER, vertex_shader_source),
                (glow::FRAGMENT_SHADER, fragment_shader_source),
            ];
            
            let mut shaders = Vec::with_capacity(shader_sources.len());

            for (shader_type, shader_source) in shader_sources.iter() {
                let shader = gl.create_shader(*shader_type).map_err(|e| e.to_string())?;
                
                let source_with_version = if *shader_type == glow::FRAGMENT_SHADER {
                    shader_source.to_string()
                } else {
                    format!("{}\n{}", shader_version.version_declaration(), shader_source)
                };

                gl.shader_source(shader, &source_with_version);
                gl.compile_shader(shader);

                if !gl.get_shader_compile_status(shader) {
                    let info_log = gl.get_shader_info_log(shader);
                    gl.delete_shader(shader);
                    // Clean up program and any previously compiled shaders
                    for prev_shader in shaders {
                        gl.detach_shader(program, prev_shader);
                        gl.delete_shader(prev_shader);
                    }
                    gl.delete_program(program);
                    return Err(info_log);
                }
                
                gl.attach_shader(program, shader);
                shaders.push(shader);
            }

            gl.link_program(program);
            if !gl.get_program_link_status(program) {
                let info_log = gl.get_program_info_log(program);
                for shader in shaders {
                    gl.detach_shader(program, shader);
                    gl.delete_shader(shader);
                }
                gl.delete_program(program);
                return Err(info_log);
            }

            // Clean up shader objects after successful linking
            for shader in shaders {
                gl.detach_shader(program, shader);
                gl.delete_shader(shader);
            }

            let vertex_array = gl.create_vertex_array().map_err(|e| e.to_string())?;

            Ok(Self { program, vertex_array })
        }
    }

    fn destroy(&self, gl: &glow::Context) {
        use glow::HasContext as _;
        // SAFETY: Deleting resources that were created with the same context.
        // This is called during cleanup when the context is still valid.
        unsafe {
            gl.delete_program(self.program);
            gl.delete_vertex_array(self.vertex_array);
        }
    }

    fn paint(&self, gl: &glow::Context, time: f32, size: egui::Vec2, uniforms: &HashMap<String, UniformInfo>) {
        use glow::HasContext as _;
        // SAFETY: Rendering with a valid OpenGL context and program.
        // All uniform locations are queried before use.
        unsafe {
            gl.use_program(Some(self.program));
            
            // Set built-in uniforms
            if let Some(loc) = gl.get_uniform_location(self.program, "u_time") {
                gl.uniform_1_f32(Some(&loc), time);
            }
            if let Some(loc) = gl.get_uniform_location(self.program, "u_resolution") {
                gl.uniform_2_f32(Some(&loc), size.x, size.y);
            }
            
            // Set custom uniforms
            for (name, uniform_info) in uniforms {
                if name == "u_resolution" || name == "u_time" {
                    continue;
                }
                if let Some(loc) = gl.get_uniform_location(self.program, name) {
                    match &uniform_info.value {
                        UniformValue::Float(val) => {
                            gl.uniform_1_f32(Some(&loc), *val);
                        }
                        UniformValue::Vec2(vals) => {
                            gl.uniform_2_f32(Some(&loc), vals[0], vals[1]);
                        }
                        UniformValue::Vec3(vals) => {
                            gl.uniform_3_f32(Some(&loc), vals[0], vals[1], vals[2]);
                        }
                        UniformValue::Vec4(vals) => {
                            gl.uniform_4_f32(Some(&loc), vals[0], vals[1], vals[2], vals[3]);
                        }
                    }
                }
            }
            
            gl.bind_vertex_array(Some(self.vertex_array));
            gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);
        }
    }
}

/// Parse GLSL shader source to detect uniform declarations
fn parse_uniforms(shader_source: &str) -> HashMap<String, UniformInfo> {
    use regex::Regex;
    
    let mut uniforms = HashMap::new();
    
    // Regex to match: uniform <type> <name>;
    let re = Regex::new(r"uniform\s+(float|vec2|vec3|vec4)\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*;")
        .expect("Invalid regex pattern");
    
    for cap in re.captures_iter(shader_source) {
        let type_str = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let name = cap.get(2).map(|m| m.as_str()).unwrap_or("");
        
        let uniform_type = match type_str {
            "float" => UniformType::Float,
            "vec2" => UniformType::Vec2,
            "vec3" => UniformType::Vec3,
            "vec4" => UniformType::Vec4,
            _ => continue,
        };
        
        let value = UniformValue::default_for_type(&uniform_type);
        
        uniforms.insert(
            name.to_string(),
            UniformInfo {
                _name: name.to_string(),
                uniform_type,
                value,
            }
        );
    }
    
    uniforms
}

fn main() {
    env_logger::init();

    let native_options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        always_on_top: true,
        ..Default::default()
    };

    eframe::run_native(
        "Shader Editor",
        native_options,
        Box::new(|cc| Box::new(ShaderApp::new(cc).expect("Failed to create ShaderApp"))),
    ).expect("Failed to run eframe");
}
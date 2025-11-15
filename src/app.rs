mod render_engine;
mod data;
mod file_io;
mod ui;

use data::*;
use crate::{RELOAD_DEBOUNCE_MS, DEFAULT_SHADER_PATH, FILE_CHECK_TIMEOUT_MS};

use render_engine::ShaderRenderer;

use std::collections::HashMap;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};
use std::path::{Path, PathBuf};
use std::process::Command;

use eframe::egui_glow;
use egui::mutex::Mutex;
use egui_glow::glow;
use notify::{RecommendedWatcher, Watcher, RecursiveMode};

pub struct ShaderApp {
    pub(crate) gl: Arc<glow::Context>,
    pub(crate) shader_renderer: Arc<Mutex<ShaderRenderer>>,
    pub(crate) time: f32,
    pub(crate) auto_time: bool,
    pub(crate) shader_error: Arc<Mutex<Option<String>>>,
    pub(crate) watcher: Option<RecommendedWatcher>,
    pub(crate) shader_update_receiver: mpsc::Receiver<()>,
    pub(crate) last_reload: Instant,
    pub(crate) uniforms: HashMap<String, UniformInfo>,
    pub(crate) current_shader_path: PathBuf,
    pub(crate) export_resolution: [u32; 2],
    pub(crate) video_duration_frames: u32,
    pub(crate) video_fps: u32,
    pub(crate) ffmpeg_available: bool,
    pub(crate) export_progress: Arc<Mutex<Option<ExportProgress>>>,
    
    // Post-processing
    pub(crate) post_process_enabled: bool,
    pub(crate) post_process_shader_path: Option<PathBuf>,
    pub(crate) post_process_renderer: Option<Arc<Mutex<ShaderRenderer>>>,
    pub(crate) post_process_uniforms: HashMap<String, UniformInfo>,
    pub(crate) post_process_error: Arc<Mutex<Option<String>>>,
    pub(crate) post_process_watcher: Option<RecommendedWatcher>,
    pub(crate) post_process_update_receiver: Option<mpsc::Receiver<()>>,
    pub(crate) post_process_last_reload: Instant,
    
    // Intermediate framebuffer
    pub(crate) intermediate_fbo: Option<glow::Framebuffer>,
    pub(crate) intermediate_texture: Option<glow::Texture>,
    pub(crate) intermediate_size: (u32, u32),
}

impl ShaderApp {
    pub fn new<'a>(cc: &'a eframe::CreationContext<'a>) -> Option<Self> {
        let gl = cc.gl.as_ref()?.clone();

        let shader_path = get_default_shader_path();

        let initial_shader_source = std::fs::read_to_string(&shader_path)
            .expect("Failed to read fragment shader on startup");
        
        let detected_uniforms = parse_uniforms(&initial_shader_source);
        
        let shader_renderer = ShaderRenderer::new(&gl, &initial_shader_source)
            .expect("Failed to compile initial shader");

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
            video_fps: 30,
            ffmpeg_available,
            export_progress: Arc::new(Mutex::new(None)),
            
            // Post-processing
            post_process_enabled: false,
            post_process_shader_path: None,
            post_process_renderer: None,
            post_process_uniforms: HashMap::new(),
            post_process_error: Arc::new(Mutex::new(None)),
            post_process_watcher: None,
            post_process_update_receiver: None,
            post_process_last_reload: Instant::now(),
            
            // Intermediate framebuffer
            intermediate_fbo: None,
            intermediate_texture: None,
            intermediate_size: (0, 0),
        })
    }

    pub fn create_watcher(path: &Path, tx: mpsc::Sender<()>) -> Option<RecommendedWatcher> {
        let mut watcher = notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                if event.kind.is_modify() || event.kind.is_create() {
                    let _ = tx.send(());
                }
            }
        }).ok()?;
        
        watcher.watch(path, RecursiveMode::NonRecursive).ok()?;
        Some(watcher)
    }

    pub fn load_shader_file(&mut self, path: PathBuf) {
        if let Some(watcher) = &mut self.watcher {
            let _ = watcher.unwatch(&self.current_shader_path);
        }

        self.current_shader_path = path;

        let (tx, rx) = mpsc::channel();
        self.watcher = Self::create_watcher(&self.current_shader_path, tx);
        self.shader_update_receiver = rx;

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

    pub fn load_post_process_shader(&mut self, path: PathBuf) {
        if let Some(watcher) = &mut self.post_process_watcher {
            if let Some(old_path) = &self.post_process_shader_path {
                let _ = watcher.unwatch(old_path);
            }
        }

        self.post_process_shader_path = Some(path.clone());

        let (tx, rx) = mpsc::channel();
        self.post_process_watcher = Self::create_watcher(&path, tx);
        self.post_process_update_receiver = Some(rx);

        match std::fs::read_to_string(&path) {
            Ok(shader_source) => {
                let new_uniforms = parse_uniforms(&shader_source);
                
                match ShaderRenderer::new(&self.gl, &shader_source) {
                    Ok(new_renderer) => {
                        if let Some(old_renderer) = &self.post_process_renderer {
                            old_renderer.lock().destroy(&self.gl);
                        }
                        
                        self.post_process_renderer = Some(Arc::new(Mutex::new(new_renderer)));
                        *self.post_process_error.lock() = None;
                        self.post_process_uniforms = new_uniforms;
                        log::info!("Post-process shader loaded: {:?}", path);
                    }
                    Err(e) => {
                        *self.post_process_error.lock() = Some(e.clone());
                        log::error!("Post-process shader compilation failed: {}", e);
                    }
                }
            }
            Err(e) => {
                let error_message = format!("Failed to read shader file: {}", e);
                *self.post_process_error.lock() = Some(error_message);
            }
        }
    }

    pub fn try_reload_shader(&mut self) {
        if self.last_reload.elapsed() < Duration::from_millis(RELOAD_DEBOUNCE_MS) {
            return;
        }
        
        log::info!("Shader file changed, attempting to reload...");
        
        match std::fs::read_to_string(&self.current_shader_path) {
            Ok(new_source) => {
                let new_uniforms = parse_uniforms(&new_source);
                
                match ShaderRenderer::new(&self.gl, &new_source) {
                    Ok(new_renderer) => {
                        {
                            let mut renderer_guard = self.shader_renderer.lock();
                            renderer_guard.destroy(&self.gl);
                            *renderer_guard = new_renderer;
                        }
                        
                        *self.shader_error.lock() = None;
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

    pub fn try_reload_post_process(&mut self) {
        if self.post_process_last_reload.elapsed() < Duration::from_millis(RELOAD_DEBOUNCE_MS) {
            return;
        }
        
        if let Some(path) = &self.post_process_shader_path {
            log::info!("Post-process shader file changed, attempting to reload...");
            
            match std::fs::read_to_string(path) {
                Ok(new_source) => {
                    let new_uniforms = parse_uniforms(&new_source);
                    
                    match ShaderRenderer::new(&self.gl, &new_source) {
                        Ok(new_renderer) => {
                            if let Some(old_renderer) = &self.post_process_renderer {
                                old_renderer.lock().destroy(&self.gl);
                            }
                            
                            self.post_process_renderer = Some(Arc::new(Mutex::new(new_renderer)));
                            *self.post_process_error.lock() = None;
                            self.merge_post_process_uniforms(new_uniforms);
                            self.post_process_last_reload = Instant::now();
                            log::info!("Post-process shader reloaded successfully!");
                        }
                        Err(e) => {
                            *self.post_process_error.lock() = Some(e.clone());
                            log::error!("Post-process shader compilation failed: {}", e);
                        }
                    }
                }
                Err(e) => {
                    let error_message = format!("Failed to read shader file: {}", e);
                    *self.post_process_error.lock() = Some(error_message);
                }
            }
        }
    }

    pub fn merge_uniforms(&mut self, new_uniforms: HashMap<String, UniformInfo>) {
        let mut merged = HashMap::new();
        
        for (name, new_info) in new_uniforms {
            if let Some(old_info) = self.uniforms.get(&name) {
                if old_info.uniform_type == new_info.uniform_type {
                    merged.insert(name, old_info.clone());
                    continue;
                }
            }
            merged.insert(name, new_info);
        }
        
        self.uniforms = merged;
    }

    pub fn merge_post_process_uniforms(&mut self, new_uniforms: HashMap<String, UniformInfo>) {
        let mut merged = HashMap::new();
        
        for (name, new_info) in new_uniforms {
            if let Some(old_info) = self.post_process_uniforms.get(&name) {
                if old_info.uniform_type == new_info.uniform_type {
                    merged.insert(name, old_info.clone());
                    continue;
                }
            }
            merged.insert(name, new_info);
        }
        
        self.post_process_uniforms = merged;
    }

    fn ensure_intermediate_fbo(&mut self, width: u32, height: u32) {
        use glow::HasContext as _;
        
        // Check if we need to recreate
        if self.intermediate_size == (width, height) && 
           self.intermediate_fbo.is_some() && 
           self.intermediate_texture.is_some() {
            return; // Already correct size
        }
        
        unsafe {
            // Clean up old resources
            if let Some(fbo) = self.intermediate_fbo {
                self.gl.delete_framebuffer(fbo);
            }
            if let Some(tex) = self.intermediate_texture {
                self.gl.delete_texture(tex);
            }
            
            // Create framebuffer
            let fbo = self.gl.create_framebuffer().unwrap();
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            
            // Create texture
            let texture = self.gl.create_texture().unwrap();
            self.gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            self.gl.tex_image_2d(
                glow::TEXTURE_2D, 0, glow::RGBA as i32,
                width as i32, height as i32, 0,
                glow::RGBA, glow::UNSIGNED_BYTE, None,
            );
            self.gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
            self.gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
            self.gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
            self.gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
            
            // Attach to framebuffer
            self.gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D, Some(texture), 0,
            );
            
            // Check status
            if self.gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
                log::error!("Intermediate framebuffer incomplete!");
            }
            
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            
            self.intermediate_fbo = Some(fbo);
            self.intermediate_texture = Some(texture);
            self.intermediate_size = (width, height);
            
            log::info!("Created intermediate framebuffer: {}x{}", width, height);
        }
    }

    pub fn custom_painting(&mut self, ui: &mut egui::Ui) {
        let (rect, _response) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());

        if self.auto_time {
            self.time += ui.input(|i| i.stable_dt);
        }
        let time = self.time;
        
        // Get the actual pixels_per_point for DPI scaling
        let pixels_per_point = ui.ctx().pixels_per_point();
        
        let size = rect.size();
        // Convert to physical pixels
        let width = (size.x * pixels_per_point) as u32;
        let height = (size.y * pixels_per_point) as u32;
        
        // Determine if we need two-pass rendering
        let use_post_process = self.post_process_enabled && 
                            self.post_process_renderer.is_some();
        
        if use_post_process {
            // Ensure intermediate framebuffer is ready with PHYSICAL pixels
            self.ensure_intermediate_fbo(width, height);
            
            let gl = self.gl.clone();
            let shader_renderer = self.shader_renderer.clone();
            let uniforms = self.uniforms.clone();
            let post_renderer = self.post_process_renderer.clone().unwrap();
            let mut post_uniforms = self.post_process_uniforms.clone();
            let intermediate_fbo = self.intermediate_fbo.unwrap();
            let intermediate_texture = self.intermediate_texture.unwrap();
            
            // Add the main pass texture to post-process uniforms
            post_uniforms.insert(
                "u_mainPass".to_string(),
                UniformInfo {
                    uniform_type: UniformType::Sampler2D,
                    value: UniformValue::Sampler2D(Some(TextureHandle {
                        path: PathBuf::from("[main_pass]"),
                        texture_id: Some(intermediate_texture),
                        width,
                        height,
                    })),
                },
            );
            
            let cb = egui_glow::CallbackFn::new(move |_info, painter| {
                use glow::HasContext as _;
                let gl = painter.gl();
                
                unsafe {
                    // === PASS 1: Render main shader to texture ===
                    gl.bind_framebuffer(glow::FRAMEBUFFER, Some(intermediate_fbo));
                    gl.viewport(0, 0, width as i32, height as i32);
                    gl.clear_color(0.0, 0.0, 0.0, 1.0);
                    gl.clear(glow::COLOR_BUFFER_BIT);
                    
                    // Use physical size for u_resolution
                    let physical_size = egui::Vec2::new(width as f32, height as f32);
                    shader_renderer.lock().paint(gl, time, physical_size, &uniforms);
                    
                    // === PASS 2: Render post-process with main pass as texture ===
                    gl.bind_framebuffer(glow::FRAMEBUFFER, None);
                    gl.viewport(0, 0, width as i32, height as i32);
                    
                    post_renderer.lock().paint(gl, time, physical_size, &post_uniforms);
                }
            });
            
            let callback = egui::PaintCallback {
                rect,
                callback: Arc::new(cb),
            };
            ui.painter().add(callback);
        } else {
            // Single-pass rendering (original behavior)
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
}

/// Get the default shader path relative to the executable
pub fn get_default_shader_path() -> PathBuf {
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let shader_path = exe_dir.join(DEFAULT_SHADER_PATH);
            if shader_path.exists() {
                return shader_path;
            }
        }
    }
    
    PathBuf::from(DEFAULT_SHADER_PATH)
}

/// Check if FFmpeg is available on the system
pub fn is_ffmpeg_available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Parse GLSL shader source to detect uniform declarations
pub fn parse_uniforms(shader_source: &str) -> HashMap<String, UniformInfo> {
    use regex::Regex;
    
    let mut uniforms = HashMap::new();
    
    let re = Regex::new(
        r"uniform\s+(float|vec2|vec3|vec4|sampler2D)\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*;"
    ).expect("Invalid regex pattern");
    
    for cap in re.captures_iter(shader_source) {
        let type_str = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let name = cap.get(2).map(|m| m.as_str()).unwrap_or("");
        
        let uniform_type = match type_str {
            "float" => UniformType::Float,
            "vec2" => UniformType::Vec2,
            "vec3" => UniformType::Vec3,
            "vec4" => UniformType::Vec4,
            "sampler2D" => UniformType::Sampler2D,
            _ => continue,
        };
        
        let value = UniformValue::default_for_type(&uniform_type);
        
        uniforms.insert(
            name.to_string(),
            UniformInfo {
                uniform_type,
                value,
            }
        );
    }
    
    uniforms
}
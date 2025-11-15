mod render_engine;
mod data;
mod export;
mod ui;

use data::*;
use crate::{RELOAD_DEBOUNCE_MS, DEFAULT_SHADER_PATH};

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
            video_fps: 30,
            ffmpeg_available,
            export_progress: Arc::new(Mutex::new(None)),
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
        
        // SAFETY: Watching a single file path that exists.
        // Errors are handled gracefully by returning None.
        watcher.watch(path, RecursiveMode::NonRecursive).ok()?;
        Some(watcher)
    }

    pub fn load_shader_file(&mut self, path: PathBuf) {
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

    pub fn try_reload_shader(&mut self) {
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

    pub fn merge_uniforms(&mut self, new_uniforms: HashMap<String, UniformInfo>) {
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

    pub fn custom_painting(&mut self, ui: &mut egui::Ui) {
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
    
    // Fallback to current working directory
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

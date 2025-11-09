#![allow(clippy::undocumented_unsafe_blocks)]

use std::sync::{mpsc, Arc};
use std::time::Duration;

use eframe::egui_glow;
use egui::mutex::Mutex;
use egui_glow::glow;
use notify::{RecommendedWatcher, Watcher, RecursiveMode};

// --- MODIFIED ---
// We now need to store the glow::Context for recompiling shaders at runtime.
// The file watcher and a channel for communication are also added.
pub struct ShaderApp {
    gl: Arc<glow::Context>,
    shader_renderer: Arc<Mutex<ShaderRenderer>>,
    time: f32,
    shader_error: Arc<Mutex<Option<String>>>,
    _watcher: RecommendedWatcher,
    shader_update_receiver: mpsc::Receiver<()>,
}

impl ShaderApp {
    pub fn new<'a>(cc: &'a eframe::CreationContext<'a>) -> Option<Self> {
        let gl = cc.gl.as_ref()?.clone();

        // --- MODIFIED ---
        // Read the initial shader source. This can still panic on startup
        // if the file is missing.
        let initial_shader_source =
    std::fs::read_to_string("shaders/shader.frag").expect("Failed to read fragment shader on startup");
        
        // Attempt to compile the initial shader.
        let shader_renderer =
            ShaderRenderer::new(&gl, &initial_shader_source).expect("Failed to compile initial shader");

        // --- NEW ---
        // Set up a channel to receive file change notifications.
        let (tx, rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                if event.kind.is_modify() || event.kind.is_create() {
                    // Send a simple signal. We don't need the event details.
                    let _ = tx.send(());
                }
            }
        }).ok()?;
        
        watcher.watch(std::path::Path::new("shaders/shader.frag"), RecursiveMode::NonRecursive).ok()?;

        Some(Self {
            gl,
            shader_renderer: Arc::new(Mutex::new(shader_renderer)),
            time: 0.0,
            shader_error: Arc::new(Mutex::new(None)),
            _watcher: watcher,
            shader_update_receiver: rx,
        })
    }
}

impl eframe::App for ShaderApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // --- NEW ---
        // Check for hot-reload messages.
        // We use a small timeout to avoid blocking the UI thread unnecessarily.
        if self.shader_update_receiver.recv_timeout(Duration::from_millis(1)).is_ok() {
            log::info!("Shader file changed, attempting to reload...");
            match std::fs::read_to_string("shaders/shader.frag") {
                Ok(new_source) => {
                    match ShaderRenderer::new(&self.gl, &new_source) {
                        Ok(new_renderer) => {
                            let mut renderer_guard = self.shader_renderer.lock();
                            renderer_guard.destroy(&self.gl); // Clean up the old shader
                            *renderer_guard = new_renderer; // Replace with the new one
                            *self.shader_error.lock() = None; // Clear any previous error
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

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("GLSL Fragment Shader with egui");
            ui.add(egui::Slider::new(&mut self.time, 0.0..=10.0).text("Time (u_time)"));

            // --- NEW ---
            // Display compilation errors in the UI if they exist.
            let error_text = self.shader_error.lock().clone();
            if let Some(error) = error_text {
                ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP).with_main_wrap(true), |ui| {
    // Adding this makes sure the error text doesn't overflow the window
    ui.style_mut().wrap = Some(true); 
    ui.label(egui::RichText::new(error).color(egui::Color32::RED));
});
            }

            egui::Frame::canvas(ui.style()).show(ui, |ui| {
                self.custom_painting(ui);
            });
        });
        ctx.request_repaint();
    }

    fn on_exit(&mut self, gl: Option<&glow::Context>) {
        if let Some(gl) = gl {
            self.shader_renderer.lock().destroy(gl);
        }
    }
}

impl ShaderApp {
    fn custom_painting(&mut self, ui: &mut egui::Ui) {
        let (rect, _response) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());

        self.time += ui.input(|i| i.stable_dt);
        let time = self.time;
        
        let shader_renderer = self.shader_renderer.clone();

        let cb = egui_glow::CallbackFn::new(move |_info, painter| {
            shader_renderer.lock().paint(painter.gl(), time, rect.size());
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

// --- MODIFIED ---
// The `new` function is now fallible and returns a Result<Self, String>
// to allow for graceful error handling on compilation failure.
impl ShaderRenderer {
    fn new(gl: &glow::Context, fragment_shader_source: &str) -> Result<Self, String> {
        use glow::HasContext as _;

        let shader_version = egui_glow::ShaderVersion::get(gl);

        unsafe {
            let program = gl.create_program().map_err(|e| e.to_string())?;

            let (vertex_shader_source, fragment_shader_source) = (
                r#"
                    const vec2 verts[4] = vec2[4](
                        vec2(-1.0, 1.0), vec2(-1.0, -1.0),
                        vec2(1.0, 1.0),  vec2(1.0, -1.0)
                    );
                    void main() {
                        gl_Position = vec4(verts[gl_VertexID], 0.0, 1.0);
                    }
                "#,
                fragment_shader_source,
            );

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
                    // Important: clean up the failed shader
                    gl.delete_shader(shader);
                    return Err(info_log);
                }
                
                gl.attach_shader(program, shader);
                shaders.push(shader);
            }

            gl.link_program(program);
            if !gl.get_program_link_status(program) {
                let info_log = gl.get_program_info_log(program);
                // Important: clean up all the shaders
                for shader in shaders {
                    gl.detach_shader(program, shader);
                    gl.delete_shader(shader);
                }
                gl.delete_program(program);
                return Err(info_log);
            }

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
        unsafe {
            gl.delete_program(self.program);
            gl.delete_vertex_array(self.vertex_array);
        }
    }

    fn paint(&self, gl: &glow::Context, time: f32, size: egui::Vec2) {
        use glow::HasContext as _;
        unsafe {
            gl.use_program(Some(self.program));
            gl.uniform_1_f32(gl.get_uniform_location(self.program, "u_time").as_ref(), time);
            gl.uniform_2_f32(gl.get_uniform_location(self.program, "u_resolution").as_ref(), size.x, size.y);
            gl.bind_vertex_array(Some(self.vertex_array));
            gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);
        }
    }
}

fn main() {
    // We need to enable logging to see the hot-reload messages.
    env_logger::init(); 

    let native_options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    
    eframe::run_native(
        "egui with GLSL Shaders",
        native_options,
        Box::new(|cc| Box::new(ShaderApp::new(cc).expect("Failed to create ShaderApp"))),
    ).expect("Failed to run eframe");
}

mod app;
use app::ShaderApp;

pub const DEFAULT_SHADER_PATH: &str = "shaders/shader.frag";
pub const DEFAULT_POST_SHADER_PATH: &str = "shaders/post.frag";
pub const RELOAD_DEBOUNCE_MS: u64 = 100;
pub const FILE_CHECK_TIMEOUT_MS: u64 = 1;

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
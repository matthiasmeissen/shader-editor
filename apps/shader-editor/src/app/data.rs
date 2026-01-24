pub use shader_parser::{UniformInfo, UniformType, UniformValue, UniformHint, TextureHandle};

#[derive(Debug, Clone)]
pub struct ExportProgress {
    pub current_frame: u32,
    pub total_frames: u32,
    pub status: String,
}

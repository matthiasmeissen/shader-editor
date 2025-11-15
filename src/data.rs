#[derive(Debug, Clone)]
pub struct UniformInfo {
    pub _name: String,
    pub uniform_type: UniformType,
    pub value: UniformValue,
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
    pub fn default_for_type(uniform_type: &UniformType) -> Self {
        match uniform_type {
            UniformType::Float => UniformValue::Float(1.0),
            UniformType::Vec2 => UniformValue::Vec2([0.5, 0.5]),
            UniformType::Vec3 => UniformValue::Vec3([0.5, 0.5, 0.5]),
            UniformType::Vec4 => UniformValue::Vec4([1.0, 1.0, 1.0, 1.0]),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExportProgress {
    pub current_frame: u32,
    pub total_frames: u32,
    pub status: String,
}
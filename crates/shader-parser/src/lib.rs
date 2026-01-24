//! Shader parsing utilities for extracting uniform declarations from GLSL source code.

use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct UniformInfo {
    pub uniform_type: UniformType,
    pub value: UniformValue,
    pub hint: Option<UniformHint>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UniformType {
    Bool,
    Float,
    Vec2,
    Vec3,
    Vec4,
    Sampler2D,
}

#[derive(Debug, Clone)]
pub enum UniformValue {
    Bool(bool),
    Float(f32),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    Sampler2D(Option<TextureHandle>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum UniformHint {
    Color,
}

#[derive(Debug, Clone)]
pub struct TextureHandle {
    pub path: PathBuf,
    pub texture_id: Option<glow::Texture>,
    pub width: u32,
    pub height: u32,
}

impl UniformValue {
    pub fn default_for_type(uniform_type: &UniformType) -> Self {
        match uniform_type {
            UniformType::Bool => UniformValue::Bool(false),
            UniformType::Float => UniformValue::Float(1.0),
            UniformType::Vec2 => UniformValue::Vec2([0.5, 0.5]),
            UniformType::Vec3 => UniformValue::Vec3([0.5, 0.5, 0.5]),
            UniformType::Vec4 => UniformValue::Vec4([1.0, 1.0, 1.0, 1.0]),
            UniformType::Sampler2D => UniformValue::Sampler2D(None),
        }
    }
}

/// Parses GLSL shader source to detect uniform declarations.
///
/// Scans the source for lines matching `uniform <type> <name>;` and optionally
/// captures a hint from trailing comments like `// color`.
///
/// # Arguments
///
/// * `shader_source` - The GLSL fragment shader source code
///
/// # Returns
///
/// A HashMap mapping uniform names to their `UniformInfo` (type, default value, and hint)
pub fn parse_uniforms(shader_source: &str) -> HashMap<String, UniformInfo> {
    use regex::Regex;

    let mut uniforms = HashMap::new();

    let re = Regex::new(
        r"uniform\s+(bool|float|vec2|vec3|vec4|sampler2D)\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*;\s*(?://\s*(\w+))?"
    ).expect("Invalid regex pattern");

    for cap in re.captures_iter(shader_source) {
        let type_str = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let name = cap.get(2).map(|m| m.as_str()).unwrap_or("");
        let hint_type = cap.get(3).map(|m| m.as_str()).unwrap_or("");

        let uniform_type = match type_str {
            "bool" => UniformType::Bool,
            "float" => UniformType::Float,
            "vec2" => UniformType::Vec2,
            "vec3" => UniformType::Vec3,
            "vec4" => UniformType::Vec4,
            "sampler2D" => UniformType::Sampler2D,
            _ => continue,
        };

        let hint = match hint_type.to_lowercase().as_str() {
            "color" => {
                if type_str == "vec3" {
                    Some(UniformHint::Color)
                } else if type_str == "vec4" {
                    Some(UniformHint::Color)
                } else {
                    None
                }
            },
            _ => None,
        };

        let value = UniformValue::default_for_type(&uniform_type);

        uniforms.insert(
            name.to_string(),
            UniformInfo {
                uniform_type,
                value,
                hint,
            }
        );
    }

    uniforms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_uniforms_detects_float() {
        let source = "uniform float uBrightness;";
        let uniforms = parse_uniforms(source);

        assert!(uniforms.contains_key("uBrightness"));
        assert_eq!(uniforms["uBrightness"].uniform_type, UniformType::Float);
    }

    #[test]
    fn parse_uniforms_detects_bool() {
        let source = "uniform bool u_Param1;";
        let uniforms = parse_uniforms(source);

        assert!(uniforms.contains_key("u_Param1"));
        assert_eq!(uniforms["u_Param1"].uniform_type, UniformType::Bool);
    }

    #[test]
    fn parse_uniforms_detects_vec2() {
        let source = "uniform vec2 u_Param2;";
        let uniforms = parse_uniforms(source);

        assert_eq!(uniforms["u_Param2"].uniform_type, UniformType::Vec2);
    }

    #[test]
    fn parse_uniforms_detects_color_hint() {
        let source = "uniform vec3 uColor; // color";
        let uniforms = parse_uniforms(source);

        assert_eq!(uniforms["uColor"].hint, Some(UniformHint::Color));
    }

    #[test]
    fn parse_uniforms_color_hint_case_insensitive() {
        let source = "uniform vec3 uColor; // COLOR";
        let uniforms = parse_uniforms(source);

        assert_eq!(uniforms["uColor"].hint, Some(UniformHint::Color));
    }

    #[test]
    fn parse_uniforms_ignores_color_hint_on_float() {
        let source = "uniform float uValue; // color";
        let uniforms = parse_uniforms(source);

        assert_eq!(uniforms["uValue"].hint, None);
    }
}

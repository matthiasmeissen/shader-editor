use super::*;

#[test]
fn parse_uniforms_detects_float() {
    let source = "uniform float uBrightness;";
    let uniforms = parse_uniforms(source);

    assert!(uniforms.contains_key("uBrightness"));
    assert_eq!(uniforms["uBrightness"].uniform_type, UniformType::Float);
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
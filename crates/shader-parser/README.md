# shader-parser

A Rust library for parsing GLSL shader uniform declarations.

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
shader-parser = { path = "../../crates/shader-parser" }
```

### Example

```rust
use shader_parser::{parse_uniforms, UniformType, UniformHint};

let shader_source = r#"
    #version 330 core
    uniform float uBrightness;
    uniform vec3 uColor; // color
    uniform sampler2D uTexture;
"#;

let uniforms = parse_uniforms(shader_source);

for (name, info) in &uniforms {
    println!("{}: {:?}", name, info.uniform_type);
}
```

## API

### `parse_uniforms(shader_source: &str) -> HashMap<String, UniformInfo>`

Parses GLSL source code and returns a map of uniform names to their metadata.

**Supported uniform types:**
- `bool`
- `float`
- `vec2`, `vec3`, `vec4`
- `sampler2D`

**UI Hints:**

Add `// color` after a `vec3` or `vec4` uniform to mark it as a color:

```glsl
uniform vec3 uBackground; // color
```

This sets `UniformHint::Color` which UI tools can use to show a color picker.

## Types

### `UniformInfo`
```rust
pub struct UniformInfo {
    pub uniform_type: UniformType,
    pub value: UniformValue,
    pub hint: Option<UniformHint>,
}
```

### `UniformType`
```rust
pub enum UniformType {
    Bool,
    Float,
    Vec2,
    Vec3,
    Vec4,
    Sampler2D,
}
```

### `UniformValue`
```rust
pub enum UniformValue {
    Bool(bool),
    Float(f32),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    Sampler2D(Option<TextureHandle>),
}
```

### `UniformHint`
```rust
pub enum UniformHint {
    Color,
}
```

### `TextureHandle`
```rust
pub struct TextureHandle {
    pub path: PathBuf,
    pub texture_id: Option<glow::Texture>,
    pub width: u32,
    pub height: u32,
}
```

## Default Values

`UniformValue::default_for_type()` provides sensible defaults:

| Type | Default |
|------|---------|
| `Bool` | `false` |
| `Float` | `1.0` |
| `Vec2` | `[0.5, 0.5]` |
| `Vec3` | `[0.5, 0.5, 0.5]` |
| `Vec4` | `[1.0, 1.0, 1.0, 1.0]` |
| `Sampler2D` | `None` |

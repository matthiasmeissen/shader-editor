# shader-editor

A real-time GLSL fragment shader editor and prototyping tool with hot-reload, uniform controls, and export capabilities.

## Features

- **Hot Reload** - Edit shaders and see changes instantly
- **Uniform Controls** - Auto-generated sliders, color pickers, and texture loaders
- **Post-Processing** - Optional second shader pass for effects
- **Export** - PNG images and MP4 videos (requires FFmpeg)

## Usage

```bash
cargo run -p shader-editor --release
```

## Quick Start

1. Edit `shaders/shader.frag` in your editor
2. The app auto-reloads on save
3. Add uniforms to control your shader:

```glsl
#version 330 core

uniform vec2 u_resolution;
uniform float u_time;
uniform float uBrightness;        // Slider (0-1)
uniform vec3 uColor; // color     // Color picker

out vec4 out_color;

void main() {
    vec2 uv = gl_FragCoord.xy / u_resolution;
    vec3 color = uColor * uBrightness * sin(uv.x + u_time);
    out_color = vec4(color, 1.0);
}
```

## Built-in Uniforms

| Uniform | Type | Description |
|---------|------|-------------|
| `u_resolution` | `vec2` | Viewport dimensions in pixels |
| `u_time` | `float` | Elapsed time in seconds |
| `u_progress` | `float` | 0.0 → 1.0 over video duration |

## Custom Uniforms

| GLSL Type | UI Control |
|-----------|------------|
| `float` | Slider (0.0 - 1.0) |
| `vec2` | Two sliders |
| `vec3` | Three sliders (or color picker with `// color`) |
| `vec4` | Four sliders (or color picker with `// color`) |
| `bool` | Checkbox |
| `sampler2D` | File picker for images |

### Color Picker Hint

Add `// color` after vec3/vec4 uniforms to show a color picker:

```glsl
uniform vec3 uBackground; // color
uniform vec4 uTint; // color
```

## Post-Processing

1. Enable "Use Post Process" in the UI
2. Load a post-processing shader
3. Access the main pass via `u_mainPass`:

```glsl
#version 330 core

uniform vec2 u_resolution;
uniform sampler2D u_mainPass;  // Main shader output

out vec4 out_color;

void main() {
    vec2 uv = gl_FragCoord.xy / u_resolution;
    vec4 color = texture(u_mainPass, uv);

    // Apply effect (e.g., grayscale)
    float gray = dot(color.rgb, vec3(0.299, 0.587, 0.114));
    out_color = vec4(vec3(gray), 1.0);
}
```

## Export

### Image Export
- Set resolution in the UI
- Click "Export Image"
- Saves as PNG

### Video Export
- Requires [FFmpeg](https://ffmpeg.org/) in PATH
- Set resolution, duration, and FPS
- Click "Export Video"
- Saves as MP4 (H.264)

The `u_progress` uniform animates from 0.0 to 1.0 over the video duration, enabling duration-independent animations.

## Architecture

See [docs/ARCHITECTURE.md](../../docs/ARCHITECTURE.md) for detailed technical documentation.

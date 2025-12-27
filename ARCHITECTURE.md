# Shader Editor - Architecture Documentation

## Overview

This is a **real-time GLSL fragment shader editor and prototyping tool** built in Rust. The application provides an interactive environment for writing and experimenting with GLSL shaders, featuring live preview, hot-reloading, and export capabilities.

## Core Functionality

- Write and edit GLSL fragment shaders with instant visual feedback
- Hot-reload shaders automatically when files change on disk
- Control shader parameters through dynamically generated UI sliders
- Load texture images as shader inputs (sampler2D uniforms)
- Apply post-processing effects via a second shader pass
- Export rendered frames as PNG images
- Export animations as MP4 videos (with FFmpeg integration)

## Project Structure

```
shader-editor/
├── src/
│   ├── main.rs                    # Entry point, app initialization
│   ├── app.rs                     # Core application logic, shader management
│   └── app/                       # Submodules
│       ├── data.rs                # Data structures and types
│       ├── render_engine.rs       # OpenGL rendering logic
│       ├── ui.rs                  # egui UI implementation
│       └── file_io.rs             # File operations, texture loading, export
├── shaders/                       # Default shader files
│   ├── shader.frag               # Main fragment shader
│   └── post.frag                 # Post-processing shader
└── Cargo.toml                    # Dependencies
```

## Module Responsibilities

### main.rs
- Application entry point
- Defines global constants (shader paths, debounce timing)
- Initializes eframe with OpenGL backend
- Configures window settings (always-on-top mode)

### app.rs (Core Module)

The heart of the application containing the `ShaderApp` struct. Key responsibilities:

**Shader Management**
- Loading, reloading, and compiling shaders
- Error handling for shader compilation failures
- Managing both main shader and optional post-processing shader

**File Watching & Hot Reload**
- Uses `notify` crate to watch shader files for changes
- Debounced automatic recompilation (100ms delay to prevent rapid reloads)
- Preserves application state during hot reloads

**Uniform System**
- Regex-based parsing of GLSL uniform declarations from shader source
- Automatic detection of uniform types (float, vec2, vec3, vec4, sampler2D)
- Smart merging of uniform values across reloads (preserves user adjustments)
- Built-in uniforms: `u_time`, `u_resolution`

**Rendering Pipeline**
- Single-pass rendering: Direct shader output to screen
- Dual-pass rendering: Main shader → Post-processing shader → Screen
- Framebuffer management for intermediate render targets

**Key Functions**
- `new()`: Initialize app with default shaders
- `create_watcher()`: Set up file system watcher
- `try_reload_shader()`: Hot-reload main shader
- `try_reload_post_process()`: Hot-reload post-processing shader
- `merge_uniforms()`: Intelligently merge old and new uniform values
- `custom_painting()`: Orchestrate rendering (single or dual-pass)

### app/data.rs

Defines data structures used throughout the application:

- **`UniformInfo`**: Stores uniform metadata (type and current value)
- **`UniformType`**: Enum for GLSL types (Float, Vec2, Vec3, Vec4, Sampler2D)
- **`UniformValue`**: Enum for actual uniform values
- **`TextureHandle`**: Stores texture metadata and OpenGL texture ID
- **`ExportProgress`**: Progress tracking for video export operations

### app/render_engine.rs

OpenGL rendering abstraction layer:

**`ShaderRenderer` Responsibilities**
- Compiles vertex and fragment shaders
- Links shaders into OpenGL program
- Manages vertex array objects
- Renders full-screen quad procedurally (no vertex buffers needed)
- Binds uniforms and textures to shader programs
- Cleanup of OpenGL resources

**Key Functions**
- `new()`: Compile and link shaders into OpenGL program
- `paint()`: Render full-screen quad with shader and uniforms
- `destroy()`: Clean up OpenGL resources

**Automatic Uniform Binding**
- Built-in uniforms: `u_time`, `u_resolution`
- Custom uniforms detected from shader source
- Texture unit management for sampler2D uniforms

### app/ui.rs

User interface implementation using egui (immediate-mode GUI):

**Side Panel Controls**
- Shader file selection and loading
- Time controls (auto-play, manual scrubbing, reset)
- Post-processing toggle and shader loading
- Export controls (resolution presets, image/video export)
- Dynamic uniform controls (sliders for floats/vectors, texture pickers for samplers)
- Shader compilation error display

**Central Panel**
- Full-screen shader preview using OpenGL callback

**Helper Functions**
- `render_uniform_controls()`: Generates UI controls based on uniform types
- Filters out built-in uniforms from UI (`u_time`, `u_resolution`, `u_mainPass`)

### app/file_io.rs

File operations and export functionality:

**Texture Loading**
- `load_texture_from_file()`: Load images, upload to GPU
- `flip_image_vertically()`: Correct orientation for OpenGL coordinate system
- `delete_texture()`: Cleanup GPU texture resources

**Image Export**
- `export_image()`: Render single frame at custom resolution, save as PNG
- `render_frame_to_buffer()`: Single-pass offscreen rendering
- `render_two_pass_to_buffer()`: Dual-pass offscreen rendering with post-processing

**Video Export**
- `export_video()`: Render animation sequence, pipe frames to FFmpeg
- FFmpeg detection and availability checking
- Progress tracking and user feedback
- Configurable FPS and duration
- H.264 encoding with optimized settings (preset: veryfast, CRF: 18)

## Technology Stack

### Core Technologies
- **Rust** (Edition 2024): System programming language
- **OpenGL**: Graphics API via `glow` crate
- **GLSL 330 Core**: Shader language

### Key Dependencies
- **eframe 0.23.0**: Application framework built on egui
- **egui 0.23.0**: Immediate-mode GUI library
- **egui_glow 0.23.0**: OpenGL backend for egui
- **glow 0.12.0**: Low-level OpenGL bindings
- **notify 6.1.1**: Cross-platform file system event watching
- **regex 1.10**: Uniform declaration parsing
- **rfd 0.12**: Native file dialogs (open/save)
- **image 0.24**: Image loading/saving (PNG, JPEG, etc.)
- **env_logger 0.11.8**: Logging infrastructure
- **log 0.4.17**: Logging facade

## Rendering Pipeline

### 1. Initialization

1. Load default shader from `shaders/shader.frag`
2. Parse uniform declarations using regex patterns
3. Compile vertex + fragment shaders into OpenGL program
4. Create vertex array for full-screen quad rendering

### 2. Vertex Shader (Hardcoded)

The vertex shader is procedurally generated and doesn't require vertex buffers:

- Generates full-screen quad using `gl_VertexID`
- Calculates UV coordinates (0-1 range) for fragment shader
- Efficient approach: no VBO/vertex data needed

### 3. Fragment Shader (User-Editable)

User writes fragment shaders that:

- Receive UV coordinates from vertex shader
- Access built-in uniforms: `u_time`, `u_resolution`
- Declare custom uniforms (float, vec2-4, sampler2D)
- Output color to `out_color`

### 4. Hot Reload Mechanism

1. `notify` crate watches shader files for modifications
2. File change event sent via mpsc channel
3. Debounced reload (100ms) prevents rapid recompilations
4. On successful compilation:
   - Replace old shader with new one
   - Merge uniforms (preserve user-adjusted values)
5. On error:
   - Keep old shader running
   - Display compilation error in UI

### 5. Rendering Modes

**Single-Pass Mode** (Default)
- Render shader directly to screen
- Uses `egui::PaintCallback` with `egui_glow::CallbackFn`

**Dual-Pass Mode** (With Post-Processing)
1. **Pass 1**: Render main shader to intermediate framebuffer texture
2. **Pass 2**: Render post-process shader to screen
   - Post-process shader receives main pass output via `u_mainPass` sampler2D
   - Both passes share the same time value
   - Enables effects like bloom, blur, color grading, etc.

### 6. Uniform System

**Auto-Detection**
- Regex parses `uniform <type> <name>;` declarations from shader source
- Supports: float, vec2, vec3, vec4, sampler2D

**Built-in Uniforms**
- `u_time`: Animated time value (auto-incremented or manually controlled)
- `u_resolution`: Viewport dimensions (vec2)
- `u_mainPass`: Auto-injected in post-processing shaders (sampler2D)

**Custom Uniforms**
- User-declared uniforms appear as UI controls
- Float/Vector uniforms: Sliders with configurable ranges
- Sampler2D uniforms: File picker to load texture images

**Value Persistence**
- User-adjusted uniform values preserved across hot reloads
- Smart merging: new uniforms added, removed uniforms discarded, existing ones keep values

### 7. Export Pipeline

**Image Export**
- Single-frame offscreen render at custom resolution
- Supports both single-pass and dual-pass rendering
- Saves as PNG format

**Video Export**
- Frame-by-frame rendering over specified duration
- Raw RGBA pixels piped directly to FFmpeg stdin
- FFmpeg encodes to H.264 MP4 format
- Progress tracking with UI feedback
- Requires FFmpeg to be installed and available in PATH

## Design Patterns & Architecture Highlights

### Strengths

- **Clean Separation of Concerns**: Rendering, UI, and I/O are cleanly separated
- **Robust Error Handling**: Shader compilation errors displayed without crashing
- **Smart State Management**: Uniform merging preserves user state across reloads
- **Efficient Rendering**: Procedural geometry (no vertex buffers)
- **Flexible Pipeline**: Easy to switch between single and dual-pass rendering
- **Production-Quality Export**: Professional-grade image and video export

### Design Patterns

- **Observer Pattern**: File system watcher with message passing (mpsc channels)
- **Strategy Pattern**: Single vs dual-pass rendering modes
- **Resource Management**: RAII principles with explicit `destroy()` cleanup
- **Immediate Mode UI**: egui for responsive, simple UI code

## Getting Started

### Prerequisites

- Rust toolchain (edition 2024)
- OpenGL-capable graphics card
- FFmpeg (optional, for video export)

### Running the Application

```bash
cargo run --release
```

### Creating Your First Shader

1. Edit `shaders/shader.frag`
2. The app will automatically detect changes and reload
3. Add custom uniforms to control your shader:

```glsl
#version 330 core

uniform vec2 u_resolution;
uniform float u_time;
uniform vec3 u_color;  // Custom uniform - will appear as slider in UI
uniform float u_scale; // Custom uniform - will appear as slider in UI

out vec4 out_color;

void main() {
    vec2 uv = gl_FragCoord.xy / u_resolution;
    vec3 color = u_color * sin(uv.x * u_scale + u_time);
    out_color = vec4(color, 1.0);
}
```

### Adding Post-Processing

1. Enable "Use Post Process" in the UI
2. Load a post-processing shader (e.g., `shaders/post.frag`)
3. The post-processing shader receives the main shader output via `u_mainPass`:

```glsl
#version 330 core

uniform vec2 u_resolution;
uniform sampler2D u_mainPass;  // Output from main shader

out vec4 out_color;

void main() {
    vec2 uv = gl_FragCoord.xy / u_resolution;
    vec4 color = texture(u_mainPass, uv);

    // Apply post-processing effect (e.g., grayscale)
    float gray = dot(color.rgb, vec3(0.299, 0.222, 0.114));
    out_color = vec4(vec3(gray), color.a);
}
```

## Future Extension Points

The architecture is designed to be extensible. Potential enhancements:

- Multiple shader passes (not just main + post)
- Geometry/compute shader support
- Shader presets and template library
- Real-time performance metrics
- Buffer/render target management UI
- Timeline-based uniform animation
- Shader includes/imports system

## Contributing

When contributing to this project, please maintain the existing architectural patterns:

- Keep modules focused on single responsibilities
- Use the existing error handling patterns
- Preserve the hot-reload functionality
- Add documentation for new uniforms or features
- Test export functionality thoroughly

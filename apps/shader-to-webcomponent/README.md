# shader-to-webcomponent

Convert GLSL fragment shaders into self-contained JavaScript Web Components.

## Usage

```bash
cargo run -p shader-to-webcomponent -- -i shader.frag -o my-shader.js
```

### Options

| Flag | Description | Default |
|------|-------------|---------|
| `-i, --input` | Input GLSL shader file (.frag) | Required |
| `-o, --output` | Output JavaScript file (.js) | Required |
| `--classname` | JavaScript class name | `ShaderComponent` |
| `--tagname` | Custom element tag name | `shader-view` |

### Example

```bash
# Convert a shader with custom tag name
cargo run -p shader-to-webcomponent -- \
  -i shaders/shader.frag \
  -o dist/my-effect.js \
  --tagname "my-effect" \
  --classname "MyEffectComponent"
```

## Output

The generated JavaScript file exports a Web Component that:

- Uses **WebGL2** for rendering
- Renders in a **Shadow DOM** for encapsulation
- Auto-resizes with **devicePixelRatio** support
- Provides **getters/setters** for all detected uniforms
- Includes a **fallback pink shader** if compilation fails
- Logs debug info comparing parsed vs GPU-active uniforms

### Using the Generated Component

```html
<script type="module" src="my-shader.js"></script>

<my-effect style="width: 400px; height: 300px;"></my-effect>

<script>
  const shader = document.querySelector('my-effect');

  // Set uniforms via properties
  shader.uBrightness = 0.8;
  shader.uColor = [1.0, 0.5, 0.2];
</script>
```

## GLSL Transpilation

The tool performs basic transpilation from desktop OpenGL to WebGL:

| Desktop (330 core) | WebGL (300 es) |
|--------------------|----------------|
| `#version 330 core` | `#version 300 es` + `precision mediump float;` |

### Supported Uniform Types

| GLSL Type | JavaScript Type | Setter |
|-----------|----------------|--------|
| `float` | `number` | `uniform1f` |
| `vec2` | `[number, number]` | `uniform2fv` |
| `vec3` | `[number, number, number]` | `uniform3fv` |
| `vec4` | `[number, number, number, number]` | `uniform4fv` |
| `bool` | `boolean` | `uniform1i` |
| `sampler2D` | (default checkerboard) | `uniform1i` |

## Built-in Uniforms

The generated component automatically provides:

- `u_resolution` / `uResolution` - Canvas dimensions (vec2)
- `u_time` / `uTime` - Elapsed time in seconds (float)

## Limitations

- Only basic `#version` transpilation (no GLSL feature conversion)
- `sampler2D` uniforms get a default checkerboard texture (no image loading API yet)
- No support for uniform arrays or structs

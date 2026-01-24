use clap::Parser;
use std::fs;
use std::path::PathBuf;
use shader_parser::{parse_uniforms, UniformType, UniformValue};

#[derive(Parser)]
#[command(about = "Convert GLSL shaders to Web Components")]
struct Args {
    /// Input GLSL shader file (.frag)
    #[arg(short, long)]
    input: PathBuf,
    /// Output JavaScript file (.js)
    #[arg(short, long)]
    output: PathBuf,
    /// JavaScript class name for the web component
    #[arg(long, default_value = "ShaderComponent")]
    classname: String,
    /// Custom element tag name (e.g., <shader-view>)
    #[arg(long, default_value = "shader-view")]
    tagname: String,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let raw_source = fs::read_to_string(&args.input)?;
    
    // 1. TRANSPILE HEADER (Desktop 330 -> WebGL 300 es)
    let fragment_source = raw_source.replace(
        "#version 330 core",
        "#version 300 es\nprecision mediump float;"
    );
    
    let uniforms = parse_uniforms(&fragment_source);

    // 2. GENERATE JS SNIPPETS
    let mut properties_code = String::new();
    let mut locations_code = String::new();
    let mut apply_defaults_code = String::new();
    let mut texture_init_code = String::new();
    
    let mut texture_unit_counter = 0;

    for (name, info) in &uniforms {
        // Location Lookup
        locations_code.push_str(&format!(
            "        this.loc_{name} = this.gl.getUniformLocation(this.program, '{name}');\n", 
            name=name
        ));

        // Detect Type
        let (_js_default, gl_setter) = match info.uniform_type {
            UniformType::Float => ("1.0", "this.gl.uniform1f"),
            UniformType::Vec2 => ("[0.0, 0.0]", "this.gl.uniform2fv"),
            UniformType::Vec3 => ("[0.0, 0.0, 0.0]", "this.gl.uniform3fv"),
            UniformType::Vec4 => ("[0.0, 0.0, 0.0, 1.0]", "this.gl.uniform4fv"),
            UniformType::Bool => ("false", "this.gl.uniform1i"), 
            UniformType::Sampler2D => ("null", "this.gl.uniform1i"), 
        };

        // Handle Textures (Sampler2D)
        if info.uniform_type == UniformType::Sampler2D {
            // Generate code to create a default checkerboard and bind it
            texture_init_code.push_str(&format!(
                "        this.createDefaultTexture({unit});\n        if(this.loc_{name}) this.gl.uniform1i(this.loc_{name}, {unit});\n",
                name=name, unit=texture_unit_counter
            ));
            texture_unit_counter += 1;
        }

        let actual_default = format_value_as_js(&info.value);
        
        // Default JS State
        apply_defaults_code.push_str(&format!(
            "        this._{name} = {val};\n", 
            name=name, val=actual_default
        ));

        // Getters / Setters
        // Note: We handle bool conversion (true -> 1, false -> 0)
        let value_processing = if info.uniform_type == UniformType::Bool { "val ? 1 : 0" } else { "val" };
        
        // Skip simple setters for samplers (requires image loading logic)
        if info.uniform_type != UniformType::Sampler2D {
            properties_code.push_str(&format!(r#"
    get {name}() {{ return this._{name}; }}
    set {name}(val) {{
        this._{name} = val;
        if (this.gl && this.program && this.loc_{name}) {{
            {gl_setter}(this.loc_{name}, {val_proc});
        }}
    }}
"#, name=name, gl_setter=gl_setter, val_proc=value_processing));
        }
    }

    // 3. HARDCODED VERTEX SHADER (With UVs)
    // Matches "in vec2 v_uv" in your shader
    let vertex_source = r#"#version 300 es
layout(location = 0) in vec2 position;
out vec2 v_uv;
void main() { 
    v_uv = position * 0.5 + 0.5;
    gl_Position = vec4(position, 0.0, 1.0); 
}
"#;

    // 4. FALLBACK SHADER (Pink)
    let fallback_frag = r#"#version 300 es
precision mediump float;
out vec4 out_color;
void main() { out_color = vec4(1.0, 0.0, 1.0, 1.0); } 
"#;

    // Generate list of parsed uniform names for debug logging
    let detected_names_js = uniforms.keys()
        .map(|k| format!("'{}'", k))
        .collect::<Vec<_>>()
        .join(", ");

    let js_content = format!(r#"
const VERTEX_SOURCE = `{vertex_source}`;
const FRAG_SOURCE = `{fragment_source}`;
const FALLBACK_SOURCE = `{fallback_frag}`;

export class {classname} extends HTMLElement {{
    constructor() {{
        super();
        this.attachShadow({{mode: 'open'}});
        this.canvas = document.createElement('canvas');
        this.canvas.style.cssText = 'width: 100%; height: 100%; display: block;';
        this.shadowRoot.appendChild(this.canvas);
        
        {apply_defaults_code}
    }}

    connectedCallback() {{
        requestAnimationFrame(() => this.initGL());
    }}

    disconnectedCallback() {{
        // Stop render loop and release WebGL context
        this.gl = null;
        this.program = null;
    }}

    initGL() {{
        const gl = this.canvas.getContext('webgl2', {{ preserveDrawingBuffer: true }});
        if (!gl) {{
            this.shadowRoot.innerHTML = '<div style="color:red">WebGL2 Not Supported</div>';
            return;
        }}
        this.gl = gl;

        // 1. Compile Shader
        let program = this.createProgram(gl, VERTEX_SOURCE, FRAG_SOURCE);
        if (!program) {{
            console.warn("Switching to fallback shader.");
            program = this.createProgram(gl, VERTEX_SOURCE, FALLBACK_SOURCE);
        }}
        this.program = program;
        gl.useProgram(program);

        // 2. Setup Geometry
        const buffer = gl.createBuffer();
        gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
        gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([
            -1, -1,  1, -1, -1,  1,
            -1,  1,  1, -1,  1,  1,
        ]), gl.STATIC_DRAW);
        gl.enableVertexAttribArray(0);
        gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);

        // 3. Init Uniforms & Textures
        {locations_code}
        {texture_init_code}

        // 4. Push Initial Values
        const keys = [{detected_names}];
        keys.forEach(k => {{ if(this[k] !== undefined) this[k] = this[k]; }});

        // 5. DEBUG: Print Uniforms
        this.printDebugInfo(keys);

        this.renderLoop();
    }}

    printDebugInfo(parsedKeys) {{
        console.groupCollapsed(`Shader: {tagname}`);
        
        console.log("%c Detected by Parser (Rust):", "color: #00AAFF; font-weight: bold", parsedKeys);
        
        // Query WebGL for what actually exists on GPU
        const numUniforms = this.gl.getProgramParameter(this.program, this.gl.ACTIVE_UNIFORMS);
        const activeUniforms = [];
        for (let i = 0; i < numUniforms; ++i) {{
            const info = this.gl.getActiveUniform(this.program, i);
            activeUniforms.push(info.name);
        }}
        
        console.log("%c Active on GPU (WebGL):", "color: #00FF00; font-weight: bold", activeUniforms);
        
        // Warning for missing
        const missing = parsedKeys.filter(k => !activeUniforms.includes(k) && !activeUniforms.includes(k + '[0]')); // Samplers sometimes add [0]
        if (missing.length > 0) {{
            console.warn("Optimized out (unused in shader):", missing);
        }}
        
        console.groupEnd();
    }}

    createDefaultTexture(unit) {{
        const gl = this.gl;
        const texture = gl.createTexture();
        gl.activeTexture(gl.TEXTURE0 + unit);
        gl.bindTexture(gl.TEXTURE_2D, texture);
        
        // 2x2 Checkerboard (White / Gray)
        const d = new Uint8Array([255, 255, 255, 255, 128, 128, 128, 255, 128, 128, 128, 255, 255, 255, 255, 255]);
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, 2, 2, 0, gl.RGBA, gl.UNSIGNED_BYTE, d);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    }}

    createProgram(gl, vSrc, fSrc) {{
        const vs = gl.createShader(gl.VERTEX_SHADER);
        gl.shaderSource(vs, vSrc);
        gl.compileShader(vs);
        if (!gl.getShaderParameter(vs, gl.COMPILE_STATUS)) {{ console.error("VS:", gl.getShaderInfoLog(vs)); return null; }}
        const fs = gl.createShader(gl.FRAGMENT_SHADER);
        gl.shaderSource(fs, fSrc);
        gl.compileShader(fs);
        if (!gl.getShaderParameter(fs, gl.COMPILE_STATUS)) {{ console.error("FS:", gl.getShaderInfoLog(fs)); return null; }}
        const prog = gl.createProgram();
        gl.attachShader(prog, vs);
        gl.attachShader(prog, fs);
        gl.linkProgram(prog);
        if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) return null;
        return prog;
    }}

    renderLoop() {{
        if (!this.gl || !this.program) return;
        
        const dPR = window.devicePixelRatio || 1;
        const displayWidth  = Math.floor(this.canvas.clientWidth * dPR);
        const displayHeight = Math.floor(this.canvas.clientHeight * dPR);

        if (this.canvas.width !== displayWidth || this.canvas.height !== displayHeight) {{
            this.canvas.width  = displayWidth;
            this.canvas.height = displayHeight;
            this.gl.viewport(0, 0, displayWidth, displayHeight);
        }}

        // Standard Auto-Uniforms
        const uRes = this.gl.getUniformLocation(this.program, "u_resolution") || this.gl.getUniformLocation(this.program, "uResolution");
        if(uRes) this.gl.uniform2f(uRes, this.canvas.width, this.canvas.height);

        const uTime = this.gl.getUniformLocation(this.program, "u_time") || this.gl.getUniformLocation(this.program, "uTime");
        if(uTime) this.gl.uniform1f(uTime, performance.now() / 1000.0);

        this.gl.drawArrays(this.gl.TRIANGLES, 0, 6);
        requestAnimationFrame(() => this.renderLoop());
    }}

    {properties_code}
}}

customElements.define('{tagname}', {classname});
"#, 
    vertex_source=vertex_source,
    fragment_source=fragment_source, 
    fallback_frag=fallback_frag,
    locations_code=locations_code,
    apply_defaults_code=apply_defaults_code,
    properties_code=properties_code,
    texture_init_code=texture_init_code,
    classname=args.classname,
    tagname=args.tagname,
    detected_names=detected_names_js // Inject the list here
    );

    fs::write(&args.output, js_content)?;
    println!("Generated web component: {}", args.output.display());
    Ok(())
}

fn format_value_as_js(val: &UniformValue) -> String {
    match val {
        UniformValue::Bool(b) => b.to_string(),
        UniformValue::Float(f) => format!("{:.4}", f),
        UniformValue::Vec2(v) => format!("[{:.4}, {:.4}]", v[0], v[1]),
        UniformValue::Vec3(v) => format!("[{:.4}, {:.4}, {:.4}]", v[0], v[1], v[2]),
        UniformValue::Vec4(v) => format!("[{:.4}, {:.4}, {:.4}, {:.4}]", v[0], v[1], v[2], v[3]),
        UniformValue::Sampler2D(_) => "null".to_string(),
    }
}
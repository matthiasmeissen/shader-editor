use crate::data::*;
use std::collections::HashMap;
use egui_glow::glow;

pub struct ShaderRenderer {
    pub program: glow::Program,
    pub vertex_array: glow::VertexArray,
}

impl ShaderRenderer {
    pub fn new(gl: &glow::Context, fragment_shader_source: &str) -> Result<Self, String> {
        use glow::HasContext as _;

        let shader_version = egui_glow::ShaderVersion::get(gl);

        // SAFETY: All OpenGL calls are made with a valid context.
        // Error handling ensures resources are cleaned up on failure.
        unsafe {
            let program = gl.create_program().map_err(|e| e.to_string())?;

            let vertex_shader_source = r#"
                out vec2 v_uv;
                
                const vec2 verts[4] = vec2[4](
                    vec2(-1.0, -1.0), vec2(1.0, -1.0),
                    vec2(-1.0, 1.0),  vec2(1.0, 1.0)
                );
                
                const vec2 uvs[4] = vec2[4](
                    vec2(0.0, 0.0), vec2(1.0, 0.0),
                    vec2(0.0, 1.0), vec2(1.0, 1.0)
                );
                
                void main() {
                    v_uv = uvs[gl_VertexID];
                    gl_Position = vec4(verts[gl_VertexID], 0.0, 1.0);
                }
            "#;

            let shader_sources = [
                (glow::VERTEX_SHADER, vertex_shader_source),
                (glow::FRAGMENT_SHADER, fragment_shader_source),
            ];
            
            let mut shaders = Vec::with_capacity(shader_sources.len());

            for (shader_type, shader_source) in shader_sources.iter() {
                let shader = gl.create_shader(*shader_type).map_err(|e| e.to_string())?;
                
                let source_with_version = if *shader_type == glow::FRAGMENT_SHADER {
                    shader_source.to_string()
                } else {
                    format!("{}\n{}", shader_version.version_declaration(), shader_source)
                };

                gl.shader_source(shader, &source_with_version);
                gl.compile_shader(shader);

                if !gl.get_shader_compile_status(shader) {
                    let info_log = gl.get_shader_info_log(shader);
                    gl.delete_shader(shader);
                    // Clean up program and any previously compiled shaders
                    for prev_shader in shaders {
                        gl.detach_shader(program, prev_shader);
                        gl.delete_shader(prev_shader);
                    }
                    gl.delete_program(program);
                    return Err(info_log);
                }
                
                gl.attach_shader(program, shader);
                shaders.push(shader);
            }

            gl.link_program(program);
            if !gl.get_program_link_status(program) {
                let info_log = gl.get_program_info_log(program);
                for shader in shaders {
                    gl.detach_shader(program, shader);
                    gl.delete_shader(shader);
                }
                gl.delete_program(program);
                return Err(info_log);
            }

            // Clean up shader objects after successful linking
            for shader in shaders {
                gl.detach_shader(program, shader);
                gl.delete_shader(shader);
            }

            let vertex_array = gl.create_vertex_array().map_err(|e| e.to_string())?;

            Ok(Self { program, vertex_array })
        }
    }

    pub fn destroy(&self, gl: &glow::Context) {
        use glow::HasContext as _;
        // SAFETY: Deleting resources that were created with the same context.
        // This is called during cleanup when the context is still valid.
        unsafe {
            gl.delete_program(self.program);
            gl.delete_vertex_array(self.vertex_array);
        }
    }

    pub fn paint(&self, gl: &glow::Context, time: f32, size: egui::Vec2, uniforms: &HashMap<String, UniformInfo>) {
        use glow::HasContext as _;
        // SAFETY: Rendering with a valid OpenGL context and program.
        // All uniform locations are queried before use.
        unsafe {
            gl.use_program(Some(self.program));
            
            // Set built-in uniforms
            if let Some(loc) = gl.get_uniform_location(self.program, "u_time") {
                gl.uniform_1_f32(Some(&loc), time);
            }
            if let Some(loc) = gl.get_uniform_location(self.program, "u_resolution") {
                gl.uniform_2_f32(Some(&loc), size.x, size.y);
            }
            
            // Set custom uniforms
            for (name, uniform_info) in uniforms {
                if name == "u_resolution" || name == "u_time" {
                    continue;
                }
                if let Some(loc) = gl.get_uniform_location(self.program, name) {
                    match &uniform_info.value {
                        UniformValue::Float(val) => {
                            gl.uniform_1_f32(Some(&loc), *val);
                        }
                        UniformValue::Vec2(vals) => {
                            gl.uniform_2_f32(Some(&loc), vals[0], vals[1]);
                        }
                        UniformValue::Vec3(vals) => {
                            gl.uniform_3_f32(Some(&loc), vals[0], vals[1], vals[2]);
                        }
                        UniformValue::Vec4(vals) => {
                            gl.uniform_4_f32(Some(&loc), vals[0], vals[1], vals[2], vals[3]);
                        }
                    }
                }
            }
            
            gl.bind_vertex_array(Some(self.vertex_array));
            gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);
        }
    }
}

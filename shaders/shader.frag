#version 330 core
precision mediump float;

in vec2 v_uv;
uniform vec2 u_resolution;
uniform float u_time;
uniform float uParam1;
uniform float uParam2;
uniform sampler2D uTexture1;

out vec4 out_color;

void main() {
    vec2 uv = v_uv;
    vec2 p = uv - 0.5;
    p.x *= u_resolution.x / u_resolution.y;

    float d = length(p + sin(u_time));

    vec3 tex_color = texture(uTexture1, uv).rgb;

    vec3 col = vec3(d);
    col *= tex_color;

    out_color = vec4(col, 1.0);
}

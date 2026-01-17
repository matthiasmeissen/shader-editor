#version 330 core
precision mediump float;

in vec2 v_uv;
uniform vec2 u_resolution;
uniform float u_time;
uniform float u_progress;
uniform float uParam1;
uniform vec3 uParam2; // Color
uniform bool uParam3;
uniform sampler2D uTexture1;

out vec4 out_color;

void main() {
    vec2 uv = v_uv;
    vec2 p = uv - 0.5;
    p.x *= u_resolution.x / u_resolution.y;

    float cut = mix(0.1, 0.9, uParam1);
    vec3 color = uParam2;
    bool invert = uParam3;

    float circle = length(vec2(p.x + sin(u_time), p.y)) * 2.0;
    float lines = fract(p.x * 4.0 + u_time * 0.8);

    float d = circle * lines;
    d = step(cut, d);
    d = mix(d, 1.0 - d, invert);

    vec3 tex_color = texture(uTexture1, uv).rgb;

    vec3 col = mix(vec3(1.0), color, d);
    //col *= tex_color;

    out_color = vec4(col, 1.0);
}

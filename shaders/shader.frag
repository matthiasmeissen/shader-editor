#version 330 core
precision mediump float;

uniform vec2 u_resolution;
uniform float u_time;

out vec4 out_color;

void main() {
    vec2 uv = gl_FragCoord.xy / u_resolution.xy;
    float d = uv.x;
    vec3 col = vec3(d);
    out_color = vec4(col, 1.0);
}
#version 330 core
precision mediump float;

in vec2 v_uv;
uniform sampler2D u_mainPass;  // Auto-injected from main pass
uniform vec2 u_resolution;
uniform float u_blur_amount;   // Your custom uniform!

out vec4 out_color;

void main() {
    vec2 texelSize = 1.0 / u_resolution;
    vec4 result = vec4(0.0);
    
    int samples = int(u_blur_amount * 10.0);
    float total = 0.0;
    
    for(int x = -samples; x <= samples; x++) {
        for(int y = -samples; y <= samples; y++) {
            vec2 offset = vec2(float(x), float(y)) * texelSize;
            result += texture(u_mainPass, v_uv + offset);
            total += 1.0;
        }
    }
    
    out_color = result / total;
}
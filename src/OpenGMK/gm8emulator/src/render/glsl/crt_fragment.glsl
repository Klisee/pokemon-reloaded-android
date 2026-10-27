uniform sampler2D scene;
uniform vec2 resolution;
uniform float curvature;
uniform float scanline;
uniform float scanline_count;
uniform float mask;
uniform float aberration;
uniform float vignette;
uniform float blur;
uniform float noise;
uniform float brightness;
uniform float saturation;
uniform float glow;
uniform float corner;
uniform float gb_strength;
uniform float gb_dither;
uniform float gb_contrast;
uniform float gb_brightness;
uniform float gb_invert;

in vec2 frag_uv;
out vec4 out_colour;

float noise_at(vec2 p) {
    return fract(sin(dot(p, vec2(12.9898, 78.233))) * 43758.5453);
}

float bayer4(vec2 p) {
    vec2 q = mod(floor(p), 4.0);
    float value = 0.0;
    if (q.y < 1.0) {
        if (q.x < 1.0) value = 0.0;
        else if (q.x < 2.0) value = 8.0;
        else if (q.x < 3.0) value = 2.0;
        else value = 10.0;
    } else if (q.y < 2.0) {
        if (q.x < 1.0) value = 12.0;
        else if (q.x < 2.0) value = 4.0;
        else if (q.x < 3.0) value = 14.0;
        else value = 6.0;
    } else if (q.y < 3.0) {
        if (q.x < 1.0) value = 3.0;
        else if (q.x < 2.0) value = 11.0;
        else if (q.x < 3.0) value = 1.0;
        else value = 9.0;
    } else {
        if (q.x < 1.0) value = 15.0;
        else if (q.x < 2.0) value = 7.0;
        else if (q.x < 3.0) value = 13.0;
        else value = 5.0;
    }
    return value / 16.0;
}

vec3 gb_palette(float level) {
    if (level < 0.25) return vec3(0.10, 0.11, 0.08);
    if (level < 0.50) return vec3(0.32, 0.33, 0.24);
    if (level < 0.75) return vec3(0.65, 0.64, 0.50);
    return vec3(0.90, 0.88, 0.74);
}

vec3 sample_rgb(vec2 uv, vec2 offset) {
    return vec3(
        texture(scene, uv + offset).r,
        texture(scene, uv).g,
        texture(scene, uv - offset).b
    );
}

void main() {
    // The game framebuffer is bottom-up in GL, while the presented quad is top-down.
    vec2 uv = vec2(frag_uv.x, 1.0 - frag_uv.y);
    vec2 centred = uv * 2.0 - 1.0;
    vec2 curved = centred * (1.0 + curvature * dot(centred, centred)) * 0.5 + 0.5;

    vec2 box = (curved - 0.5) * 2.0;
    vec2 inner = abs(box) - (1.0 - corner);
    float edge = length(max(inner, 0.0)) + min(max(inner.x, inner.y), 0.0) - corner;
    if (curved.x < 0.0 || curved.x > 1.0 || curved.y < 0.0 || curved.y > 1.0 || edge > 0.0) {
        out_colour = vec4(0.0, 0.0, 0.0, 1.0);
        return;
    }

    vec2 scene_size = vec2(textureSize(scene, 0));
    vec2 texel = 1.0 / scene_size;
    vec2 shift = (curved - 0.5) * aberration;
    vec3 colour = sample_rgb(curved, shift);

    if (blur > 0.0) {
        vec2 step_size = texel * blur;
        vec3 soft = texture(scene, curved + vec2(step_size.x, 0.0)).rgb;
        soft += texture(scene, curved - vec2(step_size.x, 0.0)).rgb;
        soft += texture(scene, curved + vec2(0.0, step_size.y)).rgb;
        soft += texture(scene, curved - vec2(0.0, step_size.y)).rgb;
        colour = mix(colour, soft * 0.25, clamp(blur, 0.0, 1.0) * 0.35);
    }

    float scan = 0.5 + 0.5 * sin(uv.y * scanline_count * 3.14159265);
    colour *= 1.0 - scanline * scan;

    float grille = mod(gl_FragCoord.x, 3.0);
    vec3 aperture = vec3(1.0);
    if (grille < 1.0) {
        aperture = vec3(1.0, 0.38, 0.38);
    } else if (grille < 2.0) {
        aperture = vec3(0.38, 1.0, 0.38);
    } else {
        aperture = vec3(0.38, 0.38, 1.0);
    }
    colour *= mix(vec3(1.0), aperture, clamp(mask, 0.0, 1.0));

    float vig = smoothstep(0.95, 0.25, length(centred));
    colour *= mix(1.0, vig, clamp(vignette, 0.0, 1.0));

    float luminance = dot(colour, vec3(0.299, 0.587, 0.114));
    colour = mix(vec3(luminance), colour, saturation);
    colour *= brightness;

    vec2 bloom_step = texel * 1.5;
    vec3 bloom = texture(scene, curved + vec2(bloom_step.x, 0.0)).rgb;
    bloom += texture(scene, curved - vec2(bloom_step.x, 0.0)).rgb;
    bloom += texture(scene, curved + vec2(0.0, bloom_step.y)).rgb;
    bloom += texture(scene, curved - vec2(0.0, bloom_step.y)).rgb;
    bloom *= 0.25;
    bloom = max(bloom - 0.45, 0.0) * 1.7;
    colour += bloom * glow;

    float grain = noise_at(gl_FragCoord.xy + resolution);
    colour += (grain - 0.5) * noise;

    if (gb_strength > 0.0) {
        float level = dot(colour, vec3(0.299, 0.587, 0.114));
        level = clamp((level - 0.5) * gb_contrast + 0.5, 0.0, 1.0);
        level = clamp(level * gb_brightness, 0.0, 1.0);
        vec2 source_pixel = floor(curved * scene_size);
        float dither_band = 4.0 * level * (1.0 - level);
        level += (bayer4(source_pixel) - 0.5) * gb_dither * dither_band * 0.30;
        level = clamp(level, 0.0, 0.999);
        if (gb_invert > 0.5) level = 1.0 - level;
        vec3 mapped = gb_palette(level);
        colour = mix(colour, mapped, clamp(gb_strength, 0.0, 1.0));
    }

    out_colour = vec4(clamp(colour, 0.0, 1.0), 1.0);
}

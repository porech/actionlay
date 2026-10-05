// Telemetry overlay: premultiplied, sRGB-encoded RGBA from actionlay-render, drawn over
// the video with BlendState::PREMULTIPLIED_ALPHA_BLENDING (out = src + dst * (1 - src.a)).
@group(0) @binding(0) var overlay_tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    // same quad as yuv.wgsl: covers the viewport egui gives the callback (the video rect)
    var corners = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
    );
    let c = corners[i];
    var out: VsOut;
    out.pos = vec4(c.x * 2.0 - 1.0, 1.0 - c.y * 2.0, 0.0, 1.0);
    out.uv = c;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(overlay_tex, samp, in.uv);
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + vec3(0.055)) / 1.055, vec3(2.4));
    return select(hi, lo, c <= vec3(0.04045));
}

// sRGB targets encode the output and blend in linear light: convert the colour
// (not the coverage) to linear, keeping it premultiplied.
@fragment
fn fs_main_srgb(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(overlay_tex, samp, in.uv);
    if (c.a <= 0.0) {
        return vec4(0.0);
    }
    return vec4(srgb_to_linear(c.rgb / c.a) * c.a, c.a);
}

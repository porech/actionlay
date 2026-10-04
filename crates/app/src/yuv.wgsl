// Full-screen quad sampling NV12 (Y + interleaved UV) and converting to RGB.
struct Params {
    // rows of the 3x4 YUV->RGB matrix
    r: vec4<f32>,
    g: vec4<f32>,
    b: vec4<f32>,
};

@group(0) @binding(0) var y_tex: texture_2d<f32>;
@group(0) @binding(1) var uv_tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;
@group(0) @binding(3) var<uniform> params: Params;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    // two triangles covering the viewport set by egui to the callback rect
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

fn convert(uv_in: vec2<f32>) -> vec3<f32> {
    let y = textureSample(y_tex, samp, uv_in).r;
    let uv = textureSample(uv_tex, samp, uv_in).rg;
    let yuv = vec4(y, uv.x, uv.y, 1.0);
    return clamp(vec3(dot(params.r, yuv), dot(params.g, yuv), dot(params.b, yuv)), vec3(0.0), vec3(1.0));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return vec4(convert(in.uv), 1.0);
}

// Same conversion for sRGB render targets: the hardware re-encodes the output,
// so undo the sRGB transfer function first to reproduce the gamma-encoded values.
fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + vec3(0.055)) / 1.055, vec3(2.4));
    return select(hi, lo, c <= vec3(0.04045));
}

@fragment
fn fs_main_srgb(in: VsOut) -> @location(0) vec4<f32> {
    return vec4(srgb_to_linear(convert(in.uv)), 1.0);
}

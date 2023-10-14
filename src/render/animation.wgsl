struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) tex_coords: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
}

struct Uniforms {
    time: f32,
}

@vertex
fn vs_main(
    model: VertexInput,
) -> VertexOutput {
    var out: VertexOutput;
    out.tex_coords = model.tex_coords;
    out.clip_position = vec4<f32>(model.position, 1.0);
    return out;
}

const TAU: f32 = 6.28318530717958647692528676655900577;
const SPEED: f32 = 1.5;

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let mode = 3f;
    let x = in.tex_coords.x;
    let y = (in.tex_coords.y - 0.5) * 2.;
    let time = uniforms.time;
    let y0 = wave(x, 2f, time);
    let y1 = wave(x, 3f, time);
    let y2 = wave(x, 5f, time);
    if ((y < y0[0] || y > y0[1]) && (y < y1[0] || y > y1[1]) && (y < y2[0] || y > y2[1])) {
        discard;
    }

    return vec4<f32>(0.5, 0.5, 0.5, 0.5);
}

fn wave(x: f32, mode: f32, time: f32) -> vec2<f32> {
    let thickness = 0.05f / mode;
    let amp = sin(time * SPEED * mode) / mode;
    let y = amp * sin(x * TAU / 2.0 * mode);
    return vec2<f32>(y - thickness, y + thickness);
}

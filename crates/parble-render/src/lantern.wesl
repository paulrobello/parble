// Paper lantern shade (crates/parble-render/src/playroom.rs): StandardMaterial + LanternExt (binding 100).
// Lit paper glows most where it is seen face on and dims towards the silhouette, where the view
// crosses the paper at a grazing angle, so the emissive is scaled by mix(edge, 1, (N.V)^power).
// Rice paper lets light through unevenly: world-space value noise (grain), stretched noise (fibres)
// and puckers between the ribs modulate the transmitted glow, and slightly the albedo. Each octave
// fades out as it nears the pixel size, so a distant or Low-tier lantern does not shimmer.
// Main forward pass only: the prepass uses StandardMaterial's own shaders.
import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
};

struct Lantern {
    // Emissive scale at the silhouette (N.V = 0).
    edge: f32,
    // Falloff exponent on N.V.
    power: f32,
    // Inner glow seen through the paper: hot * (N.V)^hot_power on top of the falloff.
    hot: f32,
    hot_power: f32,
    // Paper texture: base grain frequency (cycles per metre), and the grain, fibre and pucker
    // amplitudes as fractions of the transmitted glow. All 0 on the ribs.
    grain_freq: f32,
    grain: f32,
    fibre: f32,
    pucker: f32,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> lantern: Lantern;

fn hash(p: vec3<f32>) -> f32 {
    var q = fract(p * 0.1031);
    q += dot(q, q.zyx + 31.32);
    return fract((q.x + q.y) * q.z);
}

// Trilinear value noise in [-0.5, 0.5].
fn vnoise(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = mix(mix(hash(i), hash(i + vec3(1.0, 0.0, 0.0)), u.x),
                mix(hash(i + vec3(0.0, 1.0, 0.0)), hash(i + vec3(1.0, 1.0, 0.0)), u.x), u.y);
    let b = mix(mix(hash(i + vec3(0.0, 0.0, 1.0)), hash(i + vec3(1.0, 0.0, 1.0)), u.x),
                mix(hash(i + vec3(0.0, 1.0, 1.0)), hash(i + vec3(1.0, 1.0, 1.0)), u.x), u.y);
    return mix(a, b, u.z) - 0.5;
}

// 1 while a feature of this period spans several pixels, 0 once it nears one.
fn aa(q: vec3<f32>) -> f32 {
    return 1.0 - smoothstep(0.25, 0.5, length(fwidth(q)));
}

// Relative modulation of the transmitted light at world position p, normal n.
fn paper(p: vec3<f32>, n: vec3<f32>) -> f32 {
    var g = 0.0;
    var amp = 0.55;
    var q = p * lantern.grain_freq;
    for (var o = 0; o < 4; o++) {
        g += amp * vnoise(q) * aa(q);
        q = q * 2.03 + vec3(17.1, 5.3, 9.7);
        amp *= 0.55;
    }
    // Fibres: noise stretched along two crossing directions, kept only where it peaks.
    let f1q = vec3(p.x + p.y, p.y - p.z, p.z + p.x) * lantern.grain_freq * vec3(0.4, 6.0, 6.0);
    let f2q = vec3(p.z - p.y, p.x + p.z, p.y - p.x) * lantern.grain_freq * vec3(6.0, 0.4, 6.0);
    let fib = max(vnoise(f1q) * aa(f1q), vnoise(f2q) * aa(f2q));
    let fibres = smoothstep(0.15, 0.4, fib);
    // Puckers: slow wrinkles across each band, strongest midway between the ribs (every 0.38 rad).
    let lat = asin(clamp(n.y, -1.0, 1.0));
    let lon = atan2(n.z, n.x);
    let band = sin(3.14159265 * fract(lat / 0.38 + 0.5));
    let pq = vec3(lon * 9.0, lat * 22.0, 0.0);
    let puck = vnoise(pq) * aa(pq) * band;
    return lantern.grain * g + lantern.fibre * fibres + lantern.pucker * puck;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let n = normalize(in.world_normal);
    let n_dot_v = saturate(dot(n, pbr_input.V));
    let tex = paper(in.world_position.xyz, n);
    let k = (mix(lantern.edge, 1.0, pow(n_dot_v, lantern.power)) + lantern.hot * pow(n_dot_v, lantern.hot_power)) * (1.0 + tex);
    pbr_input.material.emissive = vec4(pbr_input.material.emissive.rgb * k, pbr_input.material.emissive.a);
    let albedo = pbr_input.material.base_color.rgb * (1.0 + 0.5 * tex);
    pbr_input.material.base_color = vec4(albedo, pbr_input.material.base_color.a);

    pbr_input.material.base_color = alpha_discard(pbr_input.material.flags, pbr_input.material.alpha_cutoff, pbr_input.material.base_color);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}

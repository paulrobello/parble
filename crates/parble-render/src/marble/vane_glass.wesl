// Fallback cat's eye glass for the Low and Medium tiers (crates/parble-render/src/marble/mod.rs).
// StandardMaterial + VaneGlassExt (bindings 100-104). The sphere is opaque: instead of Bevy's
// screen-space transmission, the fragment refracts the view ray into the marble (IOR 1.5), marches
// it through the unit sphere in the marble's local frame and composites the inner vanes it crosses,
// front to back, over a dim glass body. The vane surfaces are the same twisted petals that
// `vane_mesh` builds for the High tier: angle(p) = blade + twist·y + curl·ρ, inside the petal
// ρ < width·(1 − (y/height)²)^0.6. A blade is hit where the signed angular offset to the nearest
// blade changes sign between two steps. PBR lighting then adds the glass's own reflections.
import bevy_pbr::render::{
    mesh_functions,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
};

@if(PREPASS_PIPELINE)
import bevy_pbr::{
    prepass::io::{VertexOutput, FragmentOutput},
    deferred::functions::deferred_output,
};
@else
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
};

struct VaneParams {
    // Marble radius in mesh units (metres).
    radius: f32,
    // Petal half-height and maximum radial extent, unit-sphere units.
    height: f32,
    width: f32,
    // Twist along y and curl along ρ, radians per unit.
    twist: f32,
    curl: f32,
    // March steps across the chord (Low 16, Medium 32).
    steps: f32,
    // Opacity of one vane crossing.
    vane_alpha: f32,
    // Number of blades, alternating red and green textures.
    blades: f32,
    // Glass body tint seen where no vane is crossed (linear RGB).
    body: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> vane: VaneParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var vane_a_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var vane_a_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var vane_b_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var vane_b_sampler: sampler;

const PI: f32 = 3.14159265;

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3(2.4));
    return select(hi, lo, c <= vec3(0.04045));
}

// Signed blade coordinate: k = blade-space angle in blade units; nearest blade = round(k).
fn blade_k(p: vec3<f32>) -> f32 {
    let rho = length(p.xz);
    let a = atan2(p.z, p.x) - vane.twist * p.y - vane.curl * rho;
    return a * vane.blades / (2.0 * PI);
}

fn petal_extent(y: f32) -> f32 {
    let t = clamp(1.0 - (y / vane.height) * (y / vane.height), 0.0, 1.0);
    return vane.width * pow(t, 0.6);
}

fn inner_color(p0: vec3<f32>, d: vec3<f32>) -> vec3<f32> {
    // Unit sphere, entry on the surface: the exit is at t = -2·dot(p0, d).
    let t_exit = max(-2.0 * dot(p0, d), 0.0);
    let n = max(vane.steps, 2.0);
    let dt = t_exit / n;
    var acc = vec3(0.0);
    var trans = 1.0;
    var prev_k = blade_k(p0);
    var prev_p = p0;
    for (var i = 1.0; i <= n; i += 1.0) {
        let p = p0 + d * (dt * i);
        let k = blade_k(p);
        let b0 = round(prev_k);
        let s0 = prev_k - b0;
        let s1 = k - b0;
        // Crossed blade b0 (no wrap jump, both samples within half a blade of it).
        if (sign(s0) != sign(s1) && abs(s0) < 0.5 && abs(s1) < 0.5) {
            let f = s0 / (s0 - s1);
            let hit = mix(prev_p, p, f);
            let rho = length(hit.xz);
            let extent = petal_extent(hit.y);
            if (abs(hit.y) < vane.height && rho < extent) {
                let uv = vec2(rho / max(extent, 1e-4), 0.5 - hit.y / (2.0 * vane.height));
                let blade = i32(b0 - floor(b0 / 2.0) * 2.0);
                var c: vec3<f32>;
                if (blade == 0) {
                    c = textureSampleLevel(vane_a_texture, vane_a_sampler, uv, 0.0).rgb;
                } else {
                    c = textureSampleLevel(vane_b_texture, vane_b_sampler, uv, 0.0).rgb;
                }
                // Soft petal edge.
                let edge = 1.0 - smoothstep(0.85, 1.0, rho / max(extent, 1e-4));
                let a = vane.vane_alpha * edge;
                acc += trans * a * srgb_to_linear(c);
                trans *= 1.0 - a;
            }
        }
        prev_k = k;
        prev_p = p;
    }
    // What the ray leaves through: the dim glass body, lighter toward the top of the marble so the
    // lit room reads through it.
    let lift = 0.6 + 0.4 * clamp(d.y * 0.5 + 0.5, 0.0, 1.0);
    return acc + trans * vane.body.rgb * lift;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

@if(!PREPASS_PIPELINE) {
    let m = mesh_functions::get_world_from_local(in.instance_index);
    let center = m[3].xyz;
    let ax = normalize(m[0].xyz);
    let ay = normalize(m[1].xyz);
    let az = normalize(m[2].xyz);
    let to_local = transpose(mat3x3<f32>(ax, ay, az));
    let p0 = to_local * ((in.world_position.xyz - center) / vane.radius);
    let refracted = refract(-pbr_input.V, pbr_input.N, 1.0 / 1.5);
    let d = normalize(to_local * refracted);
    let c = inner_color(normalize(p0), d);
    pbr_input.material.base_color = vec4(c, 1.0);
}

    pbr_input.material.base_color = alpha_discard(pbr_input.material.flags, pbr_input.material.alpha_cutoff, pbr_input.material.base_color);
    var out: FragmentOutput;
@if(PREPASS_PIPELINE) {
    out = deferred_output(in, pbr_input);
} @else {
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
}
    return out;
}

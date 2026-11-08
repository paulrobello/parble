// Fallback core glass for the Low and Medium tiers (P5-T1: onionskin, opal, glow;
// crates/parble-render/src/marble/mod.rs). StandardMaterial + CoreGlassExt (bindings 100-102). The
// sphere is opaque: instead of Bevy's screen-space transmission, the fragment refracts the view ray
// into the marble (IOR 1.5), intersects the inner core sphere analytically in the marble's local
// frame and samples the core texture at the hit point (u around the axis, v pole to pole, the same
// mapping as Bevy's UV sphere that carries the texture on the High tier). The core layer is
// composited over the glass body. Opal adds a view-angle colour shift (play of colour); glow
// writes the core colour to emissive so it blooms. PBR lighting then adds the glass reflections.
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

struct CoreParams {
    // Marble radius in mesh units (metres).
    radius: f32,
    // Core radius, unit-sphere units.
    core: f32,
    // Opacity of the core layer over the body.
    layer_alpha: f32,
    // View-angle colour shift strength (opal).
    iridescence: f32,
    // Emissive gain over the core colour (glow).
    emissive: f32,
    // Glass body tint where the ray misses the core (linear RGB).
    body: vec4<f32>,
    // Emissive colour multiplied into the core colour (glow, linear RGB).
    glow_tint: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> cg: CoreParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var core_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var core_sampler: sampler;

const PI: f32 = 3.14159265;

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3(2.4));
    return select(hi, lo, c <= vec3(0.04045));
}

// A smooth hue ramp for the opal's play of colour: blue, violet, teal, green.
fn play_of_colour(t: f32) -> vec3<f32> {
    let a = vec3(0.35, 0.45, 0.80);
    let b = vec3(0.30, 0.35, 0.25);
    return clamp(a + b * cos(2.0 * PI * (vec3(t) + vec3(0.55, 0.30, 0.0))), vec3(0.0), vec3(1.0));
}

// Core colour seen along ray p0 + t·d (unit sphere, p0 on the surface), and whether it hit.
fn core_hit(p0: vec3<f32>, d: vec3<f32>) -> vec4<f32> {
    let b = dot(p0, d);
    let c = dot(p0, p0) - cg.core * cg.core;
    let disc = b * b - c;
    if (disc <= 0.0) {
        return vec4(0.0);
    }
    let t = -b - sqrt(disc);
    if (t <= 0.0) {
        return vec4(0.0);
    }
    let h = (p0 + d * t) / cg.core;
    let u = atan2(h.z, h.x) / (2.0 * PI) + 0.5;
    let v = acos(clamp(h.y, -1.0, 1.0)) / PI;
    let tex = textureSampleLevel(core_texture, core_sampler, vec2(u, v), 0.0).rgb;
    return vec4(srgb_to_linear(tex), 1.0);
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
    let p0 = normalize(to_local * ((in.world_position.xyz - center) / cg.radius));
    let refracted = refract(-pbr_input.V, pbr_input.N, 1.0 / 1.5);
    let d = normalize(to_local * refracted);
    let hit = core_hit(p0, d);
    // The body brightens toward the top of the marble, so the lit room reads through it.
    let lift = 0.6 + 0.4 * clamp(d.y * 0.5 + 0.5, 0.0, 1.0);
    let a = cg.layer_alpha * hit.w;
    var c = mix(cg.body.rgb * lift, hit.rgb, a);
    if (cg.iridescence > 0.0) {
        let ndv = clamp(dot(pbr_input.N, pbr_input.V), 0.0, 1.0);
        let fire = play_of_colour(ndv * 1.7 + dot(hit.rgb, vec3(0.6)));
        c = mix(c, fire, cg.iridescence * (0.4 + 0.6 * (1.0 - ndv)));
    }
    pbr_input.material.base_color = vec4(c, 1.0);
    if (cg.emissive > 0.0) {
        pbr_input.material.emissive = vec4(hit.rgb * cg.glow_tint.rgb * hit.w * cg.emissive, 0.0);
    }
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

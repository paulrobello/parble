// Paper lantern shade (crates/parble-render/src/playroom.rs): StandardMaterial + LanternExt (binding 100).
// Lit paper glows most where it is seen face on and dims towards the silhouette, where the view
// crosses the paper at a grazing angle, so the emissive is scaled by mix(edge, 1, (N.V)^power).
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
    pad0: f32,
    pad1: f32,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> lantern: Lantern;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let n_dot_v = saturate(dot(normalize(in.world_normal), pbr_input.V));
    let k = mix(lantern.edge, 1.0, pow(n_dot_v, lantern.power));
    pbr_input.material.emissive = vec4(pbr_input.material.emissive.rgb * k, pbr_input.material.emissive.a);

    pbr_input.material.base_color = alpha_discard(pbr_input.material.flags, pbr_input.material.alpha_cutoff, pbr_input.material.base_color);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}

// Kit wood (crates/parble-render/src/kit/mod.rs): StandardMaterial + KitWoodExt (bindings 100-104).
// The StandardMaterial half samples the piece's baked AO and bevel normal at UV0 (one atlas per piece
// type and LOD). The tileable wood is projected in world space instead, triplanar on the geometric
// normal at one repeat per `tile_m` metres (kit contract §12), so the grain runs on across pieces.
// Wood ORM is R = AO, G = roughness, B = metallic. Main forward pass only: the prepass and shadows use
// StandardMaterial's own shaders.
import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
};

@if(VISIBILITY_RANGE_DITHER)
import bevy_pbr::render::pbr_functions::visibility_range_dither;

struct KitWood {
    // 1 / repeat length in metres.
    inv_tile_m: f32,
    // 1 once the wood maps are bound, else the StandardMaterial base colour is kept.
    enabled: f32,
    // Mip bias: 1 on Low (2,048 texels per metre), 0 on High.
    mip_bias: f32,
    pad: f32,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> wood: KitWood;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var wood_base_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var wood_base_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var wood_orm_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var wood_orm_sampler: sampler;

fn triplanar(t: texture_2d<f32>, s: sampler, p: vec3<f32>, w: vec3<f32>) -> vec4<f32> {
    return textureSampleBias(t, s, p.zy, wood.mip_bias) * w.x
        + textureSampleBias(t, s, p.xz, wood.mip_bias) * w.y
        + textureSampleBias(t, s, p.xy, wood.mip_bias) * w.z;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    // This shader replaces pbr.wesl, so it must apply the LOD crossfade dither itself.
@if(VISIBILITY_RANGE_DITHER)
    visibility_range_dither(in.position, in.visibility_range_dither);

    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let p = in.world_position.xyz * wood.inv_tile_m;
    var w = pow(abs(normalize(in.world_normal)), vec3(4.0));
    w = w / max(w.x + w.y + w.z, 1e-5);
    let base = triplanar(wood_base_texture, wood_base_sampler, p, w);
    let orm = triplanar(wood_orm_texture, wood_orm_sampler, p, w);
    if (wood.enabled > 0.5) {
        pbr_input.material.base_color = vec4(base.rgb * pbr_input.material.base_color.rgb, 1.0);
        pbr_input.material.perceptual_roughness = orm.g;
        pbr_input.material.metallic = orm.b;
        pbr_input.diffuse_occlusion *= orm.r;
    }

    pbr_input.material.base_color = alpha_discard(pbr_input.material.flags, pbr_input.material.alpha_cutoff, pbr_input.material.base_color);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}

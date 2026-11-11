//! A procedural night-playroom environment map, built on the CPU at startup, so the steel and glass
//! marbles have something to reflect before the P1-T5 environment probe lands. It is only attached
//! to a camera that has no [`EnvironmentMapLight`] of its own.
//!
//! The radiance is a dim warm room (floor, walls, ceiling) plus two warm lantern lobes and a cool
//! window lobe, each a von Mises–Fisher lobe `exp(k·(cosθ − 1))`. Each specular mip is the same
//! function pre-convolved with the GGX lobe of that mip's roughness (Bevy samples mip
//! `perceptual_roughness × (levels − 1)`), using the vMF product rule `k' = k·kr / (k + kr)` with
//! `kr = 2 / α²`, `α = roughness²`, and the amplitude scaled by `k' / k` to keep the lobe's energy.
//! Textures are `Rgb9e5Ufloat` (filterable on every WebGPU adapter) in layer-major mip order.

use bevy::math::Vec3;
use bevy::{
    asset::RenderAssetUsages,
    image::Image,
    render::render_resource::{
        Extent3d, TextureDataOrder, TextureDimension, TextureFormat, TextureViewDescriptor,
        TextureViewDimension,
    },
};

/// Specular cubemap face size (power of two) and its full mip chain length.
pub const SPECULAR_SIZE: u32 = 64;
pub const DIFFUSE_SIZE: u32 = 16;

struct Lobe {
    dir: Vec3,
    color: Vec3,
    sharpness: f32,
}

fn lobes() -> [Lobe; 3] {
    [
        // Warm lantern, upper left behind the camera.
        Lobe {
            dir: Vec3::new(-0.6, 0.35, 0.5).normalize(),
            color: Vec3::new(6.0, 3.6, 1.5),
            sharpness: 900.0,
        },
        // Second, smaller lantern on the far side.
        Lobe {
            dir: Vec3::new(0.75, 0.22, -0.45).normalize(),
            color: Vec3::new(4.0, 2.4, 1.0),
            sharpness: 1_400.0,
        },
        // Cool moonlit window, wide and dim.
        Lobe {
            dir: Vec3::new(0.15, 0.45, -0.9).normalize(),
            color: Vec3::new(0.35, 0.5, 0.9),
            sharpness: 60.0,
        },
    ]
}

/// Room base radiance (no lobes) by direction: warm lit rug below, dim walls at the horizon, a
/// near-black blue ceiling.
fn room(dir: Vec3) -> Vec3 {
    let floor = Vec3::new(0.20, 0.12, 0.06);
    let horizon = Vec3::new(0.05, 0.04, 0.035);
    let ceiling = Vec3::new(0.004, 0.006, 0.014);
    let y = dir.y;
    if y < 0.0 {
        horizon.lerp(floor, (-y * 2.5).min(1.0))
    } else {
        horizon.lerp(ceiling, (y * 2.5).min(1.0))
    }
}

/// Radiance seen along `dir` through a GGX lobe of sharpness `kr` (infinite = mirror).
pub fn radiance(dir: Vec3, kr: f32) -> Vec3 {
    let mut c = room(dir);
    for l in lobes() {
        let k = if kr.is_finite() {
            l.sharpness * kr / (l.sharpness + kr)
        } else {
            l.sharpness
        };
        let amp = k / l.sharpness;
        c += l.color * amp * (k * (dir.dot(l.dir) - 1.0)).exp();
    }
    c
}

/// Direction through texel centre `(x, y)` of cubemap `face` (+X, −X, +Y, −Y, +Z, −Z).
pub fn face_dir(face: u32, x: u32, y: u32, size: u32) -> Vec3 {
    let u = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
    let v = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
    match face {
        0 => Vec3::new(1.0, -v, -u),
        1 => Vec3::new(-1.0, -v, u),
        2 => Vec3::new(u, 1.0, v),
        3 => Vec3::new(u, -1.0, -v),
        4 => Vec3::new(u, -v, 1.0),
        _ => Vec3::new(-u, -v, -1.0),
    }
    .normalize()
}

/// Pack linear RGB into the shared-exponent `Rgb9e5Ufloat` layout (wgpu / D3D convention).
pub fn pack_rgb9e5(c: Vec3) -> u32 {
    const N: i32 = 9;
    const B: i32 = 15;
    const MAX: f32 = 65_408.0;
    let [r, g, b] = c.to_array().map(|v| v.clamp(0.0, MAX));
    let m = r.max(g).max(b);
    if m <= 0.0 {
        return 0;
    }
    let mut e = (m.log2().floor() as i32).max(-B - 1) + 1 + B;
    let mut denom = 2f32.powi(e - B - N);
    if (m / denom + 0.5).floor() as i32 == 1 << N {
        denom *= 2.0;
        e += 1;
    }
    let q = |v: f32| ((v / denom + 0.5).floor() as u32).min(511);
    q(r) | (q(g) << 9) | (q(b) << 18) | ((e as u32) << 27)
}

/// Kept in the main world too: a renderer rebuilt after a lost GPU device uploads it again.
fn cubemap(size: u32, mips: u32, mip_kr: impl Fn(u32) -> f32) -> Image {
    let mut data = Vec::new();
    for face in 0..6 {
        for mip in 0..mips {
            let s = (size >> mip).max(1);
            let kr = mip_kr(mip);
            for y in 0..s {
                for x in 0..s {
                    let p = pack_rgb9e5(radiance(face_dir(face, x, y, s), kr));
                    data.extend_from_slice(&p.to_le_bytes());
                }
            }
        }
    }
    let mut image = Image::new_fill(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 6,
        },
        TextureDimension::D2,
        &[0; 4],
        TextureFormat::Rgb9e5Ufloat,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.mip_level_count = mips;
    image.data = Some(data);
    image.data_order = TextureDataOrder::LayerMajor;
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..Default::default()
    });
    image
}

/// The specular cubemap with its full mip chain, pre-convolved per mip.
pub fn specular_map() -> Image {
    let mips = SPECULAR_SIZE.trailing_zeros() + 1;
    cubemap(SPECULAR_SIZE, mips, |mip| {
        if mip == 0 {
            return f32::INFINITY;
        }
        let roughness = mip as f32 / (mips - 1) as f32;
        let alpha = roughness * roughness;
        2.0 / (alpha * alpha).max(1e-4)
    })
}

/// The diffuse (irradiance) cubemap: the radiance through a cosine-like lobe (vMF k ≈ 2).
pub fn diffuse_map() -> Image {
    cubemap(DIFFUSE_SIZE, 1, |_| 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb9e5_round_trips_within_a_mantissa_step() {
        for c in [
            Vec3::new(0.20, 0.12, 0.06),
            Vec3::new(6.0, 3.6, 1.5),
            Vec3::new(1.0, 0.0, 0.5),
        ] {
            let p = pack_rgb9e5(c);
            let e = (p >> 27) as i32 - 15 - 9;
            let scale = 2f32.powi(e);
            let back = Vec3::new(
                (p & 511) as f32 * scale,
                ((p >> 9) & 511) as f32 * scale,
                ((p >> 18) & 511) as f32 * scale,
            );
            assert!(
                (back - c).abs().max_element() <= c.max_element() / 256.0,
                "{c} -> {back}"
            );
        }
        assert_eq!(pack_rgb9e5(Vec3::ZERO), 0);
    }

    #[test]
    fn cubemaps_have_the_bytes_of_a_full_mip_chain() {
        let s = specular_map();
        let texels: u32 = (0..7).map(|m| (SPECULAR_SIZE >> m).pow(2)).sum();
        assert_eq!(s.texture_descriptor.mip_level_count, 7);
        assert_eq!(s.data.as_ref().unwrap().len() as u32, texels * 6 * 4);
        let d = diffuse_map();
        assert_eq!(
            d.data.as_ref().unwrap().len() as u32,
            DIFFUSE_SIZE.pow(2) * 6 * 4
        );
    }

    #[test]
    fn rougher_mips_spread_the_lantern() {
        let l = lobes()[0].dir;
        let off = (l + Vec3::new(0.0, 0.2, 0.0)).normalize();
        assert!(radiance(l, f32::INFINITY).x > radiance(l, 20.0).x);
        assert!(radiance(off, f32::INFINITY).x < radiance(off, 20.0).x);
    }
}

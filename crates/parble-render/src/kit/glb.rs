//! Kit GLB decode. `EXT_meshopt_compression` buffer views are decoded into a plain GLB (ported from
//! hex-world `hex-assets::meshopt_glb`), then the single primitive of each `<id>_lod0..2` node is
//! read into a [`LodMesh`]. The collider node belongs to parble-kit and is ignored here.
use bevy::math::{Mat4, Quat, Vec3};
use meshoptimizer_rs::codec::{BufferView, Filter, Mode};
use meshoptimizer_rs::{Limits, Workspace};
use serde_json::{Map, Value};
use std::borrow::Cow;

#[derive(Debug)]
pub enum GlbError {
    NotGlb,
    BadJson(String),
    Decode(String),
}

impl std::fmt::Display for GlbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for GlbError {}

const EXT: &str = "EXT_meshopt_compression";
/// Cap on one decoded view and on the whole rewritten BIN (the GLB is untrusted input).
const MAX_DECODED: usize = 256 << 20;

fn u32_at(b: &[u8], o: usize) -> Result<u32, GlbError> {
    b.get(o..o + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or(GlbError::NotGlb)
}

/// `bin[off..off + len]`, with overflow-checked bounds (the JSON is untrusted).
fn slice<'a>(bin: &'a [u8], off: usize, len: usize, what: &str) -> Result<&'a [u8], GlbError> {
    off.checked_add(len)
        .and_then(|end| bin.get(off..end))
        .ok_or_else(|| GlbError::Decode(format!("{what} out of range")))
}

fn usize_field(v: &Map<String, Value>, key: &str) -> Result<Option<usize>, GlbError> {
    match v.get(key) {
        None => Ok(None),
        Some(x) => x
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .map(Some)
            .ok_or_else(|| GlbError::BadJson(format!("{key} is not an unsigned integer"))),
    }
}

fn required(v: &Map<String, Value>, key: &str) -> Result<usize, GlbError> {
    usize_field(v, key)?.ok_or_else(|| GlbError::BadJson(format!("missing {key}")))
}

/// Decodes EXT_meshopt_compression bufferViews into a plain GLB that bevy_gltf can load (TC3).
pub fn rewrite_glb(bytes: &[u8]) -> Result<Cow<'_, [u8]>, GlbError> {
    if bytes.len() < 20 || &bytes[0..4] != b"glTF" || u32_at(bytes, 4)? != 2 {
        return Err(GlbError::NotGlb);
    }
    let json_len = u32_at(bytes, 12)? as usize;
    if &bytes[16..20] != b"JSON" {
        return Err(GlbError::NotGlb);
    }
    let json_bytes = bytes
        .get(20..20usize.saturating_add(json_len))
        .ok_or(GlbError::NotGlb)?;
    if !json_bytes.windows(EXT.len()).any(|w| w == EXT.as_bytes()) {
        return Ok(Cow::Borrowed(bytes));
    }
    let mut json: Value =
        serde_json::from_slice(json_bytes).map_err(|e| GlbError::BadJson(e.to_string()))?;
    let bin_off = 20 + json_len;
    let bin = match bytes.get(bin_off..bin_off.saturating_add(8)) {
        Some(h) if &h[4..8] == b"BIN\0" => {
            let n = u32_at(bytes, bin_off)? as usize;
            slice(bytes, bin_off + 8, n, "BIN chunk").map_err(|_| GlbError::NotGlb)?
        }
        _ => &[][..],
    };

    let root = json
        .as_object_mut()
        .ok_or_else(|| GlbError::BadJson("root is not an object".into()))?;
    let mut workspace = Workspace::new(Limits {
        max_bytes: MAX_DECODED,
        ..Limits::default()
    });
    let mut out_bin: Vec<u8> = Vec::with_capacity(bin.len().saturating_mul(4).min(MAX_DECODED));
    let views = root
        .get_mut("bufferViews")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| GlbError::BadJson("no bufferViews".into()))?;
    for view in views.iter_mut() {
        let view = view
            .as_object_mut()
            .ok_or_else(|| GlbError::BadJson("bufferView is not an object".into()))?;
        let data = decode_view(view, bin, out_bin.len(), &mut workspace)?;
        while !out_bin.len().is_multiple_of(4) {
            out_bin.push(0);
        }
        view.insert("buffer".into(), 0.into());
        view.insert("byteOffset".into(), out_bin.len().into());
        view.insert("byteLength".into(), data.len().into());
        out_bin.extend_from_slice(&data);
    }
    // The meshopt fallback buffer (uri-less placeholder) is dropped: every view now lives in BIN.
    root.insert(
        "buffers".into(),
        serde_json::json!([{ "byteLength": out_bin.len() }]),
    );
    for key in ["extensionsUsed", "extensionsRequired"] {
        let empty = root.get_mut(key).and_then(Value::as_array_mut).map(|list| {
            list.retain(|v| v != EXT);
            list.is_empty()
        });
        if empty == Some(true) {
            root.remove(key);
        }
    }
    Ok(Cow::Owned(write_glb(&json, &out_bin)?))
}

fn decode_view(
    view: &mut Map<String, Value>,
    bin: &[u8],
    out_len: usize,
    workspace: &mut Workspace,
) -> Result<Vec<u8>, GlbError> {
    let ext = match view.get("extensions").and_then(|e| e.get(EXT)) {
        None => None,
        Some(e) => Some(
            e.as_object()
                .ok_or_else(|| GlbError::BadJson(format!("{EXT} is not an object")))?,
        ),
    };
    let data: Vec<u8> = if let Some(ext) = ext {
        if usize_field(ext, "buffer")?.unwrap_or(0) != 0 {
            return Err(GlbError::Decode(
                "compressed data outside the GLB BIN".into(),
            ));
        }
        let mode = Mode::parse(ext.get("mode").and_then(Value::as_str).unwrap_or(""))
            .map_err(|e| GlbError::Decode(format!("mode: {e:?}")))?;
        let filter = Filter::parse(ext.get("filter").and_then(Value::as_str).unwrap_or("NONE"))
            .map_err(|e| GlbError::Decode(format!("filter: {e:?}")))?;
        let layout = BufferView::new(
            mode,
            filter,
            required(ext, "count")?,
            required(ext, "byteStride")?,
        )
        .map_err(|e| GlbError::Decode(format!("layout: {e:?}")))?;
        layout
            .validate_parent(
                required(view, "byteLength")?,
                usize_field(view, "byteStride")?,
            )
            .map_err(|e| GlbError::Decode(format!("parent view: {e:?}")))?;
        check_cap(out_len, layout.byte_length())?;
        let src = slice(
            bin,
            usize_field(ext, "byteOffset")?.unwrap_or(0),
            required(ext, "byteLength")?,
            "compressed view",
        )?;
        let decoded = layout
            .decode(src, workspace)
            .map_err(|e| GlbError::Decode(format!("{e:?}")))?;
        let exts = view.get_mut("extensions").and_then(Value::as_object_mut);
        if exts.is_some_and(|o| {
            o.remove(EXT);
            o.is_empty()
        }) {
            view.remove("extensions");
        }
        decoded
    } else {
        if usize_field(view, "buffer")?.unwrap_or(0) != 0 {
            return Err(GlbError::Decode("plain view outside the GLB BIN".into()));
        }
        let len = required(view, "byteLength")?;
        check_cap(out_len, len)?;
        slice(
            bin,
            usize_field(view, "byteOffset")?.unwrap_or(0),
            len,
            "view",
        )?
        .to_vec()
    };
    Ok(data)
}

fn check_cap(so_far: usize, add: usize) -> Result<(), GlbError> {
    if add > MAX_DECODED || so_far.saturating_add(add) > MAX_DECODED {
        return Err(GlbError::Decode(format!(
            "decoded size exceeds the {} MiB cap",
            MAX_DECODED >> 20
        )));
    }
    Ok(())
}

fn write_glb(json: &Value, bin: &[u8]) -> Result<Vec<u8>, GlbError> {
    let mut j = serde_json::to_vec(json).map_err(|e| GlbError::BadJson(e.to_string()))?;
    while !j.len().is_multiple_of(4) {
        j.push(b' ');
    }
    let mut b = bin.to_vec();
    while !b.len().is_multiple_of(4) {
        b.push(0);
    }
    let total = 12 + 8 + j.len() + 8 + b.len();
    let total32 = u32::try_from(total).map_err(|_| GlbError::Decode("GLB over 4 GiB".into()))?;
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&total32.to_le_bytes());
    out.extend_from_slice(&(j.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&j);
    out.extend_from_slice(&(b.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&b);
    Ok(out)
}

/// One LOD's render primitive, as plain vertex streams.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LodMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub tangents: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

impl LodMesh {
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }

    /// The same mesh with every triangle's winding reversed. A mirrored piece is drawn with a
    /// negative x scale, which flips its screen-space winding, and Bevy culls by winding alone
    /// (the determinant sign only corrects tangents), so the mirror uses this mesh to stay
    /// outside-out in every pass.
    pub fn reversed_winding(&self) -> Self {
        let mut m = self.clone();
        for t in m.indices.as_chunks_mut::<3>().0 {
            t.swap(1, 2);
        }
        m
    }

    /// Append `other`'s triangles.
    pub fn append(&mut self, other: &Self) {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&other.positions);
        self.normals.extend_from_slice(&other.normals);
        self.uvs.extend_from_slice(&other.uvs);
        self.tangents.extend_from_slice(&other.tangents);
        self.indices.extend(other.indices.iter().map(|i| i + base));
    }

    /// The mesh under the rigid transform `m` (normals and tangent directions rotate, the tangent
    /// handedness `w` is kept).
    pub fn transformed(&self, m: &Mat4) -> Self {
        let dir = |v: [f32; 3]| {
            m.transform_vector3(Vec3::from_array(v))
                .normalize()
                .to_array()
        };
        Self {
            positions: self
                .positions
                .iter()
                .map(|&p| m.transform_point3(Vec3::from_array(p)).to_array())
                .collect(),
            normals: self.normals.iter().map(|&n| dir(n)).collect(),
            uvs: self.uvs.clone(),
            tangents: self
                .tangents
                .iter()
                .map(|t| {
                    let [x, y, z] = dir([t[0], t[1], t[2]]);
                    [x, y, z, t[3]]
                })
                .collect(),
            indices: self.indices.clone(),
        }
    }
}

/// Number of render LODs per piece (kit contract §12).
pub const LODS: usize = 3;

/// `<id>_lod0`..`<id>_lod2` of a kit GLB, decoded, with every animated part's
/// `anim_<name>_lod<n>` mesh merged into LOD `n` at its rest pose (the `anim_<name>` node's
/// transform, which kit-check holds at the sidecar pivot and rest angle).
pub fn decode_lods(bytes: &[u8], id: &str) -> Result<[LodMesh; LODS], GlbError> {
    Ok(decode_lods_with_parts(bytes, id)?.map(|(body, _)| body))
}

/// [`decode_lods`], and per LOD the animated part names merged into it, in node order.
pub fn decode_lods_with_parts(
    bytes: &[u8],
    id: &str,
) -> Result<[(LodMesh, Vec<String>); LODS], GlbError> {
    Ok(decode_split(bytes, id)?.map(|lod| {
        let mut merged = lod.body;
        let mut names = Vec::with_capacity(lod.parts.len());
        for p in lod.parts {
            merged.append(&p.mesh.transformed(&p.rest));
            names.push(p.name);
        }
        (merged, names)
    }))
}

/// One animated part's mesh of one LOD, in the part frame (pivot at the origin, 0 mdeg), and its
/// rest-pose transform into the piece frame (the `anim_<name>` node's).
#[derive(Debug, Clone, PartialEq)]
pub struct PartMesh {
    /// `anim_<name>`, the sidecar part name.
    pub name: String,
    pub mesh: LodMesh,
    pub rest: Mat4,
}

/// One LOD split into the static body and its animated parts, in node order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SplitLod {
    pub body: LodMesh,
    pub parts: Vec<PartMesh>,
}

/// `<id>_lod0`..`<id>_lod2` of a kit GLB, decoded without the animated parts, and each LOD's
/// `anim_<name>_lod<n>` meshes on their own, for drawing the parts at sim angles.
pub fn decode_split(bytes: &[u8], id: &str) -> Result<[SplitLod; LODS], GlbError> {
    let plain = rewrite_glb(bytes)?;
    let json_len = u32_at(&plain, 12)? as usize;
    let json: Value = serde_json::from_slice(slice(&plain, 20, json_len, "JSON chunk")?)
        .map_err(|e| GlbError::BadJson(e.to_string()))?;
    let bin_off = 20 + json_len;
    let bin_len = u32_at(&plain, bin_off)? as usize;
    let bin = slice(&plain, bin_off + 8, bin_len, "BIN chunk")?;
    let nodes = json["nodes"]
        .as_array()
        .ok_or_else(|| GlbError::BadJson("no nodes".into()))?;
    let mut out: [SplitLod; LODS] = Default::default();
    for (lod, slot) in out.iter_mut().enumerate() {
        let name = format!("{id}_lod{lod}");
        let body = nodes
            .iter()
            .find(|n| n["name"] == name.as_str())
            .ok_or_else(|| GlbError::BadJson(format!("no node {name}")))?;
        slot.body = node_mesh(&json, bin, body, &name)?;
        let suffix = format!("_lod{lod}");
        for (i, n) in nodes.iter().enumerate() {
            let Some(part) = n["name"]
                .as_str()
                .filter(|s| s.starts_with("anim_"))
                .and_then(|s| s.strip_suffix(suffix.as_str()))
            else {
                continue;
            };
            slot.parts.push(PartMesh {
                name: part.to_owned(),
                mesh: node_mesh(&json, bin, n, &format!("{part}{suffix}"))?,
                rest: world_matrix(nodes, i)?,
            });
        }
    }
    Ok(out)
}

/// The mesh of `node`'s single primitive, untransformed.
fn node_mesh(json: &Value, bin: &[u8], node: &Value, name: &str) -> Result<LodMesh, GlbError> {
    let mesh = node["mesh"]
        .as_u64()
        .ok_or_else(|| GlbError::BadJson(format!("{name} has no mesh")))?;
    let prim = &json["meshes"][mesh as usize]["primitives"][0];
    let attr = |key: &str| {
        prim["attributes"][key]
            .as_u64()
            .ok_or_else(|| GlbError::BadJson(format!("{name} has no {key}")))
    };
    let index = prim["indices"]
        .as_u64()
        .ok_or_else(|| GlbError::BadJson(format!("{name} has no indices")))?;
    let m = LodMesh {
        positions: floats::<3>(json, bin, attr("POSITION")?)?,
        normals: floats::<3>(json, bin, attr("NORMAL")?)?,
        uvs: floats::<2>(json, bin, attr("TEXCOORD_0")?)?,
        tangents: floats::<4>(json, bin, attr("TANGENT")?)?,
        indices: indices(json, bin, index)?,
    };
    let n = m.positions.len();
    let streams_ok = m.normals.len() == n && m.uvs.len() == n && m.tangents.len() == n;
    if !streams_ok || m.indices.iter().any(|&i| i as usize >= n) {
        return Err(GlbError::Decode(format!("{name}: inconsistent streams")));
    }
    Ok(m)
}

/// A node's local TRS (`matrix` nodes are refused: the kit exporter writes TRS only).
fn local_matrix(node: &Value) -> Result<Mat4, GlbError> {
    if node.get("matrix").is_some() {
        return Err(GlbError::BadJson("matrix nodes are not supported".into()));
    }
    let v = |key: &str, default: &[f32]| -> Vec<f32> {
        node[key].as_array().map_or_else(
            || default.to_vec(),
            |a| a.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect(),
        )
    };
    let (t, r, s) = (
        v("translation", &[0.0; 3]),
        v("rotation", &[0.0, 0.0, 0.0, 1.0]),
        v("scale", &[1.0; 3]),
    );
    if t.len() != 3 || r.len() != 4 || s.len() != 3 {
        return Err(GlbError::BadJson("bad node TRS".into()));
    }
    Ok(Mat4::from_scale_rotation_translation(
        Vec3::new(s[0], s[1], s[2]),
        Quat::from_xyzw(r[0], r[1], r[2], r[3]).normalize(),
        Vec3::new(t[0], t[1], t[2]),
    ))
}

/// Node `index`'s transform in the scene: its TRS composed with every ancestor's.
fn world_matrix(nodes: &[Value], index: usize) -> Result<Mat4, GlbError> {
    let parent = |i: usize| {
        nodes.iter().position(|n| {
            n["children"]
                .as_array()
                .is_some_and(|c| c.iter().any(|c| c.as_u64() == Some(i as u64)))
        })
    };
    let mut m = local_matrix(&nodes[index])?;
    let mut at = index;
    for _ in 0..nodes.len() {
        let Some(p) = parent(at) else {
            return Ok(m);
        };
        m = local_matrix(&nodes[p])? * m;
        at = p;
    }
    Err(GlbError::BadJson("node hierarchy has a cycle".into()))
}

/// The accessor's bytes, element count, stride and componentType.
fn accessor<'a>(
    json: &Value,
    bin: &'a [u8],
    index: u64,
) -> Result<(&'a [u8], usize, usize, u64), GlbError> {
    let bad = |what: &str| GlbError::BadJson(format!("accessor {index}: {what}"));
    let a = &json["accessors"][index as usize];
    let count = a["count"].as_u64().ok_or_else(|| bad("count"))? as usize;
    let ctype = a["componentType"]
        .as_u64()
        .ok_or_else(|| bad("componentType"))?;
    let comps = match a["type"].as_str() {
        Some("SCALAR") => 1,
        Some("VEC2") => 2,
        Some("VEC3") => 3,
        Some("VEC4") => 4,
        _ => return Err(bad("type")),
    };
    let size = match ctype {
        5123 => 2,
        5125 | 5126 => 4,
        _ => return Err(bad("componentType")),
    };
    let view_index = a["bufferView"].as_u64().ok_or_else(|| bad("bufferView"))?;
    let view = &json["bufferViews"][view_index as usize];
    let elem = comps * size;
    let stride = view["byteStride"].as_u64().map_or(elem, |s| s as usize);
    let start = view["byteOffset"].as_u64().unwrap_or(0) as usize
        + a["byteOffset"].as_u64().unwrap_or(0) as usize;
    let len = if count == 0 {
        0
    } else {
        (count - 1) * stride + elem
    };
    Ok((slice(bin, start, len, "accessor")?, count, stride, ctype))
}

fn floats<const N: usize>(json: &Value, bin: &[u8], index: u64) -> Result<Vec<[f32; N]>, GlbError> {
    let (bytes, count, stride, ctype) = accessor(json, bin, index)?;
    if ctype != 5126 {
        return Err(GlbError::Decode(format!("accessor {index} is not float")));
    }
    Ok((0..count)
        .map(|i| {
            std::array::from_fn(|k| {
                let o = i * stride + k * 4;
                f32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]])
            })
        })
        .collect())
}

fn indices(json: &Value, bin: &[u8], index: u64) -> Result<Vec<u32>, GlbError> {
    let (bytes, count, stride, ctype) = accessor(json, bin, index)?;
    Ok((0..count)
        .map(|i| {
            let o = i * stride;
            match ctype {
                5123 => u32::from(u16::from_le_bytes([bytes[o], bytes[o + 1]])),
                _ => u32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]),
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each mechanism LOD is its body followed by every `anim_<name>_lod<n>` mesh, in node order,
    /// placed exactly at the sidecar's rest pose: rotated `rest_mdeg` about `axis`, then moved to
    /// `pivot_um`.
    #[test]
    fn animated_parts_merge_at_the_sidecar_rest_pose() {
        let kit = parble_kit::builtin();
        let mut checked = 0;
        for (id, bytes) in super::super::PIECES {
            let piece = kit.iter().find(|p| p.def.id.as_str() == *id).unwrap();
            if piece.animated_parts.is_empty() {
                continue;
            }
            let plain = rewrite_glb(bytes).unwrap();
            let len = u32_at(&plain, 12).unwrap() as usize;
            let json: Value = serde_json::from_slice(&plain[20..20 + len]).unwrap();
            let bin_len = u32_at(&plain, 20 + len).unwrap() as usize;
            let bin = &plain[28 + len..28 + len + bin_len];
            let nodes = json["nodes"].as_array().unwrap();
            let node = |name: &str| nodes.iter().find(|n| n["name"] == name).unwrap();
            let merged = decode_lods(bytes, id).unwrap();
            for (l, m) in merged.iter().enumerate() {
                let body = node_mesh(&json, bin, node(&format!("{id}_lod{l}")), id).unwrap();
                assert_eq!(&m.positions[..body.positions.len()], &body.positions[..]);
                let mut at = body.positions.len();
                for part in &piece.animated_parts {
                    let name = format!("{}_lod{l}", part.name);
                    let raw = node_mesh(&json, bin, node(&name), &name).unwrap();
                    let axis = Vec3::from_array(part.axis.map(|v| v as f32));
                    // A slide part's node sits rest µm along its axis from the pivot, unrotated.
                    let slide = part.joint == parble_kit::Joint::Slide;
                    let (rest, pivot) = if slide {
                        let shift = axis * part.rest_mdeg as f32 * 1e-6;
                        (
                            Quat::IDENTITY,
                            Vec3::from_array(part.pivot_um.map(|v| v as f32 * 1e-6)) + shift,
                        )
                    } else {
                        (
                            Quat::from_axis_angle(
                                axis,
                                (part.rest_mdeg as f32 * 1e-3).to_radians(),
                            ),
                            Vec3::from_array(part.pivot_um.map(|v| v as f32 * 1e-6)),
                        )
                    };
                    let mut moved = 0.0f32;
                    for (k, p) in raw.positions.iter().enumerate() {
                        let local = Vec3::from_array(*p);
                        let want = pivot + rest * local;
                        let got = Vec3::from_array(m.positions[at + k]);
                        assert!(
                            got.distance(want) < 1e-5,
                            "{name} vertex {k}: {got} vs {want}"
                        );
                        moved = moved.max(got.distance(local));
                    }
                    // The part frame is not the piece frame: the pose moved every hinge part
                    // (a slide from the piece origin at rest 0 keeps the piece frame).
                    assert!(
                        moved > 1e-3 || (slide && pivot == Vec3::ZERO),
                        "{name} was not moved off its local frame"
                    );
                    at += raw.positions.len();
                }
                assert_eq!(at, m.positions.len(), "{id} lod{l}: only body and parts");
            }
            checked += 1;
        }
        assert_eq!(checked, 7, "mechanisms with parts");
    }
}

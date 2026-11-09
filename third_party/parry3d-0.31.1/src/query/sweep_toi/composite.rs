//! Sweep time-of-impact against composite shapes (meshes, polylines, heightfields,
//! compounds): the composite is assumed stationary, its acceleration structure is queried
//! with the swept bounds of the moving shape, and each candidate element runs the convex
//! TOI with a fallback sphere when the element reports an initial overlap.

use super::sweep::Sweep;
use super::sweep_toi::{sweep_time_of_impact, SweepToiOutput, SweepToiStatus};
use super::toi_proxy::ToiProxy;
use crate::bounding_volume::BoundingVolume;
use crate::math::{Pose, Real, Vector};
use crate::shape::{Shape, TypedShape};

#[cfg(feature = "dim2")]
use crate::shape::{PolylineFlags, Segment};
#[cfg(feature = "dim3")]
use crate::shape::{TriMeshFlags, Triangle};

/// Fraction of the fast shape’s minimum extent used for the initial-overlap fallback sphere.
pub const CORE_FRACTION: Real = 0.25;

/// Parameters describing the fast (moving) shape for a composite TOI query.
#[derive(Copy, Clone)]
pub struct SweepCompositeFastShape<'a, 'b> {
    /// Point-cloud proxy of the moving shape.
    pub proxy: &'a ToiProxy<'b>,
    /// Sweep of the moving shape.
    pub sweep: &'a Sweep,
    /// Centroid of the moving shape in its local frame.
    pub local_centroid: Vector,
    /// Smallest extent (inner radius) of the moving shape, used for fallback spheres and
    /// one-sided early-outs.
    pub min_extent: Real,
    /// parble patch (`README.parble.md`): triangles of a top-level triangle-mesh target to
    /// leave out of the sweep, by triangle index. `None` sweeps every triangle.
    pub skip_triangle: Option<&'a dyn Fn(u32) -> bool>,
}

struct CompositeToiContext<'a, 'b> {
    fast: SweepCompositeFastShape<'a, 'b>,
    // Centroid of the moving shape at the sweep endpoints, in the composite’s local frame.
    local_centroid1: Vector,
    local_centroid2: Vector,
    fallback_radius: Real,
    one_sided: bool,
    #[cfg_attr(feature = "dim2", allow(dead_code))]
    target_is_sensor: bool,
    linear_slop: Real,
    max_fraction: Real,
    best: Option<SweepToiOutput>,
}

impl CompositeToiContext<'_, '_> {
    /// Runs the convex TOI of the moving shape against one composite element and keeps the
    /// earliest hit, with a fallback-sphere retry on initial overlap.
    fn toi_against_element(&mut self, element_proxy: &ToiProxy, element_sweep: &Sweep) {
        let output = sweep_time_of_impact(
            element_proxy,
            element_sweep,
            self.fast.proxy,
            self.fast.sweep,
            self.max_fraction,
            self.linear_slop,
        );

        if 0.0 < output.fraction && output.fraction < self.max_fraction {
            self.max_fraction = output.fraction;
            self.best = Some(output);
        } else if output.fraction == 0.0 {
            // Fallback to the TOI of a small ball around the fast shape centroid.
            #[cfg(feature = "dim2")]
            let radius = self.fallback_radius;
            #[cfg(feature = "dim3")]
            let radius = self.fallback_radius + self.linear_slop;

            let fallback_proxy = ToiProxy::point(self.fast.local_centroid, radius);
            let output = sweep_time_of_impact(
                element_proxy,
                element_sweep,
                &fallback_proxy,
                self.fast.sweep,
                self.max_fraction,
                self.linear_slop,
            );

            if 0.0 < output.fraction && output.fraction < self.max_fraction {
                self.max_fraction = output.fraction;
                self.best = Some(output);
            }
        }
    }

    /// One-sided early-out for a 2D chain segment.
    /// Returns `true` when the element can be skipped.
    #[cfg(feature = "dim2")]
    fn one_sided_early_out(&self, segment: &Segment) -> bool {
        if !self.one_sided {
            return false;
        }

        let e = segment.b - segment.a;
        let length = e.length();
        if length <= self.linear_slop {
            return false;
        }
        let e = e / length;

        let separation1 = (self.local_centroid1 - segment.a).perp_dot(e);
        let separation2 = (self.local_centroid2 - segment.a).perp_dot(e);
        let core_distance = CORE_FRACTION * self.fast.min_extent;

        separation1 < 0.0
            || (separation1 - separation2 < core_distance && separation2 > core_distance)
    }

    /// One-sided early-out for a 3D triangle.
    /// Returns `true` when the element can be skipped.
    #[cfg(feature = "dim3")]
    fn one_sided_early_out(&self, triangle: &Triangle) -> bool {
        if !self.one_sided {
            return false;
        }

        let n = (triangle.b - triangle.a)
            .cross(triangle.c - triangle.a)
            .normalize_or_zero();
        let offset1 = n.dot(self.local_centroid1 - triangle.a);
        let offset2 = n.dot(self.local_centroid2 - triangle.a);

        if offset1 < 0.0 {
            // Started behind.
            return true;
        }

        if !self.target_is_sensor
            && offset1 - offset2 < self.fallback_radius
            && offset2 > self.fallback_radius
        {
            // Finished in front.
            return true;
        }

        false
    }
}

/// Computes the time of impact between a stationary composite shape and a moving convex
/// shape.
///
/// Returns `None` if `composite` is not a supported composite shape (triangle mesh,
/// polyline, heightfield, or compound). When supported but nothing is hit, the returned
/// output has status [`SweepToiStatus::Separated`] and `fraction == max_fraction`.
///
/// `one_sided` enables the “started behind / finished in front” early-outs; it
/// should only be set for composites whose elements have meaningful outward normals
/// (oriented polylines, oriented meshes, heightfields).
#[allow(clippy::too_many_arguments)]
pub fn sweep_time_of_impact_composite(
    composite: &dyn Shape,
    composite_pose: &Pose,
    fast: SweepCompositeFastShape,
    one_sided: bool,
    target_is_sensor: bool,
    max_fraction: Real,
    linear_slop: Real,
) -> Option<SweepToiOutput> {
    let typed = composite.as_typed_shape();

    // Fallback sphere radius (2D: core circle; 3D: mesh/compound fallback spheres).
    #[cfg(feature = "dim2")]
    let fallback_radius = CORE_FRACTION * fast.min_extent;
    #[cfg(feature = "dim3")]
    let fallback_radius = match typed {
        TypedShape::Compound(_) => (0.75 * fast.min_extent).max(4.0 * linear_slop),
        _ => (0.5 * fast.min_extent).max(linear_slop),
    };

    // Swept bounds of the fast shape, in the composite’s local frame.
    let start_aabb = fast.proxy.compute_aabb(&fast.sweep.transform_at(0.0));
    let end_aabb = fast
        .proxy
        .compute_aabb(&fast.sweep.transform_at(max_fraction));
    let local_aabb = start_aabb
        .merged(&end_aabb)
        .transform_by(&composite_pose.inverse());

    // Centroid of the fast shape at the sweep endpoints, in the composite’s local frame.
    let centroid_world1 = fast
        .sweep
        .transform_at(0.0)
        .transform_point(fast.local_centroid);
    let centroid_world2 = fast
        .sweep
        .final_transform()
        .transform_point(fast.local_centroid);

    let mut context = CompositeToiContext {
        fast,
        local_centroid1: composite_pose.inverse_transform_point(centroid_world1),
        local_centroid2: composite_pose.inverse_transform_point(centroid_world2),
        fallback_radius,
        one_sided,
        target_is_sensor,
        linear_slop,
        max_fraction,
        best: None,
    };

    // The composite is stationary: every element sweep is degenerate at the composite pose
    // (composed with the child pose for compounds).
    let composite_sweep = Sweep::constant(composite_pose, Vector::ZERO);

    match typed {
        #[cfg(feature = "dim3")]
        TypedShape::TriMesh(mesh) => {
            let one_sided_mesh = mesh.flags().contains(TriMeshFlags::ORIENTED);
            context.one_sided = one_sided || one_sided_mesh;
            let skip = context.fast.skip_triangle;
            for tri_id in mesh.bvh().intersect_aabb(&local_aabb) {
                if skip.is_some_and(|skip| skip(tri_id)) {
                    continue;
                }
                let triangle = mesh.triangle(tri_id);
                if context.one_sided_early_out(&triangle) {
                    continue;
                }
                let proxy = ToiProxy::from_array([triangle.a, triangle.b, triangle.c], 0.0);
                context.toi_against_element(&proxy, &composite_sweep);
            }
        }
        #[cfg(feature = "dim2")]
        TypedShape::TriMesh(mesh) => {
            for tri_id in mesh.bvh().intersect_aabb(&local_aabb) {
                let triangle = mesh.triangle(tri_id);
                let proxy = ToiProxy::from_array([triangle.a, triangle.b, triangle.c], 0.0);
                context.toi_against_element(&proxy, &composite_sweep);
            }
        }
        TypedShape::Polyline(polyline) => {
            #[cfg(feature = "dim2")]
            {
                let oriented = polyline.flags().contains(PolylineFlags::ORIENTED);
                context.one_sided = one_sided || oriented;
            }
            for seg_id in polyline.bvh().intersect_aabb(&local_aabb) {
                let segment = polyline.segment(seg_id);
                #[cfg(feature = "dim2")]
                if context.one_sided_early_out(&segment) {
                    continue;
                }
                let proxy = ToiProxy::from_array([segment.a, segment.b], 0.0);
                context.toi_against_element(&proxy, &composite_sweep);
            }
        }
        TypedShape::HeightField(heightfield) => {
            #[cfg(feature = "dim2")]
            heightfield.map_elements_in_local_aabb(&local_aabb, &mut |_, segment| {
                if !context.one_sided_early_out(segment) {
                    let proxy = ToiProxy::from_array([segment.a, segment.b], 0.0);
                    context.toi_against_element(&proxy, &composite_sweep);
                }
            });
            #[cfg(feature = "dim3")]
            heightfield.map_elements_in_local_aabb(&local_aabb, &mut |_, triangle| {
                if !context.one_sided_early_out(triangle) {
                    let proxy = ToiProxy::from_array([triangle.a, triangle.b, triangle.c], 0.0);
                    context.toi_against_element(&proxy, &composite_sweep);
                }
            });
        }
        TypedShape::Compound(compound) => {
            for child_id in compound.bvh().intersect_aabb(&local_aabb) {
                let (child_pose, child_shape) = &compound.shapes()[child_id as usize];
                let child_world_pose = *composite_pose * *child_pose;
                if let Some(child_proxy) = ToiProxy::from_shape(child_shape.as_ref()) {
                    let child_sweep = Sweep::constant(&child_world_pose, Vector::ZERO);
                    context.toi_against_element(&child_proxy, &child_sweep);
                } else if let Some(hit) = sweep_time_of_impact_composite(
                    child_shape.as_ref(),
                    &child_world_pose,
                    SweepCompositeFastShape {
                        skip_triangle: None,
                        ..context.fast
                    },
                    context.one_sided,
                    target_is_sensor,
                    context.max_fraction,
                    linear_slop,
                ) {
                    if 0.0 < hit.fraction && hit.fraction < context.max_fraction {
                        context.max_fraction = hit.fraction;
                        context.best = Some(hit);
                    }
                }
                // Children that are neither point-cloud shapes nor composites are skipped.
            }
        }
        _ => return None,
    }

    Some(context.best.unwrap_or(SweepToiOutput {
        status: SweepToiStatus::Separated,
        fraction: max_fraction,
        point: Vector::ZERO,
        normal: Vector::ZERO,
    }))
}

#[cfg(all(test, feature = "dim3"))]
mod parble_tests {
    use super::*;
    use crate::math::Rotation;
    use crate::shape::TriMesh;

    fn at(y: Real) -> Pose {
        Pose::from_parts(Vector::new(0.0, y, 0.0), Rotation::IDENTITY)
    }

    /// A ball of radius 0.1 falls from y = 1 to y = -2 through two horizontal triangles, index 0
    /// at y = 0 and index 1 at y = -1.
    fn fall(skip_triangle: Option<&dyn Fn(u32) -> bool>) -> SweepToiOutput {
        let tri = |y: Real| {
            [
                Vector::new(-1.0, y, -1.0),
                Vector::new(-1.0, y, 1.0),
                Vector::new(1.0, y, 0.0),
            ]
        };
        let [a, b, c] = tri(0.0);
        let [d, e, f] = tri(-1.0);
        let mesh = TriMesh::new(vec![a, b, c, d, e, f], vec![[0, 1, 2], [3, 4, 5]]).unwrap();
        let proxy = ToiProxy::point(Vector::ZERO, 0.1);
        let sweep = Sweep::from_poses(&at(1.0), &at(-2.0), Vector::ZERO);
        let fast = SweepCompositeFastShape {
            proxy: &proxy,
            sweep: &sweep,
            local_centroid: Vector::ZERO,
            min_extent: 0.1,
            skip_triangle,
        };
        sweep_time_of_impact_composite(&mesh, &Pose::IDENTITY, fast, false, false, 1.0, 0.001)
            .unwrap()
    }

    #[test]
    fn skipped_triangles_are_not_swept() {
        let all = fall(None);
        assert_eq!(all.status, SweepToiStatus::Hit);
        assert!((all.fraction - 0.9 / 3.0).abs() < 0.01, "{}", all.fraction);

        let first = |t: u32| t == 0;
        let lower = fall(Some(&first));
        assert_eq!(lower.status, SweepToiStatus::Hit);
        assert!(
            (lower.fraction - 1.9 / 3.0).abs() < 0.01,
            "{}",
            lower.fraction
        );

        let every = |_: u32| true;
        let none = fall(Some(&every));
        assert_eq!(none.status, SweepToiStatus::Separated);
        assert_eq!(none.fraction, 1.0);
    }
}

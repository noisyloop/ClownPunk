//! Collision and raycasting. No physics library: everything here is a handful
//! of small geometric tests.
//!
//! * Moving things (player, zombies, cars) are approximated by CIRCLES on the
//!   ground plane.
//! * Static things (walls, buildings, parked cars, barriers) are ORIENTED
//!   BOXES (`Obb`) on the ground plane with a height.
//! * Circle-vs-box: move the circle into the box's local frame, clamp to the
//!   box to find the closest point, push the circle out along that direction.
//! * Rays (bullets) use the classic "slab" test against boxes and a quadratic
//!   solve against spheres.
use crate::math::{Vec2, Vec3};

/// Oriented box on the ground plane. `yaw = 0` makes it an axis-aligned box.
#[derive(Clone, Copy, Debug)]
pub struct Obb {
    pub center: Vec2,
    /// Half extents along the box's local forward (x) and right (y) axes.
    pub half: Vec2,
    pub yaw: f32,
    /// Height from the ground; used by ray tests only.
    pub height: f32,
}

impl Obb {
    pub fn new(center: Vec2, half: Vec2, yaw: f32, height: f32) -> Obb {
        Obb { center, half, yaw, height }
    }
    pub fn aabb(cx: f32, cz: f32, hx: f32, hz: f32, height: f32) -> Obb {
        Obb::new(Vec2::new(cx, cz), Vec2::new(hx, hz), 0.0, height)
    }

    /// World point -> box-local point (x along box forward, y along box right).
    fn to_local(&self, p: Vec2) -> Vec2 {
        let d = p - self.center;
        let (s, c) = self.yaw.sin_cos();
        Vec2::new(d.x * c + d.y * s, -d.x * s + d.y * c)
    }

    /// Box-local direction -> world direction (rotation only).
    fn dir_to_world(&self, l: Vec2) -> Vec2 {
        let (s, c) = self.yaw.sin_cos();
        Vec2::new(l.x * c - l.y * s, l.x * s + l.y * c)
    }

    /// If a circle overlaps this box, returns the vector to ADD to the circle
    /// centre to separate them (its direction is the contact normal).
    pub fn push_out(&self, center: Vec2, radius: f32) -> Option<Vec2> {
        // Cheap reject using the bounding circle of the box.
        let reach = self.half.len() + radius;
        if (center - self.center).len_sq() > reach * reach {
            return None;
        }
        let l = self.to_local(center);
        let closest = Vec2::new(l.x.clamp(-self.half.x, self.half.x), l.y.clamp(-self.half.y, self.half.y));
        let d = l - closest;
        let dist2 = d.len_sq();
        if dist2 >= radius * radius {
            return None;
        }
        let push_local = if dist2 > 1e-8 {
            // Centre outside the box: push along the line from the closest point.
            let dist = dist2.sqrt();
            d * ((radius - dist) / dist)
        } else {
            // Centre inside the box: escape through the nearest face.
            let px = self.half.x - l.x.abs();
            let pz = self.half.y - l.y.abs();
            if px < pz {
                Vec2::new(l.x.signum() * (px + radius), 0.0)
            } else {
                Vec2::new(0.0, l.y.signum() * (pz + radius))
            }
        };
        Some(self.dir_to_world(push_local))
    }

    /// Distance along a ray to the box surface (slab test), if it hits within `max_t`.
    pub fn ray(&self, origin: Vec3, dir: Vec3, max_t: f32) -> Option<f32> {
        let o = self.to_local(origin.xz());
        let (s, c) = self.yaw.sin_cos();
        let d = Vec2::new(dir.x * c + dir.z * s, -dir.x * s + dir.z * c);
        let mut t_min = 0.0f32;
        let mut t_max = max_t;
        // (origin, direction, lower bound, upper bound) per axis
        let axes = [
            (o.x, d.x, -self.half.x, self.half.x),
            (origin.y, dir.y, 0.0, self.height),
            (o.y, d.y, -self.half.y, self.half.y),
        ];
        for (po, pd, lo, hi) in axes {
            if pd.abs() < 1e-8 {
                if po < lo || po > hi {
                    return None;
                }
            } else {
                let mut t1 = (lo - po) / pd;
                let mut t2 = (hi - po) / pd;
                if t1 > t2 {
                    std::mem::swap(&mut t1, &mut t2);
                }
                t_min = t_min.max(t1);
                t_max = t_max.min(t2);
                if t_min > t_max {
                    return None;
                }
            }
        }
        Some(t_min)
    }
}

/// Ray (unit `dir`) vs sphere: distance to first intersection.
pub fn ray_sphere(origin: Vec3, dir: Vec3, center: Vec3, radius: f32) -> Option<f32> {
    let oc = origin - center;
    let b = oc.dot(dir);
    let c = oc.dot(oc) - radius * radius;
    let disc = b * b - c;
    if disc < 0.0 {
        return None;
    }
    let t = -b - disc.sqrt();
    if t >= 0.0 {
        Some(t)
    } else if -b + disc.sqrt() >= 0.0 {
        Some(0.0)
    } else {
        None
    }
}

/// Push a circle out of every box in `boxes`. Returns the summed contact
/// normals (zero vector when nothing was touched).
pub fn resolve_circle(pos: &mut Vec2, radius: f32, boxes: &[Obb]) -> Vec2 {
    let mut normal_sum = Vec2::ZERO;
    for b in boxes {
        if let Some(push) = b.push_out(*pos, radius) {
            *pos += push;
            normal_sum += push.norm();
        }
    }
    normal_sum
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circle_is_pushed_out_of_box() {
        let b = Obb::aabb(0.0, 0.0, 1.0, 1.0, 2.0);
        let mut p = Vec2::new(1.2, 0.0);
        resolve_circle(&mut p, 0.5, &[b]);
        assert!((p.x - 1.5).abs() < 1e-4 && p.y.abs() < 1e-4, "{p:?}");
        // Far away circles are untouched.
        let mut q = Vec2::new(5.0, 5.0);
        assert!(resolve_circle(&mut q, 0.5, &[b]).len() == 0.0);
    }

    #[test]
    fn rotated_box_collision_uses_its_own_axes() {
        // A long thin box rotated 90 degrees now extends along world Z.
        let b = Obb::new(Vec2::ZERO, Vec2::new(5.0, 0.5), std::f32::consts::FRAC_PI_2, 1.0);
        assert!(b.push_out(Vec2::new(0.0, 4.0), 0.4).is_some());
        assert!(b.push_out(Vec2::new(4.0, 0.0), 0.4).is_none());
    }

    #[test]
    fn ray_hits_box_at_expected_distance_and_respects_height() {
        let b = Obb::aabb(10.0, 0.0, 1.0, 1.0, 2.0);
        let t = b.ray(Vec3::new(0.0, 1.0, 0.0), Vec3::new(1.0, 0.0, 0.0), 100.0).unwrap();
        assert!((t - 9.0).abs() < 1e-4);
        // Fired above the box: miss.
        assert!(b.ray(Vec3::new(0.0, 3.0, 0.0), Vec3::new(1.0, 0.0, 0.0), 100.0).is_none());
    }

    #[test]
    fn ray_sphere_basic() {
        let t = ray_sphere(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), Vec3::new(5.0, 0.0, 0.0), 1.0).unwrap();
        assert!((t - 4.0).abs() < 1e-4);
        assert!(ray_sphere(Vec3::ZERO, Vec3::new(0.0, 1.0, 0.0), Vec3::new(5.0, 0.0, 0.0), 1.0).is_none());
    }
}

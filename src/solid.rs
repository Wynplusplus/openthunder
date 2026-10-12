//! Simple boolean solid modelling.
//!
//! A model is a list of **primitive shapes** (box, sphere, cylinder, cone,
//! capsule) combined with **boolean operations** (union / subtract / intersect).
//! It is evaluated as a signed-distance field and meshed with **surface nets**,
//! so subtracting a sphere from a box leaves a smooth dent rather than a blocky
//! hole.
//!
//! Everything here is dependency-free: vectors are `[f32; 3]` and the maths is
//! local. The meshes are handed to the game/editor, which turn them into Bevy
//! `Mesh`es.

use core::fmt::Write;

/// A vector helper — kept local so this module (and the lib) stays dependency-free.
type V3 = [f32; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn length(a: V3) -> f32 {
    dot(a, a).sqrt()
}
fn normalize(a: V3) -> V3 {
    let len = length(a);
    if len > 1e-6 {
        scale(a, 1.0 / len)
    } else {
        [0.0, 0.0, 0.0]
    }
}
fn max3(a: V3) -> f32 {
    a[0].max(a[1]).max(a[2])
}

/// A primitive shape. All are unit-sized (a box is 1×1×1, a sphere has
/// diameter 1); the shape's `scale` sets its real size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Primitive {
    #[default]
    Box,
    Sphere,
    Cylinder,
    Cone,
    Capsule,
}

impl Primitive {
    pub const ALL: [Primitive; 5] = [
        Primitive::Box,
        Primitive::Sphere,
        Primitive::Cylinder,
        Primitive::Cone,
        Primitive::Capsule,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Primitive::Box => "box",
            Primitive::Sphere => "sphere",
            Primitive::Cylinder => "cylinder",
            Primitive::Cone => "cone",
            Primitive::Capsule => "capsule",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Primitive::Box => "Box",
            Primitive::Sphere => "Sphere",
            Primitive::Cylinder => "Cylinder",
            Primitive::Cone => "Cone",
            Primitive::Capsule => "Capsule",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.id() == text.trim())
    }

    /// Signed distance to the unit primitive at `p` (local space).
    fn distance(self, p: V3) -> f32 {
        match self {
            Primitive::Box => {
                let q = [p[0].abs() - 0.5, p[1].abs() - 0.5, p[2].abs() - 0.5];
                let outside = [q[0].max(0.0), q[1].max(0.0), q[2].max(0.0)];
                length(outside) + max3(q).min(0.0)
            }
            Primitive::Sphere => length(p) - 0.5,
            Primitive::Cylinder => {
                let d = [length([p[0], 0.0, p[2]]) - 0.5, p[1].abs() - 0.5];
                d[0].max(d[1]).min(0.0) + length([d[0].max(0.0), d[1].max(0.0), 0.0])
            }
            Primitive::Cone => {
                // Apex at +y, base radius 0.5 at -y (a capped cone).
                let q: V3 = [0.5, -1.0, 0.0];
                let w: V3 = [length([p[0], 0.0, p[2]]), p[1], 0.0];
                let qd = dot(q, q).max(1e-6);
                let t = (dot(w, q) / qd).clamp(0.0, 1.0);
                let a = sub(w, scale(q, t));
                let b = sub(w, [q[0] * (w[0] / q[0]).clamp(0.0, 1.0), q[1], 0.0]);
                let k = q[1].signum();
                let d = dot(a, a).min(dot(b, b));
                let s = (k * (w[0] * q[1] - w[1] * q[0])).max(k * (w[1] - q[1]));
                d.sqrt() * s.signum()
            }
            Primitive::Capsule => {
                // A vertical segment from -0.25 to +0.25 with radius 0.5.
                let y = (p[1] - 0.25).min(0.0).max(p[1] + 0.25) - p[1];
                let q = [p[0], p[1] + y, p[2]];
                length(q) - 0.5
            }
        }
    }
}

/// How a shape combines with the ones before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BoolOp {
    #[default]
    Union,
    Subtract,
    Intersect,
}

impl BoolOp {
    pub const ALL: [BoolOp; 3] = [BoolOp::Union, BoolOp::Subtract, BoolOp::Intersect];

    pub fn id(self) -> &'static str {
        match self {
            BoolOp::Union => "union",
            BoolOp::Subtract => "subtract",
            BoolOp::Intersect => "intersect",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            BoolOp::Union => "Add",
            BoolOp::Subtract => "Subtract",
            BoolOp::Intersect => "Intersect",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|op| op.id() == text.trim())
    }
}

/// One primitive shape with a transform and a boolean operation.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub primitive: Primitive,
    pub op: BoolOp,
    pub position: V3,
    /// Euler rotation in degrees (XYZ).
    pub rotation: V3,
    /// Size in metres.
    pub scale: V3,
}

impl Shape {
    pub fn new(primitive: Primitive, position: V3, scale: V3) -> Self {
        Self {
            primitive,
            op: BoolOp::Union,
            position,
            rotation: [0.0, 0.0, 0.0],
            scale,
        }
    }

    /// Signed distance from a world point, in metres.
    fn distance(&self, p: V3) -> f32 {
        // Move into the shape's local frame: un-translate, un-rotate, un-scale.
        let q = sub(p, self.position);
        let q = rotate_inverse(q, self.rotation);
        let size = [
            self.scale[0].abs().max(1e-3),
            self.scale[1].abs().max(1e-3),
            self.scale[2].abs().max(1e-3),
        ];
        let local = [q[0] / size[0], q[1] / size[1], q[2] / size[2]];
        let unit = self.primitive.distance(local);
        // Approximate metric distance for non-uniform scales.
        unit * size[0].min(size[1]).min(size[2])
    }
}

/// Rotate a vector by the inverse of an XYZ Euler rotation (degrees).
fn rotate_inverse(v: V3, degrees: V3) -> V3 {
    let (sx, cx) = degrees[0].to_radians().sin_cos();
    let (sy, cy) = degrees[1].to_radians().sin_cos();
    let (sz, cz) = degrees[2].to_radians().sin_cos();
    // Inverse = apply Z, then Y, then X with negated angles.
    let v = [v[0] * cz + v[1] * sz, -v[0] * sz + v[1] * cz, v[2]];
    let v = [v[0] * cy - v[2] * sy, v[1], v[0] * sy + v[2] * cy];
    let v = [v[0], v[1] * cx + v[2] * sx, -v[1] * sx + v[2] * cx];
    v
}

/// A complete model: an ordered list of shapes combined by their operations.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Solid {
    pub parts: Vec<Shape>,
}

impl Solid {
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    /// Radius of a sphere centred on the origin that contains every part.
    pub fn extent(&self) -> f32 {
        let mut max = 0.0f32;
        for part in &self.parts {
            max = max.max(length(part.position) + length(part.scale) * 0.5);
        }
        max
    }

    /// Signed distance at a world point (negative = inside).
    pub fn distance(&self, p: V3) -> f32 {
        let mut distance = f32::INFINITY;
        for part in &self.parts {
            let d = part.distance(p);
            distance = match part.op {
                BoolOp::Union => distance.min(d),
                BoolOp::Subtract => distance.max(-d),
                BoolOp::Intersect => distance.max(d),
            };
        }
        distance
    }

    /// Surface normal (finite difference) at a point.
    pub fn normal(&self, p: V3) -> V3 {
        let e = 0.5;
        normalize([
            self.distance([p[0] + e, p[1], p[2]]) - self.distance([p[0] - e, p[1], p[2]]),
            self.distance([p[0], p[1] + e, p[2]]) - self.distance([p[0], p[1] - e, p[2]]),
            self.distance([p[0], p[1], p[2] + e]) - self.distance([p[0], p[1], p[2] - e]),
        ])
    }

    /// Mesh the solid over a `size`-metre cube centred on the origin, using a
    /// `resolution³` sampling grid (surface nets).
    pub fn mesh(&self, resolution: usize, size: f32) -> SolidMesh {
        surface_nets(self, resolution.max(4), size.max(0.1))
    }
}

/// A triangle mesh produced from a [`Solid`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SolidMesh {
    pub positions: Vec<V3>,
    pub normals: Vec<V3>,
    pub indices: Vec<u32>,
}

impl SolidMesh {
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
}

/// Surface nets: place one vertex per cell that straddles the surface, then
/// stitch quads across the grid edges that change sign.
fn surface_nets(solid: &Solid, n: usize, size: f32) -> SolidMesh {
    let step = size / n as f32;
    let half = size * 0.5;
    let corner = |x: usize, y: usize, z: usize| -> V3 {
        [
            -half + x as f32 * step,
            -half + y as f32 * step,
            -half + z as f32 * step,
        ]
    };

    // Sample the field at every grid corner.
    let dim = n + 1;
    let mut values = vec![0.0f32; dim * dim * dim];
    let at = |x: usize, y: usize, z: usize| (z * dim + y) * dim + x;
    for z in 0..dim {
        for y in 0..dim {
            for x in 0..dim {
                values[at(x, y, z)] = solid.distance(corner(x, y, z));
            }
        }
    }

    // One vertex per cell whose corners are not all inside/outside.
    let cell_index = |x: usize, y: usize, z: usize| (z * n + y) * n + x;
    let mut cell_vertex: Vec<Option<u32>> = vec![None; n * n * n];
    let mut mesh = SolidMesh::default();

    // The 12 edges of a cell, as (dx, dy, dz) corner offsets.
    const EDGES: [(usize, usize, usize, usize, usize, usize); 12] = [
        (0, 0, 0, 1, 0, 0),
        (0, 1, 0, 1, 1, 0),
        (0, 0, 1, 1, 0, 1),
        (0, 1, 1, 1, 1, 1),
        (0, 0, 0, 0, 1, 0),
        (1, 0, 0, 1, 1, 0),
        (0, 0, 1, 0, 1, 1),
        (1, 0, 1, 1, 1, 1),
        (0, 0, 0, 0, 0, 1),
        (1, 0, 0, 1, 0, 1),
        (0, 1, 0, 0, 1, 1),
        (1, 1, 0, 1, 1, 1),
    ];

    for z in 0..n {
        for y in 0..n {
            for x in 0..n {
                let corners: [f32; 8] = [
                    values[at(x, y, z)],
                    values[at(x + 1, y, z)],
                    values[at(x, y + 1, z)],
                    values[at(x + 1, y + 1, z)],
                    values[at(x, y, z + 1)],
                    values[at(x + 1, y, z + 1)],
                    values[at(x, y + 1, z + 1)],
                    values[at(x + 1, y + 1, z + 1)],
                ];
                let inside = corners.iter().filter(|v| **v < 0.0).count();
                if inside == 0 || inside == 8 {
                    continue;
                }
                let corner_value = |dx: usize, dy: usize, dz: usize| -> f32 {
                    let index = dz * 4 + dy * 2 + dx;
                    corners[index]
                };
                let mut sum = [0.0f32; 3];
                let mut count = 0.0f32;
                for (ax, ay, az, bx, by, bz) in EDGES {
                    let a = corner_value(ax, ay, az);
                    let b = corner_value(bx, by, bz);
                    if (a < 0.0) == (b < 0.0) {
                        continue;
                    }
                    let t = a / (a - b);
                    let pa = corner(x + ax, y + ay, z + az);
                    let pb = corner(x + bx, y + by, z + bz);
                    sum = add(sum, add(pa, scale(sub(pb, pa), t)));
                    count += 1.0;
                }
                if count == 0.0 {
                    continue;
                }
                let position = scale(sum, 1.0 / count);
                let index = mesh.positions.len() as u32;
                mesh.positions.push(position);
                mesh.normals.push(solid.normal(position));
                cell_vertex[cell_index(x, y, z)] = Some(index);
            }
        }
    }

    // Stitch quads: for each grid edge that changes sign, join the four cells
    // around it (when all four have a vertex).
    let mut quad = |a: Option<u32>, b: Option<u32>, c: Option<u32>, d: Option<u32>, flip: bool| {
        let (Some(a), Some(b), Some(c), Some(d)) = (a, b, c, d) else {
            return;
        };
        if flip {
            mesh.indices.extend_from_slice(&[a, b, c, a, c, d]);
        } else {
            mesh.indices.extend_from_slice(&[a, c, b, a, d, c]);
        }
    };
    let cell = |x: isize, y: isize, z: isize| -> Option<u32> {
        if x < 0 || y < 0 || z < 0 || x >= n as isize || y >= n as isize || z >= n as isize {
            return None;
        }
        cell_vertex[cell_index(x as usize, y as usize, z as usize)]
    };
    let value =
        |x: isize, y: isize, z: isize| -> f32 { values[at(x as usize, y as usize, z as usize)] };

    for z in 1..n {
        for y in 1..n {
            for x in 0..n {
                if (value(x as isize, y as isize, z as isize) < 0.0)
                    != (value(x as isize + 1, y as isize, z as isize) < 0.0)
                {
                    let flip = value(x as isize, y as isize, z as isize) < 0.0;
                    quad(
                        cell(x as isize, y as isize - 1, z as isize - 1),
                        cell(x as isize, y as isize, z as isize - 1),
                        cell(x as isize, y as isize, z as isize),
                        cell(x as isize, y as isize - 1, z as isize),
                        flip,
                    );
                }
            }
        }
    }
    for z in 1..n {
        for y in 0..n {
            for x in 1..n {
                if (value(x as isize, y as isize, z as isize) < 0.0)
                    != (value(x as isize, y as isize + 1, z as isize) < 0.0)
                {
                    let flip = value(x as isize, y as isize, z as isize) < 0.0;
                    quad(
                        cell(x as isize - 1, y as isize, z as isize - 1),
                        cell(x as isize, y as isize, z as isize - 1),
                        cell(x as isize, y as isize, z as isize),
                        cell(x as isize - 1, y as isize, z as isize),
                        !flip,
                    );
                }
            }
        }
    }
    for y in 1..n {
        for x in 1..n {
            for z in 0..n {
                if (value(x as isize, y as isize, z as isize) < 0.0)
                    != (value(x as isize, y as isize, z as isize + 1) < 0.0)
                {
                    let flip = value(x as isize, y as isize, z as isize) < 0.0;
                    quad(
                        cell(x as isize - 1, y as isize - 1, z as isize),
                        cell(x as isize, y as isize - 1, z as isize),
                        cell(x as isize, y as isize, z as isize),
                        cell(x as isize - 1, y as isize, z as isize),
                        flip,
                    );
                }
            }
        }
    }

    mesh
}

/// Serialize a solid as `[part]` blocks (used inside `plane.conf`).
impl Solid {
    pub fn to_config_string(&self) -> String {
        let mut out = String::new();
        for part in &self.parts {
            let _ = writeln!(out, "\n[part]");
            let _ = writeln!(out, "shape = {}", part.primitive.id());
            let _ = writeln!(out, "op = {}", part.op.id());
            let _ = writeln!(
                out,
                "position = {} {} {}",
                part.position[0], part.position[1], part.position[2]
            );
            let _ = writeln!(
                out,
                "rotation = {} {} {}",
                part.rotation[0], part.rotation[1], part.rotation[2]
            );
            let _ = writeln!(
                out,
                "scale = {} {} {}",
                part.scale[0], part.scale[1], part.scale[2]
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inside(solid: &Solid, p: V3) -> bool {
        solid.distance(p) < 0.0
    }

    #[test]
    fn a_box_is_inside_its_bounds() {
        let solid = Solid {
            parts: vec![Shape::new(Primitive::Box, [0.0; 3], [2.0, 2.0, 2.0])],
        };
        assert!(inside(&solid, [0.0, 0.0, 0.0]));
        assert!(inside(&solid, [0.9, 0.9, 0.9]));
        assert!(!inside(&solid, [1.2, 0.0, 0.0]));
    }

    #[test]
    fn subtract_carves_a_hole() {
        let solid = Solid {
            parts: vec![
                Shape::new(Primitive::Box, [0.0; 3], [2.0, 2.0, 2.0]),
                Shape {
                    primitive: Primitive::Sphere,
                    op: BoolOp::Subtract,
                    position: [0.0, 1.0, 0.0],
                    rotation: [0.0; 3],
                    scale: [1.0, 1.0, 1.0],
                },
            ],
        };
        // The centre is still solid, but the top-middle is carved out.
        assert!(inside(&solid, [0.0, 0.0, 0.0]));
        assert!(!inside(&solid, [0.0, 0.95, 0.0]));
    }

    #[test]
    fn intersect_keeps_the_overlap() {
        let solid = Solid {
            parts: vec![
                Shape::new(Primitive::Box, [0.0; 3], [2.0, 2.0, 2.0]),
                Shape {
                    primitive: Primitive::Sphere,
                    op: BoolOp::Intersect,
                    position: [0.0; 3],
                    rotation: [0.0; 3],
                    scale: [2.0, 2.0, 2.0],
                },
            ],
        };
        assert!(inside(&solid, [0.0, 0.0, 0.0]));
        // A box corner is outside the sphere, so the intersection drops it.
        assert!(!inside(&solid, [0.95, 0.95, 0.95]));
    }

    #[test]
    fn meshing_produces_a_closed_surface() {
        let solid = Solid {
            parts: vec![Shape::new(Primitive::Sphere, [0.0; 3], [2.0, 2.0, 2.0])],
        };
        let mesh = solid.mesh(24, 4.0);
        assert!(!mesh.is_empty(), "a sphere should mesh");
        assert_eq!(mesh.positions.len(), mesh.normals.len());
        assert_eq!(mesh.indices.len() % 3, 0);
        assert!(mesh.triangle_count() > 100, "expected a decent sphere mesh");
        // All vertices should sit near the sphere's surface (radius 1).
        for p in &mesh.positions {
            let r = length(*p);
            assert!((r - 1.0).abs() < 0.4, "vertex off the surface: {r}");
        }
    }

    #[test]
    fn subtract_changes_the_mesh() {
        let plain = Solid {
            parts: vec![Shape::new(Primitive::Box, [0.0; 3], [3.0, 3.0, 3.0])],
        };
        let mut carved = plain.clone();
        carved.parts.push(Shape {
            primitive: Primitive::Sphere,
            op: BoolOp::Subtract,
            position: [0.0, 1.4, 0.0],
            rotation: [0.0; 3],
            scale: [2.0, 2.0, 2.0],
        });
        assert_ne!(
            plain.mesh(20, 4.0).positions.len(),
            carved.mesh(20, 4.0).positions.len(),
            "subtracting a sphere should change the surface"
        );
    }

    #[test]
    fn round_trips_through_config_text() {
        let solid = Solid {
            parts: vec![
                Shape::new(Primitive::Box, [0.0, 0.0, 0.0], [4.0, 1.0, 1.0]),
                Shape {
                    primitive: Primitive::Cone,
                    op: BoolOp::Subtract,
                    position: [1.0, 0.0, 0.0],
                    rotation: [0.0, 90.0, 0.0],
                    scale: [1.0, 2.0, 1.0],
                },
            ],
        };
        let text = solid.to_config_string();
        assert!(text.contains("[part]"));
        assert!(text.contains("shape = cone"));
        assert!(text.contains("op = subtract"));
    }
}

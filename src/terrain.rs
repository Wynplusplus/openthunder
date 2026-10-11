//! Heightmap terrain.
//!
//! A terrain is a square grid of heights (`resolution × resolution`) spanning
//! `size` metres, centred on the origin. Mountains, valleys and sea floors are
//! all just heights; a global `water_level` turns everything below it into a
//! body of water. The grid is stored compactly (Base64 little-endian `f32`s)
//! inside the mission file.
//!
//! Sampling is bilinear, so the flight/ground model and the editor agree with
//! the rendered mesh.

use crate::base64;

/// The smallest and largest grid resolution we accept.
pub const MIN_RESOLUTION: usize = 9;
pub const MAX_RESOLUTION: usize = 257;

/// A square grid of terrain heights.
#[derive(Clone, Debug, PartialEq)]
pub struct Heightmap {
    /// Vertices per side (the grid is `resolution × resolution`).
    pub resolution: usize,
    /// Full width of the grid in metres (it spans `-size/2 .. size/2`).
    pub size: f32,
    /// Heights in metres, row-major (`iz * resolution + ix`).
    pub heights: Vec<f32>,
}

impl Heightmap {
    /// A flat terrain at `height`.
    pub fn flat(resolution: usize, size: f32, height: f32) -> Self {
        let resolution = resolution.clamp(MIN_RESOLUTION, MAX_RESOLUTION);
        Self {
            resolution,
            size,
            heights: vec![height; resolution * resolution],
        }
    }

    /// Distance between neighbouring grid vertices.
    pub fn cell(&self) -> f32 {
        if self.resolution <= 1 {
            self.size
        } else {
            self.size / (self.resolution - 1) as f32
        }
    }

    /// Row-major index of grid vertex `(ix, iz)`.
    pub fn index(&self, ix: usize, iz: usize) -> usize {
        iz * self.resolution + ix
    }

    /// Height at grid vertex `(ix, iz)`, clamped to the grid.
    pub fn height(&self, ix: usize, iz: usize) -> f32 {
        let ix = ix.min(self.resolution - 1);
        let iz = iz.min(self.resolution - 1);
        self.heights[self.index(ix, iz)]
    }

    pub fn set_height(&mut self, ix: usize, iz: usize, height: f32) {
        if ix < self.resolution && iz < self.resolution {
            let index = self.index(ix, iz);
            self.heights[index] = height;
        }
    }

    /// World position of grid vertex `(ix, iz)` (x, z).
    pub fn vertex(&self, ix: usize, iz: usize) -> (f32, f32) {
        let half = self.size * 0.5;
        let cell = self.cell();
        (-half + ix as f32 * cell, -half + iz as f32 * cell)
    }

    /// Bilinear height at a world position.
    pub fn sample(&self, x: f32, z: f32) -> f32 {
        let n = self.resolution;
        if n == 0 {
            return 0.0;
        }
        if n == 1 {
            return self.heights[0];
        }
        let cell = self.cell();
        let half = self.size * 0.5;
        let gx = ((x + half) / cell).clamp(0.0, (n - 1) as f32);
        let gz = ((z + half) / cell).clamp(0.0, (n - 1) as f32);
        let ix = gx.floor() as usize;
        let iz = gz.floor() as usize;
        let fx = gx - ix as f32;
        let fz = gz - iz as f32;
        let ix1 = (ix + 1).min(n - 1);
        let iz1 = (iz + 1).min(n - 1);
        let top = self.height(ix, iz) * (1.0 - fx) + self.height(ix1, iz) * fx;
        let bottom = self.height(ix, iz1) * (1.0 - fx) + self.height(ix1, iz1) * fx;
        top * (1.0 - fz) + bottom * fz
    }

    /// `(min, max)` height.
    pub fn range(&self) -> (f32, f32) {
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        for height in &self.heights {
            min = min.min(*height);
            max = max.max(*height);
        }
        if min > max { (0.0, 0.0) } else { (min, max) }
    }

    /// Smoothstep falloff `0..1` for a normalised distance `t`.
    fn falloff(t: f32) -> f32 {
        let t = (1.0 - t).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    /// The grid-index bounds of a brush centred at `(cx, cz)` with `radius`.
    fn brush_bounds(&self, cx: f32, cz: f32, radius: f32) -> (usize, usize, usize, usize) {
        let n = self.resolution;
        let cell = self.cell();
        let half = self.size * 0.5;
        let lo_x = (((cx - radius + half) / cell).floor()).max(0.0) as usize;
        let hi_x = (((cx + radius + half) / cell).ceil()).min((n - 1) as f32) as usize;
        let lo_z = (((cz - radius + half) / cell).floor()).max(0.0) as usize;
        let hi_z = (((cz + radius + half) / cell).ceil()).min((n - 1) as f32) as usize;
        (lo_x, hi_x, lo_z, hi_z)
    }

    /// Raise (`amount > 0`) or lower (`amount < 0`) the terrain inside a circular
    /// brush, with a smooth falloff — the building block for mountains and pits.
    pub fn sculpt(&mut self, cx: f32, cz: f32, radius: f32, amount: f32) {
        if radius <= 0.0 {
            return;
        }
        let (lo_x, hi_x, lo_z, hi_z) = self.brush_bounds(cx, cz, radius);
        for iz in lo_z..=hi_z {
            for ix in lo_x..=hi_x {
                let (x, z) = self.vertex(ix, iz);
                let distance = ((x - cx).powi(2) + (z - cz).powi(2)).sqrt();
                if distance > radius {
                    continue;
                }
                let weight = Self::falloff(distance / radius);
                let index = self.index(ix, iz);
                self.heights[index] += amount * weight;
            }
        }
    }

    /// Blend the terrain inside a brush toward `target` — flattening a plateau.
    pub fn flatten(&mut self, cx: f32, cz: f32, radius: f32, target: f32, strength: f32) {
        if radius <= 0.0 {
            return;
        }
        let strength = strength.clamp(0.0, 1.0);
        let (lo_x, hi_x, lo_z, hi_z) = self.brush_bounds(cx, cz, radius);
        for iz in lo_z..=hi_z {
            for ix in lo_x..=hi_x {
                let (x, z) = self.vertex(ix, iz);
                let distance = ((x - cx).powi(2) + (z - cz).powi(2)).sqrt();
                if distance > radius {
                    continue;
                }
                let weight = Self::falloff(distance / radius) * strength;
                let index = self.index(ix, iz);
                self.heights[index] += (target - self.heights[index]) * weight;
            }
        }
    }

    /// Average each vertex inside a brush with its neighbours — smoothing.
    pub fn smooth(&mut self, cx: f32, cz: f32, radius: f32, strength: f32) {
        if radius <= 0.0 {
            return;
        }
        let strength = strength.clamp(0.0, 1.0);
        let (lo_x, hi_x, lo_z, hi_z) = self.brush_bounds(cx, cz, radius);
        let source = self.heights.clone();
        let n = self.resolution;
        let at = |ix: usize, iz: usize| -> f32 { source[iz.min(n - 1) * n + ix.min(n - 1)] };
        for iz in lo_z..=hi_z {
            for ix in lo_x..=hi_x {
                let (x, z) = self.vertex(ix, iz);
                let distance = ((x - cx).powi(2) + (z - cz).powi(2)).sqrt();
                if distance > radius {
                    continue;
                }
                let weight = Self::falloff(distance / radius) * strength;
                let left = at(ix.saturating_sub(1), iz);
                let right = at((ix + 1).min(n - 1), iz);
                let up = at(ix, iz.saturating_sub(1));
                let down = at(ix, (iz + 1).min(n - 1));
                let average = (left + right + up + down + at(ix, iz) * 2.0) / 6.0;
                let index = self.index(ix, iz);
                self.heights[index] += (average - self.heights[index]) * weight;
            }
        }
    }

    /// All heights as little-endian `f32` bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.heights.len() * 4);
        for height in &self.heights {
            bytes.extend_from_slice(&height.to_le_bytes());
        }
        bytes
    }

    /// Base64 of [`Heightmap::to_bytes`].
    pub fn to_base64(&self) -> String {
        base64::encode(&self.to_bytes())
    }

    /// Rebuild from Base64 produced by [`Heightmap::to_base64`]. The byte count
    /// must match `resolution × resolution × 4`.
    pub fn from_base64(resolution: usize, size: f32, text: &str) -> Option<Self> {
        let bytes = base64::decode(text)?;
        if bytes.len() != resolution * resolution * 4 {
            return None;
        }
        let mut heights = Vec::with_capacity(resolution * resolution);
        for chunk in bytes.chunks_exact(4) {
            heights.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }
        Some(Self {
            resolution,
            size,
            heights,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_terrain_samples_its_height() {
        let map = Heightmap::flat(33, 1000.0, 12.0);
        assert_eq!(map.sample(0.0, 0.0), 12.0);
        assert_eq!(map.sample(-400.0, 250.0), 12.0);
        // Outside the grid it clamps to the edge.
        assert_eq!(map.sample(99_999.0, 0.0), 12.0);
    }

    #[test]
    fn sculpt_raises_a_mountain() {
        let mut map = Heightmap::flat(65, 2000.0, 0.0);
        map.sculpt(0.0, 0.0, 400.0, 500.0);
        let peak = map.sample(0.0, 0.0);
        assert!(peak > 400.0, "the peak should be tall, got {peak}");
        // Far from the brush nothing changed.
        assert_eq!(map.sample(900.0, 900.0), 0.0);
        // It falls off toward the edge.
        assert!(map.sample(350.0, 0.0) < peak);
    }

    #[test]
    fn flatten_and_smooth_converge() {
        let mut map = Heightmap::flat(33, 1000.0, 0.0);
        map.sculpt(0.0, 0.0, 200.0, 100.0);
        for _ in 0..50 {
            map.flatten(0.0, 0.0, 200.0, 25.0, 1.0);
        }
        assert!((map.sample(0.0, 0.0) - 25.0).abs() < 1.0);
    }

    #[test]
    fn base64_round_trips() {
        let mut map = Heightmap::flat(17, 800.0, 0.0);
        map.sculpt(100.0, -50.0, 150.0, 60.0);
        let text = map.to_base64();
        let parsed = Heightmap::from_base64(17, 800.0, &text).expect("decode");
        assert_eq!(parsed, map);
        // A wrong resolution is rejected.
        assert!(Heightmap::from_base64(33, 800.0, &text).is_none());
    }
}

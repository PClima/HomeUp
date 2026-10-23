use glam::{DMat4, DVec2, DVec3, DVec4};

#[derive(Clone, Debug)]
pub struct Camera {
    pub target: DVec3,
    pub yaw: f64,
    pub pitch: f64,
    pub dist: f64,
    pub fov_y: f64,
    pub ortho: bool,
    /// Rotation around the view direction (only Match Photo uses it).
    pub roll: f64,
    /// Viewport size in logical pixels.
    pub size: DVec2,
}

impl Default for Camera {
    fn default() -> Self {
        Camera {
            target: DVec3::new(1.0, 1.0, 0.5),
            yaw: (-60f64).to_radians(),
            pitch: 12f64.to_radians(),
            dist: 9.0,
            fov_y: 35f64.to_radians(),
            ortho: false,
            roll: 0.0,
            size: DVec2::new(800.0, 600.0),
        }
    }
}

impl Camera {
    pub fn forward(&self) -> DVec3 {
        -DVec3::new(self.pitch.cos() * self.yaw.cos(), self.pitch.cos() * self.yaw.sin(), self.pitch.sin())
    }
    pub fn eye(&self) -> DVec3 {
        self.target - self.forward() * self.dist
    }
    pub fn right(&self) -> DVec3 {
        self.forward().cross(DVec3::Z).normalize()
    }
    pub fn up(&self) -> DVec3 {
        self.right().cross(self.forward())
    }
    /// Up vector of the view, including roll.
    pub fn view_up(&self) -> DVec3 {
        if self.roll == 0.0 {
            return DVec3::Z;
        }
        let (u, r) = (self.up(), self.right());
        u * self.roll.cos() - r * self.roll.sin()
    }
    fn near_far(&self) -> (f64, f64) {
        if self.ortho {
            (-(self.dist * 200.0 + 500.0), self.dist * 200.0 + 500.0)
        } else {
            ((self.dist * 0.002).max(0.0005), self.dist * 500.0 + 500.0)
        }
    }
    fn aspect(&self) -> f64 {
        (self.size.x / self.size.y.max(1.0)).max(0.01)
    }
    pub fn view_proj(&self) -> DMat4 {
        let (n, f) = self.near_far();
        let view = look_at_rh(self.eye(), self.target, self.view_up());
        let proj = if self.ortho {
            let h = self.dist * (self.fov_y * 0.5).tan();
            orthographic_rh(h * self.aspect(), h, n, f)
        } else {
            perspective_rh(self.fov_y, self.aspect(), n, f)
        };
        proj * view
    }

    /// Ray through a viewport pixel (logical coords relative to viewport origin).
    /// The origin lies on the near plane (perspective) or on the view plane (parallel).
    pub fn ray(&self, px: DVec2) -> (DVec3, DVec3) {
        let ndc = DVec2::new(px.x / self.size.x * 2.0 - 1.0, 1.0 - px.y / self.size.y * 2.0);
        let inv = self.view_proj().inverse();
        let a = inv * DVec4::new(ndc.x, ndc.y, 0.0, 1.0);
        let b = inv * DVec4::new(ndc.x, ndc.y, 1.0, 1.0);
        let (a, b) = (a.truncate() / a.w, b.truncate() / b.w);
        let dir = (b - a).normalize();
        if self.ortho { (a, dir) } else { (self.eye(), dir) }
    }

    /// Project a world point to viewport pixels. None if behind the camera.
    pub fn project(&self, p: DVec3) -> Option<DVec2> {
        let c = self.view_proj() * p.extend(1.0);
        if c.w <= 1e-9 {
            return None;
        }
        let ndc = c.truncate() / c.w;
        Some(DVec2::new((ndc.x + 1.0) * 0.5 * self.size.x, (1.0 - ndc.y) * 0.5 * self.size.y))
    }

    /// Project a segment, clipping it against the near plane.
    pub fn project_segment(&self, a: DVec3, b: DVec3) -> Option<(DVec2, DVec2)> {
        if self.ortho {
            return Some((self.project(a)?, self.project(b)?));
        }
        let f = self.forward();
        let eye = self.eye();
        let near = self.near_far().0 * 1.5;
        let (da, db) = ((a - eye).dot(f), (b - eye).dot(f));
        if da < near && db < near {
            return None;
        }
        let clip = |p: DVec3, q: DVec3, dp: f64, dq: f64| p + (q - p) * ((near - dp) / (dq - dp));
        let (a2, b2) = (if da < near { clip(a, b, da, db) } else { a }, if db < near { clip(b, a, db, da) } else { b });
        Some((self.project(a2)?, self.project(b2)?))
    }

    /// Depth of a point along the view direction (for screen-size computations).
    pub fn depth(&self, p: DVec3) -> f64 {
        (p - self.eye()).dot(self.forward()).max(1e-6)
    }

    /// World units per pixel at view depth `depth`.
    pub fn world_per_px(&self, depth: f64) -> f64 {
        let d = if self.ortho { self.dist } else { depth };
        2.0 * d * (self.fov_y * 0.5).tan() / self.size.y.max(1.0)
    }

    pub fn orbit(&mut self, delta: DVec2) {
        self.yaw -= delta.x * 0.008;
        self.pitch = (self.pitch + delta.y * 0.008).clamp(-1.55, 1.55);
    }

    pub fn pan(&mut self, delta: DVec2) {
        let s = self.world_per_px(self.dist);
        self.target += (-self.right() * delta.x + self.up() * delta.y) * s;
    }

    /// Zoom toward `focus` (world point) by `factor` (<1 zooms in).
    pub fn zoom_at(&mut self, focus: DVec3, factor: f64) {
        let factor = factor.clamp(0.2, 5.0);
        let new_dist = (self.dist * factor).clamp(0.01, 1e6);
        let f = new_dist / self.dist;
        self.target = focus + (self.target - focus) * f;
        self.dist = new_dist;
    }

    pub fn frame_bounds(&mut self, lo: DVec3, hi: DVec3) {
        let c = (lo + hi) * 0.5;
        let r = ((hi - lo).length() * 0.5).max(0.5);
        let aspect = self.aspect().min(1.0);
        self.target = c;
        self.dist = r / ((self.fov_y * 0.5).tan() * aspect) * 1.15;
    }

    pub fn to_array(&self) -> [f64; 6] {
        [self.target.x, self.target.y, self.target.z, self.yaw, self.pitch, self.dist]
    }

    pub fn set_array(&mut self, c: [f64; 6]) {
        self.target = DVec3::new(c[0], c[1], c[2]);
        self.yaw = c[3];
        self.pitch = c[4];
        self.dist = c[5];
    }

    /// Interpolated camera between `a` and `b` (scene transitions).
    pub fn lerp(a: &Camera, b: &Camera, t: f64) -> Camera {
        let t = t * t * (3.0 - 2.0 * t);
        let mut dy = (b.yaw - a.yaw) % std::f64::consts::TAU;
        if dy > std::f64::consts::PI {
            dy -= std::f64::consts::TAU;
        } else if dy < -std::f64::consts::PI {
            dy += std::f64::consts::TAU;
        }
        Camera {
            target: a.target.lerp(b.target, t),
            yaw: a.yaw + dy * t,
            pitch: a.pitch + (b.pitch - a.pitch) * t,
            dist: (a.dist.ln() + (b.dist.ln() - a.dist.ln()) * t).exp(),
            fov_y: a.fov_y + (b.fov_y - a.fov_y) * t,
            ortho: if t < 0.5 { a.ortho } else { b.ortho },
            roll: a.roll + (b.roll - a.roll) * t,
            size: b.size,
        }
    }
}

/// Right-handed perspective projection with depth in [0, 1] (wgpu convention).
fn perspective_rh(fov_y: f64, aspect: f64, near: f64, far: f64) -> DMat4 {
    let h = 1.0 / (fov_y * 0.5).tan();
    let w = h / aspect;
    let r = far / (near - far);
    DMat4::from_cols(
        DVec4::new(w, 0.0, 0.0, 0.0),
        DVec4::new(0.0, h, 0.0, 0.0),
        DVec4::new(0.0, 0.0, r, -1.0),
        DVec4::new(0.0, 0.0, r * near, 0.0),
    )
}

/// Right-handed orthographic projection with depth in [0, 1].
pub(crate) fn orthographic_rh(half_w: f64, half_h: f64, near: f64, far: f64) -> DMat4 {
    let r = 1.0 / (near - far);
    DMat4::from_cols(
        DVec4::new(1.0 / half_w, 0.0, 0.0, 0.0),
        DVec4::new(0.0, 1.0 / half_h, 0.0, 0.0),
        DVec4::new(0.0, 0.0, r, 0.0),
        DVec4::new(0.0, 0.0, r * near, 1.0),
    )
}

/// Right-handed view matrix.
pub(crate) fn look_at_rh(eye: DVec3, center: DVec3, up: DVec3) -> DMat4 {
    let f = (center - eye).normalize();
    let s = f.cross(up).normalize();
    let u = s.cross(f);
    DMat4::from_cols(
        DVec4::new(s.x, u.x, -f.x, 0.0),
        DVec4::new(s.y, u.y, -f.y, 0.0),
        DVec4::new(s.z, u.z, -f.z, 0.0),
        DVec4::new(-s.dot(eye), -u.dot(eye), f.dot(eye), 1.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_ray_roundtrip() {
        for ortho in [false, true] {
            let c = Camera { ortho, ..Camera::default() };
            let px = DVec2::new(321.0, 123.0);
            let (o, d) = c.ray(px);
            let p = o + d * 7.0;
            let back = c.project(p).unwrap();
            assert!(back.distance(px) < 1e-6, "{ortho} {back:?}");
            let center = c.project(c.target).unwrap();
            assert!(center.distance(c.size * 0.5) < 1e-6);
        }
    }
}

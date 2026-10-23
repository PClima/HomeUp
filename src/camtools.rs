//! Camera tools: Zoom Window, Position Camera, Look Around, Walk, and the
//! previous/next view history.

use crate::app::{HomeUpApp, In, Tool, ToolState};
use crate::camera::Camera;
use crate::geom::Plane;
use crate::units::{format_len, parse_len};
use glam::{DVec2, DVec3};

pub const EYE_HEIGHT: f64 = 1.676;

impl Camera {
    /// Keep the eye where it is and look along yaw/pitch.
    pub fn set_look(&mut self, eye: DVec3, yaw: f64, pitch: f64) {
        self.yaw = yaw;
        self.pitch = pitch.clamp(-1.55, 1.55);
        self.dist = self.dist.clamp(0.05, 2.0);
        self.target = eye + self.forward() * self.dist;
    }
}

impl HomeUpApp {
    pub(crate) fn zoom_window_tool(&mut self, i: &In) {
        self.snap = None;
        if i.pressed {
            self.state = ToolState::Box { start: i.pos };
        }
        let ToolState::Box { start } = self.state else { return };
        if i.dragging_far {
            self.overlay.box_rect = Some((start, i.pos));
        }
        if !i.released {
            return;
        }
        self.state = ToolState::Idle;
        if !i.dragging_far {
            return;
        }
        let o = self.viewport_min;
        let a = DVec2::new((start.x - o.x) as f64, (start.y - o.y) as f64);
        let b = i.px;
        let c = (a + b) * 0.5;
        let (ro, rd) = self.cam.ray(c);
        let focus = self
            .cache
            .pick_face(ro, rd)
            .map(|(t, _)| ro + rd * t)
            .unwrap_or_else(|| Plane::new(self.cam.forward(), self.cam.target).intersect_ray(ro, rd).unwrap_or(self.cam.target));
        let frac = ((b - a).abs() / self.cam.size).max_element().max(0.02);
        let depth = self.cam.depth(focus);
        self.push_view();
        let f = self.cam.forward();
        self.cam.target = focus;
        self.cam.dist = (depth * frac).max(0.05);
        let _ = f;
    }

    pub(crate) fn position_camera_tool(&mut self, i: &In) {
        let s = self.snap(i.px, None);
        self.live_measure = Some(format!("Altura do olho: {}", format_len(self.eye_height)));
        if i.pressed {
            self.push_view();
            let eye = s.p + DVec3::Z * self.eye_height;
            let (yaw, pitch) = (self.cam.yaw, 0.0);
            self.cam.ortho = false;
            self.cam.dist = 1.0;
            self.cam.set_look(eye, yaw, pitch);
            self.set_tool(Tool::LookAround);
            self.message = "Câmera posicionada. Arraste para olhar em volta; digite a altura do olho.".into();
        }
    }

    pub(crate) fn look_around_tool(&mut self, i: &In) {
        self.snap = None;
        self.live_measure = Some(format!("Altura do olho: {}", format_len(self.cam.eye().z)));
        if i.primary_down {
            let eye = self.cam.eye();
            let (yaw, pitch) = (self.cam.yaw + i.delta.x * 0.004, self.cam.pitch - i.delta.y * 0.004);
            self.cam.set_look(eye, yaw, pitch);
        }
    }

    pub(crate) fn walk_tool(&mut self, i: &In) {
        self.snap = None;
        self.live_measure = Some(format!("Altura do olho: {}", format_len(self.cam.eye().z)));
        if i.pressed {
            self.walk_anchor = Some(i.pos);
            if self.cam.dist > 2.0 {
                // switch to an eye-level view first
                let eye = self.cam.eye();
                self.cam.dist = 1.0;
                self.cam.set_look(eye, self.cam.yaw, 0.0);
            }
        }
        if !i.primary_down {
            self.walk_anchor = None;
            return;
        }
        let Some(anchor) = self.walk_anchor else { return };
        // speed proportional to the distance dragged from the press point
        let dx = (i.pos.x - anchor.x) as f64;
        let dy = (i.pos.y - anchor.y) as f64;
        let dt = 1.0 / 60.0;
        let eye = self.cam.eye();
        let fwd = DVec3::new(self.cam.forward().x, self.cam.forward().y, 0.0).normalize_or_zero();
        let speed = 0.03;
        let new_eye = if i.mods.shift {
            eye + DVec3::Z * (-dy * speed * dt) + self.cam.right() * (dx * speed * dt)
        } else {
            eye + fwd * (-dy * speed * dt)
        };
        let yaw = if i.mods.shift { self.cam.yaw } else { self.cam.yaw - dx * 0.0008 };
        self.cam.set_look(new_eye, yaw, self.cam.pitch);
        self.overlay.lines.clear();
        self.repaint = true;
    }

    /// Eye height typed in the Measurements box (Position Camera, Look Around, Walk).
    pub(crate) fn vcb_camera(&mut self, text: &str) -> bool {
        if !matches!(self.tool, Tool::PositionCamera | Tool::LookAround | Tool::Walk) {
            return false;
        }
        let Some(h) = parse_len(text) else { return false };
        if self.tool == Tool::PositionCamera {
            self.eye_height = h;
            return true;
        }
        let mut eye = self.cam.eye();
        eye.z = h;
        let (y, p) = (self.cam.yaw, self.cam.pitch);
        self.cam.set_look(eye, y, p);
        true
    }

    /// Record the current view for Camera > Previous.
    pub(crate) fn push_view(&mut self) {
        let c = self.cam.clone();
        if self.view_back.last().is_some_and(|v| v.to_array() == c.to_array()) {
            return;
        }
        self.view_back.push(c);
        if self.view_back.len() > 50 {
            self.view_back.remove(0);
        }
        self.view_fwd.clear();
    }

    pub(crate) fn previous_view(&mut self, now: f64) {
        if let Some(v) = self.view_back.pop() {
            self.view_fwd.push(self.cam.clone());
            self.cam_anim = Some((self.cam.clone(), Camera { size: self.cam.size, ..v }, now));
        }
    }

    pub(crate) fn next_view(&mut self, now: f64) {
        if let Some(v) = self.view_fwd.pop() {
            self.view_back.push(self.cam.clone());
            self.cam_anim = Some((self.cam.clone(), Camera { size: self.cam.size, ..v }, now));
        }
    }

    /// Debounced view history: a view that stays still for a moment is recorded.
    pub(crate) fn track_view(&mut self, now: f64) {
        let cur = self.cam.to_array();
        if self.view_last.is_none_or(|(v, _)| v != cur) {
            if let Some((prev, t)) = self.view_last {
                if now - t > 0.6 && self.view_back.last().is_none_or(|b| b.to_array() != prev) {
                    let mut c = self.cam.clone();
                    c.set_array(prev);
                    self.view_back.push(c);
                    self.view_fwd.clear();
                }
            }
            self.view_last = Some((cur, now));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn look_around_keeps_the_eye() {
        let mut c = Camera::default();
        let eye = DVec3::new(2.0, -5.0, EYE_HEIGHT);
        c.dist = 1.0;
        c.set_look(eye, 0.3, 0.0);
        assert!(c.eye().distance(eye) < 1e-9);
        c.set_look(c.eye(), 1.2, 0.4);
        assert!(c.eye().distance(eye) < 1e-9);
        assert!((c.yaw - 1.2).abs() < 1e-12);
    }
}

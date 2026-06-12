//! Orbit camera: yaw/pitch around a focus point with dolly distance.
//! Right-drag orbits, middle-drag (or shift+right-drag) pans on the focus
//! plane, scroll dollies. `frame()` re-centres on the island.

use glam::{Mat4, Vec3};

pub struct OrbitCamera {
    pub focus: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub fov_y: f32,
}

impl OrbitCamera {
    pub fn for_island(world_size: f32) -> Self {
        Self {
            focus: Vec3::ZERO,
            yaw: std::f32::consts::FRAC_PI_4,
            pitch: -0.6,
            distance: world_size * 1.2,
            fov_y: 55f32.to_radians(),
        }
    }

    pub fn eye(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        // pitch < 0 looks down at the focus
        let dir = Vec3::new(cy * cp, sp, sy * cp);
        self.focus - dir * self.distance
    }

    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw += dx * 0.008;
        self.pitch = (self.pitch + dy * 0.008).clamp(-1.5, 1.5);
    }

    pub fn pan(&mut self, dx: f32, dy: f32) {
        let view = self.view();
        let inv = view.inverse();
        let right = inv.transform_vector3(Vec3::X);
        let up = inv.transform_vector3(Vec3::Y);
        let scale = self.distance * 0.0016;
        self.focus += -right * dx * scale + up * dy * scale;
    }

    pub fn dolly(&mut self, scroll: f32) {
        self.distance = (self.distance * (1.0 - scroll * 0.0014)).clamp(2.0, 20_000.0);
    }

    pub fn frame(&mut self, world_size: f32, height: f32) {
        self.focus = Vec3::new(0.0, height, 0.0);
        self.distance = world_size * 1.2;
    }

    pub fn view(&self) -> Mat4 {
        let eye = self.eye();
        let forward = (self.focus - eye).normalize();
        Mat4::look_at_rh(eye, eye + forward, Vec3::Y)
    }

    pub fn proj(&self, aspect: f32) -> Mat4 {
        Mat4::perspective_rh(self.fov_y, aspect.max(0.01), 0.5, 50_000.0)
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        self.proj(aspect) * self.view()
    }

    /// World-space ray through a viewport point given in [0,1]² UV.
    pub fn ray_through(&self, u: f32, v: f32, aspect: f32) -> (Vec3, Vec3) {
        let inv = self.view_proj(aspect).inverse();
        let ndc_x = u * 2.0 - 1.0;
        let ndc_y = 1.0 - v * 2.0;
        let near = inv.project_point3(Vec3::new(ndc_x, ndc_y, 0.001));
        let far = inv.project_point3(Vec3::new(ndc_x, ndc_y, 0.999));
        (self.eye(), (far - near).normalize())
    }
}

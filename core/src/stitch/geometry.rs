//! Lens models (image pixel <-> camera ray) and panorama projections (world direction <-> plane).
//!
//! Conventions: camera/world frame has x to the right, y down, z forward. A camera rotation `R`
//! maps camera rays to world directions: `d_world = R * d_cam`.

use nalgebra::{Rotation3, Vector3};
use serde::{Deserialize, Serialize};
use std::f64::consts::{FRAC_PI_2, PI};

pub type V3 = Vector3<f64>;
pub type Rot = Rotation3<f64>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum LensType {
    Rectilinear,
    FisheyeEquidistant,
    FisheyeEquisolid,
    FisheyeStereographic,
}

impl LensType {
    pub const ALL: [LensType; 4] = [
        LensType::Rectilinear,
        LensType::FisheyeEquidistant,
        LensType::FisheyeEquisolid,
        LensType::FisheyeStereographic,
    ];

    pub fn is_fisheye(self) -> bool {
        !matches!(self, LensType::Rectilinear)
    }

    /// Focal length (pixels) for a given horizontal field of view across `width` pixels.
    pub fn focal_from_hfov(self, hfov_deg: f64, width: f64) -> f64 {
        let half = (hfov_deg.to_radians() / 2.0).min(PI * 0.999);
        let r = width / 2.0;
        match self {
            LensType::Rectilinear => r / half.min(1.55).tan(),
            LensType::FisheyeEquidistant => r / half,
            LensType::FisheyeEquisolid => r / (2.0 * (half / 2.0).sin()),
            LensType::FisheyeStereographic => r / (2.0 * (half / 2.0).tan()),
        }
    }

    pub fn hfov_from_focal(self, f: f64, width: f64) -> f64 {
        let r = width / 2.0;
        let half = match self {
            LensType::Rectilinear => (r / f).atan(),
            LensType::FisheyeEquidistant => r / f,
            LensType::FisheyeEquisolid => 2.0 * (r / (2.0 * f)).min(1.0).asin(),
            LensType::FisheyeStereographic => 2.0 * (r / (2.0 * f)).atan(),
        };
        (2.0 * half).to_degrees()
    }

    /// Image-plane offset from the principal point (pixels) -> unit camera ray.
    #[inline]
    pub fn ray(self, x: f64, y: f64, f: f64) -> V3 {
        match self {
            LensType::Rectilinear => V3::new(x, y, f).normalize(),
            _ => {
                let r = (x * x + y * y).sqrt();
                if r < 1e-12 {
                    return V3::new(0.0, 0.0, 1.0);
                }
                let theta = match self {
                    LensType::FisheyeEquidistant => r / f,
                    LensType::FisheyeEquisolid => 2.0 * (r / (2.0 * f)).min(1.0).asin(),
                    LensType::FisheyeStereographic => 2.0 * (r / (2.0 * f)).atan(),
                    LensType::Rectilinear => unreachable!(),
                };
                let s = theta.sin() / r;
                V3::new(x * s, y * s, theta.cos())
            }
        }
    }

    /// Unit (or any length) camera ray -> image-plane offset from the principal point.
    #[inline]
    pub fn project(self, d: &V3, f: f64) -> Option<(f64, f64)> {
        match self {
            LensType::Rectilinear => {
                if d.z <= 1e-9 {
                    return None;
                }
                Some((f * d.x / d.z, f * d.y / d.z))
            }
            _ => {
                let n = d.norm();
                if n < 1e-12 {
                    return None;
                }
                let theta = (d.z / n).clamp(-1.0, 1.0).acos();
                let r = match self {
                    LensType::FisheyeEquidistant => f * theta,
                    LensType::FisheyeEquisolid => {
                        if theta > PI * 0.999 {
                            return None;
                        }
                        2.0 * f * (theta / 2.0).sin()
                    }
                    LensType::FisheyeStereographic => {
                        if theta > PI * 0.95 {
                            return None;
                        }
                        2.0 * f * (theta / 2.0).tan()
                    }
                    LensType::Rectilinear => unreachable!(),
                };
                let rxy = (d.x * d.x + d.y * d.y).sqrt();
                if rxy < 1e-12 {
                    return Some((0.0, 0.0));
                }
                Some((r * d.x / rxy, r * d.y / rxy))
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum Projection {
    Equirectangular,
    Cylindrical,
    Mercator,
    Rectilinear,
    Panini,
    Fisheye,
    Stereographic,
    LittlePlanet,
    Orthographic,
    TransverseMercator,
    Sinusoidal,
}

impl Projection {
    pub const ALL: [Projection; 11] = [
        Projection::Equirectangular,
        Projection::Cylindrical,
        Projection::Mercator,
        Projection::Rectilinear,
        Projection::Panini,
        Projection::Fisheye,
        Projection::Stereographic,
        Projection::LittlePlanet,
        Projection::Orthographic,
        Projection::TransverseMercator,
        Projection::Sinusoidal,
    ];

    /// Whether the projection wraps horizontally at ±180°.
    pub fn wraps(self) -> bool {
        matches!(self, Projection::Equirectangular | Projection::Cylindrical | Projection::Mercator | Projection::Sinusoidal)
    }

    /// Projections that look in one direction (with a field of view) rather than unrolling a strip.
    pub fn is_view(self) -> bool {
        matches!(
            self,
            Projection::Rectilinear | Projection::Panini | Projection::Fisheye | Projection::Stereographic | Projection::LittlePlanet | Projection::Orthographic
        )
    }

    /// Default field of view (degrees) when none is set; 0 = only the projection limits apply.
    pub fn auto_fov(self) -> f64 {
        match self {
            // wider rectilinear views stretch the edges beyond use
            Projection::Rectilinear => 130.0,
            _ => 0.0,
        }
    }

    /// Plane coordinate of a direction `a` radians away from the projection centre.
    pub fn half_extent(self, a: f64) -> f64 {
        let a = a.max(1e-3);
        match self {
            Projection::Rectilinear => a.min(1.5).tan(),
            Projection::Panini => {
                let a = a.min(PI * 0.96);
                let s = 2.0 / (1.0 + a.cos());
                s * a.sin()
            }
            Projection::Fisheye => a.min(PI),
            Projection::Stereographic | Projection::LittlePlanet => 2.0 * (a.min(PI * 0.94) / 2.0).tan(),
            Projection::Orthographic => a.min(FRAC_PI_2).sin(),
            _ => f64::MAX,
        }
    }

    /// Hard limits of the plane coordinates (u_min, u_max, v_min, v_max) for this projection.
    pub fn limits(self) -> (f64, f64, f64, f64) {
        match self {
            Projection::Equirectangular | Projection::Sinusoidal => (-PI, PI, -FRAC_PI_2, FRAC_PI_2),
            Projection::Cylindrical => (-PI, PI, -(80f64.to_radians().tan()), 80f64.to_radians().tan()),
            Projection::Mercator => {
                let m = (PI / 4.0 + 85f64.to_radians() / 2.0).tan().ln();
                (-PI, PI, -m, m)
            }
            Projection::Rectilinear => {
                let t = 80f64.to_radians().tan();
                (-t, t, -t, t)
            }
            Projection::Panini => {
                let u = 2.0 * (85f64.to_radians() / 2.0).tan() * 2.0;
                (-u, u, -4.0, 4.0)
            }
            Projection::Fisheye => (-PI, PI, -PI, PI),
            Projection::Stereographic | Projection::LittlePlanet => {
                let r = 2.0 * (160f64.to_radians() / 2.0).tan();
                (-r, r, -r, r)
            }
            Projection::Orthographic => (-1.0, 1.0, -1.0, 1.0),
            Projection::TransverseMercator => {
                let m = (PI / 4.0 + 85f64.to_radians() / 2.0).tan().ln();
                (-m, m, -PI, PI)
            }
        }
    }

    /// World direction (unit vector) -> plane coordinates (u right, v down).
    #[inline]
    pub fn forward(self, d: &V3) -> Option<(f64, f64)> {
        let (x, y, z) = (d.x, d.y, d.z);
        match self {
            Projection::Equirectangular => {
                let lon = x.atan2(z);
                let lat = (-y).atan2((x * x + z * z).sqrt());
                Some((lon, -lat))
            }
            Projection::Sinusoidal => {
                let lon = x.atan2(z);
                let lat = (-y).atan2((x * x + z * z).sqrt());
                Some((lon * lat.cos(), -lat))
            }
            Projection::Cylindrical => {
                let h = (x * x + z * z).sqrt();
                if h < 1e-9 {
                    return None;
                }
                Some((x.atan2(z), y / h))
            }
            Projection::Mercator => {
                let lon = x.atan2(z);
                let lat = (-y).atan2((x * x + z * z).sqrt());
                if lat.abs() > 89f64.to_radians() {
                    return None;
                }
                Some((lon, -((PI / 4.0 + lat / 2.0).tan().ln())))
            }
            Projection::TransverseMercator => {
                // Mercator rotated 90° about the z axis (vertical panoramas).
                let lon = (-y).atan2(z);
                let lat = (x).atan2((y * y + z * z).sqrt());
                if lat.abs() > 89f64.to_radians() {
                    return None;
                }
                Some(((PI / 4.0 + lat / 2.0).tan().ln(), -lon))
            }
            Projection::Rectilinear => {
                if z <= 1e-6 {
                    return None;
                }
                Some((x / z, y / z))
            }
            Projection::Panini => {
                let lon = x.atan2(z);
                if lon.abs() >= PI * 0.97 {
                    return None;
                }
                let h = (x * x + z * z).sqrt();
                let tan_lat = y / h;
                let s = 2.0 / (1.0 + lon.cos());
                Some((s * lon.sin(), s * tan_lat))
            }
            Projection::Fisheye => {
                let theta = z.clamp(-1.0, 1.0).acos();
                let r = (x * x + y * y).sqrt();
                if r < 1e-12 {
                    return Some((0.0, 0.0));
                }
                Some((theta * x / r, theta * y / r))
            }
            Projection::Stereographic => {
                if z <= -0.98 {
                    return None;
                }
                let k = 2.0 / (1.0 + z);
                Some((x * k, y * k))
            }
            Projection::LittlePlanet => {
                // Look straight down: nadir (0,1,0) maps to the centre, horizon to a circle.
                let (lx, ly, lz) = (x, -z, y);
                if lz <= -0.98 {
                    return None;
                }
                let k = 2.0 / (1.0 + lz);
                Some((lx * k, ly * k))
            }
            Projection::Orthographic => {
                if z < 0.0 {
                    return None;
                }
                Some((x, y))
            }
        }
    }

    /// Plane coordinates -> world direction (unit vector).
    #[inline]
    pub fn inverse(self, u: f64, v: f64) -> Option<V3> {
        match self {
            Projection::Equirectangular => {
                if v.abs() > FRAC_PI_2 || u.abs() > PI {
                    return None;
                }
                let lat = -v;
                Some(V3::new(lat.cos() * u.sin(), -lat.sin(), lat.cos() * u.cos()))
            }
            Projection::Sinusoidal => {
                if v.abs() > FRAC_PI_2 {
                    return None;
                }
                let lat = -v;
                let c = lat.cos();
                if c < 1e-9 {
                    return None;
                }
                let lon = u / c;
                if lon.abs() > PI {
                    return None;
                }
                Some(V3::new(c * lon.sin(), -lat.sin(), c * lon.cos()))
            }
            Projection::Cylindrical => {
                if u.abs() > PI {
                    return None;
                }
                Some(V3::new(u.sin(), v, u.cos()).normalize())
            }
            Projection::Mercator => {
                if u.abs() > PI {
                    return None;
                }
                // v = -ln(tan(pi/4+lat/2)) => lat = 2 atan(exp(-v)) - pi/2
                let lat = 2.0 * (-v).exp().atan() - FRAC_PI_2;
                Some(V3::new(lat.cos() * u.sin(), -lat.sin(), lat.cos() * u.cos()))
            }
            Projection::TransverseMercator => {
                if v.abs() > PI {
                    return None;
                }
                let lat = 2.0 * u.exp().atan() - FRAC_PI_2;
                let lon = -v;
                // forward: lon = atan2(-y, z), lat = atan2(x, sqrt(y²+z²))
                let c = lat.cos();
                Some(V3::new(lat.sin(), -c * lon.sin(), c * lon.cos()))
            }
            Projection::Rectilinear => Some(V3::new(u, v, 1.0).normalize()),
            Projection::Panini => {
                let lon = 2.0 * (u / 2.0).atan();
                let s = 2.0 / (1.0 + lon.cos());
                let tan_lat_y = v / s; // = y / h
                Some(V3::new(lon.sin(), tan_lat_y, lon.cos()).normalize())
            }
            Projection::Fisheye => {
                let theta = (u * u + v * v).sqrt();
                if theta > PI {
                    return None;
                }
                if theta < 1e-12 {
                    return Some(V3::new(0.0, 0.0, 1.0));
                }
                let s = theta.sin() / theta;
                Some(V3::new(u * s, v * s, theta.cos()))
            }
            Projection::Stereographic => Some(inv_stereo(u, v)),
            Projection::LittlePlanet => {
                let l = inv_stereo(u, v);
                // inverse of (lx, ly, lz) = (x, -z, y)
                Some(V3::new(l.x, l.z, -l.y))
            }
            Projection::Orthographic => {
                let r2 = u * u + v * v;
                if r2 > 1.0 {
                    return None;
                }
                Some(V3::new(u, v, (1.0 - r2).sqrt()))
            }
        }
    }
}

#[inline]
fn inv_stereo(u: f64, v: f64) -> V3 {
    let r2 = (u * u + v * v) / 4.0;
    let d = 1.0 + r2;
    V3::new(u / d, v / d, (1.0 - r2) / d)
}

/// Remove radial distortion: distorted centred pixel -> undistorted centred pixel.
/// Model: r_u = r_d (1 + k1 r_d^2 + k2 r_d^4), radii normalised by `rnorm` (half diagonal).
#[inline]
pub fn undistort(x: f64, y: f64, k1: f64, k2: f64, rnorm: f64) -> (f64, f64) {
    if k1 == 0.0 && k2 == 0.0 {
        return (x, y);
    }
    let r2 = (x * x + y * y) / (rnorm * rnorm);
    let s = 1.0 + k1 * r2 + k2 * r2 * r2;
    (x * s, y * s)
}

/// Apply radial distortion (inverse of [`undistort`]) with Newton iterations.
#[inline]
pub fn distort(xu: f64, yu: f64, k1: f64, k2: f64, rnorm: f64) -> Option<(f64, f64)> {
    if k1 == 0.0 && k2 == 0.0 {
        return Some((xu, yu));
    }
    let ru = (xu * xu + yu * yu).sqrt() / rnorm;
    if ru < 1e-12 {
        return Some((xu, yu));
    }
    let mut rd = ru;
    for _ in 0..8 {
        let r2 = rd * rd;
        let f = rd * (1.0 + k1 * r2 + k2 * r2 * r2) - ru;
        let df = 1.0 + 3.0 * k1 * r2 + 5.0 * k2 * r2 * r2;
        if df <= 1e-6 {
            return None;
        }
        rd -= f / df;
    }
    if !(rd.is_finite()) || rd < 0.0 {
        return None;
    }
    let s = rd / ru;
    Some((xu * s, yu * s))
}

/// Rotation from yaw (around y), pitch (around x, positive = looking up) and roll (around z),
/// in degrees. Maps view-space directions to world directions.
pub fn ypr_rotation(yaw: f64, pitch: f64, roll: f64) -> Rot {
    let ry = Rot::from_axis_angle(&V3::y_axis(), yaw.to_radians());
    let rx = Rot::from_axis_angle(&V3::x_axis(), pitch.to_radians());
    let rz = Rot::from_axis_angle(&V3::z_axis(), roll.to_radians());
    ry * rx * rz
}

/// Inverse of [`ypr_rotation`]: returns (yaw, pitch, roll) in degrees.
pub fn ypr_from_rotation(m: &Rot) -> (f64, f64, f64) {
    let c = m * V3::new(0.0, 0.0, 1.0);
    let pitch = (-c.y).clamp(-1.0, 1.0).asin();
    let yaw = c.x.atan2(c.z);
    let r0 = Rot::from_axis_angle(&V3::y_axis(), yaw) * Rot::from_axis_angle(&V3::x_axis(), pitch);
    let rz = r0.inverse() * m;
    let mz = rz.matrix();
    let roll = mz[(1, 0)].atan2(mz[(0, 0)]);
    (yaw.to_degrees(), pitch.to_degrees(), roll.to_degrees())
}

/// (yaw, pitch) in degrees that put world direction `d` at the centre of the view.
pub fn yaw_pitch_towards(d: &V3) -> (f64, f64) {
    let d = d.normalize();
    (d.x.atan2(d.z).to_degrees(), (-d.y).clamp(-1.0, 1.0).asin().to_degrees())
}

/// Find yaw/pitch (keeping roll) such that world direction `d` is seen along view direction `q`.
/// Used for "grab and drag" interaction. Starts from the given yaw/pitch.
pub fn solve_grab(d: &V3, q: &V3, yaw: f64, pitch: f64, roll: f64) -> (f64, f64) {
    let (mut y, mut p) = (yaw, pitch);
    let f = |y: f64, p: f64| -> V3 { ypr_rotation(y, p, roll) * q - d };
    for _ in 0..12 {
        let r = f(y, p);
        if r.norm() < 1e-10 {
            break;
        }
        let h = 1e-4;
        let jy = (f(y + h, p) - r) / h;
        let jp = (f(y, p + h) - r) / h;
        // 2x2 normal equations
        let a = jy.dot(&jy);
        let b = jy.dot(&jp);
        let c = jp.dot(&jp);
        let gy = jy.dot(&r);
        let gp = jp.dot(&r);
        let det = a * c - b * b;
        if det.abs() < 1e-18 {
            break;
        }
        let dy = (c * gy - b * gp) / det;
        let dp = (a * gp - b * gy) / det;
        y -= dy.clamp(-30.0, 30.0);
        p = (p - dp.clamp(-30.0, 30.0)).clamp(-90.0, 90.0);
    }
    let mut y = y % 360.0;
    if y > 180.0 {
        y -= 360.0;
    }
    if y < -180.0 {
        y += 360.0;
    }
    (y, p)
}

/// Smallest rotation taking unit vector `a` onto unit vector `b`.
pub fn rotation_between(a: &V3, b: &V3) -> Rot {
    Rot::rotation_between(a, b).unwrap_or_else(|| Rot::from_axis_angle(&V3::x_axis(), std::f64::consts::PI))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projections_roundtrip() {
        let dirs = [
            V3::new(0.1, 0.2, 0.9).normalize(),
            V3::new(-0.5, -0.3, 0.7).normalize(),
            V3::new(0.3, 0.1, 0.5).normalize(),
        ];
        for p in Projection::ALL {
            for d in &dirs {
                if let Some((u, v)) = p.forward(d) {
                    let back = p.inverse(u, v).unwrap_or_else(|| panic!("{:?} inverse failed", p));
                    assert!((back - d).norm() < 1e-6, "{:?} roundtrip {:?} -> {:?}", p, d, back);
                }
            }
        }
    }

    #[test]
    fn ypr_roundtrip() {
        for (y, p, r) in [(10.0, 20.0, 30.0), (-120.0, -45.0, 5.0), (170.0, 80.0, -90.0)] {
            let m = ypr_rotation(y, p, r);
            let (a, b, c) = ypr_from_rotation(&m);
            assert!((a - y).abs() < 1e-6 && (b - p).abs() < 1e-6 && (c - r).abs() < 1e-6, "{a} {b} {c}");
        }
    }

    #[test]
    fn grab_solves() {
        let d = V3::new(0.3, -0.2, 0.9).normalize();
        let q = V3::new(-0.1, 0.1, 1.0).normalize();
        let (y, p) = solve_grab(&d, &q, 0.0, 0.0, 0.0);
        let back = ypr_rotation(y, p, 0.0) * q;
        assert!((back - d).norm() < 1e-6);
    }

    #[test]
    fn distortion_roundtrip() {
        for (k1, k2) in [(-0.1, 0.02), (0.08, -0.01), (0.0, 0.0)] {
            for (x, y) in [(300.0, 200.0), (-700.0, 450.0)] {
                let (xu, yu) = undistort(x, y, k1, k2, 1000.0);
                let (xd, yd) = distort(xu, yu, k1, k2, 1000.0).unwrap();
                assert!((xd - x).abs() < 1e-6 && (yd - y).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn lens_roundtrip() {
        for l in LensType::ALL {
            let f = 800.0;
            for (x, y) in [(100.0, 50.0), (-300.0, 200.0), (0.0, 0.0)] {
                let r = l.ray(x, y, f);
                let (px, py) = l.project(&r, f).unwrap();
                assert!((px - x).abs() < 1e-6 && (py - y).abs() < 1e-6, "{:?}", l);
            }
        }
    }
}

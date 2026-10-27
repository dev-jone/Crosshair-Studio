use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CrosshairMode {
    Static,
    Dynamic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RgbColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl RgbColor {
    pub const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CrosshairAppearance {
    pub color: RgbColor,
    pub size: f32,
    pub thickness: f32,
    pub center_gap: f32,
    pub opacity: f32,
    pub outline_enabled: bool,
    pub outline_color: RgbColor,
    pub outline_thickness: f32,
    pub x_offset: f32,
    pub y_offset: f32,
    pub rotation_degrees: f32,
}

impl Default for CrosshairAppearance {
    fn default() -> Self {
        Self {
            color: RgbColor::new(80, 240, 120),
            size: 12.0,
            thickness: 2.0,
            center_gap: 4.0,
            opacity: 1.0,
            outline_enabled: true,
            outline_color: RgbColor::new(0, 0, 0),
            outline_thickness: 1.0,
            x_offset: 0.0,
            y_offset: 0.0,
            rotation_degrees: 0.0,
        }
    }
}

impl CrosshairAppearance {
    pub fn sanitize(&mut self) {
        self.size = finite_clamp(self.size, 1.0, 100.0, 12.0);
        self.thickness = finite_clamp(self.thickness, 1.0, 20.0, 2.0);
        self.center_gap = finite_clamp(self.center_gap, 0.0, 50.0, 4.0);
        self.opacity = finite_clamp(self.opacity, 0.05, 1.0, 1.0);
        self.outline_thickness = finite_clamp(self.outline_thickness, 0.0, 10.0, 1.0);
        self.x_offset = finite_clamp(self.x_offset, -250.0, 250.0, 0.0);
        self.y_offset = finite_clamp(self.y_offset, -250.0, 250.0, 0.0);
        self.rotation_degrees = finite_clamp(self.rotation_degrees, -180.0, 180.0, 0.0);
    }

    pub fn geometry(&self) -> CrosshairGeometry {
        let mut settings = *self;
        settings.sanitize();
        let theta = settings.rotation_degrees.to_radians();
        let (sin_theta, cos_theta) = theta.sin_cos();
        let arm_start = settings.center_gap;
        let arm_end = settings.center_gap + settings.size;

        let arm = |direction_x: f32, direction_y: f32| {
            let start_x = direction_x * arm_start;
            let start_y = direction_y * arm_start;
            let end_x = direction_x * arm_end;
            let end_y = direction_y * arm_end;
            Segment {
                start: Point {
                    x: start_x * cos_theta - start_y * sin_theta + settings.x_offset,
                    y: start_x * sin_theta + start_y * cos_theta + settings.y_offset,
                },
                end: Point {
                    x: end_x * cos_theta - end_y * sin_theta + settings.x_offset,
                    y: end_x * sin_theta + end_y * cos_theta + settings.y_offset,
                },
            }
        };

        CrosshairGeometry {
            arms: [arm(0.0, -1.0), arm(0.0, 1.0), arm(-1.0, 0.0), arm(1.0, 0.0)],
            thickness: settings.thickness,
            outline_thickness: if settings.outline_enabled {
                settings.outline_thickness
            } else {
                0.0
            },
            color: settings.color,
            opacity: settings.opacity,
            outline_color: settings.outline_color,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub start: Point,
    pub end: Point,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CrosshairGeometry {
    pub arms: [Segment; 4],
    pub thickness: f32,
    pub outline_thickness: f32,
    pub color: RgbColor,
    pub opacity: f32,
    pub outline_color: RgbColor,
}

fn finite_clamp(value: f32, minimum: f32, maximum: f32, default: f32) -> f32 {
    if value.is_finite() {
        value.clamp(minimum, maximum)
    } else {
        default
    }
}

impl Default for CrosshairMode {
    fn default() -> Self {
        Self::Dynamic
    }
}

pub fn crosshair_visible(mode: CrosshairMode, ads_held: bool, inverted: bool) -> bool {
    match mode {
        CrosshairMode::Static => true,
        CrosshairMode::Dynamic => {
            if inverted {
                ads_held
            } else {
                !ads_held
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CrosshairAppearance, CrosshairMode, crosshair_visible};

    #[test]
    fn visibility_matches_the_complete_truth_table() {
        let cases = [
            (CrosshairMode::Static, false, false, true),
            (CrosshairMode::Static, true, false, true),
            (CrosshairMode::Static, false, true, true),
            (CrosshairMode::Static, true, true, true),
            (CrosshairMode::Dynamic, false, false, true),
            (CrosshairMode::Dynamic, true, false, false),
            (CrosshairMode::Dynamic, false, true, false),
            (CrosshairMode::Dynamic, true, true, true),
        ];

        for (mode, ads_held, inverted, expected) in cases {
            assert_eq!(
                crosshair_visible(mode, ads_held, inverted),
                expected,
                "{mode:?}, ads_held={ads_held}, inverted={inverted}"
            );
        }
    }

    #[test]
    fn geometry_has_four_arms_with_a_center_gap() {
        let settings = CrosshairAppearance::default();
        let geometry = settings.geometry();

        assert_eq!(geometry.arms.len(), 4);
        assert_eq!(geometry.arms[0].start.y, -4.0);
        assert_eq!(geometry.arms[0].end.y, -16.0);
        assert_eq!(geometry.arms[3].start.x, 4.0);
        assert_eq!(geometry.arms[3].end.x, 16.0);
    }

    #[test]
    fn geometry_clamps_invalid_and_extreme_values() {
        let mut settings = CrosshairAppearance {
            size: f32::NAN,
            thickness: f32::INFINITY,
            center_gap: -100.0,
            opacity: 4.0,
            outline_thickness: f32::NEG_INFINITY,
            x_offset: 900.0,
            y_offset: -900.0,
            rotation_degrees: 3600.0,
            ..CrosshairAppearance::default()
        };
        settings.sanitize();

        assert_eq!(settings.size, 12.0);
        assert_eq!(settings.thickness, 2.0);
        assert_eq!(settings.center_gap, 0.0);
        assert_eq!(settings.opacity, 1.0);
        assert_eq!(settings.outline_thickness, 1.0);
        assert_eq!(settings.x_offset, 250.0);
        assert_eq!(settings.y_offset, -250.0);
        assert_eq!(settings.rotation_degrees, 180.0);
        assert!(settings.geometry().arms.iter().all(|arm| {
            arm.start.x.is_finite()
                && arm.start.y.is_finite()
                && arm.end.x.is_finite()
                && arm.end.y.is_finite()
        }));
    }
}

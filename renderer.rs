use crate::crosshair::{CrosshairAppearance, CrosshairGeometry, Point, Segment};
use iced::widget::canvas::{self, Frame, Geometry, Path};
use iced::{Color, Rectangle, Renderer, Theme};

#[derive(Debug, Clone, Copy)]
pub struct CrosshairRenderer {
    pub appearance: CrosshairAppearance,
    pub visible: bool,
}

impl<Message> canvas::Program<Message> for CrosshairRenderer {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        if self.visible {
            let geometry = self.appearance.geometry();
            let center = Point {
                x: bounds.width / 2.0,
                y: bounds.height / 2.0,
            };
            draw_geometry(&mut frame, geometry, center);
        }
        vec![frame.into_geometry()]
    }
}

fn draw_geometry(frame: &mut Frame, geometry: CrosshairGeometry, center: Point) {
    let foreground = color(geometry.color, geometry.opacity);
    let outline = color(geometry.outline_color, geometry.opacity);

    if geometry.outline_thickness > 0.0 {
        for arm in geometry.arms {
            fill_segment(
                frame,
                arm,
                center,
                geometry.thickness + geometry.outline_thickness * 2.0,
                outline,
            );
        }
    }
    for arm in geometry.arms {
        fill_segment(frame, arm, center, geometry.thickness, foreground);
    }
}

fn fill_segment(frame: &mut Frame, segment: Segment, center: Point, width: f32, color: Color) {
    let start = Point {
        x: center.x + segment.start.x,
        y: center.y + segment.start.y,
    };
    let end = Point {
        x: center.x + segment.end.x,
        y: center.y + segment.end.y,
    };
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let length = dx.hypot(dy);
    if length <= f32::EPSILON {
        return;
    }
    let perpendicular_x = -dy / length * width / 2.0;
    let perpendicular_y = dx / length * width / 2.0;
    let points = [
        (start.x + perpendicular_x, start.y + perpendicular_y),
        (end.x + perpendicular_x, end.y + perpendicular_y),
        (end.x - perpendicular_x, end.y - perpendicular_y),
        (start.x - perpendicular_x, start.y - perpendicular_y),
    ];
    let path = Path::new(|builder| {
        builder.move_to(iced::Point::new(points[0].0, points[0].1));
        for point in points.iter().skip(1) {
            builder.line_to(iced::Point::new(point.0, point.1));
        }
        builder.close();
    });
    frame.fill(&path, color);
}

fn color(color: crate::crosshair::RgbColor, alpha: f32) -> Color {
    Color::from_rgba8(color.red, color.green, color.blue, alpha)
}

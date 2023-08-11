#[derive(Debug)]
pub struct Lens {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Lens {
    pub fn from(start_point: (f64, f64), end_point: (f64, f64)) -> Self {
        let x_v = end_point.0 - start_point.0;
        let y_v = end_point.1 - start_point.1;

        let x;
        let y;
        let width;
        let height;
        if x_v > 0.0 && y_v > 0.0 {
            x = start_point.0;
            y = start_point.1;
            width = x_v;
            height = y_v;
        } else if x_v < 0.0 && y_v < 0.0 {
            x = end_point.0;
            y = end_point.1;
            width = x_v.abs();
            height = y_v.abs();
        } else if x_v > 0.0 && y_v < 0.0 {
            x = start_point.0;
            y = end_point.1;
            width = x_v;
            height = y_v.abs();
        } else if x_v < 0.0 && y_v > 0.0 {
            x = end_point.0;
            y = start_point.1;
            width = x_v.abs();
            height = y_v;
        } else {
            x = 0.0;
            y = 0.0;
            width = 0.0;
            height = 0.0;
        }

        let x = x.max(0.0) as f32;
        let y = y.max(0.0) as f32;
        let width = width as f32;
        let height = height as f32;

        Self {
            x,
            y,
            width,
            height,
        }
    }
}

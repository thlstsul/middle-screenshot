use anyhow::Result;
use screenshots::{Image, Screen};

use crate::lens::Lens;

/// TODO fork and change
pub trait ScreenExt {
    fn capture_lens(&self, lens: &Lens) -> Result<Image>;
}

impl ScreenExt for Screen {
    fn capture_lens(&self, lens: &Lens) -> Result<Image> {
        let x = lens.x as f32 / self.display_info.scale_factor;
        let y = lens.y as f32 / self.display_info.scale_factor;
        let w = lens.width as f32 / self.display_info.scale_factor;
        let h = lens.height as f32 / self.display_info.scale_factor;

        self.capture_area(x as i32, y as i32, w as u32, h as u32)
    }
}

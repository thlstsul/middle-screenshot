use anyhow::Result;
use screenshots::{Image, Screen};

use crate::lens::Lens;

/// TODO fork and change
pub trait ScreenExt {
    fn capture_lens(&self, lens: &Lens) -> Result<Image>;
}

impl ScreenExt for Screen {
    fn capture_lens(&self, lens: &Lens) -> Result<Image> {
        let scale_factor = self.display_info.scale_factor;
        let x = lens.x / scale_factor;
        let y = lens.y / scale_factor;
        let width = lens.width / scale_factor;
        let height = lens.height / scale_factor;

        self.capture_area(x as i32, y as i32, width as u32, height as u32)
    }
}

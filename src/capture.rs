use anyhow::Result;
use screenshots::Screen;

use crate::lens::Lens;

/// TODO fork and change
pub trait ScreenExt {
    fn capture_lens(&self, lens: &Lens) -> Result<image::RgbaImage>;
}

impl ScreenExt for Screen {
    fn capture_lens(&self, lens: &Lens) -> Result<image::RgbaImage> {
        let scale_factor = self.display_info.scale_factor;
        let x = lens.x / scale_factor;
        let y = lens.y / scale_factor;
        let width = lens.width / scale_factor;
        let height = lens.height / scale_factor;

        let img = self.capture_area(x as i32, y as i32, width as u32, height as u32)?;
        let (w, h) = (img.width(), img.height());
        let raw = img.into_raw();
        Ok(image::ImageBuffer::from_vec(w, h, raw).unwrap())
    }
}

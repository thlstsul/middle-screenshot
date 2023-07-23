use std::io::Cursor;

use anyhow::{Ok, Result};
use image::{DynamicImage, ImageBuffer, Rgb, Rgba};
use imageproc::contrast::adaptive_threshold;
use screenshots::Image;

pub trait ImageExt {
    fn rgb(&self) -> Vec<u8>;
    fn to_bmp(&self) -> Result<Vec<u8>>;
    fn to_tiff(&self) -> Result<Vec<u8>>;
}

impl ImageExt for Image {
    /// 转rgb bmp，windows剪贴板无法识别rgba原始数据的bitmap图片
    fn to_bmp(&self) -> Result<Vec<u8>> {
        let rgb: Option<ImageBuffer<Rgb<u8>, Vec<u8>>> =
            ImageBuffer::from_vec(self.width(), self.height(), self.rgb());
        let mut bmp: Vec<u8> = Vec::new();
        if let Some(rgb) = rgb {
            let img = DynamicImage::from(rgb);
            img.write_to(&mut Cursor::new(&mut bmp), image::ImageOutputFormat::Bmp)?;
        }

        Ok(bmp)
    }

    /// 转tiff， On windows, leptonica will only read tiff formatted files from memory.
    fn to_tiff(&self) -> Result<Vec<u8>> {
        let rgba: Option<ImageBuffer<Rgba<u8>, Vec<u8>>> =
            ImageBuffer::from_vec(self.width(), self.height(), self.rgba().to_vec());
        let mut tiff: Vec<u8> = Vec::new();
        if let Some(rgba) = rgba {
            let img = DynamicImage::from(rgba);
            let img = adaptive_threshold(&img.to_luma8(), 11);
            img.write_to(&mut Cursor::new(&mut tiff), image::ImageOutputFormat::Tiff)?;
        }
        Ok(tiff)
    }

    fn rgb(&self) -> Vec<u8> {
        let mut rgb = Vec::new();
        for (i, pixel) in self.rgba().iter().enumerate() {
            if (i + 1) % 4 != 0 {
                rgb.push(*pixel);
            }
        }
        rgb
    }
}

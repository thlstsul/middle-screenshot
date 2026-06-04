use std::io::Cursor;

use anyhow::{Ok, Result};
use image::RgbaImage;
use image::imageops::{FilterType, resize};
use image::{DynamicImage, GrayImage, ImageBuffer, ImageFormat, Rgb, Rgba};
use imageproc::contrast::adaptive_threshold;
use tracing::debug;

/// OCR最佳识别的文字高度约为30-40px
const OCR_OPTIMAL_TEXT_HEIGHT: u32 = 60;
/// 二值化后若黑色像素占比低于此值，认为预处理过度，回退到灰度图
const MIN_INK_RATIO: f32 = 0.005;

pub trait ImageExt {
    fn rgb(&self) -> Vec<u8>;
    fn to_bmp(&self) -> Result<Vec<u8>>;
    fn to_tiff(&self) -> Result<Vec<u8>>;
}

impl ImageExt for RgbaImage {
    /// 转rgb bmp，windows剪贴板无法识别rgba原始数据的bitmap图片
    fn to_bmp(&self) -> Result<Vec<u8>> {
        let rgb: Option<ImageBuffer<Rgb<u8>, Vec<u8>>> =
            ImageBuffer::from_vec(self.width(), self.height(), self.rgb());
        let mut bmp: Vec<u8> = Vec::new();
        if let Some(rgb) = rgb {
            let img = DynamicImage::from(rgb);
            img.write_to(&mut Cursor::new(&mut bmp), ImageFormat::Bmp)?;
        }

        Ok(bmp)
    }

    /// 转tiff， On windows, leptonica will only read tiff formatted files from memory.
    /// 经过OCR预处理优化：智能缩放、自适应阈值二值化（过度时自动回退灰度）
    fn to_tiff(&self) -> Result<Vec<u8>> {
        let rgba: Option<ImageBuffer<Rgba<u8>, Vec<u8>>> =
            ImageBuffer::from_vec(self.width(), self.height(), self.as_raw().to_vec());
        let mut tiff: Vec<u8> = Vec::new();
        if let Some(rgba) = rgba {
            let img = DynamicImage::from(rgba);
            let gray = img.to_luma8();
            let processed = preprocess_for_ocr(&gray);
            processed.write_to(&mut Cursor::new(&mut tiff), ImageFormat::Tiff)?;
        }
        Ok(tiff)
    }

    fn rgb(&self) -> Vec<u8> {
        let mut rgb = Vec::new();
        for (i, pixel) in self.as_raw().iter().enumerate() {
            if (i + 1) % 4 != 0 {
                rgb.push(*pixel);
            }
        }
        rgb
    }
}

/// 对灰度图进行OCR预处理优化
///
/// 处理流程：
/// 1. 智能缩放：截图较小时放大，使文字达到Tesseract最佳识别尺寸
/// 2. 自适应阈值二值化：15x15局部窗口
/// 3. 回退检测：若二值化后黑色像素过少（全白/过度），回退到灰度图
fn preprocess_for_ocr(gray: &GrayImage) -> DynamicImage {
    let (width, height) = gray.dimensions();
    debug!("OCR预处理前尺寸: {}x{}", width, height);

    // 1. 智能缩放：如果截图整体高度较小，内部文字可能不足30-40px
    let gray = if height < OCR_OPTIMAL_TEXT_HEIGHT {
        let scale = 2.0f32;
        let new_width = ((width as f32 * scale) as u32).max(1);
        let new_height = ((height as f32 * scale) as u32).max(1);
        debug!(
            "截图较小，放大 {:.1}x -> {}x{}",
            scale, new_width, new_height
        );
        resize(gray, new_width, new_height, FilterType::Lanczos3)
    } else {
        gray.clone()
    };

    // 2. 自适应阈值二值化
    // block_radius=7 对应 15x15 局部窗口
    // delta=0 保持中性，避免过严阈值把文字当成背景过滤掉
    let binary = adaptive_threshold(&gray, 7, 0);

    // 3. 回退检测：如果二值化结果几乎全白，说明预处理过度，回退到灰度图
    let total_pixels = binary.width() * binary.height();
    let black_pixels = binary.iter().filter(|&&p| p == 0).count() as u32;
    let ink_ratio = black_pixels as f32 / total_pixels as f32;
    debug!(
        "二值化黑色像素占比: {:.2}% (阈值 {:.1}%)",
        ink_ratio * 100.0,
        MIN_INK_RATIO * 100.0
    );

    if ink_ratio < MIN_INK_RATIO {
        debug!("二值化结果几乎全白，回退到灰度图");
        DynamicImage::from(gray)
    } else {
        DynamicImage::from(binary)
    }
}

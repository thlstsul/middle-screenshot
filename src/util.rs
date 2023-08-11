use std::fs;

use anyhow::{anyhow, Result};
use clipboard_win::{formats, set_clipboard};
use lazy_static::lazy_static;
use leptess::LepTess;
use log_error::LogError;
use screenshots::{Image, Screen};
use tracing::error;

use crate::{capture::ScreenExt, image::ImageExt, lens::Lens};

const DEFAULT_DPI: i32 = 72;
lazy_static! {
    static ref LANG: String = {
        let train_files: Vec<String> = fs::read_dir(".")
            .log_error("读取tesseract预训练模型失败")
            .unwrap()
            .filter_map(|f| {
                if let Ok(f) = f {
                    let file_name = f.file_name();
                    let file_name = file_name.to_string_lossy();
                    if file_name.ends_with(".traineddata") {
                        let file_name = file_name.trim_end_matches(".traineddata").to_string();
                        return Some(file_name);
                    }
                    None
                } else {
                    None
                }
            })
            .collect();
        if train_files.is_empty() {
            let err_msg = "请下载拷贝tesseract预训练模型至运行目录";
            error!("{err_msg}");
            panic!("{err_msg}");
        }
        train_files.join("+")
    };
}

/// 调用tesseract进行ocr
pub fn ocr(tiff: &[u8]) -> Result<String> {
    let mut tesseract = LepTess::new(None, &LANG)?;
    tesseract.set_image_from_mem(tiff)?;
    tesseract.set_fallback_source_resolution(DEFAULT_DPI);
    Ok(tesseract.get_utf8_text()?)
}

/// 截图
pub fn screenshot(lens: &Lens) -> Result<Image> {
    let screen = Screen::from_point(lens.x as i32, lens.y as i32)?;
    let image = screen.capture_lens(lens)?;
    Ok(image)
}

/// 复制图片到剪切板
pub fn copy_image(image: &Image) -> Result<()> {
    let bmp = image.to_bmp()?;
    set_clipboard(formats::Bitmap, bmp).map_err(|e| anyhow!(e))
}

/// 复制文字到剪切板
pub fn copy_text(text: String) -> Result<()> {
    set_clipboard(formats::Unicode, text).map_err(|e| anyhow!(e))
}

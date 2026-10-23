use std::{fs, thread, time::Duration};

use anyhow::{Result, anyhow};
use clipboard_win::{formats, set_clipboard};
use lazy_static::lazy_static;
use leptess::LepTess;
use log_error::LogError;
use screenshots::Screen;
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
    tesseract.set_variable(leptess::Variable::PreserveInterwordSpaces, "1")?;
    Ok(tesseract.get_utf8_text()?)
}

/// 截图
pub fn screenshot(lens: &Lens) -> Result<image::RgbaImage> {
    let screen = Screen::from_point(lens.x as i32, lens.y as i32)?;
    let image = screen.capture_lens(lens)?;
    Ok(image)
}

/// 复制图片到剪切板。剪贴板可能被其它进程（剪贴板工具、IM 等）短暂占用，失败时退避重试
pub fn copy_image(image: &image::RgbaImage) -> Result<()> {
    let bmp = image.to_bmp()?;
    let mut err = None;
    for i in 0..15 {
        match set_clipboard(formats::Bitmap, bmp.clone()) {
            Ok(()) => return Ok(()),
            Err(e) => {
                err = Some(e);
                // 首次立即重试，之后逐次加大间隔（总计约 500ms）
                thread::sleep(Duration::from_millis(10 + i * 8));
            }
        }
    }
    Err(anyhow!(err.unwrap()))
}

/// 复制文字到剪切板
pub fn copy_text(text: String) -> Result<()> {
    set_clipboard(formats::Unicode, text).map_err(|e| anyhow!(e))
}

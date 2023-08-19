use std::{
    sync::{
        mpsc::{channel, Receiver},
        Arc,
    },
    thread,
};

use anyhow::{anyhow, Result};
use eframe::{
    egui::{self, Ui},
    emath,
    epaint::{Color32, Pos2, Rect, Shape, Stroke, Vec2},
    IconData, Renderer,
};
use egui_extras::RetainedImage;
use image::{open, ImageBuffer, Rgba};
use lazy_static::lazy_static;
use log_error::LogError;
use screenshots::Image;

use crate::{
    image::ImageExt,
    lens::Lens,
    util::{copy_text, ocr},
};

const BORDER_WIDTH: f32 = 5.0;
const SIZE_DIFF: Vec2 = egui::vec2(BORDER_WIDTH * 2.0, BORDER_WIDTH * 2.0);
lazy_static! {
    static ref ICON: ImageBuffer<Rgba<u8>, Vec<u8>> = open("middle-screenshot.ico")
        .log_error("读取ICON失败")
        .unwrap()
        .into_rgba8();
}

pub struct Screenshot {
    image: Arc<Image>,
    size: Vec2,
    texture: Option<RetainedImage>,
    loading: bool,
    finish_channel: Option<Receiver<bool>>,
}

impl Screenshot {
    pub fn new(image: Image, size: Vec2) -> Self {
        Self {
            image: Arc::new(image),
            size,
            texture: None,
            loading: false,
            finish_channel: None,
        }
    }

    fn ocr(image: &Image) {
        image
            .to_tiff()
            .and_then(|tiff| ocr(&tiff))
            .and_then(copy_text)
            .log_error("OCR失败");
    }

    fn show_load(ui: &mut Ui) {
        let color = if ui.visuals().dark_mode {
            Color32::from_additive_luminance(196)
        } else {
            Color32::from_black_alpha(240)
        };
        ui.ctx().request_repaint();
        let time = ui.input(|i| i.time);

        let desired_size = ui.available_size();
        let (_id, rect) = ui.allocate_space(desired_size);

        let to_screen =
            emath::RectTransform::from_to(Rect::from_x_y_ranges(0.0..=1.0, -1.0..=1.0), rect);

        let mut shapes = vec![];

        for &mode in &[2, 3, 5] {
            let mode = mode as f64;
            let n = 120;
            let speed = 1.5;

            let points: Vec<Pos2> = (0..=n)
                .map(|i| {
                    let t = i as f64 / (n as f64);
                    let amp = (time * speed * mode).sin() / mode;
                    let y = amp * (t * std::f64::consts::TAU / 2.0 * mode).sin();
                    to_screen * egui::pos2(t as f32, y as f32)
                })
                .collect();

            let thickness = 10.0 / mode as f32;
            shapes.push(Shape::line(points, Stroke::new(thickness, color)));
        }

        ui.painter().extend(shapes);
    }
}

impl eframe::App for Screenshot {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // no margin
        let custom_frame = egui::Frame::default();
        egui::CentralPanel::default()
            .frame(custom_frame)
            .show(ctx, |ui| {
                ui.with_layout(
                    egui::Layout::centered_and_justified(egui::Direction::TopDown),
                    |ui| {
                        if self.loading {
                            Self::show_load(ui);
                        } else {
                            let texture = self.texture.get_or_insert_with(|| {
                                let size =
                                    [self.image.width() as usize, self.image.height() as usize];
                                RetainedImage::from_color_image(
                                    "sceenshot",
                                    egui::ColorImage::from_rgba_unmultiplied(
                                        size,
                                        self.image.rgba(),
                                    ),
                                )
                            });

                            //TODO 比例已经做到完全一致，但有时还是不清晰
                            let image_resp = texture.show_size(ui, self.size);
                            let image_resp = image_resp.interact(egui::Sense::click());
                            if image_resp.double_clicked() {
                                self.loading = true;
                                let image = Arc::clone(&self.image);
                                let (tx, rx) = channel();
                                thread::spawn(move || {
                                    Self::ocr(&image);
                                    tx.send(true).log_error("OCR完成，但发送消息错误");
                                });
                                self.finish_channel = Some(rx);
                            }
                        }
                        if let Some(rx) = &self.finish_channel {
                            if let Ok(true) = rx.try_recv() {
                                frame.close();
                            }
                        }
                    },
                )
            });
    }
}

pub fn create_window(image: Image, lens: &Lens, scale_factor: f32) -> Result<()> {
    let size = egui::vec2(
        image.width() as f32 / scale_factor,
        image.height() as f32 / scale_factor,
    );
    let position = egui::pos2(
        lens.x / scale_factor - BORDER_WIDTH,
        lens.y / scale_factor - BORDER_WIDTH,
    );
    let options = eframe::NativeOptions {
        always_on_top: true,
        decorated: false,
        transparent: true,
        icon_data: Some(IconData {
            rgba: ICON.to_vec(),
            width: ICON.width(),
            height: ICON.height(),
        }),
        initial_window_size: Some(size + SIZE_DIFF),
        initial_window_pos: Some(position),
        renderer: Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "中键截屏",
        options,
        Box::new(move |_| Box::new(Screenshot::new(image, size))),
    )
    .map_err(|e| anyhow!("{}", e))
}

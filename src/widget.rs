use std::{
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, Sender, channel},
    },
    thread,
    time::Duration,
};

use eframe::{
    Renderer,
    egui::{self, Ui, ViewportId},
    epaint::{Pos2, Vec2},
};
use image::RgbaImage;
use image::{ImageBuffer, Rgba, open};
use lazy_static::lazy_static;
use log_error::LogError;

use crate::{
    event::Event,
    image::ImageExt,
    lens::Lens,
    util::{copy_image, copy_text, ocr, screenshot},
};

const MIN_WIDTH: f32 = 10.0;
const MIN_HEIGHT: f32 = 5.0;
lazy_static! {
    static ref ICON: ImageBuffer<Rgba<u8>, Vec<u8>> = open("middle-screenshot.ico")
        .log_error("读取ICON失败")
        .unwrap()
        .into_rgba8();
    static ref SQUARE_WINDOWS: Mutex<Vec<isize>> = Mutex::new(Vec::new());
}

#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmSetWindowAttribute(hwnd: isize, attr: u32, value: *const u32, size: u32) -> i32;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn EnumWindows(callback: Option<extern "system" fn(isize, isize) -> i32>, lp: isize) -> bool;
    fn GetWindowThreadProcessId(hwnd: isize, pid: *mut u32) -> u32;
}

extern "system" fn square_corner_cb(hwnd: isize, _lp: isize) -> i32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    if pid == std::process::id() {
        let mut seen = SQUARE_WINDOWS.lock().unwrap();
        if !seen.contains(&hwnd) {
            // DWMWA_WINDOW_CORNER_PREFERENCE=33，DWMWCP_DONOTROUND=1：关闭 Win11 圆角
            let pref: u32 = 1;
            unsafe { DwmSetWindowAttribute(hwnd, 33, &pref, 4) };
            seen.push(hwnd);
        }
    }
    1
}

/// 关闭本进程所有顶层窗口的圆角（Win11 默认给顶层窗口加圆角）
fn square_window_corners() {
    unsafe { EnumWindows(Some(square_corner_cb), 0) };
}

/// 已截取的图片结果窗口数据
struct ResultShot {
    image: Arc<RgbaImage>,
    size: Vec2,
    position: Pos2,
    texture: Option<egui::TextureHandle>,
    /// 正在 OCR（双击后触发，完成后关闭窗口）
    loading: bool,
    /// 后台 OCR 线程完成通知
    ocr_done: Option<Receiver<()>>,
}

/// 松开中键后的截图流程状态
enum Pending {
    /// 等待 N 帧再截图，确保带选区边框的最后一帧预览已从屏幕清除，避免被截进图里
    Countdown(Lens, u32),
    /// 后台线程正在抓屏/写剪贴板
    InFlight,
}

/// 后台截图线程回传的消息：(截图代号, 抓屏+剪贴板结果, 窗口尺寸, 窗口位置)
type CaptureMsg = (u32, anyhow::Result<RgbaImage>, Vec2, Pos2);

/// 单事件循环宿主：负责鼠标事件消费、实时预览与截图结果展示
pub struct AppHost {
    rx: Receiver<Event>,
    capture_tx: Sender<CaptureMsg>,
    capture_rx: Receiver<CaptureMsg>,
    scale_factor: f32,
    /// 鼠标当前位置（物理像素）
    position: (f64, f64),
    /// 拖动起点，Some 表示正在按住中键
    start_point: Option<(f64, f64)>,
    /// 当前拖动选区，None 表示区域过小
    preview_lens: Option<Lens>,
    /// 截图代号：每次按下中键自增，用于丢弃上一轮尚在后台执行的过期截图
    capture_gen: u32,
    pending: Option<Pending>,
    result: Option<ResultShot>,
    /// 上一次窗口几何：隐藏期间保持不变，避免显示瞬间在 (0,0) 以旧尺寸闪现
    last_geometry: Option<(Pos2, Vec2)>,
    /// 是否已对本进程窗口应用直角样式
    corners_squared: bool,
}

impl AppHost {
    pub fn new(rx: Receiver<Event>, scale_factor: f32) -> Self {
        let (capture_tx, capture_rx) = channel();
        Self {
            rx,
            capture_tx,
            capture_rx,
            scale_factor,
            position: (0.0, 0.0),
            start_point: None,
            preview_lens: None,
            capture_gen: 0,
            pending: None,
            result: None,
            last_geometry: None,
            corners_squared: false,
        }
    }

    fn consume_events(&mut self) {
        while let Ok(e) = self.rx.try_recv() {
            match e {
                Event::Start => {
                    tracing::info!("中键按下 @({},{})", self.position.0, self.position.1);
                    // 旧结果窗口置顶常显，若不关闭会被截进新图；
                    // 代号自增使仍在后台执行的上一轮截图结果作废
                    self.result = None;
                    self.capture_gen = self.capture_gen.wrapping_add(1);
                    self.pending = None;
                    self.start_point = Some(self.position);
                }
                Event::Move(x, y) => {
                    self.position = (x, y);
                    // 按住中键拖动时实时更新预览选区
                    if let Some(start) = self.start_point {
                        let lens = Lens::from(start, self.position);
                        self.preview_lens =
                            (lens.width > MIN_WIDTH && lens.height > MIN_HEIGHT).then_some(lens);
                    }
                }
                Event::End => {
                    tracing::info!("中键松开");
                    if let Some(start) = self.start_point.take() {
                        let lens = Lens::from(start, self.position);
                        tracing::info!(
                            "选区 {}x{} @({},{})",
                            lens.width,
                            lens.height,
                            lens.x,
                            lens.y
                        );
                        if lens.width > MIN_WIDTH && lens.height > MIN_HEIGHT {
                            self.pending = Some(Pending::Countdown(lens, 2));
                        }
                    }
                    self.preview_lens = None;
                }
            }
        }
    }

    /// 在后台线程执行抓屏与剪贴板写入（剪贴板重试最多阻塞约 500ms，不能卡住 UI 事件循环）
    fn spawn_capture(&mut self, lens: Lens) {
        let scale = self.scale_factor;
        let size = egui::vec2(lens.width / scale, lens.height / scale);
        let position = egui::pos2(lens.x / scale, lens.y / scale);
        let shot_gen = self.capture_gen;
        let tx = self.capture_tx.clone();
        thread::spawn(move || {
            let outcome = match screenshot(&lens) {
                Ok(image) => {
                    tracing::info!("抓屏完成 {}x{}，写入剪贴板", image.width(), image.height());
                    copy_image(&image).map(|_| image)
                }
                Err(e) => Err(e),
            };
            let _ = tx.send((shot_gen, outcome, size, position));
        });
        self.pending = Some(Pending::InFlight);
    }

    /// 非阻塞地推进截图流程（倒计时 / 收取后台线程结果）
    fn poll_pending(&mut self) {
        match self.pending.take() {
            Some(Pending::Countdown(lens, countdown)) => {
                if countdown > 0 {
                    self.pending = Some(Pending::Countdown(lens, countdown - 1));
                } else {
                    self.spawn_capture(lens);
                }
            }
            Some(Pending::InFlight) => {
                // 只保留最新消息（正常在途最多一个）
                let mut latest = None;
                while let Ok(msg) = self.capture_rx.try_recv() {
                    latest = Some(msg);
                }
                if let Some((shot_gen, outcome, size, position)) = latest {
                    if shot_gen == self.capture_gen {
                        match outcome {
                            Ok(image) => {
                                tracing::info!("截图完成，打开结果窗口");
                                self.result = Some(ResultShot {
                                    image: Arc::new(image),
                                    size,
                                    position,
                                    texture: None,
                                    loading: false,
                                    ocr_done: None,
                                });
                            }
                            Err(e) => tracing::error!("截图失败: {e}"),
                        }
                    } else {
                        tracing::debug!("丢弃过期截图结果（代号 {shot_gen} 已失效）");
                    }
                } else {
                    // 后台仍在执行，下一帧继续收
                    self.pending = Some(Pending::InFlight);
                }
            }
            None => {}
        }
    }

    /// 截图窗口：按住中键拖动时为半透明选区预览（窗口大小=选区大小），
    /// 松开后在同一窗口显示抓取的截图，双击关闭
    fn show_shot(&mut self, ctx: &egui::Context, dragging: bool) {
        // 几何：有结果显示结果图，拖动中跟随选区；其余时刻窗口隐藏
        let geometry = if let Some(r) = self.result.as_ref() {
            Some((r.position, r.size))
        } else if dragging && let Some(lens) = &self.preview_lens {
            Some((
                egui::pos2(lens.x / self.scale_factor, lens.y / self.scale_factor),
                egui::vec2(
                    lens.width / self.scale_factor,
                    lens.height / self.scale_factor,
                ),
            ))
        } else {
            None
        };
        let showing_result = self.result.is_some();
        // 隐藏期间沿用上次几何：若每次隐藏都把窗口重置到 (0,0) 1x1，
        // 显示瞬间会先在左上角闪现再跳到选区位置
        if let Some(g) = geometry {
            self.last_geometry = Some(g);
        }
        let (position, size) = geometry
            .or(self.last_geometry)
            .unwrap_or((Pos2::ZERO, egui::vec2(1.0, 1.0)));
        ctx.show_viewport_immediate(
            ViewportId::from_hash_of("shot"),
            egui::ViewportBuilder::default()
                .with_title("中键截屏")
                .with_always_on_top()
                .with_decorations(false)
                .with_transparent(true)
                .with_resizable(false)
                .with_taskbar(false)
                .with_active(false)
                // 创建时隐藏，可见性随拖动/结果状态切换（命令只在变化时真正下发），
                // 空闲时不覆盖屏幕，不会被 Windows 判定为全屏应用（免打扰）
                .with_visible(false)
                .with_icon(egui::IconData {
                    rgba: ICON.to_vec(),
                    width: ICON.width(),
                    height: ICON.height(),
                })
                .with_position(position)
                .with_inner_size(size),
            |ui, _class| {
                if !self.corners_squared {
                    square_window_corners();
                    self.corners_squared = true;
                }
                let ctx = ui.ctx();
                // 预览阶段鼠标穿透（不干扰拖动）；结果显示阶段需响应双击
                ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(!showing_result));
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(geometry.is_some()));
                let Some((_, size)) = geometry else {
                    return;
                };
                if showing_result && let Some(r) = self.result.as_mut() {
                    if r.loading {
                        // OCR 进行中：显示加载动画并等待后台完成
                        ui.with_layout(
                            egui::Layout::centered_and_justified(egui::Direction::TopDown),
                            |ui| {
                                ui.add(egui::Spinner::new().size(24.0));
                            },
                        );
                        ui.ctx().request_repaint();
                        if let Some(rx) = &r.ocr_done
                            && rx.try_recv() == Ok(())
                        {
                            self.result = None;
                        }
                        return;
                    }
                    let texture = r.texture.get_or_insert_with(|| {
                        let size = [r.image.width() as usize, r.image.height() as usize];
                        ui.ctx().load_texture(
                            "screenshot",
                            egui::ColorImage::from_rgba_unmultiplied(size, r.image.as_raw()),
                            egui::TextureOptions::default(),
                        )
                    });
                    //TODO 比例已经做到完全一致，但有时还是不清晰
                    let image_resp = ui.image(egui::load::SizedTexture::new(texture.id(), size));
                    let image_resp = image_resp.interact(egui::Sense::click());
                    if image_resp.double_clicked() {
                        // 双击开始 OCR：转 tiff -> tesseract 识别 -> 文本写入剪贴板
                        let image = Arc::clone(&r.image);
                        let (tx, rx) = channel();
                        thread::spawn(move || {
                            image
                                .to_tiff()
                                .and_then(|tiff| ocr(&tiff))
                                .and_then(copy_text)
                                .log_error("OCR失败");
                            let _ = tx.send(());
                        });
                        r.loading = true;
                        r.ocr_done = Some(rx);
                    }
                } else {
                    // 预览阶段不绘制任何内容：窗口本身（半透明、选区大小）即预览区域
                    let _ = ui;
                }
            },
        );
    }
}

impl eframe::App for AppHost {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.consume_events();

        let dragging = self.start_point.is_some();
        // 截图窗口：拖动时选区预览，有结果时显示截图，空闲隐藏
        self.show_shot(&ctx, dragging);
        self.poll_pending();

        let busy = dragging || self.result.is_some() || self.pending.is_some();
        ui.ctx()
            .request_repaint_after(Duration::from_millis(if busy { 16 } else { 33 }));
    }
}

pub fn base_options(viewport: egui::ViewportBuilder) -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport,
        // Glow(OpenGL) 渲染器：wgpu 在 Windows 上透明窗口不可用——DX12 对 HWND
        // 表面只支持 Opaque alpha mode（透明会变黑），Vulkan 又会触发 Intel 核显
        // igvk64.dll 驱动崩溃；OpenGL 像素格式原生支持 alpha，透明窗口正常工作
        renderer: Renderer::Glow,
        ..Default::default()
    }
}

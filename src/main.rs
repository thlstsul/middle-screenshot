#![windows_subsystem = "windows"]
use std::{
    collections::HashMap,
    fs,
    sync::atomic::{AtomicBool, Ordering},
    thread,
};

use ::image::{open, ImageBuffer, Rgba};
use anyhow::{anyhow, Result};
use clipboard_win::{formats, set_clipboard};
use event::Event;
use lazy_static::lazy_static;
use log_error::*;
use rdev::{Button, EventType};
use render::State;
use screenshots::{Image, Screen};
use time::{macros::format_description, UtcOffset};
use tracing::{error, info};
use tracing_subscriber::fmt::time::OffsetTime;
use tray_icon::{TrayEvent, TrayIconBuilder};
use winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{EventLoopBuilder, EventLoopProxy, EventLoopWindowTarget},
    window::{WindowBuilder, WindowId, WindowLevel},
};

use crate::{capture::ScreenExt, image::ImageExt, lens::Lens};

mod capture;
mod event;
mod image;
mod lens;
mod render;

const MIN_WIDTH: u32 = 10;
const MIN_HEIGHT: u32 = 10;
static PAUSED: AtomicBool = AtomicBool::new(false);
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
    static ref ICON: ImageBuffer<Rgba<u8>, Vec<u8>> = open("middle-screenshot.ico")
        .log_error("读取ICON失败")
        .unwrap()
        .into_rgba8();
}

/// 拦截鼠标事件
fn listen(event_tx: EventLoopProxy<Event>) {
    rdev::grab(move |event| {
        if PAUSED.load(Ordering::Relaxed) {
            return Some(event);
        }
        let event_mapper = match event.event_type {
            EventType::ButtonPress(Button::Middle) => Some(Event::Start),
            EventType::ButtonRelease(Button::Middle) => Some(Event::End),
            EventType::MouseMove { x, y } => Some(Event::Move(x, y)),
            _ => None,
        };
        if let Some(mouse_event) = event_mapper {
            let bool = mouse_event == Event::Start || mouse_event == Event::End;
            event_tx
                .send_event(mouse_event)
                .log_error("发送鼠标事件失败");
            if bool {
                None
            } else {
                Some(event)
            }
        } else {
            Some(event)
        }
    })
    .log_error_with(|e| format!("鼠标监听失败{e:?}"));
}

/// 暂停或恢复
fn pause_or_resume(event_tx: EventLoopProxy<Event>) {
    while TrayEvent::receiver().recv().is_ok() {
        if PAUSED.swap(false, Ordering::Relaxed) {
            info!("恢复");
            event_tx
                .send_event(Event::Resume)
                .log_error("发送恢复事件失败");
        } else {
            PAUSED.swap(true, Ordering::Relaxed);
            info!("暂停");
            event_tx
                .send_event(Event::Pause)
                .log_error("发送暂停事件失败");
        }
    }
}

// 截图并保存剪切板
fn screenshot(lens: &Lens) -> anyhow::Result<Image> {
    let screen = Screen::from_point(lens.x, lens.y)?;

    let image = screen.capture_lens(lens)?;
    let bmp = image.to_bmp()?;
    set_clipboard(formats::Bitmap, bmp).map_err(|e| anyhow!(e))?;
    Ok(image)
}

fn create_window(
    event_loop: &EventLoopWindowTarget<Event>,
    image: Image,
    size: PhysicalSize<u32>,
    position: PhysicalPosition<i32>,
) -> Result<State> {
    let window = WindowBuilder::new()
        .with_title("中键截屏（OCR）")
        .with_window_icon(get_window_icon().ok())
        .with_inner_size(size)
        .with_position(position)
        .with_window_level(WindowLevel::AlwaysOnTop)
        .with_decorations(false)
        .with_resizable(false)
        .build(event_loop)?;
    Ok(pollster::block_on(async {
        State::new(window, image, size).await
    }))
}

fn get_tray_icon() -> std::result::Result<tray_icon::icon::Icon, tray_icon::icon::BadIcon> {
    tray_icon::icon::Icon::from_rgba(ICON.to_vec(), ICON.width(), ICON.height())
}

fn get_window_icon() -> std::result::Result<winit::window::Icon, winit::window::BadIcon> {
    winit::window::Icon::from_rgba(ICON.to_vec(), ICON.width(), ICON.height())
}

fn main() -> Result<()> {
    let file_appender = tracing_appender::rolling::never(".", "middle-screenshot.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    let offset = UtcOffset::current_local_offset().expect("should get local offset!");
    let timer = OffsetTime::new(
        offset,
        format_description!("[year]-[month]-[day] [hour]:[minute]:[second]"),
    );
    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_timer(timer)
        .with_ansi(false)
        .init();

    let event_loop = EventLoopBuilder::<Event>::with_user_event().build();
    let mut tray_icon = TrayIconBuilder::new()
        .with_tooltip("中键截屏")
        .with_icon(get_tray_icon()?)
        .build()?;

    let mouse_event_tx: EventLoopProxy<Event> = event_loop.create_proxy();
    let tray_event_tx = mouse_event_tx.clone();
    let _mouse_handle = thread::spawn(|| listen(mouse_event_tx));
    let _tray_handle = thread::spawn(|| pause_or_resume(tray_event_tx));

    let mut position = (0.0f64, 0.0f64);
    let mut start_point = None;
    let mut windows: HashMap<WindowId, State> = HashMap::new();

    event_loop.run(move |event, event_loop, control_flow| {
        control_flow.set_wait();

        match event {
            winit::event::Event::WindowEvent {
                window_id,
                event: WindowEvent::CloseRequested,
                ..
            } => {
                windows.remove(&window_id);
            }
            winit::event::Event::WindowEvent {
                window_id,
                event:
                    WindowEvent::MouseInput {
                        state: ElementState::Released,
                        button: MouseButton::Right,
                        ..
                    },
                ..
            } => {
                if let Some(state) = windows.get(&window_id) {
                    let text = state.ocr();
                    if let Ok(text) = text {
                        if let Err(e) = set_clipboard(formats::Unicode, text) {
                            error!("复制失败：{:?}", e);
                        }
                    } else {
                        error!("OCR失败：{:?}", text.err());
                    }

                    windows.remove(&window_id);
                }
            }
            winit::event::Event::UserEvent(event) => match event {
                Event::Start => {
                    start_point = Some(position);
                }
                Event::Move(x, y) => {
                    position = (x, y);
                }
                Event::End => {
                    if let Some(start) = start_point {
                        let lens = Lens::from(start, position);

                        if let Lens {
                            x,
                            y,
                            width: width @ MIN_WIDTH..,
                            height: height @ MIN_HEIGHT..,
                        } = lens
                        {
                            let image: Result<Image, anyhow::Error> = screenshot(&lens);
                            if let Ok(image) = image {
                                let state = create_window(
                                    event_loop,
                                    image,
                                    PhysicalSize { width, height },
                                    PhysicalPosition { x, y },
                                );
                                if let Ok(mut state) = state {
                                    if state.render().is_ok() {
                                        windows.insert(state.get_id(), state);
                                    }
                                } else {
                                    error!("创建窗口失败：{:?}", state.err());
                                }
                            } else {
                                error!("截图失败：{:?}", image.err());
                            }
                        }
                        start_point = None;
                    }
                }
                Event::Pause => {
                    tray_icon
                        .set_tooltip(Some("中键截屏（关）"))
                        .log_error("变更TIP失败");
                }
                Event::Resume => {
                    tray_icon
                        .set_tooltip(Some("中键截屏"))
                        .log_error("变更TIP失败");
                }
            },
            _ => (),
        }
    })
}

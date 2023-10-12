#![windows_subsystem = "windows"]
use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread,
};

use anyhow::Result;
use event::Event;
use log_error::*;
use rdev::{Button, EventType};
use screenshots::Image;
use time::{macros::format_description, UtcOffset};
use tracing::info;
use tracing_subscriber::fmt::time::OffsetTime;
use tray_icon::{TrayIconBuilder, TrayIconEvent};
use winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{EventLoopBuilder, EventLoopProxy},
};

use crate::lens::Lens;
use crate::windows::Windows;

mod capture;
mod event;
mod image;
mod lens;
mod render;
mod util;
mod windows;

const MIN_WIDTH: u32 = 10;
const MIN_HEIGHT: u32 = 10;
static PAUSED: AtomicBool = AtomicBool::new(false);

/// 拦截鼠标事件
fn listen(event_tx: EventLoopProxy<Event>) {
    rdev::grab(move |event| {
        if PAUSED.load(Ordering::Relaxed) {
            return Some(event);
        }
        let event_mapper = match event.event_type {
            EventType::ButtonPress(Button::Middle) => Some(Event::Start),
            EventType::ButtonRelease(Button::Middle) => Some(Event::End),
            // EventType::KeyPress(Key::ControlRight) => Some(Event::Start),
            // EventType::KeyRelease(Key::ControlRight) => Some(Event::End),
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
    while TrayIconEvent::receiver().recv().is_ok() {
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
fn screenshot(lens: &Lens) -> Result<Image> {
    let image = util::screenshot(lens)?;
    util::copy_image(&image)?;
    Ok(image)
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
    let tray_icon = TrayIconBuilder::new()
        .with_tooltip("中键截屏")
        .with_icon(util::get_tray_icon()?)
        .build()?;

    let mouse_event_tx: EventLoopProxy<Event> = event_loop.create_proxy();
    let tray_event_tx = mouse_event_tx.clone();
    let window_event_tx = mouse_event_tx.clone();
    let _mouse_handle = thread::spawn(|| listen(mouse_event_tx));
    let _tray_handle = thread::spawn(|| pause_or_resume(tray_event_tx));

    let mut position = (0.0f64, 0.0f64);
    let mut start_point = None;
    let mut windows = Windows::new(window_event_tx);

    event_loop.run(move |event, event_loop, control_flow| {
        control_flow.set_wait();

        match event {
            winit::event::Event::WindowEvent {
                window_id,
                event: WindowEvent::CloseRequested,
                ..
            } => {
                windows.destroy(&window_id);
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
                windows.ocr(&window_id).log_error("OCR失败");
            }
            winit::event::Event::UserEvent(event) => match event {
                Event::Start => {
                    if start_point.is_none() {
                        start_point = Some(position);
                    }
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
                            screenshot(&lens)
                                .and_then(|image| {
                                    windows.create(
                                        event_loop,
                                        image,
                                        PhysicalSize { width, height },
                                        PhysicalPosition { x, y },
                                    )
                                })
                                .log_error("截图失败");
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
                Event::Close(window_id) => {
                    windows.destroy(&window_id);
                }
                Event::Redraw(window_id) => {
                    windows.redraw(window_id).log_error("重绘失败");
                }
            },
            _ => (),
        }
    })
}

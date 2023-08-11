#![windows_subsystem = "windows"]
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{channel, Sender},
    },
    thread,
};

use ::image::{open, ImageBuffer, Rgba};
use anyhow::Result;
use event::Event;
use lazy_static::lazy_static;
use log_error::*;
use rdev::{Button, EventType};
use screenshots::Screen;
use time::{macros::format_description, UtcOffset};
use tracing::info;
use tracing_subscriber::fmt::time::OffsetTime;
use tray_icon::{TrayEvent, TrayIconBuilder};

use crate::{
    lens::Lens,
    util::{copy_image, screenshot},
    widget::create_window,
};

mod capture;
mod event;
mod image;
mod lens;
mod util;
mod widget;

const MIN_WIDTH: f32 = 10.0;
const MIN_HEIGHT: f32 = 5.0;
static PAUSED: AtomicBool = AtomicBool::new(false);
lazy_static! {
    static ref ICON: ImageBuffer<Rgba<u8>, Vec<u8>> = open("middle-screenshot.ico")
        .log_error("读取ICON失败")
        .unwrap()
        .into_rgba8();
}

/// 拦截鼠标事件
fn listen(event_tx: Sender<Event>) {
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
            event_tx.send(mouse_event).log_error("发送鼠标事件失败");
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
fn pause_or_resume(event_tx: Sender<Event>) {
    while TrayEvent::receiver().recv().is_ok() {
        if PAUSED.swap(false, Ordering::Relaxed) {
            info!("恢复");
            event_tx.send(Event::Resume).log_error("发送恢复事件失败");
        } else {
            PAUSED.swap(true, Ordering::Relaxed);
            info!("暂停");
            event_tx.send(Event::Pause).log_error("发送暂停事件失败");
        }
    }
}

fn get_tray_icon() -> std::result::Result<tray_icon::icon::Icon, tray_icon::icon::BadIcon> {
    tray_icon::icon::Icon::from_rgba(ICON.to_vec(), ICON.width(), ICON.height())
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

    let (mouse_event_tx, rx) = channel();
    let mut tray_icon = TrayIconBuilder::new()
        .with_tooltip("中键截屏")
        .with_icon(get_tray_icon()?)
        .build()?;

    let scale_factor: f32 = Screen::from_point(0, 0)?.display_info.scale_factor;

    let tray_event_tx = mouse_event_tx.clone();
    let _mouse_handle = thread::spawn(|| listen(mouse_event_tx));
    let _tray_handle = thread::spawn(|| pause_or_resume(tray_event_tx));

    let mut position = (0.0f64, 0.0f64);
    let mut start_point = None;
    while let Ok(e) = rx.recv() {
        match e {
            Event::Start => {
                start_point = Some(position);
            }
            Event::Move(x, y) => {
                position = (x, y);
            }
            Event::End => {
                if let Some(start) = start_point {
                    let lens = Lens::from(start, position);
                    info!("physical: {lens:?}");

                    if lens.width > MIN_WIDTH && lens.height > MIN_HEIGHT {
                        screenshot(&lens)
                            .and_then(|image| {
                                copy_image(&image).and(create_window(image, &lens, scale_factor))
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
        }
    }
    Ok(())
}

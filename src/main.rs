#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use std::{
    process::exit,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{Sender, channel},
    },
    thread,
};

use anyhow::{Result, anyhow};
use eframe::egui;
use log_error::*;
use rdev::{Button, EventType};
use time::{UtcOffset, macros::format_description};
use tracing_subscriber::{
    EnvFilter, fmt::time::OffsetTime, layer::SubscriberExt, util::SubscriberInitExt,
};
use tray_item::{IconSource, TrayItem};

use crate::{
    event::Event,
    widget::{AppHost, base_options},
};

mod capture;
mod event;
mod image;
mod lens;
mod util;
mod widget;

static PAUSED: AtomicBool = AtomicBool::new(false);

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
            if bool { None } else { Some(event) }
        } else {
            Some(event)
        }
    })
    .log_error_with(|e| format!("鼠标监听失败{e:?}"));
}

fn main() -> Result<()> {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let offset = UtcOffset::current_local_offset().expect("should get local offset!");
    let timer = OffsetTime::new(
        offset,
        format_description!("[year]-[month]-[day] [hour]:[minute]:[second]"),
    );

    let file_appender = tracing_appender::rolling::never(".", "middle-screenshot.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking)
        .with_timer(timer.clone())
        .with_ansi(false);

    #[cfg(debug_assertions)]
    {
        let stdout_layer = tracing_subscriber::fmt::layer()
            .with_timer(timer)
            .with_ansi(true);
        tracing_subscriber::registry()
            .with(env_filter)
            .with(stdout_layer)
            .with(file_layer)
            .init();
    }

    #[cfg(not(debug_assertions))]
    {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(file_layer)
            .init();
    }

    let (mouse_event_tx, rx) = channel();
    let mut tray_icon = TrayItem::new("中键截屏", IconSource::Resource("exe-icon"))?;
    tray_icon.add_label("中键截屏")?;
    tray_icon.add_menu_item("暂停", || {
        PAUSED.swap(true, Ordering::Relaxed);
    })?;
    tray_icon.inner_mut().add_separator()?;
    tray_icon.add_menu_item("恢复", || {
        PAUSED.swap(false, Ordering::Relaxed);
    })?;
    tray_icon.inner_mut().add_separator()?;
    tray_icon.add_menu_item("退出", || {
        exit(0);
    })?;

    let scale_factor = screenshots::Screen::from_point(0, 0)?
        .display_info
        .scale_factor;

    let _mouse_handle = thread::spawn(|| listen(mouse_event_tx));

    // 单事件循环：主 viewport 缩为 1x1 移出屏幕，预览与结果窗口均为其 viewport。
    // 主窗口必须保持 OS 级可见——eframe 只在“自身或后代可见”时才运行 UI pass，
    // 完全隐藏会导致 App::ui 停转、鼠标事件无人消费；1x1 离屏保证用户看不见它
    let options = base_options(
        egui::ViewportBuilder::default()
            .with_title("中键截屏")
            .with_visible(true)
            .with_active(false)
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_taskbar(false)
            .with_inner_size(egui::vec2(1.0, 1.0))
            .with_position(egui::pos2(-32000.0, -32000.0)),
    );

    eframe::run_native(
        "中键截屏",
        options,
        Box::new(move |_| Ok(Box::new(AppHost::new(rx, scale_factor)))),
    )
    .map_err(|e| anyhow!("{}", e))
}

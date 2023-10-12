use crate::event::Event;
use crate::render::State;
use crate::util;
use anyhow::Result;
use log_error::LogError;
use screenshots::Image;
use std::collections::HashMap;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event_loop::{EventLoopProxy, EventLoopWindowTarget};
use winit::window::{WindowBuilder, WindowId, WindowLevel};

pub struct Windows {
    windows: HashMap<WindowId, State>,
    event_loop: EventLoopProxy<Event>,
}

impl Windows {
    pub fn new(event_loop: EventLoopProxy<Event>) -> Self {
        Self {
            windows: HashMap::new(),
            event_loop,
        }
    }

    pub fn create(
        &mut self,
        event_loop: &EventLoopWindowTarget<Event>,
        image: Image,
        size: PhysicalSize<u32>,
        position: PhysicalPosition<i32>,
    ) -> Result<()> {
        let window = WindowBuilder::new()
            .with_title("中键截屏（OCR）")
            .with_window_icon(util::get_window_icon().ok())
            .with_visible(false)
            .with_inner_size(size)
            .with_position(position)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_decorations(false)
            .with_resizable(false)
            .with_transparent(true)
            .build(event_loop)?;
        let mut state = pollster::block_on(async { State::new(window, image, size).await });
        state.render()?;
        state.visible();
        self.windows.insert(state.get_id(), state);
        Ok(())
    }

    pub fn ocr(&mut self, window_id: &WindowId) -> Result<()> {
        let state = self.windows.get_mut(window_id);
        if let Some(state) = state {
            let event_loop = self.event_loop.clone();
            state.ocr(event_loop)?;
        }
        Ok(())
    }

    pub fn redraw(&mut self, window_id: WindowId) -> Result<()> {
        let state = self.windows.get_mut(&window_id);
        if let Some(state) = state {
            state.render()?;
            if state.ocring() {
                self.event_loop
                    .send_event(Event::Redraw(window_id))
                    .log_error("发起重绘失败");
            }
        }
        Ok(())
    }

    pub fn destroy(&mut self, window_id: &WindowId) {
        self.windows.remove(window_id);
    }
}

use winit::window::WindowId;

#[derive(Debug, PartialEq)]
pub enum Event {
    Start,
    Move(f64, f64),
    End,
    Pause,
    Resume,
    Close(WindowId),
    Redraw(WindowId),
}

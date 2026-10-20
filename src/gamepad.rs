//! Controller input for the big-picture UI, read with gilrs on a worker
//! thread and turned into navigation events.

use gilrs::{Axis, Button, EventType, Gilrs};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
    Accept,
    Back,
    Options,
    Menu,
}

/// `guide_is_menu`: whether the Guide/Mode button opens the menu. In Game
/// Mode it belongs to the quick overlay instead.
pub fn from_button_with(b: Button, guide_is_menu: bool) -> Option<Nav> {
    if b == Button::Mode && !guide_is_menu {
        return None;
    }
    Some(match b {
        Button::DPadUp => Nav::Up,
        Button::DPadDown => Nav::Down,
        Button::DPadLeft => Nav::Left,
        Button::DPadRight => Nav::Right,
        Button::South => Nav::Accept,
        Button::East => Nav::Back,
        Button::North => Nav::Options,
        Button::Start | Button::Mode => Nav::Menu,
        _ => return None,
    })
}

/// Stick position → direction, with a dead zone.
pub fn from_stick(x: f32, y: f32) -> Option<Nav> {
    const DEAD: f32 = 0.55;
    if x.abs() < DEAD && y.abs() < DEAD {
        return None;
    }
    Some(if x.abs() > y.abs() {
        if x > 0.0 { Nav::Right } else { Nav::Left }
    } else if y > 0.0 {
        Nav::Up
    } else {
        Nav::Down
    })
}

/// What the Guide button does in Game Mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuideAction {
    /// Pressed and released alone: show/hide the quick overlay.
    Toggle,
    /// Guide + A.
    Screenshot,
    /// Guide + B.
    CloseGame,
}

/// Tracks Guide-button chords.
#[derive(Default)]
pub struct GuideChord {
    held: bool,
    used: bool,
}

impl GuideChord {
    pub fn press(&mut self, b: Button) -> Option<GuideAction> {
        match b {
            Button::Mode => {
                self.held = true;
                self.used = false;
                None
            }
            Button::South if self.held => {
                self.used = true;
                Some(GuideAction::Screenshot)
            }
            Button::East if self.held => {
                self.used = true;
                Some(GuideAction::CloseGame)
            }
            _ => None,
        }
    }

    pub fn release(&mut self, b: Button) -> Option<GuideAction> {
        if b != Button::Mode || !self.held {
            return None;
        }
        self.held = false;
        (!self.used).then_some(GuideAction::Toggle)
    }

    /// While Guide is held, other buttons belong to the chord.
    pub fn holding(&self) -> bool {
        self.held
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    Nav(Nav),
    Guide(GuideAction),
}

/// Start the reader. Returns `None` when no gamepad backend is available.
pub fn start(ctx: egui::Context) -> Option<flume::Receiver<Nav>> {
    let (tx, rx) = flume::unbounded();
    let guide_is_menu = !crate::gamescope::active();
    let inputs = start_inputs(ctx, guide_is_menu)?;
    std::thread::Builder::new()
        .name("gamepad-nav".into())
        .spawn(move || {
            for i in inputs.iter() {
                if let Input::Nav(n) = i {
                    if tx.send(n).is_err() {
                        return;
                    }
                }
            }
        })
        .ok()?;
    Some(rx)
}

/// Navigation plus Guide chords. gilrs reads evdev directly, so this works
/// while another window (the game) has focus.
pub fn start_inputs(ctx: egui::Context, guide_is_menu: bool) -> Option<flume::Receiver<Input>> {
    let (tx, rx) = flume::unbounded();
    std::thread::Builder::new()
        .name("gamepad".into())
        .spawn(move || {
            let Ok(mut gilrs) = Gilrs::new() else {
                return;
            };
            let (mut x, mut y) = (0.0f32, 0.0f32);
            let mut chord = GuideChord::default();
            let mut held: Option<(Nav, Instant)> = None;
            loop {
                while let Some(ev) = gilrs.next_event_blocking(Some(Duration::from_millis(50))) {
                    match ev.event {
                        EventType::ButtonPressed(b, _) => {
                            if !guide_is_menu {
                                if let Some(g) = chord.press(b) {
                                    let _ = tx.send(Input::Guide(g));
                                    ctx.request_repaint();
                                    continue;
                                }
                                if chord.holding() {
                                    continue;
                                }
                            }
                            if let Some(n) = from_button_with(b, guide_is_menu) {
                                let _ = tx.send(Input::Nav(n));
                                ctx.request_repaint();
                            }
                        }
                        EventType::ButtonReleased(b, _) if !guide_is_menu => {
                            if let Some(g) = chord.release(b) {
                                let _ = tx.send(Input::Guide(g));
                                ctx.request_repaint();
                            }
                        }
                        EventType::AxisChanged(Axis::LeftStickX, v, _) => x = v,
                        EventType::AxisChanged(Axis::LeftStickY, v, _) => y = v,
                        _ => {}
                    }
                }
                // Stick: first step immediately, then repeat while held.
                match (from_stick(x, y), held) {
                    (Some(n), Some((h, at))) if n == h => {
                        if at.elapsed() > Duration::from_millis(180) {
                            let _ = tx.send(Input::Nav(n));
                            ctx.request_repaint();
                            held = Some((n, Instant::now()));
                        }
                    }
                    (Some(n), _) => {
                        let _ = tx.send(Input::Nav(n));
                        ctx.request_repaint();
                        held = Some((n, Instant::now() + Duration::from_millis(220)));
                    }
                    (None, _) => held = None,
                }
                if tx.is_disconnected() {
                    return;
                }
            }
        })
        .ok()?;
    Some(rx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping() {
        assert_eq!(from_button_with(Button::South, true), Some(Nav::Accept));
        assert_eq!(from_button_with(Button::East, true), Some(Nav::Back));
        assert_eq!(from_button_with(Button::LeftTrigger, true), None);
        assert_eq!(from_stick(0.1, 0.2), None);
        assert_eq!(from_stick(0.9, 0.2), Some(Nav::Right));
        assert_eq!(from_stick(0.1, -0.9), Some(Nav::Down));
        assert_eq!(from_button_with(Button::Mode, true), Some(Nav::Menu));
        assert_eq!(from_button_with(Button::Mode, false), None);
    }

    #[test]
    fn guide_chords() {
        let mut c = GuideChord::default();
        assert_eq!(c.press(Button::Mode), None);
        assert_eq!(c.release(Button::Mode), Some(GuideAction::Toggle));
        c.press(Button::Mode);
        assert_eq!(c.press(Button::South), Some(GuideAction::Screenshot));
        assert_eq!(c.release(Button::Mode), None);
        c.press(Button::Mode);
        assert_eq!(c.press(Button::East), Some(GuideAction::CloseGame));
        assert_eq!(c.release(Button::Mode), None);
        // A without Guide is just a button.
        assert_eq!(c.press(Button::South), None);
        assert_eq!(c.release(Button::South), None);
    }
}

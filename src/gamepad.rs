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

pub fn from_button(b: Button) -> Option<Nav> {
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

/// Start the reader. Returns `None` when no gamepad backend is available.
pub fn start(ctx: egui::Context) -> Option<flume::Receiver<Nav>> {
    let (tx, rx) = flume::unbounded();
    std::thread::Builder::new()
        .name("gamepad".into())
        .spawn(move || {
            let Ok(mut gilrs) = Gilrs::new() else {
                return;
            };
            let (mut x, mut y) = (0.0f32, 0.0f32);
            let mut held: Option<(Nav, Instant)> = None;
            loop {
                while let Some(ev) = gilrs.next_event_blocking(Some(Duration::from_millis(50))) {
                    match ev.event {
                        EventType::ButtonPressed(b, _) => {
                            if let Some(n) = from_button(b) {
                                let _ = tx.send(n);
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
                            let _ = tx.send(n);
                            ctx.request_repaint();
                            held = Some((n, Instant::now()));
                        }
                    }
                    (Some(n), _) => {
                        let _ = tx.send(n);
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
        assert_eq!(from_button(Button::South), Some(Nav::Accept));
        assert_eq!(from_button(Button::East), Some(Nav::Back));
        assert_eq!(from_button(Button::LeftTrigger), None);
        assert_eq!(from_stick(0.1, 0.2), None);
        assert_eq!(from_stick(0.9, 0.2), Some(Nav::Right));
        assert_eq!(from_stick(0.1, -0.9), Some(Nav::Down));
    }
}

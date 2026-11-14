use egui::Rect;
use std::time::{Duration, Instant};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Left,
    Right,
    Up,
    Down,
    Accept,
    Back,
    PreviousTab,
    NextTab,
    Menu,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputMethod {
    Keyboard,
    Mouse,
    Touch,
    Gamepad,
}
#[derive(Clone, Debug)]
pub struct NavItem {
    pub id: String,
    pub rect: Rect,
}
#[derive(Default)]
pub struct Navigation {
    pub focused: Option<String>,
    pub items: Vec<NavItem>,
    pub reveal: bool,
}
impl Navigation {
    pub fn move_focus(&mut self, action: Action) {
        if self.items.is_empty() {
            return;
        }
        let current = self
            .focused
            .as_ref()
            .and_then(|id| self.items.iter().find(|i| &i.id == id));
        let Some(current) = current else {
            self.focused = Some(self.items[0].id.clone());
            self.reveal = true;
            return;
        };
        let origin = current.rect.center();
        let mut best: Option<(f32, &NavItem)> = None;
        for candidate in &self.items {
            if candidate.id == current.id {
                continue;
            }
            let d = candidate.rect.center() - origin;
            let (forward, side) = match action {
                Action::Left => (-d.x, d.y.abs()),
                Action::Right => (d.x, d.y.abs()),
                Action::Up => (-d.y, d.x.abs()),
                Action::Down => (d.y, d.x.abs()),
                _ => return,
            };
            if forward < 1.0 {
                continue;
            }
            // Favor the same row/column while still allowing movement to headers/footers.
            let score = forward + side * 3.0 + side * side / forward.max(1.0);
            if best.is_none_or(|(s, _)| score < s) {
                best = Some((score, candidate));
            }
        }
        if let Some((_, item)) = best {
            self.focused = Some(item.id.clone());
            self.reveal = true;
        }
    }
    pub fn ensure_valid(&mut self) {
        if self
            .focused
            .as_ref()
            .is_none_or(|f| !self.items.iter().any(|i| &i.id == f))
        {
            self.focused = self.items.first().map(|i| i.id.clone());
        }
    }
    pub fn reset(&mut self) {
        self.focused = None;
        self.items.clear();
        self.reveal = true;
    }
}

#[derive(Default)]
pub struct Controller {
    previous: u16,
    direction: Option<Action>,
    repeat: Option<Instant>,
    active: Option<u32>,
    armed: bool,
}
impl Controller {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    pub fn connected(&self) -> bool {
        self.active.is_some()
    }
    pub fn poll(&mut self, now: Instant) -> Vec<Action> {
        use windows::Win32::UI::Input::XboxController::*;
        let mut found = None;
        for index in 0..4 {
            let mut state = XINPUT_STATE::default();
            if unsafe { XInputGetState(index, &mut state) } == 0 {
                if self.active == Some(index) || found.is_none() {
                    found = Some((index, state));
                }
                if self.active == Some(index) {
                    break;
                }
            }
        }
        self.process_state(found, now)
    }
    fn process_state(
        &mut self,
        found: Option<(u32, windows::Win32::UI::Input::XboxController::XINPUT_STATE)>,
        now: Instant,
    ) -> Vec<Action> {
        use windows::Win32::UI::Input::XboxController::*;
        let Some((index, state)) = found else {
            self.reset();
            return vec![];
        };
        let pad = state.Gamepad;
        let buttons = pad.wButtons.0;
        if self.active != Some(index) {
            self.reset();
            self.active = Some(index);
        }
        let direction = if buttons & XINPUT_GAMEPAD_DPAD_UP.0 != 0 || pad.sThumbLY > 16000 {
            Some(Action::Up)
        } else if buttons & XINPUT_GAMEPAD_DPAD_DOWN.0 != 0 || pad.sThumbLY < -16000 {
            Some(Action::Down)
        } else if buttons & XINPUT_GAMEPAD_DPAD_LEFT.0 != 0 || pad.sThumbLX < -16000 {
            Some(Action::Left)
        } else if buttons & XINPUT_GAMEPAD_DPAD_RIGHT.0 != 0 || pad.sThumbLX > 16000 {
            Some(Action::Right)
        } else {
            None
        };
        // Require neutral after reconnect/focus regain; held A must never launch a game.
        if !self.armed {
            self.previous = buttons;
            if buttons == 0 && direction.is_none() {
                self.armed = true;
            }
            return vec![];
        }
        let pressed = buttons & !self.previous;
        self.previous = buttons;
        let mut out = vec![];
        for (flag, action) in [
            (XINPUT_GAMEPAD_A, Action::Accept),
            (XINPUT_GAMEPAD_B, Action::Back),
            (XINPUT_GAMEPAD_LEFT_SHOULDER, Action::PreviousTab),
            (XINPUT_GAMEPAD_RIGHT_SHOULDER, Action::NextTab),
            (XINPUT_GAMEPAD_START, Action::Menu),
        ] {
            if pressed & flag.0 != 0 {
                out.push(action);
            }
        }
        if direction != self.direction {
            self.direction = direction;
            self.repeat = direction.map(|_| now + Duration::from_millis(330));
            if let Some(d) = direction {
                out.push(d);
            }
        } else if direction.is_some() && self.repeat.is_some_and(|r| now >= r) {
            out.push(direction.unwrap());
            self.repeat = Some(now + Duration::from_millis(110));
        }
        out
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use egui::Pos2;
    fn item(id: &str, x: f32, y: f32) -> NavItem {
        NavItem {
            id: id.into(),
            rect: Rect::from_min_size(Pos2::new(x, y), egui::vec2(80., 60.)),
        }
    }
    #[test]
    fn spatial_navigation_stays_in_column() {
        let mut n = Navigation {
            focused: Some("a".into()),
            items: vec![
                item("a", 0., 0.),
                item("b", 100., 0.),
                item("c", 0., 100.),
                item("d", 100., 100.),
            ],
            reveal: false,
        };
        n.move_focus(Action::Down);
        assert_eq!(n.focused.as_deref(), Some("c"));
        n.move_focus(Action::Right);
        assert_eq!(n.focused.as_deref(), Some("d"));
        n.move_focus(Action::Down);
        assert_eq!(n.focused.as_deref(), Some("d"));
    }
    #[test]
    fn controller_requires_neutral_after_reconnect() {
        use windows::Win32::UI::Input::XboxController::*;
        let mut controller = Controller::default();
        let now = Instant::now();
        let mut held = XINPUT_STATE::default();
        held.Gamepad.wButtons = XINPUT_GAMEPAD_A;
        assert!(controller.process_state(Some((0, held)), now).is_empty());
        assert!(
            controller
                .process_state(Some((0, XINPUT_STATE::default())), now)
                .is_empty()
        );
        assert_eq!(
            controller.process_state(Some((0, held)), now),
            vec![Action::Accept]
        );
        assert!(controller.process_state(Some((0, held)), now).is_empty());
        assert!(controller.process_state(None, now).is_empty());
        assert!(!controller.connected());
        assert!(controller.process_state(Some((1, held)), now).is_empty());
        controller.reset();
        assert!(controller.process_state(Some((1, held)), now).is_empty());
    }
    #[test]
    fn stick_deadzone_and_repeat_timing() {
        use windows::Win32::UI::Input::XboxController::*;
        let mut controller = Controller::default();
        let t = Instant::now();
        controller.process_state(Some((0, XINPUT_STATE::default())), t);
        let mut state = XINPUT_STATE::default();
        state.Gamepad.sThumbLX = 15000;
        assert!(controller.process_state(Some((0, state)), t).is_empty());
        state.Gamepad.sThumbLX = 24000;
        assert_eq!(
            controller.process_state(Some((0, state)), t),
            vec![Action::Right]
        );
        assert!(
            controller
                .process_state(Some((0, state)), t + Duration::from_millis(329))
                .is_empty()
        );
        assert_eq!(
            controller.process_state(Some((0, state)), t + Duration::from_millis(330)),
            vec![Action::Right]
        );
        assert!(
            controller
                .process_state(Some((0, state)), t + Duration::from_millis(400))
                .is_empty()
        );
        assert_eq!(
            controller.process_state(Some((0, state)), t + Duration::from_millis(440)),
            vec![Action::Right]
        );
    }
}

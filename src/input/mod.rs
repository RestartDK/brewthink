pub mod control;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Millivolts(u16);

impl Millivolts {
    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u16 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct BatteryVoltage(Millivolts);

impl BatteryVoltage {
    pub const fn from_millivolts(voltage: Millivolts) -> Self {
        Self(voltage)
    }

    pub const fn millivolts(self) -> Millivolts {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UsbState {
    Disconnected,
    Connected,
}

impl UsbState {
    pub const fn from_connected(connected: bool) -> Self {
        if connected {
            Self::Connected
        } else {
            Self::Disconnected
        }
    }

    pub const fn is_connected(self) -> bool {
        matches!(self, Self::Connected)
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Disconnected => "disconnected",
            Self::Connected => "connected",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawInputSample {
    pub battery_pin_voltage: Millivolts,
    pub navigation_voltage: Millivolts,
    pub page_voltage: Millivolts,
    pub power_pressed: bool,
    pub usb_state: UsbState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Button {
    Back,
    Confirm,
    Left,
    Right,
    Up,
    Down,
    Power,
}

impl Button {
    const ALL: [Self; 7] = [
        Self::Back,
        Self::Confirm,
        Self::Left,
        Self::Right,
        Self::Up,
        Self::Down,
        Self::Power,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Back => "back",
            Self::Confirm => "confirm",
            Self::Left => "left",
            Self::Right => "right",
            Self::Up => "up",
            Self::Down => "down",
            Self::Power => "power",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|button| button.name() == name)
    }

    const fn mask(self) -> u8 {
        1 << self as u8
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PressedButtons(u8);

impl PressedButtons {
    pub const fn none() -> Self {
        Self(0)
    }

    pub fn insert(&mut self, button: Button) {
        self.0 |= button.mask();
    }

    pub const fn contains(self, button: Button) -> bool {
        self.0 & button.mask() != 0
    }

    pub fn iter(self) -> impl Iterator<Item = Button> {
        Button::ALL
            .into_iter()
            .filter(move |button| self.contains(*button))
    }

    const fn difference(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ButtonTransition {
    Pressed,
    Released,
}

impl ButtonTransition {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Pressed => "pressed",
            Self::Released => "released",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ButtonEvent {
    button: Button,
    transition: ButtonTransition,
}

impl ButtonEvent {
    pub const fn button(self) -> Button {
        self.button
    }

    pub const fn transition(self) -> ButtonTransition {
        self.transition
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ButtonChanges {
    pressed: PressedButtons,
    released: PressedButtons,
}

impl ButtonChanges {
    pub const fn pressed(self) -> PressedButtons {
        self.pressed
    }

    pub const fn released(self) -> PressedButtons {
        self.released
    }

    pub fn events(self) -> impl Iterator<Item = ButtonEvent> {
        self.pressed
            .iter()
            .map(|button| ButtonEvent {
                button,
                transition: ButtonTransition::Pressed,
            })
            .chain(self.released.iter().map(|button| ButtonEvent {
                button,
                transition: ButtonTransition::Released,
            }))
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ButtonDebouncer {
    stable: PressedButtons,
    candidate: PressedButtons,
    candidate_samples: u8,
}

impl ButtonDebouncer {
    const REQUIRED_SAMPLES: u8 = 3;

    pub const fn new() -> Self {
        Self {
            stable: PressedButtons::none(),
            candidate: PressedButtons::none(),
            candidate_samples: 0,
        }
    }

    pub fn update(&mut self, observed: PressedButtons) -> Option<ButtonChanges> {
        if observed == self.stable {
            self.reset_candidate();
            return None;
        }

        if observed != self.candidate {
            self.candidate = observed;
            self.candidate_samples = 1;
            return None;
        }

        self.candidate_samples = self.candidate_samples.saturating_add(1);
        if self.candidate_samples < Self::REQUIRED_SAMPLES {
            return None;
        }

        let previous = self.stable;
        self.stable = observed;
        self.reset_candidate();
        Some(ButtonChanges {
            pressed: observed.difference(previous),
            released: previous.difference(observed),
        })
    }

    pub fn reject_sample(&mut self) {
        self.reset_candidate();
    }

    fn reset_candidate(&mut self) {
        self.candidate = self.stable;
        self.candidate_samples = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::{Button, ButtonDebouncer, ButtonEvent, ButtonTransition, PressedButtons};

    fn pressed(button: Button) -> PressedButtons {
        let mut buttons = PressedButtons::none();
        buttons.insert(button);
        buttons
    }

    #[test]
    fn debouncer_emits_one_press_and_one_release() {
        let left = pressed(Button::Left);
        let mut debouncer = ButtonDebouncer::new();

        assert_eq!(debouncer.update(left), None);
        assert_eq!(debouncer.update(left), None);
        let press = debouncer.update(left).unwrap();
        assert_eq!(press.pressed(), left);
        assert_eq!(press.released(), PressedButtons::none());
        assert!(press.events().eq([ButtonEvent {
            button: Button::Left,
            transition: ButtonTransition::Pressed,
        }]));
        assert_eq!(debouncer.update(left), None);

        assert_eq!(debouncer.update(PressedButtons::none()), None);
        assert_eq!(debouncer.update(PressedButtons::none()), None);
        let release = debouncer.update(PressedButtons::none()).unwrap();
        assert_eq!(release.pressed(), PressedButtons::none());
        assert_eq!(release.released(), left);
        assert!(release.events().eq([ButtonEvent {
            button: Button::Left,
            transition: ButtonTransition::Released,
        }]));
        assert_eq!(debouncer.update(PressedButtons::none()), None);
    }

    #[test]
    fn rejected_sample_breaks_a_debounce_run() {
        let right = pressed(Button::Right);
        let mut debouncer = ButtonDebouncer::new();

        assert_eq!(debouncer.update(right), None);
        assert_eq!(debouncer.update(right), None);
        debouncer.reject_sample();
        assert_eq!(debouncer.update(right), None);
        assert_eq!(debouncer.update(right), None);
        assert!(debouncer.update(right).is_some());
    }

    #[test]
    fn pressed_button_iteration_has_stable_domain_order() {
        let mut buttons = PressedButtons::none();
        buttons.insert(Button::Power);
        buttons.insert(Button::Back);
        buttons.insert(Button::Up);

        assert!(buttons.iter().eq([Button::Back, Button::Up, Button::Power]));
    }
}

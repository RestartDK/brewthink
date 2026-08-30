use crate::input::{BatteryVoltage, UsbState};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DisconnectedPowerCapture {
    samples: u32,
    minimum: BatteryVoltage,
    maximum: BatteryVoltage,
}

impl DisconnectedPowerCapture {
    fn first(voltage: BatteryVoltage) -> Self {
        Self {
            samples: 1,
            minimum: voltage,
            maximum: voltage,
        }
    }

    fn record(&mut self, voltage: BatteryVoltage) {
        self.samples = self.samples.saturating_add(1);
        self.minimum = self.minimum.min(voltage);
        self.maximum = self.maximum.max(voltage);
    }

    pub const fn samples(self) -> u32 {
        self.samples
    }

    pub const fn minimum(self) -> BatteryVoltage {
        self.minimum
    }

    pub const fn maximum(self) -> BatteryVoltage {
        self.maximum
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PowerUsbEvent {
    Initial(UsbState),
    Disconnected,
    Reconnected(DisconnectedPowerCapture),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PowerUsbState {
    Waiting,
    Connected,
    Disconnected(DisconnectedPowerCapture),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PowerUsbTracker {
    state: PowerUsbState,
    last_disconnected_capture: Option<DisconnectedPowerCapture>,
}

impl PowerUsbTracker {
    pub const fn new() -> Self {
        Self {
            state: PowerUsbState::Waiting,
            last_disconnected_capture: None,
        }
    }

    pub fn observe(&mut self, usb: UsbState, battery: BatteryVoltage) -> Option<PowerUsbEvent> {
        match (self.state, usb) {
            (PowerUsbState::Waiting, UsbState::Connected) => {
                self.state = PowerUsbState::Connected;
                Some(PowerUsbEvent::Initial(UsbState::Connected))
            }
            (PowerUsbState::Waiting, UsbState::Disconnected) => {
                self.state = PowerUsbState::Disconnected(DisconnectedPowerCapture::first(battery));
                Some(PowerUsbEvent::Initial(UsbState::Disconnected))
            }
            (PowerUsbState::Connected, UsbState::Connected) => None,
            (PowerUsbState::Connected, UsbState::Disconnected) => {
                self.state = PowerUsbState::Disconnected(DisconnectedPowerCapture::first(battery));
                self.last_disconnected_capture = None;
                Some(PowerUsbEvent::Disconnected)
            }
            (PowerUsbState::Disconnected(mut capture), UsbState::Disconnected) => {
                capture.record(battery);
                self.state = PowerUsbState::Disconnected(capture);
                None
            }
            (PowerUsbState::Disconnected(capture), UsbState::Connected) => {
                self.state = PowerUsbState::Connected;
                self.last_disconnected_capture = Some(capture);
                Some(PowerUsbEvent::Reconnected(capture))
            }
        }
    }

    pub const fn last_disconnected_capture(self) -> Option<DisconnectedPowerCapture> {
        self.last_disconnected_capture
    }
}

impl Default for PowerUsbTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{PowerUsbEvent, PowerUsbTracker};
    use crate::input::{BatteryVoltage, Millivolts, UsbState};

    fn battery(millivolts: u16) -> BatteryVoltage {
        BatteryVoltage::from_x4_divided_pin(Millivolts::new(millivolts / 2))
    }

    #[test]
    fn reports_initial_state_once() {
        let mut tracker = PowerUsbTracker::new();

        assert_eq!(
            tracker.observe(UsbState::Connected, battery(4_200)),
            Some(PowerUsbEvent::Initial(UsbState::Connected))
        );
        assert_eq!(tracker.observe(UsbState::Connected, battery(4_198)), None);
    }

    #[test]
    fn summarizes_battery_samples_recorded_while_usb_was_disconnected() {
        let mut tracker = PowerUsbTracker::new();
        tracker.observe(UsbState::Connected, battery(4_200));

        assert_eq!(
            tracker.observe(UsbState::Disconnected, battery(4_100)),
            Some(PowerUsbEvent::Disconnected)
        );
        assert_eq!(
            tracker.observe(UsbState::Disconnected, battery(4_080)),
            None
        );
        assert_eq!(
            tracker.observe(UsbState::Disconnected, battery(4_090)),
            None
        );

        let Some(PowerUsbEvent::Reconnected(capture)) =
            tracker.observe(UsbState::Connected, battery(4_150))
        else {
            panic!("expected reconnection summary");
        };
        assert_eq!(capture.samples(), 3);
        assert_eq!(capture.minimum(), battery(4_080));
        assert_eq!(capture.maximum(), battery(4_100));
        assert_eq!(tracker.last_disconnected_capture(), Some(capture));

        assert_eq!(
            tracker.observe(UsbState::Disconnected, battery(4_050)),
            Some(PowerUsbEvent::Disconnected)
        );
        assert_eq!(tracker.last_disconnected_capture(), None);
    }

    #[test]
    fn initial_disconnected_state_is_included_in_the_summary() {
        let mut tracker = PowerUsbTracker::new();
        assert_eq!(
            tracker.observe(UsbState::Disconnected, battery(3_900)),
            Some(PowerUsbEvent::Initial(UsbState::Disconnected))
        );

        let Some(PowerUsbEvent::Reconnected(capture)) =
            tracker.observe(UsbState::Connected, battery(3_950))
        else {
            panic!("expected reconnection summary");
        };
        assert_eq!(capture.samples(), 1);
        assert_eq!(capture.minimum(), battery(3_900));
        assert_eq!(capture.maximum(), battery(3_900));
        assert_eq!(tracker.last_disconnected_capture(), Some(capture));
    }
}

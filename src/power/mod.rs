use crate::input::{BatteryVoltage, UsbState};

const ESTIMATOR_SAMPLES: u8 = 16;
const DISCHARGE_CURVE: [(u16, u8); 8] = [
    (3_400, 0),
    (3_600, 8),
    (3_700, 18),
    (3_800, 35),
    (3_900, 55),
    (4_000, 75),
    (4_100, 90),
    (4_200, 100),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BatteryPercent(u8);

impl BatteryPercent {
    pub const fn new(value: u8) -> Self {
        Self(if value > 100 { 100 } else { value })
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BatteryLevel {
    Unknown,
    Percent(BatteryPercent),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BatteryStatus {
    level: BatteryLevel,
    usb: UsbState,
}

impl BatteryStatus {
    pub const fn unknown() -> Self {
        Self {
            level: BatteryLevel::Unknown,
            usb: UsbState::Disconnected,
        }
    }

    pub const fn from_percent(percent: u8, usb: UsbState) -> Self {
        Self {
            level: BatteryLevel::Percent(BatteryPercent::new(percent)),
            usb,
        }
    }

    pub fn from_voltage(voltage: BatteryVoltage, usb: UsbState) -> Self {
        Self::from_percent(estimate_percent(voltage.millivolts().get()), usb)
    }

    pub const fn level(self) -> BatteryLevel {
        self.level
    }

    pub const fn usb(self) -> UsbState {
        self.usb
    }
}

impl Default for BatteryStatus {
    fn default() -> Self {
        Self::unknown()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BatteryEstimator {
    millivolts: u32,
    samples: u8,
    usb: UsbState,
    last: BatteryStatus,
}

impl BatteryEstimator {
    pub const fn new() -> Self {
        Self {
            millivolts: 0,
            samples: 0,
            usb: UsbState::Disconnected,
            last: BatteryStatus::unknown(),
        }
    }

    pub fn observe(&mut self, voltage: BatteryVoltage, usb: UsbState) -> Option<BatteryStatus> {
        self.millivolts = self
            .millivolts
            .saturating_add(u32::from(voltage.millivolts().get()));
        self.samples = self.samples.saturating_add(1);
        let usb_changed = self.usb != usb;
        self.usb = usb;
        if self.samples < ESTIMATOR_SAMPLES && !usb_changed {
            return None;
        }
        let divisor = u32::from(self.samples.max(1));
        let average = (self.millivolts / divisor).min(u32::from(u16::MAX)) as u16;
        self.millivolts = 0;
        self.samples = 0;
        let next = BatteryStatus::from_percent(estimate_percent(average), usb);
        if next == self.last {
            return None;
        }
        self.last = next;
        Some(next)
    }

    pub const fn last(self) -> BatteryStatus {
        self.last
    }
}

impl Default for BatteryEstimator {
    fn default() -> Self {
        Self::new()
    }
}

fn estimate_percent(millivolts: u16) -> u8 {
    if millivolts <= DISCHARGE_CURVE[0].0 {
        return DISCHARGE_CURVE[0].1;
    }
    for points in DISCHARGE_CURVE.windows(2) {
        let (lower_mv, lower_percent) = points[0];
        let (upper_mv, upper_percent) = points[1];
        if millivolts <= upper_mv {
            let position = u32::from(millivolts - lower_mv);
            let span = u32::from(upper_mv - lower_mv);
            let percent_span = u32::from(upper_percent - lower_percent);
            return lower_percent + ((position * percent_span) / span) as u8;
        }
    }
    100
}

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
    use super::{BatteryEstimator, BatteryStatus, PowerUsbEvent, PowerUsbTracker};
    use crate::input::{BatteryVoltage, Millivolts, UsbState};

    fn battery(millivolts: u16) -> BatteryVoltage {
        BatteryVoltage::from_x4_divided_pin(Millivolts::new(millivolts / 2))
    }

    #[test]
    fn estimates_battery_charge_from_a_smoothed_voltage_curve() {
        let mut estimator = BatteryEstimator::new();
        for _ in 0..15 {
            assert_eq!(
                estimator.observe(battery(3_900), UsbState::Disconnected),
                None
            );
        }
        assert_eq!(
            estimator.observe(battery(3_900), UsbState::Disconnected),
            Some(BatteryStatus::from_percent(55, UsbState::Disconnected))
        );
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

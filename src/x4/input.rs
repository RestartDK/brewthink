use crate::input::{BatteryVoltage, Button, Millivolts, PressedButtons, RawInputSample};

impl BatteryVoltage {
    pub const fn from_x4_divided_pin(pin_voltage: Millivolts) -> Self {
        Self::from_millivolts(Millivolts::new(pin_voltage.get().saturating_mul(2)))
    }
}

impl RawInputSample {
    pub const fn battery_voltage(self) -> BatteryVoltage {
        BatteryVoltage::from_x4_divided_pin(self.battery_pin_voltage)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum X4ButtonDecodeError {
    Navigation(Millivolts),
    Page(Millivolts),
}

impl X4ButtonDecodeError {
    pub const fn channel_name(self) -> &'static str {
        match self {
            Self::Navigation(_) => "navigation",
            Self::Page(_) => "page",
        }
    }

    pub const fn voltage(self) -> Millivolts {
        match self {
            Self::Navigation(voltage) | Self::Page(voltage) => voltage,
        }
    }
}

#[derive(Clone, Copy)]
struct VoltageBand {
    minimum: Millivolts,
    maximum: Millivolts,
}

impl VoltageBand {
    const fn new(minimum: u16, maximum: u16) -> Self {
        Self {
            minimum: Millivolts::new(minimum),
            maximum: Millivolts::new(maximum),
        }
    }

    const fn contains(self, voltage: Millivolts) -> bool {
        voltage.get() >= self.minimum.get() && voltage.get() <= self.maximum.get()
    }
}

#[derive(Clone, Copy)]
struct ButtonBand {
    voltage: VoltageBand,
    button: Button,
}

impl ButtonBand {
    const fn new(minimum: u16, maximum: u16, button: Button) -> Self {
        Self {
            voltage: VoltageBand::new(minimum, maximum),
            button,
        }
    }
}

const IDLE_BAND: VoltageBand = VoltageBand::new(2_800, 3_200);
const NAVIGATION_BANDS: [ButtonBand; 4] = [
    ButtonBand::new(2_400, 2_700, Button::Left),
    ButtonBand::new(1_800, 2_150, Button::Right),
    ButtonBand::new(950, 1_250, Button::Up),
    ButtonBand::new(0, 150, Button::Down),
];
const PAGE_BANDS: [ButtonBand; 2] = [
    ButtonBand::new(1_500, 1_800, Button::Up),
    ButtonBand::new(0, 150, Button::Down),
];

pub fn decode_buttons(sample: RawInputSample) -> Result<PressedButtons, X4ButtonDecodeError> {
    let navigation = decode_ladder(sample.navigation_voltage, &NAVIGATION_BANDS)
        .map_err(X4ButtonDecodeError::Navigation)?;
    let page =
        decode_ladder(sample.page_voltage, &PAGE_BANDS).map_err(X4ButtonDecodeError::Page)?;
    let mut pressed = PressedButtons::none();

    if let Some(button) = navigation {
        pressed.insert(button);
    }
    if let Some(button) = page {
        pressed.insert(button);
    }
    if sample.power_pressed {
        pressed.insert(Button::Power);
    }

    Ok(pressed)
}

fn decode_ladder(voltage: Millivolts, bands: &[ButtonBand]) -> Result<Option<Button>, Millivolts> {
    if IDLE_BAND.contains(voltage) {
        return Ok(None);
    }

    bands
        .iter()
        .find(|band| band.voltage.contains(voltage))
        .map(|band| Some(band.button))
        .ok_or(voltage)
}

#[cfg(target_arch = "riscv32")]
mod hardware {
    use embassy_time::Timer;
    use esp_hal::{
        Blocking,
        analog::adc::{Adc, AdcCalCurve, AdcCalScheme, AdcChannel, AdcConfig, AdcPin, Attenuation},
        gpio::{Input, InputConfig, Pull},
        peripherals::{ADC1, GPIO0, GPIO1, GPIO2, GPIO3, GPIO20},
    };

    use crate::input::{Millivolts, RawInputSample, UsbState};

    type X4Adc = ADC1<'static>;
    type X4AdcDriver = Adc<'static, X4Adc, Blocking>;
    type CalibratedAdcPin<P> = AdcPin<P, X4Adc, AdcCalCurve<X4Adc>>;

    const ADC_ATTEMPTS: usize = 400;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum InputReadError {
        Battery,
        Navigation,
        Page,
    }

    impl InputReadError {
        pub const fn name(self) -> &'static str {
            match self {
                Self::Battery => "battery",
                Self::Navigation => "navigation",
                Self::Page => "page",
            }
        }
    }

    pub struct X4InputPeripherals {
        adc: ADC1<'static>,
        battery: GPIO0<'static>,
        navigation: GPIO1<'static>,
        page: GPIO2<'static>,
        power: GPIO3<'static>,
        usb_detect: GPIO20<'static>,
    }

    impl X4InputPeripherals {
        pub fn new(
            adc: ADC1<'static>,
            battery: GPIO0<'static>,
            navigation: GPIO1<'static>,
            page: GPIO2<'static>,
            power: GPIO3<'static>,
            usb_detect: GPIO20<'static>,
        ) -> Self {
            Self {
                adc,
                battery,
                navigation,
                page,
                power,
                usb_detect,
            }
        }

        pub fn initialize(self) -> X4InputHardware {
            X4InputHardware::new(
                self.adc,
                self.battery,
                self.navigation,
                self.page,
                self.power,
                self.usb_detect,
            )
        }
    }

    pub struct X4InputHardware {
        adc: X4AdcDriver,
        battery: CalibratedAdcPin<GPIO0<'static>>,
        navigation: CalibratedAdcPin<GPIO1<'static>>,
        page: CalibratedAdcPin<GPIO2<'static>>,
        power: GPIO3<'static>,
        usb_detect: Input<'static>,
    }

    impl X4InputHardware {
        fn new(
            adc: ADC1<'static>,
            battery: GPIO0<'static>,
            navigation: GPIO1<'static>,
            page: GPIO2<'static>,
            power: GPIO3<'static>,
            usb_detect: GPIO20<'static>,
        ) -> Self {
            let mut config = AdcConfig::new();
            let battery =
                config.enable_pin_with_cal::<_, AdcCalCurve<X4Adc>>(battery, Attenuation::_11dB);
            let navigation =
                config.enable_pin_with_cal::<_, AdcCalCurve<X4Adc>>(navigation, Attenuation::_11dB);
            let page =
                config.enable_pin_with_cal::<_, AdcCalCurve<X4Adc>>(page, Attenuation::_11dB);
            let adc = Adc::new(adc, config);
            let mut power = power;
            {
                let _configured =
                    Input::new(power.reborrow(), InputConfig::default().with_pull(Pull::Up));
            }
            let usb_detect = Input::new(usb_detect, InputConfig::default());

            Self {
                adc,
                battery,
                navigation,
                page,
                power,
                usb_detect,
            }
        }

        pub async fn sample(&mut self) -> Result<RawInputSample, InputReadError> {
            let battery_pin_voltage = Millivolts::new(
                read_adc(&mut self.adc, &mut self.battery)
                    .await
                    .map_err(|()| InputReadError::Battery)?,
            );
            let navigation_voltage = Millivolts::new(
                read_adc(&mut self.adc, &mut self.navigation)
                    .await
                    .map_err(|()| InputReadError::Navigation)?,
            );
            let page_voltage = Millivolts::new(
                read_adc(&mut self.adc, &mut self.page)
                    .await
                    .map_err(|()| InputReadError::Page)?,
            );

            let power_pressed = {
                let power = Input::new(
                    self.power.reborrow(),
                    InputConfig::default().with_pull(Pull::Up),
                );
                power.is_low()
            };

            Ok(RawInputSample {
                battery_pin_voltage,
                navigation_voltage,
                page_voltage,
                power_pressed,
                usb_state: UsbState::from_connected(self.usb_detect.is_high()),
            })
        }

        pub fn into_power_pin(self) -> GPIO3<'static> {
            self.power
        }
    }

    async fn read_adc<P, CS>(
        adc: &mut X4AdcDriver,
        pin: &mut AdcPin<P, X4Adc, CS>,
    ) -> Result<u16, ()>
    where
        P: AdcChannel,
        CS: AdcCalScheme<X4Adc>,
    {
        for _ in 0..ADC_ATTEMPTS {
            match adc.read_oneshot(pin) {
                Ok(value) => return Ok(value),
                Err(nb::Error::WouldBlock) => Timer::after_micros(50).await,
                Err(nb::Error::Other(_)) => return Err(()),
            }
        }

        Err(())
    }
}

#[cfg(target_arch = "riscv32")]
pub use hardware::{InputReadError, X4InputHardware, X4InputPeripherals};

#[cfg(test)]
mod tests {
    use super::{X4ButtonDecodeError, decode_buttons};
    use crate::input::{BatteryVoltage, Button, Millivolts, RawInputSample, UsbState};

    #[test]
    fn battery_voltage_applies_the_x4_divider_ratio() {
        let sample = RawInputSample {
            battery_pin_voltage: Millivolts::new(1_950),
            navigation_voltage: Millivolts::new(0),
            page_voltage: Millivolts::new(0),
            power_pressed: false,
            usb_state: UsbState::Disconnected,
        };

        assert_eq!(
            sample.battery_voltage(),
            BatteryVoltage::from_x4_divided_pin(Millivolts::new(1_950))
        );
        assert_eq!(sample.battery_voltage().millivolts().get(), 3_900);
    }

    #[test]
    fn battery_voltage_saturates_instead_of_wrapping() {
        let voltage = BatteryVoltage::from_x4_divided_pin(Millivolts::new(u16::MAX));
        assert_eq!(voltage.millivolts().get(), u16::MAX);
    }

    fn sample(navigation_mv: u16, page_mv: u16, power_pressed: bool) -> RawInputSample {
        RawInputSample {
            battery_pin_voltage: Millivolts::new(2_100),
            navigation_voltage: Millivolts::new(navigation_mv),
            page_voltage: Millivolts::new(page_mv),
            power_pressed,
            usb_state: UsbState::Connected,
        }
    }

    #[test]
    fn decodes_every_measured_button_voltage() {
        let cases = [
            (2_563, 2_973, false, Button::Left),
            (1_985, 2_973, false, Button::Right),
            (1_110, 2_973, false, Button::Up),
            (3, 2_973, false, Button::Down),
            (2_973, 1_655, false, Button::Up),
            (2_973, 4, false, Button::Down),
            (2_973, 2_973, true, Button::Power),
        ];

        for (navigation_mv, page_mv, power_pressed, expected) in cases {
            let decoded = decode_buttons(sample(navigation_mv, page_mv, power_pressed)).unwrap();
            assert!(decoded.iter().eq([expected]));
        }
    }

    #[test]
    fn decodes_idle_and_concurrent_channels() {
        let idle = decode_buttons(sample(2_973, 2_973, false)).unwrap();
        assert_eq!(idle.iter().count(), 0);

        let concurrent = decode_buttons(sample(1_985, 1_655, true)).unwrap();
        assert!(
            concurrent
                .iter()
                .eq([Button::Right, Button::Up, Button::Power])
        );
    }

    #[test]
    fn rejects_unmeasured_navigation_voltage() {
        assert_eq!(
            decode_buttons(sample(2_300, 2_973, false)),
            Err(X4ButtonDecodeError::Navigation(Millivolts::new(2_300)))
        );
    }

    #[test]
    fn rejects_unmeasured_page_voltage() {
        assert_eq!(
            decode_buttons(sample(2_973, 800, false)),
            Err(X4ButtonDecodeError::Page(Millivolts::new(800)))
        );
    }
}

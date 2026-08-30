#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Gpio(u8);

impl Gpio {
    const fn new(number: u8) -> Self {
        Self(number)
    }

    pub const fn number(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DisplayPins {
    pub chip_select: Gpio,
    pub data_command: Gpio,
    pub reset: Gpio,
    pub busy: Gpio,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SharedSpiPins {
    pub clock: Gpio,
    pub mosi: Gpio,
    pub sd_miso: Gpio,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InputPins {
    pub battery_adc: Gpio,
    pub buttons_primary_adc: Gpio,
    pub buttons_secondary_adc: Gpio,
    pub power_button: Gpio,
    pub usb_detect: Gpio,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct X4PinMap {
    pub display: DisplayPins,
    pub shared_spi: SharedSpiPins,
    pub sd_chip_select: Gpio,
    pub inputs: InputPins,
    pub board_reserved: &'static [Gpio],
    pub flash: &'static [Gpio],
    pub usb: &'static [Gpio],
}

pub const X4: X4PinMap = X4PinMap {
    display: DisplayPins {
        chip_select: Gpio::new(21),
        data_command: Gpio::new(4),
        reset: Gpio::new(5),
        busy: Gpio::new(6),
    },
    shared_spi: SharedSpiPins {
        clock: Gpio::new(8),
        mosi: Gpio::new(10),
        sd_miso: Gpio::new(7),
    },
    sd_chip_select: Gpio::new(12),
    inputs: InputPins {
        battery_adc: Gpio::new(0),
        buttons_primary_adc: Gpio::new(1),
        buttons_secondary_adc: Gpio::new(2),
        power_button: Gpio::new(3),
        usb_detect: Gpio::new(20),
    },
    board_reserved: &[Gpio::new(13)],
    flash: &[
        Gpio::new(11),
        Gpio::new(14),
        Gpio::new(15),
        Gpio::new(16),
        Gpio::new(17),
    ],
    usb: &[Gpio::new(18), Gpio::new(19)],
};

pub const SAFE_INIT_OUTPUTS: [Gpio; 2] = [X4.display.chip_select, X4.sd_chip_select];

#[cfg(test)]
mod tests {
    use super::{Gpio, SAFE_INIT_OUTPUTS, X4};

    #[test]
    fn pin_map_matches_verified_x4_hardware() {
        assert_eq!(X4.display.chip_select.number(), 21);
        assert_eq!(X4.display.data_command.number(), 4);
        assert_eq!(X4.display.reset.number(), 5);
        assert_eq!(X4.display.busy.number(), 6);
        assert_eq!(X4.shared_spi.clock.number(), 8);
        assert_eq!(X4.shared_spi.mosi.number(), 10);
        assert_eq!(X4.shared_spi.sd_miso.number(), 7);
        assert_eq!(X4.sd_chip_select.number(), 12);
        assert_eq!(X4.inputs.battery_adc.number(), 0);
        assert_eq!(X4.inputs.buttons_primary_adc.number(), 1);
        assert_eq!(X4.inputs.buttons_secondary_adc.number(), 2);
        assert_eq!(X4.inputs.power_button.number(), 3);
        assert_eq!(X4.inputs.usb_detect.number(), 20);
    }

    #[test]
    fn safe_init_only_owns_deselected_chip_selects() {
        assert_eq!(
            SAFE_INIT_OUTPUTS,
            [X4.display.chip_select, X4.sd_chip_select]
        );
        assert_ne!(X4.display.chip_select, X4.sd_chip_select);
    }

    #[test]
    fn safe_init_preserves_reserved_flash_and_usb_pins() {
        for pin in SAFE_INIT_OUTPUTS {
            assert!(!X4.board_reserved.contains(&pin));
            assert!(!X4.flash.contains(&pin));
            assert!(!X4.usb.contains(&pin));
        }
    }

    #[test]
    fn gpio13_remains_reserved() {
        assert_eq!(X4.board_reserved, &[Gpio::new(13)]);
        assert!(!SAFE_INIT_OUTPUTS.contains(&Gpio::new(13)));
    }
}

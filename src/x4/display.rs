use esp_hal::{
    Blocking,
    delay::Delay,
    gpio::{Input, InputConfig, Output, OutputConfig},
    peripherals::{GPIO4, GPIO5, GPIO6, GPIO8, GPIO10, SPI2},
    spi::{
        Mode,
        master::{Config, ConfigError, Spi},
    },
    time::Rate,
};

use crate::display::bus::EpdBus;

use super::SharedSpiChipSelects;

pub type X4EpdBus<'d> =
    EpdBus<Spi<'d, Blocking>, Output<'d>, Output<'d>, Output<'d>, Input<'d>, Delay>;

pub struct X4DisplayHardware<'d> {
    epd: X4EpdBus<'d>,
    sd_chip_select: Output<'d>,
}

impl<'d> X4DisplayHardware<'d> {
    pub fn new(
        spi: SPI2<'d>,
        clock: GPIO8<'d>,
        mosi: GPIO10<'d>,
        data_command: GPIO4<'d>,
        reset: GPIO5<'d>,
        busy: GPIO6<'d>,
        chip_selects: SharedSpiChipSelects<'d>,
    ) -> Result<Self, ConfigError> {
        let (display_chip_select, sd_chip_select) = chip_selects.into_parts();
        let spi = Spi::new(
            spi,
            Config::default()
                .with_frequency(Rate::from_mhz(20))
                .with_mode(Mode::_0),
        )?
        .with_sck(clock)
        .with_mosi(mosi);
        let data_command = Output::new(
            data_command,
            esp_hal::gpio::Level::High,
            OutputConfig::default(),
        );
        let reset = Output::new(reset, esp_hal::gpio::Level::High, OutputConfig::default());
        let busy = Input::new(busy, InputConfig::default());
        let epd = EpdBus::new(
            spi,
            display_chip_select,
            data_command,
            reset,
            busy,
            Delay::new(),
        );

        Ok(Self {
            epd,
            sd_chip_select,
        })
    }

    pub fn into_parts(self) -> (X4EpdBus<'d>, Output<'d>) {
        (self.epd, self.sd_chip_select)
    }
}

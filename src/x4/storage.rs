#[cfg(feature = "sd-card")]
use core::{
    cell::{Cell, RefCell},
    fmt,
};

#[cfg(feature = "sd-card")]
use embedded_sdmmc::{Block, BlockCount, BlockDevice, BlockIdx};
use esp_hal::{
    Blocking,
    delay::Delay,
    gpio::{Input, InputConfig, Level, Output, OutputConfig},
    peripherals::{GPIO4, GPIO5, GPIO6, GPIO7, GPIO8, GPIO10, SPI2},
    spi::{
        Error as SpiError, Mode,
        master::{Config, ConfigError, Spi},
    },
    time::Rate,
};

#[cfg(feature = "sd-write-diagnostic")]
use crate::storage::ExplicitWriteSdCard;
use crate::storage::{ReadOnlySdSpi, SdSpiClock};

use super::{
    SharedSpiChipSelects,
    shared_display::SharedDisplayBus,
    shared_spi::{SharedSpi, SharedSpiDevice, SharedSpiError},
};

const INITIALIZATION_FREQUENCY: Rate = Rate::from_khz(400);
const TRANSFER_FREQUENCY: Rate = Rate::from_mhz(10);
const DISPLAY_FREQUENCY: Rate = Rate::from_mhz(40);

fn apply_sd_config(shared: &mut X4SharedSpi<'_>, clock: SdSpiClock) -> Result<(), X4StorageError> {
    let frequency = match clock {
        SdSpiClock::Initialization => INITIALIZATION_FREQUENCY,
        SdSpiClock::Transfer => TRANSFER_FREQUENCY,
    };
    shared
        .spi_mut()
        .map_err(X4StorageError::Spi)?
        .apply_config(
            &Config::default()
                .with_frequency(frequency)
                .with_mode(Mode::_0),
        )
        .map_err(X4StorageError::Configuration)
}

pub type X4SharedSpi<'d> = SharedSpi<Spi<'d, Blocking>, Output<'d>, Output<'d>>;

pub type X4SharedDisplayBus<'a, 'd> = SharedDisplayBus<
    'a,
    Spi<'d, Blocking>,
    Output<'d>,
    Output<'d>,
    Output<'d>,
    Output<'d>,
    Input<'d>,
    Delay,
>;

#[derive(Clone, Copy, Debug, defmt::Format)]
pub enum X4StorageError {
    Configuration(ConfigError),
    Spi(SharedSpiError<SpiError>),
}

#[cfg(feature = "sd-card")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum X4FatBlockDeviceError {
    Card,
    BlockIndexOverflow,
    CapacityOverflow,
    ReadOnly,
}

#[cfg(feature = "sd-card")]
impl fmt::Display for X4FatBlockDeviceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Card => "SD card operation failed",
            Self::BlockIndexOverflow => "block index overflowed",
            Self::CapacityOverflow => "card capacity exceeds the FAT block-device limit",
            Self::ReadOnly => "book storage is read-only",
        })
    }
}

#[cfg(feature = "sd-card")]
impl core::error::Error for X4FatBlockDeviceError {}

#[cfg(feature = "sd-card")]
pub struct X4ReadOnlyFatBlockDevice<'d> {
    card: RefCell<crate::storage::ReadOnlySdCard<X4StorageHardware<'d>>>,
    sectors_read: Cell<u32>,
}

#[cfg(feature = "sd-write-diagnostic")]
pub struct X4FatBlockDevice<'d> {
    card: RefCell<ExplicitWriteSdCard<X4StorageHardware<'d>>>,
    sectors_read: Cell<u32>,
    sectors_written: Cell<u32>,
}

pub struct X4SharedSpiPeripherals<'d> {
    spi: SPI2<'d>,
    clock: GPIO8<'d>,
    mosi: GPIO10<'d>,
    miso: GPIO7<'d>,
    display_data_command: GPIO4<'d>,
    display_reset: GPIO5<'d>,
    display_busy: GPIO6<'d>,
}

pub struct X4StorageHardware<'d> {
    shared: X4SharedSpi<'d>,
    sd_clock: SdSpiClock,
    sd_configured: bool,
    display_data_command: Output<'d>,
    display_reset: Output<'d>,
    display_busy: Input<'d>,
    delay: Delay,
}

impl<'d> X4SharedSpiPeripherals<'d> {
    pub fn new(
        spi: SPI2<'d>,
        clock: GPIO8<'d>,
        mosi: GPIO10<'d>,
        miso: GPIO7<'d>,
        display_data_command: GPIO4<'d>,
        display_reset: GPIO5<'d>,
        display_busy: GPIO6<'d>,
    ) -> Self {
        Self {
            spi,
            clock,
            mosi,
            miso,
            display_data_command,
            display_reset,
            display_busy,
        }
    }
}

impl<'d> X4StorageHardware<'d> {
    pub fn new(
        peripherals: X4SharedSpiPeripherals<'d>,
        chip_selects: SharedSpiChipSelects<'d>,
    ) -> Result<Self, ConfigError> {
        let (display_chip_select, sd_chip_select) = chip_selects.into_parts();
        let spi = Spi::new(
            peripherals.spi,
            Config::default()
                .with_frequency(INITIALIZATION_FREQUENCY)
                .with_mode(Mode::_0),
        )?
        .with_sck(peripherals.clock)
        .with_mosi(peripherals.mosi)
        .with_miso(peripherals.miso);

        Ok(Self {
            shared: SharedSpi::new(spi, display_chip_select, sd_chip_select),
            sd_clock: SdSpiClock::Initialization,
            sd_configured: true,
            display_data_command: Output::new(
                peripherals.display_data_command,
                Level::High,
                OutputConfig::default(),
            ),
            display_reset: Output::new(
                peripherals.display_reset,
                Level::High,
                OutputConfig::default(),
            ),
            display_busy: Input::new(peripherals.display_busy, InputConfig::default()),
            delay: Delay::new(),
        })
    }

    pub fn display_bus(&mut self) -> Result<X4SharedDisplayBus<'_, 'd>, X4StorageError> {
        self.shared
            .spi_mut()
            .map_err(X4StorageError::Spi)?
            .apply_config(
                &Config::default()
                    .with_frequency(DISPLAY_FREQUENCY)
                    .with_mode(Mode::_0),
            )
            .map_err(X4StorageError::Configuration)?;
        self.sd_configured = false;
        Ok(SharedDisplayBus::new(
            &mut self.shared,
            &mut self.display_data_command,
            &mut self.display_reset,
            &mut self.display_busy,
            &mut self.delay,
        ))
    }

    pub fn display_is_deselected(&mut self) -> bool {
        self.shared.display_is_deselected()
    }

    pub fn sd_is_deselected(&mut self) -> bool {
        self.shared.sd_is_deselected()
    }

    pub fn both_are_deselected(&mut self) -> bool {
        self.shared.both_are_deselected()
    }
}

#[cfg(feature = "sd-card")]
impl<'d> X4ReadOnlyFatBlockDevice<'d> {
    pub fn new(card: crate::storage::ReadOnlySdCard<X4StorageHardware<'d>>) -> Self {
        Self {
            card: RefCell::new(card),
            sectors_read: Cell::new(0),
        }
    }

    pub fn sectors_read(&self) -> u32 {
        self.sectors_read.get()
    }

    pub fn chip_select_states(&self) -> (bool, bool, bool) {
        let mut card = self.card.borrow_mut();
        let hardware = card.bus_mut();
        let display_high = hardware.display_is_deselected();
        let sd_high = hardware.sd_is_deselected();
        let both_high = hardware.both_are_deselected();
        (display_high, sd_high, both_high)
    }

    pub fn with_hardware<R>(
        &mut self,
        function: impl FnOnce(&mut X4StorageHardware<'d>) -> R,
    ) -> R {
        function(self.card.get_mut().bus_mut())
    }

    pub fn into_card(self) -> crate::storage::ReadOnlySdCard<X4StorageHardware<'d>> {
        self.card.into_inner()
    }

    fn block_index(start: u32, offset: usize) -> Result<u32, X4FatBlockDeviceError> {
        let offset =
            u32::try_from(offset).map_err(|_| X4FatBlockDeviceError::BlockIndexOverflow)?;
        start
            .checked_add(offset)
            .ok_or(X4FatBlockDeviceError::BlockIndexOverflow)
    }
}

#[cfg(feature = "sd-card")]
impl BlockDevice for X4ReadOnlyFatBlockDevice<'_> {
    type Error = X4FatBlockDeviceError;

    fn read(&self, blocks: &mut [Block], start_block_idx: BlockIdx) -> Result<(), Self::Error> {
        let mut card = self.card.borrow_mut();
        for (offset, block) in blocks.iter_mut().enumerate() {
            let index = Self::block_index(start_block_idx.0, offset)?;
            card.read_block(index, &mut block.contents)
                .map_err(|_| X4FatBlockDeviceError::Card)?;
            self.sectors_read
                .set(self.sectors_read.get().saturating_add(1));
        }
        Ok(())
    }

    fn write(&self, _blocks: &[Block], _start_block_idx: BlockIdx) -> Result<(), Self::Error> {
        Err(X4FatBlockDeviceError::ReadOnly)
    }

    fn num_blocks(&self) -> Result<BlockCount, Self::Error> {
        let count = self
            .card
            .borrow()
            .card_info()
            .ok_or(X4FatBlockDeviceError::Card)?
            .block_count;
        let count = u32::try_from(count).map_err(|_| X4FatBlockDeviceError::CapacityOverflow)?;
        Ok(BlockCount(count))
    }
}

#[cfg(feature = "sd-write-diagnostic")]
impl<'d> X4FatBlockDevice<'d> {
    pub fn new(card: ExplicitWriteSdCard<X4StorageHardware<'d>>) -> Self {
        Self {
            card: RefCell::new(card),
            sectors_read: Cell::new(0),
            sectors_written: Cell::new(0),
        }
    }

    pub fn sectors_read(&self) -> u32 {
        self.sectors_read.get()
    }

    pub fn sectors_written(&self) -> u32 {
        self.sectors_written.get()
    }

    pub fn chip_select_states(&self) -> (bool, bool, bool) {
        let mut card = self.card.borrow_mut();
        let hardware = card.bus_mut();
        let display_high = hardware.display_is_deselected();
        let sd_high = hardware.sd_is_deselected();
        let both_high = hardware.both_are_deselected();
        (display_high, sd_high, both_high)
    }

    pub fn into_card(self) -> ExplicitWriteSdCard<X4StorageHardware<'d>> {
        self.card.into_inner()
    }

    fn block_index(start: u32, offset: usize) -> Result<u32, X4FatBlockDeviceError> {
        let offset =
            u32::try_from(offset).map_err(|_| X4FatBlockDeviceError::BlockIndexOverflow)?;
        start
            .checked_add(offset)
            .ok_or(X4FatBlockDeviceError::BlockIndexOverflow)
    }
}

#[cfg(feature = "sd-write-diagnostic")]
impl BlockDevice for X4FatBlockDevice<'_> {
    type Error = X4FatBlockDeviceError;

    fn read(&self, blocks: &mut [Block], start_block_idx: BlockIdx) -> Result<(), Self::Error> {
        let mut card = self.card.borrow_mut();
        for (offset, block) in blocks.iter_mut().enumerate() {
            let index = Self::block_index(start_block_idx.0, offset)?;
            card.read_block(index, &mut block.contents)
                .map_err(|_| X4FatBlockDeviceError::Card)?;
            self.sectors_read
                .set(self.sectors_read.get().saturating_add(1));
        }
        Ok(())
    }

    fn write(&self, blocks: &[Block], start_block_idx: BlockIdx) -> Result<(), Self::Error> {
        let mut card = self.card.borrow_mut();
        for (offset, block) in blocks.iter().enumerate() {
            let index = Self::block_index(start_block_idx.0, offset)?;
            card.write_block(index, &block.contents)
                .map_err(|_| X4FatBlockDeviceError::Card)?;
            self.sectors_written
                .set(self.sectors_written.get().saturating_add(1));
        }
        Ok(())
    }

    fn num_blocks(&self) -> Result<BlockCount, Self::Error> {
        let count = self
            .card
            .borrow()
            .card_info()
            .ok_or(X4FatBlockDeviceError::Card)?
            .block_count;
        let count = u32::try_from(count).map_err(|_| X4FatBlockDeviceError::CapacityOverflow)?;
        Ok(BlockCount(count))
    }
}

impl ReadOnlySdSpi for X4StorageHardware<'_> {
    type Error = X4StorageError;

    fn set_clock(&mut self, clock: SdSpiClock) -> Result<(), Self::Error> {
        apply_sd_config(&mut self.shared, clock)?;
        self.sd_clock = clock;
        self.sd_configured = true;
        Ok(())
    }

    fn idle_clocks(&mut self, byte_count: usize) -> Result<(), Self::Error> {
        self.shared
            .idle_clocks(byte_count)
            .map_err(X4StorageError::Spi)
    }

    fn begin_sd(&mut self) -> Result<(), Self::Error> {
        if !self.sd_configured {
            apply_sd_config(&mut self.shared, self.sd_clock)?;
            self.sd_configured = true;
        }
        self.shared
            .begin(SharedSpiDevice::SdCard)
            .map_err(X4StorageError::Spi)
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        self.shared
            .write(SharedSpiDevice::SdCard, bytes)
            .map_err(X4StorageError::Spi)
    }

    fn transfer_in_place(&mut self, bytes: &mut [u8]) -> Result<(), Self::Error> {
        self.shared
            .transfer_in_place(SharedSpiDevice::SdCard, bytes)
            .map_err(X4StorageError::Spi)
    }

    fn end_sd(&mut self) -> Result<(), Self::Error> {
        self.shared
            .end(SharedSpiDevice::SdCard)
            .map_err(X4StorageError::Spi)
    }

    fn delay_us(&mut self, microseconds: u32) {
        embedded_hal::delay::DelayNs::delay_us(&mut self.delay, microseconds);
    }
}

use core::marker::PhantomData;

use super::framebuffer::{Frame, Rotation};

pub const WIDTH: usize = 800;
pub const HEIGHT: usize = 480;
pub const ROW_BYTES: usize = WIDTH / 8;
pub const FRAME_BYTES: usize = ROW_BYTES * HEIGHT;

const CMD_DRIVER_OUTPUT_CONTROL: u8 = 0x01;
const CMD_BOOSTER_SOFT_START: u8 = 0x0C;
const CMD_DEEP_SLEEP: u8 = 0x10;
const CMD_DATA_ENTRY_MODE: u8 = 0x11;
const CMD_SW_RESET: u8 = 0x12;
const CMD_WRITE_TEMP: u8 = 0x1A;
const CMD_TEMP_SENSOR: u8 = 0x18;
const CMD_MASTER_ACTIVATION: u8 = 0x20;
const CMD_DISPLAY_UPDATE_CTRL1: u8 = 0x21;
const CMD_DISPLAY_UPDATE_CTRL2: u8 = 0x22;
const CMD_WRITE_RAM_BW: u8 = 0x24;
const CMD_WRITE_RAM_RED: u8 = 0x26;
const CMD_BORDER_WAVEFORM: u8 = 0x3C;
const CMD_SET_RAM_X_RANGE: u8 = 0x44;
const CMD_SET_RAM_Y_RANGE: u8 = 0x45;
const CMD_AUTO_WRITE_BW_RAM: u8 = 0x46;
const CMD_AUTO_WRITE_RED_RAM: u8 = 0x47;
const CMD_SET_RAM_X_COUNTER: u8 = 0x4E;
const CMD_SET_RAM_Y_COUNTER: u8 = 0x4F;

const RAM_X_RANGE: [u8; 4] = [0x00, 0x00, 0x1F, 0x03];
const RAM_Y_RANGE: [u8; 4] = [0xDF, 0x01, 0x00, 0x00];
const RAM_X_COUNTER: [u8; 2] = [0x00, 0x00];
const RAM_Y_COUNTER: [u8; 2] = [0xDF, 0x01];
const SOLID_CHUNK: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum X4DriveProfile {
    OpenX4FastDu,
    StockParity,
}

impl X4DriveProfile {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "openx4-fast-du" => Some(Self::OpenX4FastDu),
            "stock-parity" => Some(Self::StockParity),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::OpenX4FastDu => "openx4-fast-du",
            Self::StockParity => "stock-parity",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreviousFrameStorage {
    HostRam,
    ControllerRam,
}

impl PreviousFrameStorage {
    pub const fn name(self) -> &'static str {
        match self {
            Self::HostRam => "host-ram",
            Self::ControllerRam => "controller-ram",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefreshMode {
    FullClean,
    QuickClean,
    Differential,
}

impl RefreshMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "full-clean" => Some(Self::FullClean),
            "quick-clean" => Some(Self::QuickClean),
            "differential" => Some(Self::Differential),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::FullClean => "full-clean",
            Self::QuickClean => "quick-clean",
            Self::Differential => "differential",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefreshPolicyMode {
    Automatic,
    Fixed(RefreshMode),
}

impl RefreshPolicyMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "automatic" => Some(Self::Automatic),
            value => RefreshMode::parse(value).map(Self::Fixed),
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::Fixed(mode) => mode.name(),
        }
    }
}

pub struct RefreshPolicy {
    mode: RefreshPolicyMode,
    differential_since_clean: u8,
}

impl RefreshPolicy {
    pub const DIFFERENTIALS_BEFORE_CLEAN: u8 = 15;

    pub const fn new(mode: RefreshPolicyMode) -> Self {
        Self {
            mode,
            differential_since_clean: 0,
        }
    }

    pub const fn mode(&self) -> RefreshPolicyMode {
        self.mode
    }

    pub const fn requested_mode(&self) -> RefreshMode {
        match self.mode {
            RefreshPolicyMode::Automatic
                if self.differential_since_clean >= Self::DIFFERENTIALS_BEFORE_CLEAN =>
            {
                RefreshMode::QuickClean
            }
            RefreshPolicyMode::Automatic => RefreshMode::Differential,
            RefreshPolicyMode::Fixed(mode) => mode,
        }
    }

    pub fn commit(&mut self, applied: RefreshMode) {
        match applied {
            RefreshMode::Differential => {
                self.differential_since_clean = self.differential_since_clean.saturating_add(1);
            }
            RefreshMode::FullClean | RefreshMode::QuickClean => {
                self.differential_since_clean = 0;
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BaselineState {
    Unknown,
    Synchronized,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InitOperation {
    Reset,
    WaitReady,
    Command { command: u8, data: &'static [u8] },
}

pub const INIT_SEQUENCE: &[InitOperation] = &[
    InitOperation::Reset,
    InitOperation::Command {
        command: CMD_SW_RESET,
        data: &[],
    },
    InitOperation::WaitReady,
    InitOperation::Command {
        command: CMD_TEMP_SENSOR,
        data: &[0x80],
    },
    InitOperation::Command {
        command: CMD_BOOSTER_SOFT_START,
        data: &[0xAE, 0xC7, 0xC3, 0xC0, 0x40],
    },
    InitOperation::Command {
        command: CMD_DRIVER_OUTPUT_CONTROL,
        data: &[0xDF, 0x01, 0x02],
    },
    InitOperation::Command {
        command: CMD_BORDER_WAVEFORM,
        data: &[0x01],
    },
    InitOperation::Command {
        command: CMD_DATA_ENTRY_MODE,
        data: &[0x01],
    },
    InitOperation::Command {
        command: CMD_SET_RAM_X_RANGE,
        data: &RAM_X_RANGE,
    },
    InitOperation::Command {
        command: CMD_SET_RAM_Y_RANGE,
        data: &RAM_Y_RANGE,
    },
    InitOperation::Command {
        command: CMD_AUTO_WRITE_BW_RAM,
        data: &[0xF7],
    },
    InitOperation::WaitReady,
    InitOperation::Command {
        command: CMD_AUTO_WRITE_RED_RAM,
        data: &[0xF7],
    },
    InitOperation::WaitReady,
    InitOperation::Command {
        command: CMD_DISPLAY_UPDATE_CTRL1,
        data: &[0x40, 0x00],
    },
    InitOperation::Command {
        command: CMD_DISPLAY_UPDATE_CTRL2,
        data: &[0xF7],
    },
];

const STOCK_INIT_SEQUENCE: &[InitOperation] = &[
    InitOperation::Reset,
    InitOperation::Command {
        command: CMD_SW_RESET,
        data: &[],
    },
    InitOperation::WaitReady,
    InitOperation::Command {
        command: CMD_TEMP_SENSOR,
        data: &[0x80],
    },
    InitOperation::Command {
        command: CMD_BOOSTER_SOFT_START,
        data: &[0xAE, 0xC7, 0xC3, 0xC0, 0x80],
    },
    InitOperation::Command {
        command: CMD_DRIVER_OUTPUT_CONTROL,
        data: &[0xDF, 0x01, 0x02],
    },
    InitOperation::Command {
        command: CMD_BORDER_WAVEFORM,
        data: &[0x80],
    },
    InitOperation::Command {
        command: CMD_DATA_ENTRY_MODE,
        data: &[0x01],
    },
    InitOperation::Command {
        command: CMD_SET_RAM_X_RANGE,
        data: &RAM_X_RANGE,
    },
    InitOperation::Command {
        command: CMD_SET_RAM_Y_RANGE,
        data: &RAM_Y_RANGE,
    },
    InitOperation::Command {
        command: CMD_AUTO_WRITE_BW_RAM,
        data: &[0xF7],
    },
    InitOperation::WaitReady,
    InitOperation::Command {
        command: CMD_AUTO_WRITE_RED_RAM,
        data: &[0xF7],
    },
    InitOperation::WaitReady,
];

pub trait DisplayBus {
    type Error;

    fn reset(&mut self);
    fn command(&mut self, command: u8, data: &[u8]) -> Result<(), Self::Error>;
    fn begin_ram_write(&mut self, command: u8) -> Result<(), Self::Error>;
    fn write_ram(&mut self, data: &[u8]) -> Result<(), Self::Error>;
    fn end_ram_write(&mut self) -> Result<(), Self::Error>;
    fn wait_ready(&mut self) -> Result<(), Self::Error>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error<E> {
    Bus(E),
    InvalidFrameLength { expected: usize, actual: usize },
}

pub struct Uninitialized;

pub struct Ready;

pub struct Ssd1677<S> {
    state: PhantomData<S>,
    profile: X4DriveProfile,
}

enum PreviousFrame<'a> {
    HostRam(&'a mut [u8; FRAME_BYTES]),
    ControllerRam,
}

pub struct BufferedDisplay<'a> {
    controller: Ssd1677<Ready>,
    previous: PreviousFrame<'a>,
    rotation: Rotation,
    baseline: BaselineState,
}

impl Ssd1677<Uninitialized> {
    pub const fn new() -> Self {
        Self::with_profile(X4DriveProfile::OpenX4FastDu)
    }

    pub const fn with_profile(profile: X4DriveProfile) -> Self {
        Self {
            state: PhantomData,
            profile,
        }
    }

    pub fn initialize<B>(self, bus: &mut B) -> Result<Ssd1677<Ready>, Error<B::Error>>
    where
        B: DisplayBus,
    {
        let sequence = match self.profile {
            X4DriveProfile::OpenX4FastDu => INIT_SEQUENCE,
            X4DriveProfile::StockParity => STOCK_INIT_SEQUENCE,
        };
        for operation in sequence {
            match operation {
                InitOperation::Reset => bus.reset(),
                InitOperation::WaitReady => bus.wait_ready().map_err(Error::Bus)?,
                InitOperation::Command { command, data } => {
                    bus.command(*command, data).map_err(Error::Bus)?
                }
            }
        }

        Ok(Ssd1677 {
            state: PhantomData,
            profile: self.profile,
        })
    }
}

impl Default for Ssd1677<Uninitialized> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> BufferedDisplay<'a> {
    pub fn with_host_ram(
        controller: Ssd1677<Ready>,
        previous: &'a mut [u8; FRAME_BYTES],
        rotation: Rotation,
    ) -> Self {
        Self {
            controller,
            previous: PreviousFrame::HostRam(previous),
            rotation,
            baseline: BaselineState::Unknown,
        }
    }

    pub fn with_controller_ram(controller: Ssd1677<Ready>, rotation: Rotation) -> Self {
        Self {
            controller,
            previous: PreviousFrame::ControllerRam,
            rotation,
            baseline: BaselineState::Unknown,
        }
    }

    pub const fn baseline_state(&self) -> BaselineState {
        self.baseline
    }

    pub const fn drive_profile(&self) -> X4DriveProfile {
        self.controller.profile
    }

    pub const fn previous_frame_storage(&self) -> PreviousFrameStorage {
        match &self.previous {
            PreviousFrame::HostRam(_) => PreviousFrameStorage::HostRam,
            PreviousFrame::ControllerRam => PreviousFrameStorage::ControllerRam,
        }
    }

    pub fn refresh<B>(
        &mut self,
        bus: &mut B,
        next: &[u8; FRAME_BYTES],
        requested: RefreshMode,
    ) -> Result<RefreshMode, Error<B::Error>>
    where
        B: DisplayBus,
    {
        let applied = match (self.baseline, requested) {
            (BaselineState::Unknown, RefreshMode::Differential) => RefreshMode::QuickClean,
            _ => requested,
        };
        let next_frame = Frame::from_array(next, self.rotation);
        self.baseline = BaselineState::Unknown;
        match &mut self.previous {
            PreviousFrame::HostRam(previous) => {
                let previous_frame = Frame::from_array(previous, self.rotation);
                self.controller.refresh_with_host_baseline(
                    bus,
                    previous_frame,
                    next_frame,
                    applied,
                )?;
                previous.copy_from_slice(next);
            }
            PreviousFrame::ControllerRam => {
                self.controller
                    .refresh_with_controller_baseline(bus, next_frame, applied)?;
            }
        }
        self.baseline = BaselineState::Synchronized;
        Ok(applied)
    }

    pub fn enter_deep_sleep<B>(self, bus: &mut B) -> Result<(), Error<B::Error>>
    where
        B: DisplayBus,
    {
        self.controller.enter_deep_sleep(bus)
    }
}

impl Ssd1677<Ready> {
    pub fn write_solid_frame<B>(&mut self, bus: &mut B, value: u8) -> Result<(), Error<B::Error>>
    where
        B: DisplayBus,
    {
        self.prepare_full_window(bus)?;
        self.write_solid_plane(bus, CMD_WRITE_RAM_BW, value)?;
        self.write_solid_plane(bus, CMD_WRITE_RAM_RED, value)
    }

    pub fn refresh_solid<B>(&mut self, bus: &mut B, value: u8) -> Result<(), Error<B::Error>>
    where
        B: DisplayBus,
    {
        self.write_solid_frame(bus, value)?;
        self.activate_full_refresh(bus)
    }

    pub fn write_white_frame<B>(&mut self, bus: &mut B) -> Result<(), Error<B::Error>>
    where
        B: DisplayBus,
    {
        self.write_solid_frame(bus, 0xFF)
    }

    pub fn refresh_white<B>(&mut self, bus: &mut B) -> Result<(), Error<B::Error>>
    where
        B: DisplayBus,
    {
        self.refresh_solid(bus, 0xFF)
    }

    pub fn write_generated_frame<B, F>(
        &mut self,
        bus: &mut B,
        mut fill: F,
    ) -> Result<(), Error<B::Error>>
    where
        B: DisplayBus,
        F: FnMut(usize, &mut [u8]),
    {
        self.prepare_full_window(bus)?;
        self.write_generated_plane(bus, CMD_WRITE_RAM_BW, &mut fill)?;
        self.write_generated_plane(bus, CMD_WRITE_RAM_RED, &mut fill)
    }

    pub fn refresh_generated_frame<B, F>(
        &mut self,
        bus: &mut B,
        fill: F,
    ) -> Result<(), Error<B::Error>>
    where
        B: DisplayBus,
        F: FnMut(usize, &mut [u8]),
    {
        self.write_generated_frame(bus, fill)?;
        self.activate_full_refresh(bus)
    }

    pub fn write_logical_frame<B>(
        &mut self,
        bus: &mut B,
        frame: Frame<'_>,
    ) -> Result<(), Error<B::Error>>
    where
        B: DisplayBus,
    {
        self.write_generated_frame(bus, |offset, output| frame.fill_panel_chunk(offset, output))
    }

    pub fn refresh_logical_frame<B>(
        &mut self,
        bus: &mut B,
        frame: Frame<'_>,
    ) -> Result<(), Error<B::Error>>
    where
        B: DisplayBus,
    {
        self.write_logical_frame(bus, frame)?;
        self.activate_full_refresh(bus)
    }

    fn refresh_with_host_baseline<B>(
        &mut self,
        bus: &mut B,
        previous: Frame<'_>,
        next: Frame<'_>,
        mode: RefreshMode,
    ) -> Result<(), Error<B::Error>>
    where
        B: DisplayBus,
    {
        debug_assert_eq!(previous.rotation(), next.rotation());
        self.prepare_full_window(bus)?;
        self.write_logical_plane(bus, CMD_WRITE_RAM_BW, next)?;
        let red = if mode == RefreshMode::Differential {
            previous
        } else {
            next
        };
        self.write_logical_plane(bus, CMD_WRITE_RAM_RED, red)?;
        self.activate_refresh(bus, mode)
    }

    fn refresh_with_controller_baseline<B>(
        &mut self,
        bus: &mut B,
        next: Frame<'_>,
        mode: RefreshMode,
    ) -> Result<(), Error<B::Error>>
    where
        B: DisplayBus,
    {
        self.prepare_full_window(bus)?;
        self.write_logical_plane(bus, CMD_WRITE_RAM_BW, next)?;
        if mode != RefreshMode::Differential {
            self.write_logical_plane(bus, CMD_WRITE_RAM_RED, next)?;
        }
        self.activate_refresh(bus, mode)?;

        self.prepare_full_window(bus)?;
        if self.profile == X4DriveProfile::StockParity {
            self.write_logical_plane(bus, CMD_WRITE_RAM_BW, next)?;
        }
        self.write_logical_plane(bus, CMD_WRITE_RAM_RED, next)
    }

    pub fn enter_deep_sleep<B>(self, bus: &mut B) -> Result<(), Error<B::Error>>
    where
        B: DisplayBus,
    {
        bus.command(CMD_DEEP_SLEEP, &[0x03]).map_err(Error::Bus)
    }

    pub fn write_frame<B>(&mut self, bus: &mut B, frame: &[u8]) -> Result<(), Error<B::Error>>
    where
        B: DisplayBus,
    {
        if frame.len() != FRAME_BYTES {
            return Err(Error::InvalidFrameLength {
                expected: FRAME_BYTES,
                actual: frame.len(),
            });
        }

        self.prepare_full_window(bus)?;
        self.write_frame_plane(bus, CMD_WRITE_RAM_BW, frame)?;
        self.write_frame_plane(bus, CMD_WRITE_RAM_RED, frame)
    }

    pub fn refresh_frame<B>(&mut self, bus: &mut B, frame: &[u8]) -> Result<(), Error<B::Error>>
    where
        B: DisplayBus,
    {
        self.write_frame(bus, frame)?;
        self.activate_full_refresh(bus)
    }

    pub fn activate_full_refresh<B>(&mut self, bus: &mut B) -> Result<(), Error<B::Error>>
    where
        B: DisplayBus,
    {
        self.activate_refresh(bus, RefreshMode::FullClean)
    }

    fn activate_refresh<B>(&mut self, bus: &mut B, mode: RefreshMode) -> Result<(), Error<B::Error>>
    where
        B: DisplayBus,
    {
        let compare = if mode == RefreshMode::Differential {
            &[0x00, 0x00][..]
        } else {
            &[0x40, 0x00][..]
        };
        self.command(bus, CMD_DISPLAY_UPDATE_CTRL1, compare)?;

        if self.profile == X4DriveProfile::StockParity {
            self.command(bus, CMD_BORDER_WAVEFORM, &[0xC0])?;
        }
        let sequence = match (self.profile, mode) {
            (X4DriveProfile::OpenX4FastDu, RefreshMode::FullClean) => 0xF4,
            (X4DriveProfile::OpenX4FastDu, RefreshMode::QuickClean) => {
                self.command(bus, CMD_WRITE_TEMP, &[0x5A])?;
                0xD4
            }
            (X4DriveProfile::OpenX4FastDu, RefreshMode::Differential) => 0x1C,
            (X4DriveProfile::StockParity, RefreshMode::FullClean) => 0xF7,
            (X4DriveProfile::StockParity, RefreshMode::QuickClean) => {
                self.command(bus, CMD_WRITE_TEMP, &[0x5A])?;
                0xD7
            }
            (X4DriveProfile::StockParity, RefreshMode::Differential) => 0xFC,
        };
        self.command(bus, CMD_DISPLAY_UPDATE_CTRL2, &[sequence])?;
        self.command(bus, CMD_MASTER_ACTIVATION, &[])?;
        bus.wait_ready().map_err(Error::Bus)
    }

    fn prepare_full_window<B>(&mut self, bus: &mut B) -> Result<(), Error<B::Error>>
    where
        B: DisplayBus,
    {
        self.command(bus, CMD_SET_RAM_X_RANGE, &RAM_X_RANGE)?;
        self.command(bus, CMD_SET_RAM_Y_RANGE, &RAM_Y_RANGE)?;
        self.command(bus, CMD_SET_RAM_X_COUNTER, &RAM_X_COUNTER)?;
        self.command(bus, CMD_SET_RAM_Y_COUNTER, &RAM_Y_COUNTER)
    }

    fn write_logical_plane<B>(
        &mut self,
        bus: &mut B,
        command: u8,
        frame: Frame<'_>,
    ) -> Result<(), Error<B::Error>>
    where
        B: DisplayBus,
    {
        self.write_generated_plane(bus, command, &mut |offset, output| {
            frame.fill_panel_chunk(offset, output);
        })
    }

    fn write_solid_plane<B>(
        &mut self,
        bus: &mut B,
        command: u8,
        value: u8,
    ) -> Result<(), Error<B::Error>>
    where
        B: DisplayBus,
    {
        let chunk = [value; SOLID_CHUNK];
        bus.begin_ram_write(command).map_err(Error::Bus)?;

        let mut remaining = FRAME_BYTES;
        while remaining > 0 {
            let length = remaining.min(SOLID_CHUNK);
            if let Err(error) = bus.write_ram(&chunk[..length]) {
                let _ = bus.end_ram_write();
                return Err(Error::Bus(error));
            }
            remaining -= length;
        }

        bus.end_ram_write().map_err(Error::Bus)
    }

    fn write_generated_plane<B, F>(
        &mut self,
        bus: &mut B,
        command: u8,
        fill: &mut F,
    ) -> Result<(), Error<B::Error>>
    where
        B: DisplayBus,
        F: FnMut(usize, &mut [u8]),
    {
        let mut chunk = [0; SOLID_CHUNK];
        bus.begin_ram_write(command).map_err(Error::Bus)?;

        let mut offset = 0;
        while offset < FRAME_BYTES {
            let length = (FRAME_BYTES - offset).min(SOLID_CHUNK);
            fill(offset, &mut chunk[..length]);
            if let Err(error) = bus.write_ram(&chunk[..length]) {
                let _ = bus.end_ram_write();
                return Err(Error::Bus(error));
            }
            offset += length;
        }

        bus.end_ram_write().map_err(Error::Bus)
    }

    fn write_frame_plane<B>(
        &mut self,
        bus: &mut B,
        command: u8,
        frame: &[u8],
    ) -> Result<(), Error<B::Error>>
    where
        B: DisplayBus,
    {
        bus.begin_ram_write(command).map_err(Error::Bus)?;

        for chunk in frame.chunks(SOLID_CHUNK) {
            if let Err(error) = bus.write_ram(chunk) {
                let _ = bus.end_ram_write();
                return Err(Error::Bus(error));
            }
        }

        bus.end_ram_write().map_err(Error::Bus)
    }

    fn command<B>(&mut self, bus: &mut B, command: u8, data: &[u8]) -> Result<(), Error<B::Error>>
    where
        B: DisplayBus,
    {
        bus.command(command, data).map_err(Error::Bus)
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::{vec, vec::Vec};

    use super::{
        BaselineState, BufferedDisplay, CMD_DEEP_SLEEP, CMD_DISPLAY_UPDATE_CTRL1,
        CMD_DISPLAY_UPDATE_CTRL2, CMD_MASTER_ACTIVATION, CMD_WRITE_RAM_BW, CMD_WRITE_RAM_RED,
        DisplayBus, Error, FRAME_BYTES, PreviousFrame, PreviousFrameStorage, RefreshMode,
        RefreshPolicy, RefreshPolicyMode, Ssd1677, X4DriveProfile,
    };
    use crate::display::framebuffer::Rotation;

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum Event {
        Reset,
        WaitReady,
        Command(u8, Vec<u8>),
        BeginRam(u8),
        Ram(Vec<u8>),
        EndRam,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum FakeError {
        Wait,
    }

    #[derive(Default)]
    struct FakeBus {
        events: Vec<Event>,
        fail_next_wait: bool,
    }

    impl DisplayBus for FakeBus {
        type Error = FakeError;

        fn reset(&mut self) {
            self.events.push(Event::Reset);
        }

        fn command(&mut self, command: u8, data: &[u8]) -> Result<(), Self::Error> {
            self.events.push(Event::Command(command, data.into()));
            Ok(())
        }

        fn begin_ram_write(&mut self, command: u8) -> Result<(), Self::Error> {
            self.events.push(Event::BeginRam(command));
            Ok(())
        }

        fn write_ram(&mut self, data: &[u8]) -> Result<(), Self::Error> {
            self.events.push(Event::Ram(data.into()));
            Ok(())
        }

        fn end_ram_write(&mut self) -> Result<(), Self::Error> {
            self.events.push(Event::EndRam);
            Ok(())
        }

        fn wait_ready(&mut self) -> Result<(), Self::Error> {
            self.events.push(Event::WaitReady);
            if self.fail_next_wait {
                self.fail_next_wait = false;
                Err(FakeError::Wait)
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn configuration_names_round_trip() {
        for profile in [X4DriveProfile::OpenX4FastDu, X4DriveProfile::StockParity] {
            assert_eq!(X4DriveProfile::parse(profile.name()), Some(profile));
        }
        for mode in [
            RefreshMode::FullClean,
            RefreshMode::QuickClean,
            RefreshMode::Differential,
        ] {
            assert_eq!(RefreshMode::parse(mode.name()), Some(mode));
        }
        assert_eq!(PreviousFrameStorage::HostRam.name(), "host-ram");
        assert_eq!(PreviousFrameStorage::ControllerRam.name(), "controller-ram");
        assert_eq!(
            RefreshPolicyMode::parse("automatic"),
            Some(RefreshPolicyMode::Automatic)
        );
        assert_eq!(
            RefreshPolicyMode::parse("differential"),
            Some(RefreshPolicyMode::Fixed(RefreshMode::Differential))
        );
        assert_eq!(X4DriveProfile::parse("other"), None);
        assert_eq!(RefreshMode::parse("other"), None);
    }

    #[test]
    fn automatic_policy_cleans_after_fifteen_differential_updates() {
        let mut policy = RefreshPolicy::new(RefreshPolicyMode::Automatic);

        for _ in 0..RefreshPolicy::DIFFERENTIALS_BEFORE_CLEAN {
            assert_eq!(policy.requested_mode(), RefreshMode::Differential);
            policy.commit(RefreshMode::Differential);
        }
        assert_eq!(policy.requested_mode(), RefreshMode::QuickClean);
        policy.commit(RefreshMode::QuickClean);
        assert_eq!(policy.requested_mode(), RefreshMode::Differential);
    }

    #[test]
    fn fixed_policy_always_requests_its_selected_mode() {
        let mut policy = RefreshPolicy::new(RefreshPolicyMode::Fixed(RefreshMode::FullClean));

        policy.commit(RefreshMode::Differential);

        assert_eq!(policy.requested_mode(), RefreshMode::FullClean);
    }

    #[test]
    fn initialization_matches_x4_golden_transcript() {
        let mut bus = FakeBus::default();
        let _display = Ssd1677::new().initialize(&mut bus).unwrap();

        assert_eq!(
            bus.events,
            vec![
                Event::Reset,
                Event::Command(0x12, vec![]),
                Event::WaitReady,
                Event::Command(0x18, vec![0x80]),
                Event::Command(0x0C, vec![0xAE, 0xC7, 0xC3, 0xC0, 0x40]),
                Event::Command(0x01, vec![0xDF, 0x01, 0x02]),
                Event::Command(0x3C, vec![0x01]),
                Event::Command(0x11, vec![0x01]),
                Event::Command(0x44, vec![0x00, 0x00, 0x1F, 0x03]),
                Event::Command(0x45, vec![0xDF, 0x01, 0x00, 0x00]),
                Event::Command(0x46, vec![0xF7]),
                Event::WaitReady,
                Event::Command(0x47, vec![0xF7]),
                Event::WaitReady,
                Event::Command(0x21, vec![0x40, 0x00]),
                Event::Command(0x22, vec![0xF7]),
            ]
        );
    }

    #[test]
    fn stock_parity_profile_uses_the_stock_booster_and_border() {
        let mut bus = FakeBus::default();
        let display = Ssd1677::with_profile(X4DriveProfile::StockParity)
            .initialize(&mut bus)
            .unwrap();

        assert_eq!(display.profile, X4DriveProfile::StockParity);
        assert!(
            bus.events
                .contains(&Event::Command(0x0C, vec![0xAE, 0xC7, 0xC3, 0xC0, 0x80]))
        );
        assert!(bus.events.contains(&Event::Command(0x3C, vec![0x80])));
        assert!(!bus.events.iter().any(|event| matches!(
            event,
            Event::Command(CMD_DISPLAY_UPDATE_CTRL1 | CMD_DISPLAY_UPDATE_CTRL2, _)
        )));
    }

    #[test]
    fn initialization_stops_on_busy_failure() {
        let mut bus = FakeBus {
            fail_next_wait: true,
            ..FakeBus::default()
        };

        let result = Ssd1677::new().initialize(&mut bus);

        assert!(matches!(result, Err(Error::Bus(FakeError::Wait))));
        assert_eq!(
            bus.events,
            [Event::Reset, Event::Command(0x12, vec![]), Event::WaitReady]
        );
    }

    #[test]
    fn white_ram_stage_does_not_activate_the_panel() {
        let mut bus = FakeBus::default();
        let mut display = Ssd1677::new().initialize(&mut bus).unwrap();
        bus.events.clear();

        display.write_white_frame(&mut bus).unwrap();

        assert!(!bus.events.iter().any(|event| matches!(
            event,
            Event::Command(CMD_MASTER_ACTIVATION, _)
                | Event::Command(CMD_DISPLAY_UPDATE_CTRL2, _)
                | Event::WaitReady
        )));
        assert_eq!(bus.events.last(), Some(&Event::EndRam));
    }

    #[test]
    fn white_refresh_writes_both_planes_and_activates() {
        let mut bus = FakeBus::default();
        let mut display = Ssd1677::new().initialize(&mut bus).unwrap();
        bus.events.clear();

        display.refresh_white(&mut bus).unwrap();

        let begin_commands: Vec<u8> = bus
            .events
            .iter()
            .filter_map(|event| match event {
                Event::BeginRam(command) => Some(*command),
                _ => None,
            })
            .collect();
        let ram: Vec<&[u8]> = bus
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Ram(data) => Some(data.as_slice()),
                _ => None,
            })
            .collect();
        let ram_bytes: usize = ram.iter().map(|chunk| chunk.len()).sum();

        assert_eq!(begin_commands, [CMD_WRITE_RAM_BW, CMD_WRITE_RAM_RED]);
        assert_eq!(ram_bytes, FRAME_BYTES * 2);
        assert!(
            ram.iter()
                .all(|chunk| chunk.iter().all(|byte| *byte == 0xFF))
        );
        assert!(bus.events.ends_with(&[
            Event::Command(CMD_DISPLAY_UPDATE_CTRL1, vec![0x40, 0x00]),
            Event::Command(CMD_DISPLAY_UPDATE_CTRL2, vec![0xF4]),
            Event::Command(CMD_MASTER_ACTIVATION, vec![]),
            Event::WaitReady,
        ]));
    }

    #[test]
    fn frame_refresh_transfers_exactly_two_full_planes() {
        let mut bus = FakeBus::default();
        let mut display = Ssd1677::new().initialize(&mut bus).unwrap();
        bus.events.clear();
        let frame = vec![0xA5; FRAME_BYTES];

        display.refresh_frame(&mut bus, &frame).unwrap();

        let ram: Vec<&[u8]> = bus
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Ram(data) => Some(data.as_slice()),
                _ => None,
            })
            .collect();
        assert_eq!(
            ram.iter().map(|chunk| chunk.len()).sum::<usize>(),
            FRAME_BYTES * 2
        );
        assert!(
            ram.iter()
                .all(|chunk| chunk.iter().all(|byte| *byte == 0xA5))
        );
    }

    #[test]
    fn generated_frame_restarts_at_zero_for_each_plane() {
        let mut bus = FakeBus::default();
        let mut display = Ssd1677::new().initialize(&mut bus).unwrap();
        bus.events.clear();

        display
            .write_generated_frame(&mut bus, |offset, output| {
                for (index, byte) in output.iter_mut().enumerate() {
                    *byte = ((offset + index) % 251) as u8;
                }
            })
            .unwrap();

        let ram: Vec<u8> = bus
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Ram(data) => Some(data.as_slice()),
                _ => None,
            })
            .flatten()
            .copied()
            .collect();
        assert_eq!(ram.len(), FRAME_BYTES * 2);
        assert_eq!(ram[..FRAME_BYTES], ram[FRAME_BYTES..]);
        assert_eq!(ram[0], 0);
        assert_eq!(ram[250], 250);
        assert_eq!(ram[251], 0);
    }

    #[test]
    fn first_differential_refresh_is_promoted_and_seeds_the_previous_frame() {
        let mut bus = FakeBus::default();
        let controller = Ssd1677::new().initialize(&mut bus).unwrap();
        let mut displayed = [0xFF; FRAME_BYTES];
        let next = [0x00; FRAME_BYTES];
        let mut display =
            BufferedDisplay::with_host_ram(controller, &mut displayed, Rotation::Degrees0);
        bus.events.clear();

        let applied = display
            .refresh(&mut bus, &next, RefreshMode::Differential)
            .unwrap();

        assert_eq!(applied, RefreshMode::QuickClean);
        assert_eq!(display.baseline_state(), BaselineState::Synchronized);
        assert_eq!(
            display.previous_frame_storage(),
            PreviousFrameStorage::HostRam
        );
        assert!(host_frame(&display).iter().all(|byte| *byte == 0x00));
        assert_eq!(plane_bytes(&bus.events, CMD_WRITE_RAM_BW), next);
        assert_eq!(plane_bytes(&bus.events, CMD_WRITE_RAM_RED), next);
        assert!(bus.events.ends_with(&[
            Event::Command(CMD_DISPLAY_UPDATE_CTRL1, vec![0x40, 0x00]),
            Event::Command(0x1A, vec![0x5A]),
            Event::Command(CMD_DISPLAY_UPDATE_CTRL2, vec![0xD4]),
            Event::Command(CMD_MASTER_ACTIVATION, vec![]),
            Event::WaitReady,
        ]));
    }

    #[test]
    fn successful_refresh_becomes_the_next_differential_baseline() {
        let mut bus = FakeBus::default();
        let controller = Ssd1677::new().initialize(&mut bus).unwrap();
        let mut displayed = [0xFF; FRAME_BYTES];
        let first = [0x3C; FRAME_BYTES];
        let second = [0xA5; FRAME_BYTES];
        let mut display =
            BufferedDisplay::with_host_ram(controller, &mut displayed, Rotation::Degrees0);

        display
            .refresh(&mut bus, &first, RefreshMode::QuickClean)
            .unwrap();
        bus.events.clear();
        display
            .refresh(&mut bus, &second, RefreshMode::Differential)
            .unwrap();

        assert_eq!(plane_bytes(&bus.events, CMD_WRITE_RAM_BW), second);
        assert_eq!(plane_bytes(&bus.events, CMD_WRITE_RAM_RED), first);
        assert_eq!(host_frame(&display), &second);
    }

    #[test]
    fn openx4_differential_refresh_compares_new_bw_against_previous_red() {
        let mut bus = FakeBus::default();
        let controller = Ssd1677::new().initialize(&mut bus).unwrap();
        let mut displayed = [0x00; FRAME_BYTES];
        let next = [0xA5; FRAME_BYTES];
        let mut display =
            BufferedDisplay::with_host_ram(controller, &mut displayed, Rotation::Degrees0);
        display.baseline = BaselineState::Synchronized;
        bus.events.clear();

        let applied = display
            .refresh(&mut bus, &next, RefreshMode::Differential)
            .unwrap();

        assert_eq!(applied, RefreshMode::Differential);
        assert_eq!(plane_bytes(&bus.events, CMD_WRITE_RAM_BW), next);
        assert_eq!(
            plane_bytes(&bus.events, CMD_WRITE_RAM_RED),
            [0x00; FRAME_BYTES]
        );
        assert!(bus.events.ends_with(&[
            Event::Command(CMD_DISPLAY_UPDATE_CTRL1, vec![0x00, 0x00]),
            Event::Command(CMD_DISPLAY_UPDATE_CTRL2, vec![0x1C]),
            Event::Command(CMD_MASTER_ACTIVATION, vec![]),
            Event::WaitReady,
        ]));
    }

    #[test]
    fn controller_ram_differential_uses_retained_red_then_seeds_the_next_baseline() {
        let mut bus = FakeBus::default();
        let controller = Ssd1677::new().initialize(&mut bus).unwrap();
        let next = [0xA5; FRAME_BYTES];
        let mut display = BufferedDisplay::with_controller_ram(controller, Rotation::Degrees0);
        display.baseline = BaselineState::Synchronized;
        bus.events.clear();

        display
            .refresh(&mut bus, &next, RefreshMode::Differential)
            .unwrap();

        assert_eq!(
            display.previous_frame_storage(),
            PreviousFrameStorage::ControllerRam
        );
        assert_eq!(
            ram_begin_commands(&bus.events),
            [CMD_WRITE_RAM_BW, CMD_WRITE_RAM_RED]
        );
        let refresh_finished = bus
            .events
            .iter()
            .position(|event| *event == Event::WaitReady)
            .unwrap();
        let red_seed = bus
            .events
            .iter()
            .position(|event| *event == Event::BeginRam(CMD_WRITE_RAM_RED))
            .unwrap();
        assert!(red_seed > refresh_finished);
        assert_eq!(plane_bytes(&bus.events, CMD_WRITE_RAM_RED), next);
    }

    #[test]
    fn stock_controller_ram_reseeds_both_controller_planes() {
        let mut bus = FakeBus::default();
        let controller = Ssd1677::with_profile(X4DriveProfile::StockParity)
            .initialize(&mut bus)
            .unwrap();
        let next = [0xA5; FRAME_BYTES];
        let mut display = BufferedDisplay::with_controller_ram(controller, Rotation::Degrees0);
        display.baseline = BaselineState::Synchronized;
        bus.events.clear();

        display
            .refresh(&mut bus, &next, RefreshMode::Differential)
            .unwrap();

        assert_eq!(
            ram_begin_commands(&bus.events),
            [CMD_WRITE_RAM_BW, CMD_WRITE_RAM_BW, CMD_WRITE_RAM_RED]
        );
        assert_eq!(
            plane_bytes(&bus.events, CMD_WRITE_RAM_BW).len(),
            FRAME_BYTES * 2
        );
        assert_eq!(plane_bytes(&bus.events, CMD_WRITE_RAM_RED), next);
    }

    #[test]
    fn stock_clean_refreshes_use_absolute_sequences() {
        let mut bus = FakeBus::default();
        let mut controller = Ssd1677::with_profile(X4DriveProfile::StockParity)
            .initialize(&mut bus)
            .unwrap();
        bus.events.clear();

        controller
            .activate_refresh(&mut bus, RefreshMode::FullClean)
            .unwrap();

        assert_eq!(
            bus.events,
            [
                Event::Command(CMD_DISPLAY_UPDATE_CTRL1, vec![0x40, 0x00]),
                Event::Command(0x3C, vec![0xC0]),
                Event::Command(CMD_DISPLAY_UPDATE_CTRL2, vec![0xF7]),
                Event::Command(CMD_MASTER_ACTIVATION, vec![]),
                Event::WaitReady,
            ]
        );
        bus.events.clear();

        controller
            .activate_refresh(&mut bus, RefreshMode::QuickClean)
            .unwrap();

        assert_eq!(
            bus.events,
            [
                Event::Command(CMD_DISPLAY_UPDATE_CTRL1, vec![0x40, 0x00]),
                Event::Command(0x3C, vec![0xC0]),
                Event::Command(0x1A, vec![0x5A]),
                Event::Command(CMD_DISPLAY_UPDATE_CTRL2, vec![0xD7]),
                Event::Command(CMD_MASTER_ACTIVATION, vec![]),
                Event::WaitReady,
            ]
        );
    }

    #[test]
    fn stock_differential_refresh_uses_the_stock_partial_sequence() {
        let mut bus = FakeBus::default();
        let controller = Ssd1677::with_profile(X4DriveProfile::StockParity)
            .initialize(&mut bus)
            .unwrap();
        let mut displayed = [0x00; FRAME_BYTES];
        let next = [0xA5; FRAME_BYTES];
        let mut display =
            BufferedDisplay::with_host_ram(controller, &mut displayed, Rotation::Degrees0);
        display.baseline = BaselineState::Synchronized;
        bus.events.clear();

        display
            .refresh(&mut bus, &next, RefreshMode::Differential)
            .unwrap();

        assert!(bus.events.ends_with(&[
            Event::Command(CMD_DISPLAY_UPDATE_CTRL1, vec![0x00, 0x00]),
            Event::Command(0x3C, vec![0xC0]),
            Event::Command(CMD_DISPLAY_UPDATE_CTRL2, vec![0xFC]),
            Event::Command(CMD_MASTER_ACTIVATION, vec![]),
            Event::WaitReady,
        ]));
    }

    #[test]
    fn failed_refresh_invalidates_the_baseline_without_committing_the_frame() {
        let mut bus = FakeBus::default();
        let controller = Ssd1677::new().initialize(&mut bus).unwrap();
        let mut displayed = [0x00; FRAME_BYTES];
        let next = [0xA5; FRAME_BYTES];
        let mut display =
            BufferedDisplay::with_host_ram(controller, &mut displayed, Rotation::Degrees0);
        display.baseline = BaselineState::Synchronized;
        bus.events.clear();
        bus.fail_next_wait = true;

        let result = display.refresh(&mut bus, &next, RefreshMode::Differential);

        assert_eq!(result, Err(Error::Bus(FakeError::Wait)));
        assert_eq!(display.baseline_state(), BaselineState::Unknown);
        assert!(host_frame(&display).iter().all(|byte| *byte == 0x00));
    }

    #[test]
    fn deep_sleep_uses_the_ssd1677_check_code_without_waiting() {
        let mut bus = FakeBus::default();
        let display = Ssd1677::new().initialize(&mut bus).unwrap();
        bus.events.clear();

        display.enter_deep_sleep(&mut bus).unwrap();

        assert_eq!(bus.events, [Event::Command(CMD_DEEP_SLEEP, vec![0x03])]);
    }

    #[test]
    fn invalid_frame_length_does_not_touch_the_bus() {
        let mut bus = FakeBus::default();
        let mut display = Ssd1677::new().initialize(&mut bus).unwrap();
        bus.events.clear();

        let result = display.refresh_frame(&mut bus, &[0xFF; 32]);

        assert_eq!(
            result,
            Err(Error::InvalidFrameLength {
                expected: FRAME_BYTES,
                actual: 32,
            })
        );
        assert!(bus.events.is_empty());
    }

    fn host_frame<'a>(display: &'a BufferedDisplay<'_>) -> &'a [u8; FRAME_BYTES] {
        match &display.previous {
            PreviousFrame::HostRam(frame) => frame,
            PreviousFrame::ControllerRam => panic!("expected host RAM baseline"),
        }
    }

    fn ram_begin_commands(events: &[Event]) -> Vec<u8> {
        events
            .iter()
            .filter_map(|event| match event {
                Event::BeginRam(command) => Some(*command),
                _ => None,
            })
            .collect()
    }

    fn plane_bytes(events: &[Event], command: u8) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut collecting = false;
        for event in events {
            match event {
                Event::BeginRam(actual) => collecting = *actual == command,
                Event::Ram(chunk) if collecting => bytes.extend_from_slice(chunk),
                Event::EndRam => collecting = false,
                _ => {}
            }
        }
        bytes
    }

    #[test]
    fn ready_panel_refreshes_across_bus_sessions_without_reinitialization() {
        let mut bus = FakeBus::default();
        let mut display = Ssd1677::new().initialize(&mut bus).unwrap();
        bus.events.clear();

        display.refresh_white(&mut bus).unwrap();
        let first_session_events = bus.events.len();
        display.refresh_solid(&mut bus, 0x00).unwrap();

        assert!(
            !bus.events
                .iter()
                .any(|event| matches!(event, Event::Reset | Event::Command(0x12, _))),
            "a ready panel never re-runs the reset or software-reset sequence"
        );
        assert!(bus.events.len() > first_session_events);
    }
}

use super::framebuffer::Frame;

pub const WIDTH: usize = 800;
pub const HEIGHT: usize = 480;
pub const ROW_BYTES: usize = WIDTH / 8;
pub const FRAME_BYTES: usize = ROW_BYTES * HEIGHT;

const CMD_DRIVER_OUTPUT_CONTROL: u8 = 0x01;
const CMD_BOOSTER_SOFT_START: u8 = 0x0C;
const CMD_DEEP_SLEEP: u8 = 0x10;
const CMD_DATA_ENTRY_MODE: u8 = 0x11;
const CMD_SW_RESET: u8 = 0x12;
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

pub struct Ssd1677<B> {
    bus: B,
}

pub struct InitializedSsd1677<'a, B> {
    bus: &'a mut B,
}

impl<B> Ssd1677<B>
where
    B: DisplayBus,
{
    pub fn new(bus: B) -> Self {
        Self { bus }
    }

    pub fn initialize(&mut self) -> Result<InitializedSsd1677<'_, B>, Error<B::Error>> {
        for operation in INIT_SEQUENCE {
            match operation {
                InitOperation::Reset => self.bus.reset(),
                InitOperation::WaitReady => self.bus.wait_ready().map_err(Error::Bus)?,
                InitOperation::Command { command, data } => {
                    self.bus.command(*command, data).map_err(Error::Bus)?
                }
            }
        }

        Ok(InitializedSsd1677 { bus: &mut self.bus })
    }

    pub fn into_bus(self) -> B {
        self.bus
    }
}

impl<B> InitializedSsd1677<'_, B>
where
    B: DisplayBus,
{
    pub fn write_solid_frame(&mut self, value: u8) -> Result<(), Error<B::Error>> {
        self.prepare_full_window()?;
        self.write_solid_plane(CMD_WRITE_RAM_BW, value)?;
        self.write_solid_plane(CMD_WRITE_RAM_RED, value)
    }

    pub fn refresh_solid(&mut self, value: u8) -> Result<(), Error<B::Error>> {
        self.write_solid_frame(value)?;
        self.activate_full_refresh()
    }

    pub fn write_white_frame(&mut self) -> Result<(), Error<B::Error>> {
        self.write_solid_frame(0xFF)
    }

    pub fn refresh_white(&mut self) -> Result<(), Error<B::Error>> {
        self.refresh_solid(0xFF)
    }

    pub fn write_generated_frame<F>(&mut self, mut fill: F) -> Result<(), Error<B::Error>>
    where
        F: FnMut(usize, &mut [u8]),
    {
        self.prepare_full_window()?;
        self.write_generated_plane(CMD_WRITE_RAM_BW, &mut fill)?;
        self.write_generated_plane(CMD_WRITE_RAM_RED, &mut fill)
    }

    pub fn refresh_generated_frame<F>(&mut self, fill: F) -> Result<(), Error<B::Error>>
    where
        F: FnMut(usize, &mut [u8]),
    {
        self.write_generated_frame(fill)?;
        self.activate_full_refresh()
    }

    pub fn write_logical_frame(&mut self, frame: Frame<'_>) -> Result<(), Error<B::Error>> {
        self.write_generated_frame(|offset, output| frame.fill_panel_chunk(offset, output))
    }

    pub fn refresh_logical_frame(&mut self, frame: Frame<'_>) -> Result<(), Error<B::Error>> {
        self.write_logical_frame(frame)?;
        self.activate_full_refresh()
    }

    pub fn enter_deep_sleep(mut self) -> Result<(), Error<B::Error>> {
        self.command(CMD_DEEP_SLEEP, &[0x03])
    }

    pub fn write_frame(&mut self, frame: &[u8]) -> Result<(), Error<B::Error>> {
        if frame.len() != FRAME_BYTES {
            return Err(Error::InvalidFrameLength {
                expected: FRAME_BYTES,
                actual: frame.len(),
            });
        }

        self.prepare_full_window()?;
        self.write_frame_plane(CMD_WRITE_RAM_BW, frame)?;
        self.write_frame_plane(CMD_WRITE_RAM_RED, frame)
    }

    pub fn refresh_frame(&mut self, frame: &[u8]) -> Result<(), Error<B::Error>> {
        self.write_frame(frame)?;
        self.activate_full_refresh()
    }

    pub fn activate_full_refresh(&mut self) -> Result<(), Error<B::Error>> {
        self.command(CMD_DISPLAY_UPDATE_CTRL1, &[0x40, 0x00])?;
        self.command(CMD_DISPLAY_UPDATE_CTRL2, &[0xF4])?;
        self.command(CMD_MASTER_ACTIVATION, &[])?;
        self.bus.wait_ready().map_err(Error::Bus)
    }

    fn prepare_full_window(&mut self) -> Result<(), Error<B::Error>> {
        self.command(CMD_SET_RAM_X_RANGE, &RAM_X_RANGE)?;
        self.command(CMD_SET_RAM_Y_RANGE, &RAM_Y_RANGE)?;
        self.command(CMD_SET_RAM_X_COUNTER, &RAM_X_COUNTER)?;
        self.command(CMD_SET_RAM_Y_COUNTER, &RAM_Y_COUNTER)
    }

    fn write_solid_plane(&mut self, command: u8, value: u8) -> Result<(), Error<B::Error>> {
        let chunk = [value; SOLID_CHUNK];
        self.bus.begin_ram_write(command).map_err(Error::Bus)?;

        let mut remaining = FRAME_BYTES;
        while remaining > 0 {
            let length = remaining.min(SOLID_CHUNK);
            if let Err(error) = self.bus.write_ram(&chunk[..length]) {
                let _ = self.bus.end_ram_write();
                return Err(Error::Bus(error));
            }
            remaining -= length;
        }

        self.bus.end_ram_write().map_err(Error::Bus)
    }

    fn write_generated_plane<F>(&mut self, command: u8, fill: &mut F) -> Result<(), Error<B::Error>>
    where
        F: FnMut(usize, &mut [u8]),
    {
        let mut chunk = [0; SOLID_CHUNK];
        self.bus.begin_ram_write(command).map_err(Error::Bus)?;

        let mut offset = 0;
        while offset < FRAME_BYTES {
            let length = (FRAME_BYTES - offset).min(SOLID_CHUNK);
            fill(offset, &mut chunk[..length]);
            if let Err(error) = self.bus.write_ram(&chunk[..length]) {
                let _ = self.bus.end_ram_write();
                return Err(Error::Bus(error));
            }
            offset += length;
        }

        self.bus.end_ram_write().map_err(Error::Bus)
    }

    fn write_frame_plane(&mut self, command: u8, frame: &[u8]) -> Result<(), Error<B::Error>> {
        self.bus.begin_ram_write(command).map_err(Error::Bus)?;

        for chunk in frame.chunks(SOLID_CHUNK) {
            if let Err(error) = self.bus.write_ram(chunk) {
                let _ = self.bus.end_ram_write();
                return Err(Error::Bus(error));
            }
        }

        self.bus.end_ram_write().map_err(Error::Bus)
    }

    fn command(&mut self, command: u8, data: &[u8]) -> Result<(), Error<B::Error>> {
        self.bus.command(command, data).map_err(Error::Bus)
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::{vec, vec::Vec};

    use super::{
        CMD_DEEP_SLEEP, CMD_DISPLAY_UPDATE_CTRL1, CMD_DISPLAY_UPDATE_CTRL2, CMD_MASTER_ACTIVATION,
        CMD_WRITE_RAM_BW, CMD_WRITE_RAM_RED, DisplayBus, Error, FRAME_BYTES, Ssd1677,
    };

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
    fn initialization_matches_x4_golden_transcript() {
        let mut controller = Ssd1677::new(FakeBus::default());
        {
            let _display = controller.initialize().unwrap();
        }
        let bus = controller.into_bus();

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
    fn initialization_stops_on_busy_failure() {
        let bus = FakeBus {
            fail_next_wait: true,
            ..FakeBus::default()
        };

        let mut controller = Ssd1677::new(bus);
        let result = controller.initialize();

        assert!(matches!(result, Err(Error::Bus(FakeError::Wait))));
        assert_eq!(
            controller.into_bus().events,
            [Event::Reset, Event::Command(0x12, vec![]), Event::WaitReady]
        );
    }

    #[test]
    fn white_ram_stage_does_not_activate_the_panel() {
        let mut controller = Ssd1677::new(FakeBus::default());
        let mut display = controller.initialize().unwrap();
        display.bus.events.clear();

        display.write_white_frame().unwrap();

        assert!(!display.bus.events.iter().any(|event| matches!(
            event,
            Event::Command(CMD_MASTER_ACTIVATION, _)
                | Event::Command(CMD_DISPLAY_UPDATE_CTRL2, _)
                | Event::WaitReady
        )));
        assert_eq!(display.bus.events.last(), Some(&Event::EndRam));
    }

    #[test]
    fn white_refresh_writes_both_planes_and_activates() {
        let mut controller = Ssd1677::new(FakeBus::default());
        let mut display = controller.initialize().unwrap();
        display.bus.events.clear();

        display.refresh_white().unwrap();

        let begin_commands: Vec<u8> = display
            .bus
            .events
            .iter()
            .filter_map(|event| match event {
                Event::BeginRam(command) => Some(*command),
                _ => None,
            })
            .collect();
        let ram: Vec<&[u8]> = display
            .bus
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
        assert!(display.bus.events.ends_with(&[
            Event::Command(CMD_DISPLAY_UPDATE_CTRL1, vec![0x40, 0x00]),
            Event::Command(CMD_DISPLAY_UPDATE_CTRL2, vec![0xF4]),
            Event::Command(CMD_MASTER_ACTIVATION, vec![]),
            Event::WaitReady,
        ]));
    }

    #[test]
    fn frame_refresh_transfers_exactly_two_full_planes() {
        let mut controller = Ssd1677::new(FakeBus::default());
        let mut display = controller.initialize().unwrap();
        display.bus.events.clear();
        let frame = vec![0xA5; FRAME_BYTES];

        display.refresh_frame(&frame).unwrap();

        let ram: Vec<&[u8]> = display
            .bus
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
        let mut controller = Ssd1677::new(FakeBus::default());
        let mut display = controller.initialize().unwrap();
        display.bus.events.clear();

        display
            .write_generated_frame(|offset, output| {
                for (index, byte) in output.iter_mut().enumerate() {
                    *byte = ((offset + index) % 251) as u8;
                }
            })
            .unwrap();

        let ram: Vec<u8> = display
            .bus
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
    fn deep_sleep_uses_the_ssd1677_check_code_without_waiting() {
        let mut controller = Ssd1677::new(FakeBus::default());
        let mut display = controller.initialize().unwrap();
        display.bus.events.clear();

        display.enter_deep_sleep().unwrap();

        assert_eq!(
            controller.into_bus().events,
            [Event::Command(CMD_DEEP_SLEEP, vec![0x03])]
        );
    }

    #[test]
    fn invalid_frame_length_does_not_touch_the_bus() {
        let mut controller = Ssd1677::new(FakeBus::default());
        let mut display = controller.initialize().unwrap();
        display.bus.events.clear();

        let result = display.refresh_frame(&[0xFF; 32]);

        assert_eq!(
            result,
            Err(Error::InvalidFrameLength {
                expected: FRAME_BYTES,
                actual: 32,
            })
        );
        assert!(display.bus.events.is_empty());
    }
}

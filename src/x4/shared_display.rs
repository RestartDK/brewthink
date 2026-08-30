use core::convert::Infallible;

use embedded_hal::{
    delay::DelayNs,
    digital::{InputPin, OutputPin, StatefulOutputPin},
    spi::SpiBus,
};

use crate::display::ssd1677::DisplayBus;

use super::shared_spi::{SharedSpi, SharedSpiDevice, SharedSpiError};

pub const DISPLAY_BUSY_TIMEOUT_POLLS: usize = 15_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(target_arch = "riscv32", derive(defmt::Format))]
pub enum SharedDisplayError<E> {
    Spi(SharedSpiError<E>),
    BusyTimeout,
}

pub struct SharedDisplayBus<'a, SPI, DisplayCs, SdCs, DataCommand, Reset, Busy, Delay> {
    shared: &'a mut SharedSpi<SPI, DisplayCs, SdCs>,
    data_command: &'a mut DataCommand,
    reset: &'a mut Reset,
    busy: &'a mut Busy,
    delay: &'a mut Delay,
}

impl<'a, SPI, DisplayCs, SdCs, DataCommand, Reset, Busy, Delay>
    SharedDisplayBus<'a, SPI, DisplayCs, SdCs, DataCommand, Reset, Busy, Delay>
where
    SPI: SpiBus<u8>,
    DisplayCs: StatefulOutputPin<Error = Infallible>,
    SdCs: StatefulOutputPin<Error = Infallible>,
{
    pub fn new(
        shared: &'a mut SharedSpi<SPI, DisplayCs, SdCs>,
        data_command: &'a mut DataCommand,
        reset: &'a mut Reset,
        busy: &'a mut Busy,
        delay: &'a mut Delay,
    ) -> Self {
        Self {
            shared,
            data_command,
            reset,
            busy,
            delay,
        }
    }

    fn finish<T>(
        &mut self,
        operation: Result<T, SharedDisplayError<SPI::Error>>,
    ) -> Result<T, SharedDisplayError<SPI::Error>> {
        let end = self
            .shared
            .end(SharedSpiDevice::Display)
            .map_err(SharedDisplayError::Spi);
        match (operation, end) {
            (Err(error), _) | (Ok(_), Err(error)) => Err(error),
            (Ok(value), Ok(())) => Ok(value),
        }
    }
}

impl<SPI, DisplayCs, SdCs, DataCommand, Reset, Busy, Delay> DisplayBus
    for SharedDisplayBus<'_, SPI, DisplayCs, SdCs, DataCommand, Reset, Busy, Delay>
where
    SPI: SpiBus<u8>,
    DisplayCs: StatefulOutputPin<Error = Infallible>,
    SdCs: StatefulOutputPin<Error = Infallible>,
    DataCommand: OutputPin<Error = Infallible>,
    Reset: OutputPin<Error = Infallible>,
    Busy: InputPin<Error = Infallible>,
    Delay: DelayNs,
{
    type Error = SharedDisplayError<SPI::Error>;

    fn reset(&mut self) {
        let _ = self.reset.set_high();
        self.delay.delay_ms(20);
        let _ = self.reset.set_low();
        self.delay.delay_ms(2);
        let _ = self.reset.set_high();
        self.delay.delay_ms(20);
    }

    fn command(&mut self, command: u8, data: &[u8]) -> Result<(), Self::Error> {
        self.shared
            .begin(SharedSpiDevice::Display)
            .map_err(SharedDisplayError::Spi)?;
        let _ = self.data_command.set_low();
        let operation = (|| {
            self.shared
                .write(SharedSpiDevice::Display, &[command])
                .map_err(SharedDisplayError::Spi)?;
            self.shared
                .flush(SharedSpiDevice::Display)
                .map_err(SharedDisplayError::Spi)?;
            if !data.is_empty() {
                let _ = self.data_command.set_high();
                self.shared
                    .write(SharedSpiDevice::Display, data)
                    .map_err(SharedDisplayError::Spi)?;
            }
            Ok(())
        })();
        self.finish(operation)
    }

    fn begin_ram_write(&mut self, command: u8) -> Result<(), Self::Error> {
        self.shared
            .begin(SharedSpiDevice::Display)
            .map_err(SharedDisplayError::Spi)?;
        let _ = self.data_command.set_low();
        let command_result = self
            .shared
            .write(SharedSpiDevice::Display, &[command])
            .and_then(|()| self.shared.flush(SharedSpiDevice::Display))
            .map_err(SharedDisplayError::Spi);
        if let Err(error) = command_result {
            let _ = self.finish(Ok(()));
            return Err(error);
        }
        let _ = self.data_command.set_high();
        Ok(())
    }

    fn write_ram(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        self.shared
            .write(SharedSpiDevice::Display, data)
            .map_err(SharedDisplayError::Spi)
    }

    fn end_ram_write(&mut self) -> Result<(), Self::Error> {
        self.finish(Ok(()))
    }

    fn wait_ready(&mut self) -> Result<(), Self::Error> {
        self.delay.delay_ms(1);
        for _ in 0..DISPLAY_BUSY_TIMEOUT_POLLS {
            if self.busy.is_low().unwrap_or(false) {
                return Ok(());
            }
            self.delay.delay_ms(1);
        }
        Err(SharedDisplayError::BusyTimeout)
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use core::convert::Infallible;
    use std::{
        cell::{Cell, RefCell},
        collections::VecDeque,
        rc::Rc,
        vec,
        vec::Vec,
    };

    use embedded_hal::{
        delay::DelayNs,
        digital::{ErrorType as DigitalErrorType, InputPin, OutputPin, StatefulOutputPin},
        spi::{ErrorType as SpiErrorType, SpiBus},
    };

    use super::{
        DISPLAY_BUSY_TIMEOUT_POLLS, SharedDisplayBus, SharedDisplayError, SharedSpi, SharedSpiError,
    };
    use crate::display::ssd1677::DisplayBus;

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum Event {
        DisplayChipSelect(bool),
        SdChipSelect(bool),
        DataCommand(bool),
        Reset(bool),
        Spi(Vec<u8>),
        Flush,
        Delay(u32),
    }

    struct FakeChipSelect {
        display: bool,
        high: bool,
        events: Rc<RefCell<Vec<Event>>>,
    }

    impl DigitalErrorType for FakeChipSelect {
        type Error = Infallible;
    }

    impl OutputPin for FakeChipSelect {
        fn set_low(&mut self) -> Result<(), Self::Error> {
            self.high = false;
            self.events.borrow_mut().push(self.event(false));
            Ok(())
        }

        fn set_high(&mut self) -> Result<(), Self::Error> {
            self.high = true;
            self.events.borrow_mut().push(self.event(true));
            Ok(())
        }
    }

    impl StatefulOutputPin for FakeChipSelect {
        fn is_set_high(&mut self) -> Result<bool, Self::Error> {
            Ok(self.high)
        }

        fn is_set_low(&mut self) -> Result<bool, Self::Error> {
            Ok(!self.high)
        }
    }

    impl FakeChipSelect {
        fn event(&self, high: bool) -> Event {
            if self.display {
                Event::DisplayChipSelect(high)
            } else {
                Event::SdChipSelect(high)
            }
        }
    }

    struct FakeOutput {
        data_command: bool,
        events: Rc<RefCell<Vec<Event>>>,
    }

    impl DigitalErrorType for FakeOutput {
        type Error = Infallible;
    }

    impl OutputPin for FakeOutput {
        fn set_low(&mut self) -> Result<(), Self::Error> {
            self.events.borrow_mut().push(self.event(false));
            Ok(())
        }

        fn set_high(&mut self) -> Result<(), Self::Error> {
            self.events.borrow_mut().push(self.event(true));
            Ok(())
        }
    }

    impl FakeOutput {
        fn event(&self, high: bool) -> Event {
            if self.data_command {
                Event::DataCommand(high)
            } else {
                Event::Reset(high)
            }
        }
    }

    struct FakeBusy {
        levels: VecDeque<bool>,
    }

    impl DigitalErrorType for FakeBusy {
        type Error = Infallible;
    }

    impl InputPin for FakeBusy {
        fn is_high(&mut self) -> Result<bool, Self::Error> {
            Ok(self.levels.pop_front().unwrap_or(true))
        }

        fn is_low(&mut self) -> Result<bool, Self::Error> {
            self.is_high().map(|high| !high)
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum FakeSpiError {
        Write,
    }

    impl embedded_hal::spi::Error for FakeSpiError {
        fn kind(&self) -> embedded_hal::spi::ErrorKind {
            embedded_hal::spi::ErrorKind::Other
        }
    }

    struct FakeSpi {
        events: Rc<RefCell<Vec<Event>>>,
        fail_next_write: Rc<Cell<bool>>,
    }

    impl SpiErrorType for FakeSpi {
        type Error = FakeSpiError;
    }

    impl SpiBus<u8> for FakeSpi {
        fn read(&mut self, words: &mut [u8]) -> Result<(), Self::Error> {
            words.fill(0xFF);
            Ok(())
        }

        fn write(&mut self, words: &[u8]) -> Result<(), Self::Error> {
            if self.fail_next_write.replace(false) {
                return Err(FakeSpiError::Write);
            }
            self.events.borrow_mut().push(Event::Spi(words.into()));
            Ok(())
        }

        fn transfer(&mut self, read: &mut [u8], _write: &[u8]) -> Result<(), Self::Error> {
            read.fill(0xFF);
            Ok(())
        }

        fn transfer_in_place(&mut self, words: &mut [u8]) -> Result<(), Self::Error> {
            words.fill(0xFF);
            Ok(())
        }

        fn flush(&mut self) -> Result<(), Self::Error> {
            self.events.borrow_mut().push(Event::Flush);
            Ok(())
        }
    }

    struct FakeDelay {
        events: Rc<RefCell<Vec<Event>>>,
        elapsed_ms: Rc<Cell<usize>>,
    }

    impl DelayNs for FakeDelay {
        fn delay_ns(&mut self, ns: u32) {
            self.events.borrow_mut().push(Event::Delay(ns));
            self.elapsed_ms
                .set(self.elapsed_ms.get() + ns as usize / 1_000_000);
        }
    }

    struct Fixture {
        shared: SharedSpi<FakeSpi, FakeChipSelect, FakeChipSelect>,
        data_command: FakeOutput,
        reset: FakeOutput,
        busy: FakeBusy,
        delay: FakeDelay,
        events: Rc<RefCell<Vec<Event>>>,
        elapsed_ms: Rc<Cell<usize>>,
        fail_next_write: Rc<Cell<bool>>,
    }

    impl Fixture {
        fn new(busy_levels: &[bool]) -> Self {
            let events = Rc::new(RefCell::new(Vec::new()));
            let elapsed_ms = Rc::new(Cell::new(0));
            let fail_next_write = Rc::new(Cell::new(false));
            let fixture = Self {
                shared: SharedSpi::new(
                    FakeSpi {
                        events: Rc::clone(&events),
                        fail_next_write: Rc::clone(&fail_next_write),
                    },
                    FakeChipSelect {
                        display: true,
                        high: false,
                        events: Rc::clone(&events),
                    },
                    FakeChipSelect {
                        display: false,
                        high: false,
                        events: Rc::clone(&events),
                    },
                ),
                data_command: FakeOutput {
                    data_command: true,
                    events: Rc::clone(&events),
                },
                reset: FakeOutput {
                    data_command: false,
                    events: Rc::clone(&events),
                },
                busy: FakeBusy {
                    levels: busy_levels.iter().copied().collect(),
                },
                delay: FakeDelay {
                    events: Rc::clone(&events),
                    elapsed_ms: Rc::clone(&elapsed_ms),
                },
                events,
                elapsed_ms,
                fail_next_write,
            };
            fixture.events.borrow_mut().clear();
            fixture
        }

        fn bus(
            &mut self,
        ) -> SharedDisplayBus<
            '_,
            FakeSpi,
            FakeChipSelect,
            FakeChipSelect,
            FakeOutput,
            FakeOutput,
            FakeBusy,
            FakeDelay,
        > {
            SharedDisplayBus::new(
                &mut self.shared,
                &mut self.data_command,
                &mut self.reset,
                &mut self.busy,
                &mut self.delay,
            )
        }

        fn events(&self) -> Vec<Event> {
            self.events.borrow().clone()
        }
    }

    #[test]
    fn reset_uses_reference_timing() {
        let mut fixture = Fixture::new(&[]);

        fixture.bus().reset();

        assert_eq!(
            fixture.events(),
            vec![
                Event::Reset(true),
                Event::Delay(20_000_000),
                Event::Reset(false),
                Event::Delay(2_000_000),
                Event::Reset(true),
                Event::Delay(20_000_000),
            ]
        );
    }

    #[test]
    fn command_selects_display_before_dropping_data_command() {
        let mut fixture = Fixture::new(&[]);

        fixture.bus().command(0x01, &[0xDF, 0x01, 0x02]).unwrap();

        assert_eq!(
            fixture.events(),
            vec![
                Event::DisplayChipSelect(true),
                Event::SdChipSelect(true),
                Event::DisplayChipSelect(false),
                Event::DataCommand(false),
                Event::Spi(vec![0x01]),
                Event::Flush,
                Event::DataCommand(true),
                Event::Spi(vec![0xDF, 0x01, 0x02]),
                Event::Flush,
                Event::DisplayChipSelect(true),
                Event::SdChipSelect(true),
            ]
        );
    }

    #[test]
    fn command_without_data_keeps_data_command_low() {
        let mut fixture = Fixture::new(&[]);

        fixture.bus().command(0x12, &[]).unwrap();

        assert_eq!(
            fixture.events(),
            vec![
                Event::DisplayChipSelect(true),
                Event::SdChipSelect(true),
                Event::DisplayChipSelect(false),
                Event::DataCommand(false),
                Event::Spi(vec![0x12]),
                Event::Flush,
                Event::Flush,
                Event::DisplayChipSelect(true),
                Event::SdChipSelect(true),
            ]
        );
    }

    #[test]
    fn spi_failure_still_deselects_both_devices() {
        let mut fixture = Fixture::new(&[]);
        fixture.fail_next_write.set(true);

        let result = fixture.bus().command(0x01, &[0xDF]);

        assert_eq!(
            result,
            Err(SharedDisplayError::Spi(SharedSpiError::Spi(
                FakeSpiError::Write
            )))
        );
        assert_eq!(
            fixture.events(),
            vec![
                Event::DisplayChipSelect(true),
                Event::SdChipSelect(true),
                Event::DisplayChipSelect(false),
                Event::DataCommand(false),
                Event::Flush,
                Event::DisplayChipSelect(true),
                Event::SdChipSelect(true),
            ]
        );
    }

    #[test]
    fn ram_write_holds_chip_select_until_end() {
        let mut fixture = Fixture::new(&[]);
        {
            let mut bus = fixture.bus();
            bus.begin_ram_write(0x24).unwrap();
            bus.write_ram(&[0xAA; 256]).unwrap();
            bus.write_ram(&[0x55; 256]).unwrap();
            bus.end_ram_write().unwrap();
        }

        assert_eq!(
            fixture.events(),
            vec![
                Event::DisplayChipSelect(true),
                Event::SdChipSelect(true),
                Event::DisplayChipSelect(false),
                Event::DataCommand(false),
                Event::Spi(vec![0x24]),
                Event::Flush,
                Event::DataCommand(true),
                Event::Spi(vec![0xAA; 256]),
                Event::Spi(vec![0x55; 256]),
                Event::Flush,
                Event::DisplayChipSelect(true),
                Event::SdChipSelect(true),
            ]
        );
    }

    #[test]
    fn wait_ready_returns_when_busy_clears() {
        let mut fixture = Fixture::new(&[true, true, false]);

        fixture.bus().wait_ready().unwrap();

        assert_eq!(fixture.elapsed_ms.get(), 3);
    }

    #[test]
    fn wait_ready_times_out_after_poll_budget() {
        let mut fixture = Fixture::new(&[]);

        let result = fixture.bus().wait_ready();

        assert_eq!(result, Err(SharedDisplayError::BusyTimeout));
        assert_eq!(fixture.elapsed_ms.get(), DISPLAY_BUSY_TIMEOUT_POLLS + 1);
    }
}

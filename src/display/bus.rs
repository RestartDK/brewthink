use core::convert::Infallible;

use embedded_hal::{
    delay::DelayNs,
    digital::{InputPin, OutputPin},
    spi::SpiBus,
};

use super::ssd1677::DisplayBus;

const BUSY_TIMEOUT_POLLS: usize = 15_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error<E> {
    Spi(E),
    BusyTimeout,
}

pub struct EpdBus<SPI, CS, DC, RESET, BUSY, DELAY> {
    spi: SPI,
    chip_select: CS,
    data_command: DC,
    reset: RESET,
    busy: BUSY,
    delay: DELAY,
}

impl<SPI, CS, DC, RESET, BUSY, DELAY> EpdBus<SPI, CS, DC, RESET, BUSY, DELAY>
where
    SPI: SpiBus<u8>,
    CS: OutputPin<Error = Infallible>,
    DC: OutputPin<Error = Infallible>,
    RESET: OutputPin<Error = Infallible>,
    BUSY: InputPin<Error = Infallible>,
    DELAY: DelayNs,
{
    pub fn new(
        spi: SPI,
        mut chip_select: CS,
        mut data_command: DC,
        mut reset: RESET,
        busy: BUSY,
        delay: DELAY,
    ) -> Self {
        let _ = chip_select.set_high();
        let _ = data_command.set_high();
        let _ = reset.set_high();

        Self {
            spi,
            chip_select,
            data_command,
            reset,
            busy,
            delay,
        }
    }

    fn deselect(&mut self) {
        let _ = self.chip_select.set_high();
    }
}

impl<SPI, CS, DC, RESET, BUSY, DELAY> DisplayBus for EpdBus<SPI, CS, DC, RESET, BUSY, DELAY>
where
    SPI: SpiBus<u8>,
    CS: OutputPin<Error = Infallible>,
    DC: OutputPin<Error = Infallible>,
    RESET: OutputPin<Error = Infallible>,
    BUSY: InputPin<Error = Infallible>,
    DELAY: DelayNs,
{
    type Error = Error<SPI::Error>;

    fn reset(&mut self) {
        let _ = self.reset.set_high();
        self.delay.delay_ms(20);
        let _ = self.reset.set_low();
        self.delay.delay_ms(2);
        let _ = self.reset.set_high();
        self.delay.delay_ms(20);
    }

    fn command(&mut self, command: u8, data: &[u8]) -> Result<(), Self::Error> {
        let _ = self.data_command.set_low();
        let _ = self.chip_select.set_low();

        let result = (|| {
            self.spi.write(&[command]).map_err(Error::Spi)?;
            self.spi.flush().map_err(Error::Spi)?;

            if !data.is_empty() {
                let _ = self.data_command.set_high();
                self.spi.write(data).map_err(Error::Spi)?;
                self.spi.flush().map_err(Error::Spi)?;
            }

            Ok(())
        })();
        self.deselect();
        result
    }

    fn begin_ram_write(&mut self, command: u8) -> Result<(), Self::Error> {
        let _ = self.data_command.set_low();
        let _ = self.chip_select.set_low();

        let result = self
            .spi
            .write(&[command])
            .and_then(|()| self.spi.flush())
            .map_err(Error::Spi);
        if result.is_err() {
            self.deselect();
            return result;
        }

        let _ = self.data_command.set_high();
        Ok(())
    }

    fn write_ram(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        self.spi.write(data).map_err(Error::Spi)
    }

    fn end_ram_write(&mut self) -> Result<(), Self::Error> {
        let result = self.spi.flush().map_err(Error::Spi);
        self.deselect();
        result
    }

    fn wait_ready(&mut self) -> Result<(), Self::Error> {
        self.delay.delay_ms(1);

        for _ in 0..BUSY_TIMEOUT_POLLS {
            if self.busy.is_low().unwrap_or(false) {
                return Ok(());
            }
            self.delay.delay_ms(1);
        }

        Err(Error::BusyTimeout)
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
        digital::{ErrorType as DigitalErrorType, InputPin, OutputPin},
        spi::{ErrorType as SpiErrorType, SpiBus},
    };

    use super::{BUSY_TIMEOUT_POLLS, EpdBus, Error};
    use crate::display::ssd1677::DisplayBus;

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum Event {
        Pin(&'static str, bool),
        Spi(Vec<u8>),
        Flush,
        Delay(u32),
    }

    struct FakeOutput {
        name: &'static str,
        events: Rc<RefCell<Vec<Event>>>,
    }

    impl DigitalErrorType for FakeOutput {
        type Error = Infallible;
    }

    impl OutputPin for FakeOutput {
        fn set_low(&mut self) -> Result<(), Self::Error> {
            self.events.borrow_mut().push(Event::Pin(self.name, false));
            Ok(())
        }

        fn set_high(&mut self) -> Result<(), Self::Error> {
            self.events.borrow_mut().push(Event::Pin(self.name, true));
            Ok(())
        }
    }

    struct FakeInput {
        levels: VecDeque<bool>,
    }

    impl DigitalErrorType for FakeInput {
        type Error = Infallible;
    }

    impl InputPin for FakeInput {
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
            words.fill(0);
            Ok(())
        }

        fn write(&mut self, words: &[u8]) -> Result<(), Self::Error> {
            if self.fail_next_write.replace(false) {
                return Err(FakeSpiError::Write);
            }
            self.events.borrow_mut().push(Event::Spi(words.into()));
            Ok(())
        }

        fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), Self::Error> {
            read.fill(0);
            self.write(write)
        }

        fn transfer_in_place(&mut self, words: &mut [u8]) -> Result<(), Self::Error> {
            self.events.borrow_mut().push(Event::Spi(words.into()));
            words.fill(0);
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

    type FakeEpdBus = EpdBus<FakeSpi, FakeOutput, FakeOutput, FakeOutput, FakeInput, FakeDelay>;

    fn fake_bus(
        levels: &[bool],
    ) -> (
        FakeEpdBus,
        Rc<RefCell<Vec<Event>>>,
        Rc<Cell<usize>>,
        Rc<Cell<bool>>,
    ) {
        let events = Rc::new(RefCell::new(Vec::new()));
        let elapsed_ms = Rc::new(Cell::new(0));
        let fail_next_write = Rc::new(Cell::new(false));
        let output = |name| FakeOutput {
            name,
            events: events.clone(),
        };

        let bus = EpdBus::new(
            FakeSpi {
                events: events.clone(),
                fail_next_write: fail_next_write.clone(),
            },
            output("cs"),
            output("dc"),
            output("reset"),
            FakeInput {
                levels: levels.iter().copied().collect(),
            },
            FakeDelay {
                events: events.clone(),
                elapsed_ms: elapsed_ms.clone(),
            },
        );

        events.borrow_mut().clear();
        (bus, events, elapsed_ms, fail_next_write)
    }

    #[test]
    fn reset_uses_reference_timing() {
        let (mut bus, events, _, _) = fake_bus(&[]);

        bus.reset();

        assert_eq!(
            *events.borrow(),
            vec![
                Event::Pin("reset", true),
                Event::Delay(20_000_000),
                Event::Pin("reset", false),
                Event::Delay(2_000_000),
                Event::Pin("reset", true),
                Event::Delay(20_000_000),
            ]
        );
    }

    #[test]
    fn command_holds_chip_select_across_command_and_data() {
        let (mut bus, events, _, _) = fake_bus(&[]);

        bus.command(0x01, &[0xDF, 0x01, 0x02]).unwrap();

        assert_eq!(
            *events.borrow(),
            vec![
                Event::Pin("dc", false),
                Event::Pin("cs", false),
                Event::Spi(vec![0x01]),
                Event::Flush,
                Event::Pin("dc", true),
                Event::Spi(vec![0xDF, 0x01, 0x02]),
                Event::Flush,
                Event::Pin("cs", true),
            ]
        );
    }

    #[test]
    fn spi_failure_deselects_display() {
        let (mut bus, events, _, fail_next_write) = fake_bus(&[]);
        fail_next_write.set(true);

        let result = bus.command(0x01, &[0xDF, 0x01, 0x02]);

        assert_eq!(result, Err(Error::Spi(FakeSpiError::Write)));
        assert_eq!(
            *events.borrow(),
            vec![
                Event::Pin("dc", false),
                Event::Pin("cs", false),
                Event::Pin("cs", true),
            ]
        );
    }

    #[test]
    fn busy_wait_returns_when_active_high_signal_clears() {
        let (mut bus, _, elapsed_ms, _) = fake_bus(&[true, true, false]);

        bus.wait_ready().unwrap();

        assert_eq!(elapsed_ms.get(), 3);
    }

    #[test]
    fn busy_wait_times_out() {
        let (mut bus, _, elapsed_ms, _) = fake_bus(&[]);

        let result = bus.wait_ready();

        assert_eq!(result, Err(Error::BusyTimeout));
        assert_eq!(elapsed_ms.get(), BUSY_TIMEOUT_POLLS + 1);
    }
}

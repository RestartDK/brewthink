use core::convert::Infallible;

use embedded_hal::{digital::StatefulOutputPin, spi::SpiBus};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(target_arch = "riscv32", derive(defmt::Format))]
pub enum SharedSpiDevice {
    Display,
    SdCard,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(target_arch = "riscv32", derive(defmt::Format))]
pub enum SharedSpiError<E> {
    Spi(E),
    Busy {
        active: SharedSpiDevice,
        requested: SharedSpiDevice,
    },
    NotSelected(SharedSpiDevice),
}

pub struct SharedSpi<SPI, DisplayCs, SdCs> {
    spi: SPI,
    display_chip_select: DisplayCs,
    sd_chip_select: SdCs,
    selected: Option<SharedSpiDevice>,
}

impl<SPI, DisplayCs, SdCs> SharedSpi<SPI, DisplayCs, SdCs>
where
    SPI: SpiBus<u8>,
    DisplayCs: StatefulOutputPin<Error = Infallible>,
    SdCs: StatefulOutputPin<Error = Infallible>,
{
    pub fn new(spi: SPI, mut display_chip_select: DisplayCs, mut sd_chip_select: SdCs) -> Self {
        let _ = display_chip_select.set_high();
        let _ = sd_chip_select.set_high();

        Self {
            spi,
            display_chip_select,
            sd_chip_select,
            selected: None,
        }
    }

    pub fn begin(&mut self, device: SharedSpiDevice) -> Result<(), SharedSpiError<SPI::Error>> {
        if let Some(active) = self.selected {
            return Err(SharedSpiError::Busy {
                active,
                requested: device,
            });
        }

        self.deselect_both();
        match device {
            SharedSpiDevice::Display => {
                let _ = self.display_chip_select.set_low();
            }
            SharedSpiDevice::SdCard => {
                let _ = self.sd_chip_select.set_low();
            }
        }
        self.selected = Some(device);
        Ok(())
    }

    pub fn write(
        &mut self,
        device: SharedSpiDevice,
        words: &[u8],
    ) -> Result<(), SharedSpiError<SPI::Error>> {
        self.require_selected(device)?;
        self.spi.write(words).map_err(SharedSpiError::Spi)
    }

    pub fn transfer_in_place(
        &mut self,
        device: SharedSpiDevice,
        words: &mut [u8],
    ) -> Result<(), SharedSpiError<SPI::Error>> {
        self.require_selected(device)?;
        self.spi
            .transfer_in_place(words)
            .map_err(SharedSpiError::Spi)
    }

    pub fn flush(&mut self, device: SharedSpiDevice) -> Result<(), SharedSpiError<SPI::Error>> {
        self.require_selected(device)?;
        self.spi.flush().map_err(SharedSpiError::Spi)
    }

    pub fn end(&mut self, device: SharedSpiDevice) -> Result<(), SharedSpiError<SPI::Error>> {
        self.require_selected(device)?;
        let result = self.spi.flush().map_err(SharedSpiError::Spi);
        self.deselect_both();
        self.selected = None;
        result
    }

    pub fn idle_clocks(&mut self, byte_count: usize) -> Result<(), SharedSpiError<SPI::Error>> {
        if let Some(active) = self.selected {
            return Err(SharedSpiError::Busy {
                active,
                requested: SharedSpiDevice::SdCard,
            });
        }

        self.deselect_both();
        const CLOCKS: [u8; 16] = [0xFF; 16];
        let mut remaining = byte_count;
        while remaining > 0 {
            let count = remaining.min(CLOCKS.len());
            self.spi
                .write(&CLOCKS[..count])
                .map_err(SharedSpiError::Spi)?;
            remaining -= count;
        }
        self.spi.flush().map_err(SharedSpiError::Spi)
    }

    pub fn spi_mut(&mut self) -> Result<&mut SPI, SharedSpiError<SPI::Error>> {
        if let Some(active) = self.selected {
            return Err(SharedSpiError::Busy {
                active,
                requested: active,
            });
        }
        self.deselect_both();
        Ok(&mut self.spi)
    }

    pub fn display_is_deselected(&mut self) -> bool {
        self.display_chip_select.is_set_high().unwrap_or(false)
    }

    pub fn sd_is_deselected(&mut self) -> bool {
        self.sd_chip_select.is_set_high().unwrap_or(false)
    }

    pub fn both_are_deselected(&mut self) -> bool {
        self.selected.is_none() && self.display_is_deselected() && self.sd_is_deselected()
    }

    fn require_selected(&self, device: SharedSpiDevice) -> Result<(), SharedSpiError<SPI::Error>> {
        match self.selected {
            Some(active) if active == device => Ok(()),
            Some(active) => Err(SharedSpiError::Busy {
                active,
                requested: device,
            }),
            None => Err(SharedSpiError::NotSelected(device)),
        }
    }

    fn deselect_both(&mut self) {
        let _ = self.display_chip_select.set_high();
        let _ = self.sd_chip_select.set_high();
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use core::convert::Infallible;
    use std::{cell::RefCell, rc::Rc, vec, vec::Vec};

    use embedded_hal::{
        digital::{ErrorType as DigitalErrorType, OutputPin, StatefulOutputPin},
        spi::{ErrorType as SpiErrorType, SpiBus},
    };

    use super::{SharedSpi, SharedSpiDevice, SharedSpiError};

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Event {
        Display(bool),
        Sd(bool),
        Write(usize),
        Transfer(usize),
        Flush,
    }

    struct FakePin {
        device: SharedSpiDevice,
        high: bool,
        events: Rc<RefCell<Vec<Event>>>,
    }

    impl DigitalErrorType for FakePin {
        type Error = Infallible;
    }

    impl OutputPin for FakePin {
        fn set_low(&mut self) -> Result<(), Self::Error> {
            self.high = false;
            self.events.borrow_mut().push(match self.device {
                SharedSpiDevice::Display => Event::Display(false),
                SharedSpiDevice::SdCard => Event::Sd(false),
            });
            Ok(())
        }

        fn set_high(&mut self) -> Result<(), Self::Error> {
            self.high = true;
            self.events.borrow_mut().push(match self.device {
                SharedSpiDevice::Display => Event::Display(true),
                SharedSpiDevice::SdCard => Event::Sd(true),
            });
            Ok(())
        }
    }

    impl StatefulOutputPin for FakePin {
        fn is_set_high(&mut self) -> Result<bool, Self::Error> {
            Ok(self.high)
        }

        fn is_set_low(&mut self) -> Result<bool, Self::Error> {
            Ok(!self.high)
        }
    }

    struct FakeSpi {
        events: Rc<RefCell<Vec<Event>>>,
    }

    impl SpiErrorType for FakeSpi {
        type Error = Infallible;
    }

    impl SpiBus<u8> for FakeSpi {
        fn read(&mut self, words: &mut [u8]) -> Result<(), Self::Error> {
            words.fill(0xFF);
            Ok(())
        }

        fn write(&mut self, words: &[u8]) -> Result<(), Self::Error> {
            self.events.borrow_mut().push(Event::Write(words.len()));
            Ok(())
        }

        fn transfer(&mut self, read: &mut [u8], _write: &[u8]) -> Result<(), Self::Error> {
            read.fill(0xFF);
            Ok(())
        }

        fn transfer_in_place(&mut self, words: &mut [u8]) -> Result<(), Self::Error> {
            self.events.borrow_mut().push(Event::Transfer(words.len()));
            words.fill(0xFF);
            Ok(())
        }

        fn flush(&mut self) -> Result<(), Self::Error> {
            self.events.borrow_mut().push(Event::Flush);
            Ok(())
        }
    }

    fn fake_shared_spi() -> (
        SharedSpi<FakeSpi, FakePin, FakePin>,
        Rc<RefCell<Vec<Event>>>,
    ) {
        let events = Rc::new(RefCell::new(Vec::new()));
        let spi = FakeSpi {
            events: Rc::clone(&events),
        };
        let display = FakePin {
            device: SharedSpiDevice::Display,
            high: false,
            events: Rc::clone(&events),
        };
        let sd = FakePin {
            device: SharedSpiDevice::SdCard,
            high: false,
            events: Rc::clone(&events),
        };
        (SharedSpi::new(spi, display, sd), events)
    }

    #[test]
    fn construction_and_idle_clocks_keep_both_devices_deselected() {
        let (mut shared, events) = fake_shared_spi();
        assert!(shared.both_are_deselected());

        events.borrow_mut().clear();
        shared.idle_clocks(18).unwrap();

        assert_eq!(
            *events.borrow(),
            vec![
                Event::Display(true),
                Event::Sd(true),
                Event::Write(16),
                Event::Write(2),
                Event::Flush,
            ]
        );
        assert!(shared.both_are_deselected());
    }

    #[test]
    fn an_sd_session_deselects_display_before_selecting_sd() {
        let (mut shared, events) = fake_shared_spi();
        events.borrow_mut().clear();

        shared.begin(SharedSpiDevice::SdCard).unwrap();
        shared.write(SharedSpiDevice::SdCard, &[1, 2]).unwrap();
        shared.end(SharedSpiDevice::SdCard).unwrap();

        assert_eq!(
            *events.borrow(),
            vec![
                Event::Display(true),
                Event::Sd(true),
                Event::Sd(false),
                Event::Write(2),
                Event::Flush,
                Event::Display(true),
                Event::Sd(true),
            ]
        );
        assert!(shared.both_are_deselected());
    }

    #[test]
    fn a_second_device_cannot_start_during_an_active_session() {
        let (mut shared, _) = fake_shared_spi();
        shared.begin(SharedSpiDevice::Display).unwrap();

        assert_eq!(
            shared.begin(SharedSpiDevice::SdCard),
            Err(SharedSpiError::Busy {
                active: SharedSpiDevice::Display,
                requested: SharedSpiDevice::SdCard,
            })
        );
        assert!(matches!(
            shared.write(SharedSpiDevice::SdCard, &[0xFF]),
            Err(SharedSpiError::Busy { .. })
        ));
        shared.end(SharedSpiDevice::Display).unwrap();
        assert!(shared.both_are_deselected());
    }

    #[test]
    fn an_active_display_session_can_flush_before_changing_phase() {
        let (mut shared, events) = fake_shared_spi();
        events.borrow_mut().clear();

        shared.begin(SharedSpiDevice::Display).unwrap();
        shared.write(SharedSpiDevice::Display, &[0x24]).unwrap();
        shared.flush(SharedSpiDevice::Display).unwrap();
        shared
            .write(SharedSpiDevice::Display, &[0xAA, 0x55])
            .unwrap();
        shared.end(SharedSpiDevice::Display).unwrap();

        assert_eq!(
            *events.borrow(),
            vec![
                Event::Display(true),
                Event::Sd(true),
                Event::Display(false),
                Event::Write(1),
                Event::Flush,
                Event::Write(2),
                Event::Flush,
                Event::Display(true),
                Event::Sd(true),
            ]
        );
    }
}

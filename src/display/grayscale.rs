use embedded_hal::delay::DelayNs;

use super::{
    framebuffer::{Rotation, fill_panel_chunk_with},
    ssd1677::{DisplayBus, Error, FRAME_BYTES, Ssd1677, X4DriveProfile},
};
use crate::image::{PackedBitmap, PixelDepth};

pub(super) fn paint<B: DisplayBus, D: DelayNs>(
    bus: &mut B,
    delay: &mut D,
    image: PackedBitmap<'_>,
    rotation: Rotation,
) -> Result<(), Error<B::Error>> {
    let mut bus = SettledBus { inner: bus, delay };
    let display = Ssd1677::with_profile(X4DriveProfile::StockParity).initialize(&mut bus)?;
    write_pass(&mut bus, &FACTORY_WAVEFORM, 0xC7, |bit, offset, output| {
        fill_image_plane(image, rotation, bit, offset, output);
    })?;
    display.enter_deep_sleep(&mut bus)
}

fn fill_image_plane(
    image: PackedBitmap<'_>,
    rotation: Rotation,
    bit: u8,
    offset: usize,
    output: &mut [u8],
) {
    fill_panel_chunk_with(rotation, offset, output, |x, y| {
        let level = image.level(x, y);
        let state = match image.depth() {
            PixelDepth::Monochrome => {
                if level == 0 {
                    3
                } else {
                    0
                }
            }
            PixelDepth::Four => 3 - level,
        };
        state & (1 << bit) == 0
    });
}

pub(super) fn write_pass<B: DisplayBus>(
    bus: &mut B,
    waveform: &[u8; 110],
    control2: u8,
    mut fill: impl FnMut(u8, usize, &mut [u8]),
) -> Result<(), Error<B::Error>> {
    for (command, bit) in [(0x24, 0), (0x26, 1)] {
        bus.command(0x4E, &[0x00, 0x00]).map_err(Error::Bus)?;
        bus.command(0x4F, &[0xDF, 0x01]).map_err(Error::Bus)?;
        bus.begin_ram_write(command).map_err(Error::Bus)?;
        let mut bytes = [0; 256];
        for offset in (0..FRAME_BYTES).step_by(bytes.len()) {
            let count = bytes.len().min(FRAME_BYTES - offset);
            fill(bit, offset, &mut bytes[..count]);
            if let Err(error) = bus.write_ram(&bytes[..count]) {
                let _ = bus.end_ram_write();
                return Err(Error::Bus(error));
            }
        }
        bus.end_ram_write().map_err(Error::Bus)?;
    }
    for (command, bytes) in [
        (0x32, &waveform[..105]),
        (0x03, &waveform[105..106]),
        (0x04, &waveform[106..109]),
        (0x2C, &waveform[109..110]),
        (0x3C, &[0xC0][..]),
        (0x21, &[0x00, 0x00][..]),
        (0x22, &[control2][..]),
        (0x20, &[][..]),
    ] {
        bus.command(command, bytes).map_err(Error::Bus)?;
    }
    bus.wait_ready().map_err(Error::Bus)
}

pub(super) struct SettledBus<'a, B, D> {
    pub(super) inner: &'a mut B,
    pub(super) delay: &'a mut D,
}

impl<B: DisplayBus, D: DelayNs> DisplayBus for SettledBus<'_, B, D> {
    type Error = B::Error;
    fn reset(&mut self) {
        self.inner.reset();
    }
    fn command(&mut self, command: u8, bytes: &[u8]) -> Result<(), Self::Error> {
        self.inner.command(command, bytes)?;
        // BUSY can assert after the first sample following reset or activation.
        if command == 0x12 || command == 0x20 {
            self.delay.delay_ms(10);
        }
        Ok(())
    }
    fn begin_ram_write(&mut self, command: u8) -> Result<(), Self::Error> {
        self.inner.begin_ram_write(command)
    }
    fn write_ram(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        self.inner.write_ram(bytes)
    }
    fn end_ram_write(&mut self) -> Result<(), Self::Error> {
        self.inner.end_ram_write()
    }
    fn wait_ready(&mut self) -> Result<(), Self::Error> {
        self.inner.wait_ready()
    }
}

pub(super) const FACTORY_WAVEFORM: [u8; 110] = [
    0x00, 0x4A, 0x88, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x62, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x88, 0x60, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xA8, 0x44,
    0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x08, 0x0B, 0x02, 0x03, 0x00, 0x0C, 0x02, 0x07, 0x02, 0x00, 0x01, 0x00, 0x02, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x01, 0x22, 0x22, 0x22, 0x22, 0x22, 0x17, 0x41, 0xA8, 0x32, 0x30,
];

#[cfg(feature = "grayscale-bench")]
pub(super) const ADJUSTMENT_WAVEFORM: [u8; 110] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x54, 0x54, 0x40, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0xAA, 0xA0, 0xA8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xA2, 0x22,
    0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x01, 0x01, 0x01, 0x01, 0x00, 0x01, 0x01, 0x01, 0x01, 0x00, 0x01, 0x01, 0x01, 0x01,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x8F, 0x8F, 0x8F, 0x8F, 0x8F, 0x17, 0x41, 0xA8, 0x32, 0x30,
];

#[cfg(feature = "grayscale-bench")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct TransitionRecipe {
    pub(super) base: u8,
    pub(super) adjustment: u8,
}

#[cfg(feature = "grayscale-bench")]
pub(super) const EIGHT_RECIPES: [TransitionRecipe; 8] = [
    TransitionRecipe {
        base: 3,
        adjustment: 0,
    },
    TransitionRecipe {
        base: 2,
        adjustment: 1,
    },
    TransitionRecipe {
        base: 2,
        adjustment: 0,
    },
    TransitionRecipe {
        base: 1,
        adjustment: 1,
    },
    TransitionRecipe {
        base: 0,
        adjustment: 1,
    },
    TransitionRecipe {
        base: 1,
        adjustment: 0,
    },
    TransitionRecipe {
        base: 1,
        adjustment: 2,
    },
    TransitionRecipe {
        base: 0,
        adjustment: 0,
    },
];

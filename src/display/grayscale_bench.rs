use embedded_hal::delay::DelayNs;

use super::ssd1677::{DisplayBus, Error, FRAME_BYTES, Ssd1677, WIDTH, X4DriveProfile};

const FACTORY_WAVEFORM: [u8; 110] = [
    0x00, 0x4A, 0x88, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x62, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x88, 0x60, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xA8, 0x44,
    0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x08, 0x0B, 0x02, 0x03, 0x00, 0x0C, 0x02, 0x07, 0x02, 0x00, 0x01, 0x00, 0x02, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x01, 0x22, 0x22, 0x22, 0x22, 0x22, 0x17, 0x41, 0xA8, 0x32, 0x30,
];

const ADJUSTMENT_WAVEFORM: [u8; 110] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x54, 0x54, 0x40, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0xAA, 0xA0, 0xA8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xA2, 0x22,
    0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x01, 0x01, 0x01, 0x01, 0x00, 0x01, 0x01, 0x01, 0x01, 0x00, 0x01, 0x01, 0x01, 0x01,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x8F, 0x8F, 0x8F, 0x8F, 0x8F, 0x17, 0x41, 0xA8, 0x32, 0x30,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Pattern {
    White,
    Black,
    Binary,
    FourStates,
    SixteenTransitions,
    EightToneRepeat,
}

impl Pattern {
    pub const fn name(self) -> &'static str {
        match self {
            Self::White => "white",
            Self::Black => "black",
            Self::Binary => "binary",
            Self::FourStates => "four",
            Self::SixteenTransitions => "sixteen-probe",
            Self::EightToneRepeat => "eight-repeat",
        }
    }

    pub const fn is_probe(self) -> bool {
        match self {
            Self::SixteenTransitions | Self::EightToneRepeat => true,
            Self::White | Self::Black | Self::Binary | Self::FourStates => false,
        }
    }

    fn passes(self) -> &'static [PaintPass] {
        match self {
            Self::White => &[PaintPass::White],
            Self::Black => &[PaintPass::Black],
            Self::Binary => &[PaintPass::Binary],
            Self::FourStates => &[PaintPass::FourStates],
            Self::SixteenTransitions => &[
                PaintPass::ProbeBase(ProbeLayout::SixteenTransitions),
                PaintPass::ProbeAdjustment(ProbeLayout::SixteenTransitions),
            ],
            Self::EightToneRepeat => &[
                PaintPass::ProbeBase(ProbeLayout::EightToneRepeat),
                PaintPass::ProbeAdjustment(ProbeLayout::EightToneRepeat),
            ],
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TransitionRecipe {
    base: u8,
    adjustment: u8,
}

const EIGHT_RECIPES: [TransitionRecipe; 8] = [
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

const EIGHT_LAYOUT: [[usize; 4]; 4] = [[0, 4, 2, 6], [7, 3, 5, 1], [6, 2, 4, 0], [1, 5, 3, 7]];

#[derive(Clone, Copy)]
enum ProbeLayout {
    SixteenTransitions,
    EightToneRepeat,
}

impl ProbeLayout {
    fn recipe(self, row: u8, column: u8) -> TransitionRecipe {
        match self {
            Self::SixteenTransitions => TransitionRecipe {
                base: column,
                adjustment: row,
            },
            Self::EightToneRepeat => {
                EIGHT_RECIPES[EIGHT_LAYOUT[usize::from(row)][usize::from(column)]]
            }
        }
    }
}

#[derive(Clone, Copy)]
enum PaintPass {
    White,
    Black,
    Binary,
    FourStates,
    ProbeBase(ProbeLayout),
    ProbeAdjustment(ProbeLayout),
}

impl PaintPass {
    fn waveform(self) -> Option<(&'static [u8; 110], u8)> {
        match self {
            Self::White | Self::Black | Self::Binary => None,
            Self::FourStates | Self::ProbeBase(_) => Some((&FACTORY_WAVEFORM, 0xC7)),
            Self::ProbeAdjustment(_) => Some((&ADJUSTMENT_WAVEFORM, 0xCF)),
        }
    }

    fn ram_state(self, x: usize, y: usize) -> u8 {
        match self {
            Self::White => return 3,
            Self::Black => return 0,
            _ => {}
        }
        let state = if (32..448).contains(&x)
            && (144..656).contains(&y)
            && (x - 32) % 104 < 88
            && (y - 144) % 128 < 104
        {
            let column = ((x - 32) / 104) as u8;
            let row = ((y - 144) / 128) as u8;
            match self {
                Self::ProbeBase(layout) => layout.recipe(row, column).base,
                Self::ProbeAdjustment(layout) => layout.recipe(row, column).adjustment,
                _ => (column + row) % 4,
            }
        } else if matches!(self, Self::ProbeAdjustment(_)) {
            0
        } else if (32..448).contains(&x) && (48..112).contains(&y) {
            if x < 240 { 0 } else { 3 }
        } else {
            0
        };
        match self {
            Self::FourStates | Self::ProbeBase(_) | Self::ProbeAdjustment(_) => state,
            Self::Binary => {
                if state >= 2 {
                    3
                } else {
                    0
                }
            }
            _ => unreachable!(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Command {
    Status,
    Paint(Pattern),
}

impl Command {
    pub fn parse(line: &[u8]) -> Option<Self> {
        match line {
            b"BREWGRAY/1 status" => Some(Self::Status),
            b"BREWGRAY/1 white" => Some(Self::Paint(Pattern::White)),
            b"BREWGRAY/1 black" => Some(Self::Paint(Pattern::Black)),
            b"BREWGRAY/1 binary" => Some(Self::Paint(Pattern::Binary)),
            b"BREWGRAY/1 four" => Some(Self::Paint(Pattern::FourStates)),
            b"BREWGRAY/1 sixteen-probe" => Some(Self::Paint(Pattern::SixteenTransitions)),
            b"BREWGRAY/1 eight-repeat" => Some(Self::Paint(Pattern::EightToneRepeat)),
            _ => None,
        }
    }
}

#[derive(Default)]
pub struct CommandLines {
    bytes: [u8; 32],
    length: usize,
    overflow: bool,
}

impl CommandLines {
    pub fn push(&mut self, byte: u8) -> Option<Option<Command>> {
        if byte == b'\n' {
            let command = if self.overflow {
                None
            } else {
                Command::parse(&self.bytes[..self.length])
            };
            self.length = 0;
            self.overflow = false;
            return Some(command);
        }
        if self.length == self.bytes.len() {
            self.overflow = true;
        } else {
            self.bytes[self.length] = byte;
            self.length += 1;
        }
        None
    }
}

pub fn paint<B: DisplayBus, D: DelayNs>(
    bus: &mut B,
    delay: &mut D,
    pattern: Pattern,
) -> Result<(), Error<B::Error>> {
    let mut bus = SettledBus { inner: bus, delay };
    let mut display = Ssd1677::with_profile(X4DriveProfile::StockParity).initialize(&mut bus)?;
    for &pass in pattern.passes() {
        let Some((waveform, control2)) = pass.waveform() else {
            display.refresh_generated_frame(&mut bus, |offset, output| {
                fill_plane(pass, 0, offset, output)
            })?;
            continue;
        };
        for (command, bit) in [(0x24, 0), (0x26, 1)] {
            bus.command(0x4E, &[0x00, 0x00]).map_err(Error::Bus)?;
            bus.command(0x4F, &[0xDF, 0x01]).map_err(Error::Bus)?;
            bus.begin_ram_write(command).map_err(Error::Bus)?;
            let mut bytes = [0; 256];
            for offset in (0..FRAME_BYTES).step_by(bytes.len()) {
                let count = bytes.len().min(FRAME_BYTES - offset);
                fill_plane(pass, bit, offset, &mut bytes[..count]);
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
        bus.wait_ready().map_err(Error::Bus)?;
    }
    display.enter_deep_sleep(&mut bus)
}

fn fill_plane(pass: PaintPass, bit: u8, offset: usize, output: &mut [u8]) {
    for (index, byte) in output.iter_mut().enumerate() {
        *byte = 0;
        for shift in 0..8 {
            let pixel = (offset + index) * 8 + shift;
            let x = 479 - pixel / WIDTH;
            let y = pixel % WIDTH;
            *byte |= ((pass.ram_state(x, y) >> bit) & 1) << (7 - shift);
        }
    }
}

struct SettledBus<'a, B, D> {
    inner: &'a mut B,
    delay: &'a mut D,
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

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::{vec, vec::Vec};

    #[derive(Default)]
    struct Bus {
        commands: Vec<(u8, Vec<u8>)>,
        planes: Vec<(u8, Vec<u8>)>,
        waits: usize,
        fail_wait: Option<usize>,
        fail_write: bool,
        fail_plane: Option<usize>,
        selected: bool,
        resets: usize,
    }

    impl DisplayBus for Bus {
        type Error = ();
        fn reset(&mut self) {
            self.resets += 1;
        }
        fn command(&mut self, cmd: u8, data: &[u8]) -> Result<(), ()> {
            self.commands.push((cmd, data.to_vec()));
            Ok(())
        }
        fn begin_ram_write(&mut self, cmd: u8) -> Result<(), ()> {
            self.selected = true;
            self.planes.push((cmd, Vec::new()));
            Ok(())
        }
        fn write_ram(&mut self, data: &[u8]) -> Result<(), ()> {
            if self.fail_write || self.fail_plane == Some(self.planes.len()) {
                return Err(());
            }
            self.planes.last_mut().unwrap().1.extend_from_slice(data);
            Ok(())
        }
        fn end_ram_write(&mut self) -> Result<(), ()> {
            self.selected = false;
            Ok(())
        }
        fn wait_ready(&mut self) -> Result<(), ()> {
            self.waits += 1;
            if self.fail_wait == Some(self.waits) {
                Err(())
            } else {
                Ok(())
            }
        }
    }
    #[derive(Default)]
    struct Delay(Vec<u32>);
    impl DelayNs for Delay {
        fn delay_ns(&mut self, ns: u32) {
            self.0.push(ns);
        }
    }

    #[test]
    fn factory_transaction_writes_two_planes_one_lut_and_one_activation() {
        let mut bus = Bus::default();
        let mut delay = Delay::default();
        paint(&mut bus, &mut delay, Pattern::FourStates).unwrap();
        assert_eq!(bus.resets, 1);
        assert_eq!(bus.waits, 4);
        assert_eq!(bus.planes.len(), 2);
        assert_eq!(bus.planes[0].0, 0x24);
        assert_eq!(bus.planes[1].0, 0x26);
        assert!(
            bus.planes
                .iter()
                .all(|(_, bytes)| bytes.len() == FRAME_BYTES)
        );
        assert_eq!(
            bus.commands.iter().filter(|(cmd, _)| *cmd == 0x20).count(),
            1
        );
        let tail = &bus.commands[bus.commands.len() - 9..];
        assert_eq!(
            tail,
            &[
                (0x32, FACTORY_WAVEFORM[..105].to_vec()),
                (0x03, vec![0x17]),
                (0x04, vec![0x41, 0xA8, 0x32]),
                (0x2C, vec![0x30]),
                (0x3C, vec![0xC0]),
                (0x21, vec![0, 0]),
                (0x22, vec![0xC7]),
                (0x20, vec![]),
                (0x10, vec![3]),
            ]
        );
        assert_eq!(delay.0, vec![10_000_000, 10_000_000]);
        assert!(!bus.selected);
        assert!(
            bus.commands
                .iter()
                .all(|(cmd, _)| ![0x28, 0x2A, 0x30, 0x36, 0x39].contains(cmd))
        );
    }

    #[test]
    fn binary_reference_uses_no_custom_waveform_or_voltage_writes() {
        let mut bus = Bus::default();
        paint(&mut bus, &mut Delay::default(), Pattern::Binary).unwrap();
        assert_eq!(bus.planes[0].1, bus.planes[1].1);
        assert!(bus.commands.contains(&(0x22, vec![0xF7])));
        assert!(
            bus.commands
                .iter()
                .all(|(cmd, _)| ![0x32, 0x03, 0x04, 0x2C].contains(cmd))
        );
        assert_eq!(bus.commands.last(), Some(&(0x10, vec![3])));
    }

    #[test]
    fn panel_rotation_preserves_uniform_patch_codes() {
        let mut planes = [vec![0; FRAME_BYTES], vec![0; FRAME_BYTES]];
        for (bit, plane) in planes.iter_mut().enumerate() {
            for (chunk, bytes) in plane.chunks_mut(127).enumerate() {
                fill_plane(PaintPass::FourStates, bit as u8, chunk * 127, bytes);
            }
        }
        for row in 0..4 {
            for col in 0..4 {
                for y in 144 + row * 128..248 + row * 128 {
                    for x in 32 + col * 104..120 + col * 104 {
                        let panel_pixel = (479 - x) * 800 + y;
                        let bit = 7 - panel_pixel % 8;
                        let state = ((planes[0][panel_pixel / 8] >> bit) & 1)
                            | (((planes[1][panel_pixel / 8] >> bit) & 1) << 1);
                        assert_eq!(usize::from(state), (row + col) % 4);
                    }
                }
            }
        }
    }

    #[test]
    fn busy_failure_aborts_without_activation_or_retry() {
        let mut bus = Bus {
            fail_wait: Some(1),
            ..Bus::default()
        };
        assert_eq!(
            paint(&mut bus, &mut Delay::default(), Pattern::FourStates),
            Err(Error::Bus(()))
        );
        assert_eq!(bus.resets, 1);
        assert!(!bus.commands.iter().any(|(cmd, _)| *cmd == 0x20));
        assert!(bus.planes.is_empty());
    }

    #[test]
    fn transfer_failure_releases_selection_without_activation() {
        let mut bus = Bus {
            fail_write: true,
            ..Bus::default()
        };
        assert_eq!(
            paint(&mut bus, &mut Delay::default(), Pattern::FourStates),
            Err(Error::Bus(()))
        );
        assert!(!bus.selected);
        assert!(!bus.commands.iter().any(|(cmd, _)| *cmd == 0x20));
    }

    #[test]
    fn failed_waveform_wait_does_not_sleep_or_retry() {
        let mut bus = Bus {
            fail_wait: Some(4),
            ..Bus::default()
        };
        assert!(paint(&mut bus, &mut Delay::default(), Pattern::FourStates).is_err());
        assert_eq!(bus.commands.last(), Some(&(0x20, vec![])));
        assert_eq!(
            bus.commands.iter().filter(|(cmd, _)| *cmd == 0x20).count(),
            1
        );
    }

    #[test]
    fn transition_probe_has_exactly_two_fixed_activations_and_then_sleeps() {
        let mut bus = Bus::default();
        let mut delay = Delay::default();
        paint(&mut bus, &mut delay, Pattern::SixteenTransitions).unwrap();
        assert_eq!(bus.resets, 1);
        assert_eq!(bus.waits, 5);
        assert_eq!(
            bus.planes
                .iter()
                .map(|(command, _)| *command)
                .collect::<Vec<_>>(),
            vec![0x24, 0x26, 0x24, 0x26]
        );
        assert!(
            bus.planes
                .iter()
                .all(|(_, bytes)| bytes.len() == FRAME_BYTES)
        );
        let luts: Vec<_> = bus
            .commands
            .iter()
            .filter(|(command, _)| *command == 0x32)
            .map(|(_, bytes)| bytes.as_slice())
            .collect();
        assert_eq!(
            luts,
            vec![&FACTORY_WAVEFORM[..105], &ADJUSTMENT_WAVEFORM[..105]]
        );
        let controls: Vec<_> = bus
            .commands
            .iter()
            .filter(|(command, _)| *command == 0x22)
            .map(|(_, bytes)| bytes.as_slice())
            .collect();
        assert_eq!(controls, vec![&[0xC7][..], &[0xCF][..]]);
        assert_eq!(
            bus.commands
                .iter()
                .filter(|(command, _)| *command == 0x20)
                .count(),
            2
        );
        for (command, expected) in [
            (0x03, &[0x17][..]),
            (0x04, &[0x41, 0xA8, 0x32][..]),
            (0x2C, &[0x30][..]),
        ] {
            assert_eq!(
                bus.commands
                    .iter()
                    .filter(|(cmd, _)| *cmd == command)
                    .count(),
                2
            );
            assert!(
                bus.commands
                    .iter()
                    .filter(|(cmd, _)| *cmd == command)
                    .all(|(_, bytes)| bytes == expected)
            );
        }
        assert_eq!(bus.commands.last(), Some(&(0x10, vec![3])));
        assert_eq!(delay.0, vec![10_000_000; 3]);
        assert!(!bus.selected);
        assert!(
            bus.commands
                .iter()
                .all(|(cmd, _)| ![0x28, 0x2A, 0x30, 0x36, 0x39].contains(cmd))
        );
    }

    #[test]
    fn transition_probe_transmits_all_sixteen_uniform_input_pairs() {
        let mut bus = Bus::default();
        paint(&mut bus, &mut Delay::default(), Pattern::SixteenTransitions).unwrap();
        for row in 0..4 {
            for column in 0..4 {
                for y in 144 + row * 128..248 + row * 128 {
                    for x in 32 + column * 104..120 + column * 104 {
                        let pixel = (479 - x) * 800 + y;
                        let shift = 7 - pixel % 8;
                        let base = ((bus.planes[0].1[pixel / 8] >> shift) & 1)
                            | (((bus.planes[1].1[pixel / 8] >> shift) & 1) << 1);
                        let adjustment = ((bus.planes[2].1[pixel / 8] >> shift) & 1)
                            | (((bus.planes[3].1[pixel / 8] >> shift) & 1) << 1);
                        assert_eq!((usize::from(base), usize::from(adjustment)), (column, row));
                    }
                }
            }
        }
        for (x, y) in [
            (0, 0),
            (100, 80),
            (300, 80),
            (200, 700),
            (125, 150),
            (400, 799),
        ] {
            assert_eq!(
                PaintPass::ProbeAdjustment(ProbeLayout::SixteenTransitions).ram_state(x, y),
                0
            );
        }
    }

    #[test]
    fn failed_base_wait_does_not_start_the_adjustment() {
        let mut bus = Bus {
            fail_wait: Some(4),
            ..Bus::default()
        };
        assert!(paint(&mut bus, &mut Delay::default(), Pattern::SixteenTransitions).is_err());
        assert_eq!(bus.planes.len(), 2);
        assert_eq!(
            bus.commands.iter().filter(|(cmd, _)| *cmd == 0x20).count(),
            1
        );
        assert!(!bus.commands.contains(&(0x22, vec![0xCF])));
    }

    #[test]
    fn failed_adjustment_transfer_releases_selection_without_activation() {
        let mut bus = Bus {
            fail_plane: Some(3),
            ..Bus::default()
        };
        assert!(paint(&mut bus, &mut Delay::default(), Pattern::SixteenTransitions).is_err());
        assert!(!bus.selected);
        assert_eq!(bus.planes.len(), 3);
        assert_eq!(
            bus.commands.iter().filter(|(cmd, _)| *cmd == 0x20).count(),
            1
        );
    }

    #[test]
    fn stock_waveform_frame_budgets_and_voltage_tails_are_fixed() {
        for (lut, frames) in [(&FACTORY_WAVEFORM, 50), (&ADJUSTMENT_WAVEFORM, 12)] {
            let total: usize = lut[50..100]
                .chunks_exact(5)
                .map(|group| {
                    group[..4]
                        .iter()
                        .map(|count| usize::from(*count))
                        .sum::<usize>()
                        * (usize::from(group[4]) + 1)
                })
                .sum();
            assert_eq!(total, frames);
            assert_eq!(&lut[105..], &[0x17, 0x41, 0xA8, 0x32, 0x30]);
        }
    }

    #[test]
    fn eight_repeat_changes_only_ram_data_not_the_drive_sequence() {
        let mut original = Bus::default();
        let mut repeat = Bus::default();
        paint(
            &mut original,
            &mut Delay::default(),
            Pattern::SixteenTransitions,
        )
        .unwrap();
        paint(&mut repeat, &mut Delay::default(), Pattern::EightToneRepeat).unwrap();
        assert_eq!(repeat.commands, original.commands);
        assert_eq!(repeat.waits, original.waits);
        assert_eq!(repeat.resets, original.resets);
        assert_eq!(repeat.planes.len(), 4);
        assert!(
            repeat
                .planes
                .iter()
                .all(|(_, bytes)| bytes.len() == FRAME_BYTES)
        );
        assert!(!repeat.selected);
        assert_ne!(repeat.planes[0].1, original.planes[0].1);
        assert_ne!(repeat.planes[2].1, original.planes[2].1);
    }

    #[test]
    fn eight_repeat_transmits_two_permuted_copies_of_the_frozen_palette() {
        let expected_recipes = [
            (3, 0),
            (2, 1),
            (2, 0),
            (1, 1),
            (0, 1),
            (1, 0),
            (1, 2),
            (0, 0),
        ];
        let expected_layout = [[0, 4, 2, 6], [7, 3, 5, 1], [6, 2, 4, 0], [1, 5, 3, 7]];
        let mut counts = [0; 8];
        let mut bus = Bus::default();
        paint(&mut bus, &mut Delay::default(), Pattern::EightToneRepeat).unwrap();
        for (row, columns) in expected_layout.iter().enumerate() {
            for (column, &tone) in columns.iter().enumerate() {
                counts[tone] += 1;
                for y in 144 + row * 128..248 + row * 128 {
                    for x in 32 + column * 104..120 + column * 104 {
                        let pixel = (479 - x) * 800 + y;
                        let shift = 7 - pixel % 8;
                        let base = ((bus.planes[0].1[pixel / 8] >> shift) & 1)
                            | (((bus.planes[1].1[pixel / 8] >> shift) & 1) << 1);
                        let adjustment = ((bus.planes[2].1[pixel / 8] >> shift) & 1)
                            | (((bus.planes[3].1[pixel / 8] >> shift) & 1) << 1);
                        assert_eq!((base, adjustment), expected_recipes[tone]);
                    }
                }
            }
        }
        assert_eq!(counts, [2; 8]);
        for (x, y) in [
            (0, 0),
            (100, 80),
            (300, 80),
            (200, 700),
            (125, 150),
            (400, 799),
        ] {
            assert_eq!(
                PaintPass::ProbeAdjustment(ProbeLayout::EightToneRepeat).ram_state(x, y),
                0
            );
        }
    }

    #[test]
    fn both_probe_commands_are_classified_as_one_shot_probes() {
        assert!(Pattern::SixteenTransitions.is_probe());
        assert!(Pattern::EightToneRepeat.is_probe());
        for pattern in [
            Pattern::White,
            Pattern::Black,
            Pattern::Binary,
            Pattern::FourStates,
        ] {
            assert!(!pattern.is_probe());
        }
    }

    #[test]
    fn eight_repeat_stops_before_adjustment_if_base_wait_fails() {
        let mut bus = Bus {
            fail_wait: Some(4),
            ..Bus::default()
        };
        assert!(paint(&mut bus, &mut Delay::default(), Pattern::EightToneRepeat).is_err());
        assert_eq!(bus.planes.len(), 2);
        assert_eq!(
            bus.commands.iter().filter(|(cmd, _)| *cmd == 0x20).count(),
            1
        );
        assert!(!bus.commands.contains(&(0x22, vec![0xCF])));
    }

    #[test]
    fn command_parser_rejects_unknown_depths_and_overflow() {
        assert_eq!(
            Command::parse(b"BREWGRAY/1 four"),
            Some(Command::Paint(Pattern::FourStates))
        );
        assert_eq!(
            Command::parse(b"BREWGRAY/1 sixteen-probe"),
            Some(Command::Paint(Pattern::SixteenTransitions))
        );
        assert_eq!(
            Command::parse(b"BREWGRAY/1 eight-repeat"),
            Some(Command::Paint(Pattern::EightToneRepeat))
        );
        assert_eq!(Command::parse(b"BREWGRAY/1 sixteen"), None);
        assert_eq!(Command::parse(b"BREWGRAY/1 eight"), None);
        assert_eq!(Command::parse(b"BREWGRAY/1 four trailing"), None);
        let mut lines = CommandLines::default();
        for _ in 0..64 {
            assert!(lines.push(b'x').is_none());
        }
        assert_eq!(lines.push(b'\n'), Some(None));
        for byte in b"BREWGRAY/1 status" {
            assert!(lines.push(*byte).is_none());
        }
        assert_eq!(lines.push(b'\n'), Some(Some(Command::Status)));
    }
}

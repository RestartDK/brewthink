const CMD0_GO_IDLE: u8 = 0;
const CMD8_SEND_IF_COND: u8 = 8;
const CMD9_SEND_CSD: u8 = 9;
const CMD16_SET_BLOCKLEN: u8 = 16;
const CMD17_READ_SINGLE_BLOCK: u8 = 17;
#[cfg(feature = "sd-write-diagnostic")]
const CMD24_WRITE_SINGLE_BLOCK: u8 = 24;
const CMD41_APP_SEND_OP_COND: u8 = 41;
const CMD55_APP_CMD: u8 = 55;
const CMD58_READ_OCR: u8 = 58;
const CMD59_CRC_ON_OFF: u8 = 59;

const R1_IDLE: u8 = 0x01;
const R1_ILLEGAL_COMMAND: u8 = 0x04;
const INIT_ATTEMPTS: usize = 1_000;
const READY_POLLS: usize = 10_000;
const RESPONSE_POLLS: usize = 8;
const DATA_TOKEN_POLLS: usize = 10_000;
const DATA_START_TOKEN: u8 = 0xFE;
#[cfg(feature = "sd-write-diagnostic")]
const DATA_RESPONSE_ACCEPTED: u8 = 0x05;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SdSpiClock {
    Initialization,
    Transfer,
}

pub trait ReadOnlySdSpi {
    type Error;

    fn set_clock(&mut self, clock: SdSpiClock) -> Result<(), Self::Error>;
    fn idle_clocks(&mut self, byte_count: usize) -> Result<(), Self::Error>;
    fn begin_sd(&mut self) -> Result<(), Self::Error>;
    fn write(&mut self, bytes: &[u8]) -> Result<(), Self::Error>;
    fn transfer_in_place(&mut self, bytes: &mut [u8]) -> Result<(), Self::Error>;
    fn end_sd(&mut self) -> Result<(), Self::Error>;
    fn delay_us(&mut self, microseconds: u32);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CardVersion {
    Version1,
    Version2,
}

impl CardVersion {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Version1 => "v1",
            Self::Version2 => "v2",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CardType {
    StandardCapacity,
    HighCapacity,
}

impl CardType {
    pub const fn name(self) -> &'static str {
        match self {
            Self::StandardCapacity => "sdsc",
            Self::HighCapacity => "sdhc_or_sdxc",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CardInfo {
    pub version: CardVersion,
    pub card_type: CardType,
    pub block_count: u64,
}

impl CardInfo {
    pub const fn capacity_bytes(self) -> u64 {
        self.block_count * Sector::LEN as u64
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SdProtocolError {
    NotInitialized,
    CardDidNotEnterIdle,
    ResponseTimeout(u8),
    ReadyTimeout(u8),
    UnexpectedResponse {
        command: u8,
        response: u8,
    },
    InterfaceConditionMismatch([u8; 4]),
    InitializationTimeout,
    OperatingVoltageUnsupported,
    DataTokenTimeout(u8),
    DataErrorToken(u8),
    DataCrcMismatch {
        expected: u16,
        actual: u16,
    },
    UnsupportedCsdVersion(u8),
    InvalidCapacity,
    AddressOverflow(u32),
    BlockOutOfRange {
        block: u32,
        block_count: u64,
    },
    #[cfg(feature = "sd-write-diagnostic")]
    WriteRejected(u8),
    #[cfg(feature = "sd-write-diagnostic")]
    WriteBusyTimeout,
}

impl SdProtocolError {
    pub const fn name(self) -> &'static str {
        match self {
            Self::NotInitialized => "not_initialized",
            Self::CardDidNotEnterIdle => "card_did_not_enter_idle",
            Self::ResponseTimeout(_) => "response_timeout",
            Self::ReadyTimeout(_) => "ready_timeout",
            Self::UnexpectedResponse { .. } => "unexpected_response",
            Self::InterfaceConditionMismatch(_) => "interface_condition_mismatch",
            Self::InitializationTimeout => "initialization_timeout",
            Self::OperatingVoltageUnsupported => "operating_voltage_unsupported",
            Self::DataTokenTimeout(_) => "data_token_timeout",
            Self::DataErrorToken(_) => "data_error_token",
            Self::DataCrcMismatch { .. } => "data_crc_mismatch",
            Self::UnsupportedCsdVersion(_) => "unsupported_csd_version",
            Self::InvalidCapacity => "invalid_capacity",
            Self::AddressOverflow(_) => "address_overflow",
            Self::BlockOutOfRange { .. } => "block_out_of_range",
            #[cfg(feature = "sd-write-diagnostic")]
            Self::WriteRejected(_) => "write_rejected",
            #[cfg(feature = "sd-write-diagnostic")]
            Self::WriteBusyTimeout => "write_busy_timeout",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SdError<E> {
    Bus(E),
    Protocol(SdProtocolError),
}

impl<E> SdError<E> {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Bus(_) => "bus",
            Self::Protocol(error) => error.name(),
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct Sector {
    bytes: [u8; Self::LEN],
}

impl Sector {
    pub const LEN: usize = 512;

    pub const fn zeroed() -> Self {
        Self {
            bytes: [0; Self::LEN],
        }
    }

    pub const fn as_bytes(&self) -> &[u8; Self::LEN] {
        &self.bytes
    }

    pub fn as_bytes_mut(&mut self) -> &mut [u8; Self::LEN] {
        &mut self.bytes
    }
}

pub struct ReadOnlySdCard<B> {
    bus: B,
    info: Option<CardInfo>,
}

#[cfg(feature = "sd-write-diagnostic")]
pub struct ExplicitWriteSdCard<B> {
    inner: ReadOnlySdCard<B>,
}

impl<B> ReadOnlySdCard<B>
where
    B: ReadOnlySdSpi,
{
    pub const fn new(bus: B) -> Self {
        Self { bus, info: None }
    }

    pub fn initialize(&mut self) -> Result<CardInfo, SdError<B::Error>> {
        self.bus
            .set_clock(SdSpiClock::Initialization)
            .map_err(SdError::Bus)?;
        self.bus.idle_clocks(10).map_err(SdError::Bus)?;

        let mut idle = false;
        for _ in 0..10 {
            if self.command(CMD0_GO_IDLE, 0, &mut [])? == R1_IDLE {
                idle = true;
                break;
            }
        }
        if !idle {
            return Err(SdError::Protocol(SdProtocolError::CardDidNotEnterIdle));
        }

        let crc_response = self.command(CMD59_CRC_ON_OFF, 1, &mut [])?;
        self.expect_response(CMD59_CRC_ON_OFF, crc_response, R1_IDLE)?;

        let mut interface_condition = [0xFF; 4];
        let cmd8 = self.command(CMD8_SEND_IF_COND, 0x0000_01AA, &mut interface_condition)?;
        let version = if cmd8 == R1_IDLE {
            if interface_condition != [0x00, 0x00, 0x01, 0xAA] {
                return Err(SdError::Protocol(
                    SdProtocolError::InterfaceConditionMismatch(interface_condition),
                ));
            }
            CardVersion::Version2
        } else if cmd8 & R1_ILLEGAL_COMMAND != 0 {
            CardVersion::Version1
        } else {
            return Err(SdError::Protocol(SdProtocolError::UnexpectedResponse {
                command: CMD8_SEND_IF_COND,
                response: cmd8,
            }));
        };

        let acmd41_argument = if version == CardVersion::Version2 {
            0x4000_0000
        } else {
            0
        };
        let mut initialized = false;
        for _ in 0..INIT_ATTEMPTS {
            let app = self.command(CMD55_APP_CMD, 0, &mut [])?;
            if app != 0 && app != R1_IDLE {
                return Err(SdError::Protocol(SdProtocolError::UnexpectedResponse {
                    command: CMD55_APP_CMD,
                    response: app,
                }));
            }
            let response = self.command(CMD41_APP_SEND_OP_COND, acmd41_argument, &mut [])?;
            if response == 0 {
                initialized = true;
                break;
            }
            if response != R1_IDLE {
                return Err(SdError::Protocol(SdProtocolError::UnexpectedResponse {
                    command: CMD41_APP_SEND_OP_COND,
                    response,
                }));
            }
            self.bus.delay_us(1_000);
        }
        if !initialized {
            return Err(SdError::Protocol(SdProtocolError::InitializationTimeout));
        }

        let card_type = if version == CardVersion::Version2 {
            let mut ocr = [0xFF; 4];
            let response = self.command(CMD58_READ_OCR, 0, &mut ocr)?;
            self.expect_response(CMD58_READ_OCR, response, 0)?;
            if ocr[1] == 0 && ocr[2] & 0x80 == 0 {
                return Err(SdError::Protocol(
                    SdProtocolError::OperatingVoltageUnsupported,
                ));
            }
            if ocr[0] & 0x40 != 0 {
                CardType::HighCapacity
            } else {
                CardType::StandardCapacity
            }
        } else {
            CardType::StandardCapacity
        };

        if card_type == CardType::StandardCapacity {
            let response = self.command(CMD16_SET_BLOCKLEN, Sector::LEN as u32, &mut [])?;
            self.expect_response(CMD16_SET_BLOCKLEN, response, 0)?;
        }

        self.bus
            .set_clock(SdSpiClock::Transfer)
            .map_err(SdError::Bus)?;
        let mut csd = [0; 16];
        self.read_data(CMD9_SEND_CSD, 0, &mut csd)?;
        let block_count = parse_csd_block_count(&csd).map_err(SdError::Protocol)?;
        let info = CardInfo {
            version,
            card_type,
            block_count,
        };
        self.info = Some(info);
        Ok(info)
    }

    pub fn read_sector(
        &mut self,
        block_index: u32,
        sector: &mut Sector,
    ) -> Result<(), SdError<B::Error>> {
        self.read_block(block_index, sector.as_bytes_mut())
    }

    pub fn read_block(
        &mut self,
        block_index: u32,
        block: &mut [u8; Sector::LEN],
    ) -> Result<(), SdError<B::Error>> {
        let address = self.block_address(block_index)?;
        self.read_data(CMD17_READ_SINGLE_BLOCK, address, block)
    }

    pub const fn card_info(&self) -> Option<CardInfo> {
        self.info
    }

    pub fn bus_mut(&mut self) -> &mut B {
        &mut self.bus
    }

    #[cfg(feature = "sd-write-diagnostic")]
    pub fn enable_write_diagnostic(self) -> ExplicitWriteSdCard<B> {
        ExplicitWriteSdCard { inner: self }
    }

    pub fn into_bus(self) -> B {
        self.bus
    }

    fn block_address(&self, block_index: u32) -> Result<u32, SdError<B::Error>> {
        let info = self
            .info
            .ok_or(SdError::Protocol(SdProtocolError::NotInitialized))?;
        if u64::from(block_index) >= info.block_count {
            return Err(SdError::Protocol(SdProtocolError::BlockOutOfRange {
                block: block_index,
                block_count: info.block_count,
            }));
        }
        match info.card_type {
            CardType::HighCapacity => Ok(block_index),
            CardType::StandardCapacity => {
                block_index
                    .checked_mul(Sector::LEN as u32)
                    .ok_or(SdError::Protocol(SdProtocolError::AddressOverflow(
                        block_index,
                    )))
            }
        }
    }

    fn command(
        &mut self,
        command: u8,
        argument: u32,
        trailing: &mut [u8],
    ) -> Result<u8, SdError<B::Error>> {
        self.bus.begin_sd().map_err(SdError::Bus)?;
        let result = (|| {
            if command != CMD0_GO_IDLE {
                self.wait_ready(command)?;
            }
            self.write_command(command, argument)?;
            let response = self.read_response(command)?;
            if !trailing.is_empty() {
                trailing.fill(0xFF);
                self.bus.transfer_in_place(trailing).map_err(SdError::Bus)?;
            }
            Ok(response)
        })();
        self.finish(result)
    }

    fn read_data(
        &mut self,
        command: u8,
        argument: u32,
        data: &mut [u8],
    ) -> Result<(), SdError<B::Error>> {
        self.bus.begin_sd().map_err(SdError::Bus)?;
        let result = (|| {
            self.wait_ready(command)?;
            self.write_command(command, argument)?;
            let response = self.read_response(command)?;
            self.expect_response(command, response, 0)?;

            let mut token = 0xFF;
            for _ in 0..DATA_TOKEN_POLLS {
                let mut byte = [0xFF];
                self.bus
                    .transfer_in_place(&mut byte)
                    .map_err(SdError::Bus)?;
                token = byte[0];
                if token == DATA_START_TOKEN {
                    break;
                }
                if token != 0xFF {
                    return Err(SdError::Protocol(SdProtocolError::DataErrorToken(token)));
                }
                self.bus.delay_us(10);
            }
            if token != DATA_START_TOKEN {
                return Err(SdError::Protocol(SdProtocolError::DataTokenTimeout(
                    command,
                )));
            }

            data.fill(0xFF);
            self.bus.transfer_in_place(data).map_err(SdError::Bus)?;
            let mut crc = [0xFF; 2];
            self.bus.transfer_in_place(&mut crc).map_err(SdError::Bus)?;
            let expected = u16::from_be_bytes(crc);
            let actual = crc16(data);
            if expected != actual {
                return Err(SdError::Protocol(SdProtocolError::DataCrcMismatch {
                    expected,
                    actual,
                }));
            }
            Ok(())
        })();
        self.finish(result)
    }

    #[cfg(feature = "sd-write-diagnostic")]
    fn write_block_data(
        &mut self,
        block_index: u32,
        data: &[u8; Sector::LEN],
    ) -> Result<(), SdError<B::Error>> {
        let address = self.block_address(block_index)?;
        self.bus.begin_sd().map_err(SdError::Bus)?;
        let result = (|| {
            self.wait_ready(CMD24_WRITE_SINGLE_BLOCK)?;
            self.write_command(CMD24_WRITE_SINGLE_BLOCK, address)?;
            let response = self.read_response(CMD24_WRITE_SINGLE_BLOCK)?;
            self.expect_response(CMD24_WRITE_SINGLE_BLOCK, response, 0)?;

            self.bus.write(&[DATA_START_TOKEN]).map_err(SdError::Bus)?;
            self.bus.write(data).map_err(SdError::Bus)?;
            self.bus
                .write(&crc16(data).to_be_bytes())
                .map_err(SdError::Bus)?;

            let mut response = [0xFF];
            for _ in 0..RESPONSE_POLLS {
                self.bus
                    .transfer_in_place(&mut response)
                    .map_err(SdError::Bus)?;
                if response[0] != 0xFF {
                    break;
                }
            }
            if response[0] & 0x1F != DATA_RESPONSE_ACCEPTED {
                return Err(SdError::Protocol(SdProtocolError::WriteRejected(
                    response[0],
                )));
            }

            for _ in 0..50_000 {
                let mut byte = [0xFF];
                self.bus
                    .transfer_in_place(&mut byte)
                    .map_err(SdError::Bus)?;
                if byte[0] == 0xFF {
                    return Ok(());
                }
                self.bus.delay_us(10);
            }
            Err(SdError::Protocol(SdProtocolError::WriteBusyTimeout))
        })();
        self.finish(result)
    }

    fn wait_ready(&mut self, command: u8) -> Result<(), SdError<B::Error>> {
        for _ in 0..READY_POLLS {
            let mut byte = [0xFF];
            self.bus
                .transfer_in_place(&mut byte)
                .map_err(SdError::Bus)?;
            if byte[0] == 0xFF {
                return Ok(());
            }
            self.bus.delay_us(10);
        }
        Err(SdError::Protocol(SdProtocolError::ReadyTimeout(command)))
    }

    fn write_command(&mut self, command: u8, argument: u32) -> Result<(), SdError<B::Error>> {
        let argument = argument.to_be_bytes();
        let mut packet = [
            0x40 | command,
            argument[0],
            argument[1],
            argument[2],
            argument[3],
            0,
        ];
        packet[5] = (crc7(&packet[..5]) << 1) | 1;
        self.bus.write(&packet).map_err(SdError::Bus)
    }

    fn read_response(&mut self, command: u8) -> Result<u8, SdError<B::Error>> {
        for _ in 0..RESPONSE_POLLS {
            let mut byte = [0xFF];
            self.bus
                .transfer_in_place(&mut byte)
                .map_err(SdError::Bus)?;
            if byte[0] & 0x80 == 0 {
                return Ok(byte[0]);
            }
        }
        Err(SdError::Protocol(SdProtocolError::ResponseTimeout(command)))
    }

    fn finish<T>(
        &mut self,
        operation: Result<T, SdError<B::Error>>,
    ) -> Result<T, SdError<B::Error>> {
        let end = self.bus.end_sd().map_err(SdError::Bus);
        let idle = self.bus.idle_clocks(1).map_err(SdError::Bus);
        match (operation, end, idle) {
            (_, Err(error), _) | (_, _, Err(error)) => Err(error),
            (result, Ok(()), Ok(())) => result,
        }
    }

    fn expect_response(
        &self,
        command: u8,
        actual: u8,
        expected: u8,
    ) -> Result<(), SdError<B::Error>> {
        if actual == expected {
            Ok(())
        } else {
            Err(SdError::Protocol(SdProtocolError::UnexpectedResponse {
                command,
                response: actual,
            }))
        }
    }
}

#[cfg(feature = "sd-write-diagnostic")]
impl<B> ExplicitWriteSdCard<B>
where
    B: ReadOnlySdSpi,
{
    pub const fn card_info(&self) -> Option<CardInfo> {
        self.inner.card_info()
    }

    pub fn read_block(
        &mut self,
        block_index: u32,
        block: &mut [u8; Sector::LEN],
    ) -> Result<(), SdError<B::Error>> {
        self.inner.read_block(block_index, block)
    }

    pub fn write_block(
        &mut self,
        block_index: u32,
        block: &[u8; Sector::LEN],
    ) -> Result<(), SdError<B::Error>> {
        self.inner.write_block_data(block_index, block)
    }

    pub fn bus_mut(&mut self) -> &mut B {
        &mut self.inner.bus
    }

    pub fn into_read_only(self) -> ReadOnlySdCard<B> {
        self.inner
    }
}

fn parse_csd_block_count(csd: &[u8; 16]) -> Result<u64, SdProtocolError> {
    match csd[0] >> 6 {
        0 => {
            let read_block_length = u32::from(csd[5] & 0x0F);
            let device_size = (u32::from(csd[6] & 0x03) << 10)
                | (u32::from(csd[7]) << 2)
                | (u32::from(csd[8]) >> 6);
            let size_multiplier = (u32::from(csd[9] & 0x03) << 1) | (u32::from(csd[10]) >> 7);
            let block_length = 1_u64 << read_block_length;
            let multiplier = 1_u64 << (size_multiplier + 2);
            let capacity_bytes = u64::from(device_size + 1) * multiplier * block_length;
            let blocks = capacity_bytes / Sector::LEN as u64;
            if blocks == 0 {
                Err(SdProtocolError::InvalidCapacity)
            } else {
                Ok(blocks)
            }
        }
        1 => {
            let device_size =
                (u32::from(csd[7] & 0x3F) << 16) | (u32::from(csd[8]) << 8) | u32::from(csd[9]);
            Ok(u64::from(device_size + 1) * 1_024)
        }
        version => Err(SdProtocolError::UnsupportedCsdVersion(version)),
    }
}

fn crc7(bytes: &[u8]) -> u8 {
    let mut crc = 0_u8;
    for byte in bytes {
        let mut data = *byte;
        for _ in 0..8 {
            crc <<= 1;
            if (data ^ crc) & 0x80 != 0 {
                crc ^= 0x09;
            }
            data <<= 1;
        }
    }
    crc & 0x7F
}

fn crc16(bytes: &[u8]) -> u16 {
    let mut crc = 0_u16;
    for byte in bytes {
        crc ^= u16::from(*byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    extern crate std;

    use core::convert::Infallible;
    use std::{collections::VecDeque, vec, vec::Vec};

    use super::{
        CardInfo, CardType, CardVersion, ReadOnlySdCard, ReadOnlySdSpi, SdError, SdProtocolError,
        SdSpiClock, Sector, crc7, crc16, parse_csd_block_count,
    };

    #[test]
    fn parses_high_capacity_csd_into_512_byte_block_count() {
        let mut csd = [0_u8; 16];
        csd[0] = 0x40;
        csd[7] = 0x01;
        csd[8] = 0x23;
        csd[9] = 0x45;

        assert_eq!(parse_csd_block_count(&csd), Ok((0x01_2345_u64 + 1) * 1_024));
    }

    #[test]
    fn parses_standard_capacity_csd() {
        let mut csd = [0_u8; 16];
        csd[5] = 9;
        csd[7] = 0xFF;
        csd[8] = 0xC0;
        csd[10] = 0x80;

        assert_eq!(parse_csd_block_count(&csd), Ok(8_192));
    }

    #[test]
    fn command_crc7_matches_mandatory_sd_command_values() {
        assert_eq!((crc7(&[0x40, 0, 0, 0, 0]) << 1) | 1, 0x95);
        assert_eq!((crc7(&[0x48, 0, 0, 1, 0xAA]) << 1) | 1, 0x87);
    }

    #[test]
    fn crc16_matches_the_sd_spec_polynomial_check_value() {
        assert_eq!(crc16(b"123456789"), 0x31C3);
    }

    struct NeverUsedBus;

    impl ReadOnlySdSpi for NeverUsedBus {
        type Error = ();

        fn set_clock(&mut self, _clock: SdSpiClock) -> Result<(), Self::Error> {
            Ok(())
        }

        fn idle_clocks(&mut self, _byte_count: usize) -> Result<(), Self::Error> {
            Ok(())
        }

        fn begin_sd(&mut self) -> Result<(), Self::Error> {
            panic!("an uninitialized card must not access the bus")
        }

        fn write(&mut self, _bytes: &[u8]) -> Result<(), Self::Error> {
            panic!("an uninitialized card must not access the bus")
        }

        fn transfer_in_place(&mut self, _bytes: &mut [u8]) -> Result<(), Self::Error> {
            panic!("an uninitialized card must not access the bus")
        }

        fn end_sd(&mut self) -> Result<(), Self::Error> {
            panic!("an uninitialized card must not access the bus")
        }

        fn delay_us(&mut self, _microseconds: u32) {}
    }

    #[test]
    fn sector_reads_are_rejected_before_initialization() {
        let mut card = ReadOnlySdCard::new(NeverUsedBus);
        let mut sector = Sector::zeroed();
        assert_eq!(
            card.read_sector(0, &mut sector),
            Err(SdError::Protocol(SdProtocolError::NotInitialized))
        );
    }

    #[test]
    fn card_capacity_uses_fixed_512_byte_sectors() {
        let info = CardInfo {
            version: CardVersion::Version2,
            card_type: CardType::HighCapacity,
            block_count: 62_500_000,
        };
        assert_eq!(info.capacity_bytes(), 32_000_000_000);
    }

    struct ScriptedCard {
        response: VecDeque<u8>,
        commands: Vec<(u8, u32)>,
        clocks: Vec<SdSpiClock>,
        idle_clock_bytes: usize,
        selected: bool,
        sector_zero: [u8; Sector::LEN],
        #[cfg(feature = "sd-write-diagnostic")]
        write_stage: u8,
        #[cfg(feature = "sd-write-diagnostic")]
        written_sector: [u8; Sector::LEN],
    }

    impl ScriptedCard {
        fn new() -> Self {
            let mut sector_zero = [0_u8; Sector::LEN];
            sector_zero[510..512].copy_from_slice(&[0x55, 0xAA]);
            Self {
                response: VecDeque::new(),
                commands: Vec::new(),
                clocks: Vec::new(),
                idle_clock_bytes: 0,
                selected: false,
                sector_zero,
                #[cfg(feature = "sd-write-diagnostic")]
                write_stage: 0,
                #[cfg(feature = "sd-write-diagnostic")]
                written_sector: [0; Sector::LEN],
            }
        }

        fn queue_data(&mut self, bytes: &[u8]) {
            self.response.push_back(0);
            self.response.push_back(super::DATA_START_TOKEN);
            self.response.extend(bytes);
            self.response.extend(crc16(bytes).to_be_bytes());
        }

        #[cfg(feature = "sd-write-diagnostic")]
        fn accept_write_data(&mut self, bytes: &[u8]) {
            match self.write_stage {
                1 => {
                    assert_eq!(bytes, [super::DATA_START_TOKEN]);
                    self.write_stage = 2;
                }
                2 => {
                    self.written_sector.copy_from_slice(bytes);
                    self.write_stage = 3;
                }
                3 => {
                    assert_eq!(bytes, crc16(&self.written_sector).to_be_bytes());
                    self.response
                        .extend([super::DATA_RESPONSE_ACCEPTED, 0, 0xFF]);
                    self.write_stage = 4;
                }
                _ => panic!("unexpected write stage {}", self.write_stage),
            }
        }
    }

    impl ReadOnlySdSpi for ScriptedCard {
        type Error = Infallible;

        fn set_clock(&mut self, clock: SdSpiClock) -> Result<(), Self::Error> {
            self.clocks.push(clock);
            Ok(())
        }

        fn idle_clocks(&mut self, byte_count: usize) -> Result<(), Self::Error> {
            assert!(!self.selected);
            self.idle_clock_bytes += byte_count;
            Ok(())
        }

        fn begin_sd(&mut self) -> Result<(), Self::Error> {
            assert!(!self.selected);
            self.selected = true;
            Ok(())
        }

        fn write(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
            assert!(self.selected);
            #[cfg(feature = "sd-write-diagnostic")]
            if bytes.len() != 6 {
                self.accept_write_data(bytes);
                return Ok(());
            }
            assert_eq!(bytes.len(), 6);
            assert_eq!(bytes[5], (crc7(&bytes[..5]) << 1) | 1);
            let command = bytes[0] & 0x3F;
            let argument = u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
            self.commands.push((command, argument));
            match command {
                super::CMD0_GO_IDLE => self.response.push_back(super::R1_IDLE),
                super::CMD59_CRC_ON_OFF => self.response.push_back(super::R1_IDLE),
                super::CMD8_SEND_IF_COND => {
                    self.response.extend([super::R1_IDLE, 0, 0, 1, 0xAA]);
                }
                super::CMD55_APP_CMD => self.response.push_back(super::R1_IDLE),
                super::CMD41_APP_SEND_OP_COND => self.response.push_back(0),
                super::CMD58_READ_OCR => self.response.extend([0, 0xC0, 0xFF, 0x80, 0]),
                super::CMD9_SEND_CSD => {
                    let mut csd = [0_u8; 16];
                    csd[0] = 0x40;
                    csd[9] = 1;
                    self.queue_data(&csd);
                }
                super::CMD17_READ_SINGLE_BLOCK => {
                    let sector = self.sector_zero;
                    self.queue_data(&sector);
                }
                #[cfg(feature = "sd-write-diagnostic")]
                super::CMD24_WRITE_SINGLE_BLOCK => {
                    self.response.push_back(0);
                    self.write_stage = 1;
                }
                _ => panic!("unexpected command {command}"),
            }
            Ok(())
        }

        fn transfer_in_place(&mut self, bytes: &mut [u8]) -> Result<(), Self::Error> {
            assert!(self.selected);
            for byte in bytes {
                *byte = self.response.pop_front().unwrap_or(0xFF);
            }
            Ok(())
        }

        fn end_sd(&mut self) -> Result<(), Self::Error> {
            assert!(self.selected);
            assert!(self.response.is_empty());
            self.selected = false;
            Ok(())
        }

        fn delay_us(&mut self, _microseconds: u32) {}
    }

    #[test]
    fn initializes_and_reads_without_exposing_block_write_operations() {
        let mut card = ReadOnlySdCard::new(ScriptedCard::new());
        assert_eq!(
            card.initialize(),
            Ok(CardInfo {
                version: CardVersion::Version2,
                card_type: CardType::HighCapacity,
                block_count: 2_048,
            })
        );

        let mut sector = Sector::zeroed();
        card.read_sector(0, &mut sector).unwrap();
        assert_eq!(&sector.as_bytes()[510..512], &[0x55, 0xAA]);

        let bus = card.into_bus();
        assert_eq!(
            bus.commands,
            vec![
                (super::CMD0_GO_IDLE, 0),
                (super::CMD59_CRC_ON_OFF, 1),
                (super::CMD8_SEND_IF_COND, 0x1AA),
                (super::CMD55_APP_CMD, 0),
                (super::CMD41_APP_SEND_OP_COND, 0x4000_0000),
                (super::CMD58_READ_OCR, 0),
                (super::CMD9_SEND_CSD, 0),
                (super::CMD17_READ_SINGLE_BLOCK, 0),
            ]
        );
        assert_eq!(
            bus.clocks,
            vec![SdSpiClock::Initialization, SdSpiClock::Transfer]
        );
        assert_eq!(bus.idle_clock_bytes, 18);
    }

    #[cfg(feature = "sd-write-diagnostic")]
    #[test]
    fn explicit_write_capability_writes_one_crc_protected_block() {
        let mut card = ReadOnlySdCard::new(ScriptedCard::new());
        card.initialize().unwrap();
        let mut card = card.enable_write_diagnostic();
        let block = [0xA5; Sector::LEN];

        card.write_block(7, &block).unwrap();

        let bus = card.into_read_only().into_bus();
        assert_eq!(
            bus.commands.last(),
            Some(&(super::CMD24_WRITE_SINGLE_BLOCK, 7))
        );
        assert_eq!(bus.written_sector, block);
        assert_eq!(bus.write_stage, 4);
        assert_eq!(bus.idle_clock_bytes, 18);
    }
}

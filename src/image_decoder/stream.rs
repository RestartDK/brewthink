use crc32fast::Hasher;
use miniz_oxide::MZStatus;

use super::{DecodeReport, ImageDecodeError, ImageFormat, Transform, checked_size};
use crate::{
    image::{PackedImage, RenderOptions, Size},
    zip_stream::{InflateWorkspace, ReadAt},
};

#[cfg(test)]
mod tests;

const ROW_BYTES: usize = 16 * 1024 + 1;
pub const MAX_IMAGE_FILE_BYTES: u32 = 8 * 1024 * 1024;

pub struct Input<'a, R> {
    reader: &'a R,
    position: u32,
}

impl<'a, R: ReadAt> Input<'a, R> {
    pub const fn new(reader: &'a R) -> Self {
        Self {
            reader,
            position: 0,
        }
    }
}

impl<R> embedded_io::ErrorType for Input<'_, R> {
    type Error = embedded_io::ErrorKind;
}

impl<R: ReadAt> embedded_io::Read for Input<'_, R> {
    fn read(&mut self, output: &mut [u8]) -> Result<usize, Self::Error> {
        let count = self
            .reader
            .read_at(self.position, output)
            .map_err(|_| embedded_io::ErrorKind::Other)?;
        self.position += count as u32;
        Ok(count)
    }
}

pub fn format<R: ReadAt>(reader: &R) -> Result<ImageFormat, ImageDecodeError> {
    if reader.is_empty() || reader.len() > MAX_IMAGE_FILE_BYTES {
        return Err(ImageDecodeError::InvalidImage);
    }
    let mut magic = [0; 8];
    exact(reader, 0, &mut magic)?;
    ImageFormat::detect(&magic).ok_or(ImageDecodeError::FormatMismatch)
}

pub fn dimensions<R: ReadAt>(reader: &R) -> Result<Size, ImageDecodeError> {
    match format(reader)? {
        ImageFormat::Png => {
            let mut chunk = Chunk::at(reader, 8)?;
            let mut header = [0; 13];
            if chunk.kind != *b"IHDR" || chunk.remaining != 13 {
                return Err(ImageDecodeError::InvalidImage);
            }
            chunk.read(reader, &mut header)?;
            chunk.finish(reader)?;
            png_header(&header).map(|header| header.size)
        }
        ImageFormat::Jpeg => {
            let mut offset = 2;
            while offset < reader.len() {
                let mut marker = [0; 2];
                exact(reader, offset, &mut marker)?;
                if marker[0] != 255 {
                    return Err(ImageDecodeError::InvalidImage);
                }
                if marker[1] == 255 {
                    offset += 1;
                    continue;
                }
                offset += 2;
                if matches!(marker[1], 0xD8 | 0xD9 | 0xDA | 0) {
                    return Err(ImageDecodeError::InvalidImage);
                }
                if matches!(marker[1], 0x01 | 0xD0..=0xD7) {
                    continue;
                }
                let mut size = [0; 2];
                exact(reader, offset, &mut size)?;
                let length = u32::from(u16::from_be_bytes(size));
                if length < 2
                    || offset
                        .checked_add(length)
                        .is_none_or(|end| end > reader.len())
                {
                    return Err(ImageDecodeError::InvalidImage);
                }
                if marker[1] == 0xC0 || marker[1] == 0xC1 {
                    let mut fields = [0; 5];
                    exact(reader, offset + 2, &mut fields)?;
                    if length < 8 || fields[0] != 8 {
                        return Err(ImageDecodeError::InvalidImage);
                    }
                    return checked_size(
                        usize::from(u16::from_be_bytes([fields[3], fields[4]])),
                        usize::from(u16::from_be_bytes([fields[1], fields[2]])),
                    );
                }
                if marker[1] == 0xC2 {
                    return Err(ImageDecodeError::InvalidImage);
                }
                offset += length;
            }
            Err(ImageDecodeError::InvalidImage)
        }
    }
}

pub struct PngWorkspace {
    inflate: InflateWorkspace,
    rows: [[u8; ROW_BYTES]; 2],
    input: [u8; 1024],
    palette: [u8; 768],
    alpha: [u8; 256],
}

impl PngWorkspace {
    pub fn new() -> Self {
        Self {
            inflate: InflateWorkspace::new(),
            rows: [[0; ROW_BYTES]; 2],
            input: [0; 1024],
            palette: [0; 768],
            alpha: [255; 256],
        }
    }

    pub(crate) unsafe fn initialize_in_place(storage: *mut Self) {
        // SAFETY: the caller supplies aligned exclusive storage. Arrays are zero-valid;
        // the pinned inflater initializer establishes its state before any reference.
        unsafe {
            core::ptr::addr_of_mut!((*storage).rows).write_bytes(0, 1);
            core::ptr::addr_of_mut!((*storage).input).write_bytes(0, 1);
            core::ptr::addr_of_mut!((*storage).palette).write_bytes(0, 1);
            core::ptr::addr_of_mut!((*storage).alpha).write([255; 256]);
            InflateWorkspace::initialize_in_place(core::ptr::addr_of_mut!((*storage).inflate));
        }
    }
}

impl Default for PngWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

struct Header {
    size: Size,
    depth: usize,
    color: u8,
    channels: usize,
    row_bytes: usize,
}

fn png_header(bytes: &[u8; 13]) -> Result<Header, ImageDecodeError> {
    let size = checked_size(
        u32::from_be_bytes(bytes[..4].try_into().unwrap()) as usize,
        u32::from_be_bytes(bytes[4..8].try_into().unwrap()) as usize,
    )?;
    let depth = usize::from(bytes[8]);
    let color = bytes[9];
    let channels = match color {
        0 | 3 => 1,
        2 => 3,
        4 => 2,
        6 => 4,
        _ => return Err(ImageDecodeError::InvalidImage),
    };
    let valid_depth = match color {
        0 => matches!(depth, 1 | 2 | 4 | 8 | 16),
        3 => matches!(depth, 1 | 2 | 4 | 8),
        _ => matches!(depth, 8 | 16),
    };
    if !valid_depth || bytes[10..] != [0, 0, 0] {
        return Err(ImageDecodeError::InvalidImage);
    }
    let row_bytes = (size.width() * channels * depth).div_ceil(8);
    if row_bytes + 1 > ROW_BYTES {
        return Err(ImageDecodeError::DimensionsOutOfRange);
    }
    Ok(Header {
        size,
        depth,
        color,
        channels,
        row_bytes,
    })
}

struct Chunk {
    kind: [u8; 4],
    offset: u32,
    remaining: u32,
    checksum: Hasher,
}

impl Chunk {
    fn at<R: ReadAt>(reader: &R, offset: u32) -> Result<Self, ImageDecodeError> {
        let mut header = [0; 8];
        exact(reader, offset, &mut header)?;
        let length = u32::from_be_bytes(header[..4].try_into().unwrap());
        if offset
            .checked_add(12)
            .and_then(|value| value.checked_add(length))
            .is_none_or(|end| end > reader.len())
        {
            return Err(ImageDecodeError::InvalidImage);
        }
        let kind = header[4..].try_into().unwrap();
        let mut checksum = Hasher::new();
        checksum.update(&header[4..]);
        Ok(Self {
            kind,
            offset: offset + 8,
            remaining: length,
            checksum,
        })
    }

    fn read<R: ReadAt>(&mut self, reader: &R, output: &mut [u8]) -> Result<(), ImageDecodeError> {
        if output.len() > self.remaining as usize {
            return Err(ImageDecodeError::InvalidImage);
        }
        exact(reader, self.offset, output)?;
        self.checksum.update(output);
        self.remaining -= output.len() as u32;
        self.offset += output.len() as u32;
        Ok(())
    }

    fn skip<R: ReadAt>(&mut self, reader: &R, buffer: &mut [u8]) -> Result<(), ImageDecodeError> {
        while self.remaining != 0 {
            let length = buffer.len().min(self.remaining as usize);
            self.read(reader, &mut buffer[..length])?;
        }
        Ok(())
    }

    fn finish<R: ReadAt>(&self, reader: &R) -> Result<u32, ImageDecodeError> {
        let mut bytes = [0; 4];
        if self.remaining != 0 {
            return Err(ImageDecodeError::InvalidImage);
        }
        exact(reader, self.offset, &mut bytes)?;
        if self.checksum.clone().finalize() != u32::from_be_bytes(bytes) {
            return Err(ImageDecodeError::InvalidImage);
        }
        Ok(self.offset + 4)
    }
}

pub fn decode_png<R: ReadAt>(
    reader: &R,
    target: &mut PackedImage<'_>,
    options: RenderOptions,
    workspace: &mut PngWorkspace,
) -> Result<DecodeReport, ImageDecodeError> {
    if format(reader)? != ImageFormat::Png {
        return Err(ImageDecodeError::FormatMismatch);
    }
    let mut chunk = Chunk::at(reader, 8)?;
    let mut bytes = [0; 13];
    if chunk.kind != *b"IHDR" || chunk.remaining != 13 {
        return Err(ImageDecodeError::InvalidImage);
    }
    chunk.read(reader, &mut bytes)?;
    let header = png_header(&bytes)?;
    let mut palette_length = 0;
    let mut transparency = [0; 6];
    let mut transparency_length = 0;
    workspace.alpha.fill(255);
    loop {
        chunk = Chunk::at(reader, chunk.finish(reader)?)?;
        match &chunk.kind {
            b"IDAT" => break,
            b"PLTE" => {
                if palette_length != 0
                    || chunk.remaining == 0
                    || chunk.remaining > 768
                    || chunk.remaining % 3 != 0
                {
                    return Err(ImageDecodeError::InvalidImage);
                }
                palette_length = chunk.remaining as usize / 3;
                chunk.read(reader, &mut workspace.palette[..palette_length * 3])?;
            }
            b"tRNS" => {
                let length = chunk.remaining as usize;
                match header.color {
                    3 if palette_length > 0 && length <= palette_length => {
                        chunk.read(reader, &mut workspace.alpha[..length])?
                    }
                    0 if length == 2 => chunk.read(reader, &mut transparency[..length])?,
                    2 if length == 6 => chunk.read(reader, &mut transparency[..length])?,
                    _ => return Err(ImageDecodeError::InvalidImage),
                }
                transparency_length = length;
            }
            kind if kind[0] & 32 != 0 => chunk.skip(reader, &mut workspace.input)?,
            _ => return Err(ImageDecodeError::InvalidImage),
        }
    }
    if header.color == 3 && palette_length == 0 {
        return Err(ImageDecodeError::InvalidImage);
    }
    workspace.inflate.reset_zlib();
    let [first, second] = &mut workspace.rows;
    let mut current = &mut first[..header.row_bytes + 1];
    let mut previous = &mut second[..header.row_bytes + 1];
    previous.fill(0);
    let mut compressed = PngInflate {
        reader,
        chunk,
        input: &mut workspace.input,
        position: 0,
        length: 0,
        inflate: &mut workspace.inflate,
        ended: false,
    };
    let transform = Transform::new(header.size, target.size(), options.scale);
    target.clear_white();
    for y in 0..header.size.height() {
        compressed.read_exact(current)?;
        unfilter(
            current,
            previous,
            (header.channels * header.depth).div_ceil(8),
        )?;
        for x in 0..header.size.width() {
            let sample =
                |channel| sample(&current[1..], x * header.channels + channel, header.depth);
            let byte = |value: u16| -> u32 {
                if header.depth == 16 {
                    u32::from(value >> 8)
                } else {
                    u32::from(value) * 255 / ((1 << header.depth) - 1)
                }
            };
            let key = |offset| u16::from_be_bytes([transparency[offset], transparency[offset + 1]]);
            let (gray, alpha) = match header.color {
                0 => (
                    byte(sample(0)),
                    if transparency_length == 2 && sample(0) == key(0) {
                        0
                    } else {
                        255
                    },
                ),
                2 | 6 => {
                    let gray =
                        (byte(sample(0)) * 54 + byte(sample(1)) * 183 + byte(sample(2)) * 19 + 128)
                            >> 8;
                    let alpha = if header.color == 6 {
                        byte(sample(3))
                    } else if transparency_length == 6
                        && sample(0) == key(0)
                        && sample(1) == key(2)
                        && sample(2) == key(4)
                    {
                        0
                    } else {
                        255
                    };
                    (gray, alpha)
                }
                3 => {
                    let index = sample(0) as usize;
                    if index >= palette_length {
                        return Err(ImageDecodeError::InvalidImage);
                    }
                    let rgb = &workspace.palette[index * 3..index * 3 + 3];
                    let gray = (u32::from(rgb[0]) * 54
                        + u32::from(rgb[1]) * 183
                        + u32::from(rgb[2]) * 19
                        + 128)
                        >> 8;
                    (gray, u32::from(workspace.alpha[index]))
                }
                4 => (byte(sample(0)), byte(sample(1))),
                _ => unreachable!(),
            };
            let luma = ((gray * alpha + 255 * (255 - alpha) + 127) / 255) as u8;
            transform.draw_source_pixel(target, x, y, luma, options.dither);
        }
        core::mem::swap(&mut current, &mut previous);
    }
    compressed.finish()?;
    Ok(DecodeReport {
        format: ImageFormat::Png,
        source: header.size,
        target: target.size(),
    })
}

struct PngInflate<'a, R> {
    reader: &'a R,
    chunk: Chunk,
    input: &'a mut [u8; 1024],
    position: usize,
    length: usize,
    inflate: &'a mut InflateWorkspace,
    ended: bool,
}

impl<R: ReadAt> PngInflate<'_, R> {
    fn step(&mut self, output: &mut [u8]) -> Result<usize, ImageDecodeError> {
        if self.ended {
            return Ok(0);
        }
        if self.position == self.length {
            while self.chunk.kind == *b"IDAT" && self.chunk.remaining == 0 {
                self.chunk = Chunk::at(self.reader, self.chunk.finish(self.reader)?)?;
            }
            self.length = if self.chunk.kind == *b"IDAT" {
                self.input.len().min(self.chunk.remaining as usize)
            } else {
                0
            };
            self.position = 0;
            self.chunk
                .read(self.reader, &mut self.input[..self.length])?;
        }
        let result = self
            .inflate
            .decode(&self.input[self.position..self.length], output);
        self.position += result.bytes_consumed;
        match result.status {
            Ok(MZStatus::StreamEnd) => self.ended = true,
            Ok(MZStatus::Ok) if result.bytes_consumed != 0 || result.bytes_written != 0 => {}
            _ => return Err(ImageDecodeError::InvalidImage),
        }
        Ok(result.bytes_written)
    }

    fn read_exact(&mut self, mut output: &mut [u8]) -> Result<(), ImageDecodeError> {
        while !output.is_empty() {
            if self.ended {
                return Err(ImageDecodeError::InvalidImage);
            }
            let count = self.step(output)?;
            output = &mut output[count..];
        }
        Ok(())
    }

    fn finish(mut self) -> Result<(), ImageDecodeError> {
        while !self.ended {
            if self.step(&mut [0])? != 0 {
                return Err(ImageDecodeError::InvalidImage);
            }
        }
        if self.position != self.length
            || (self.chunk.kind == *b"IDAT" && self.chunk.remaining != 0)
        {
            return Err(ImageDecodeError::InvalidImage);
        }
        if self.chunk.kind == *b"IDAT" {
            self.chunk = Chunk::at(self.reader, self.chunk.finish(self.reader)?)?;
        }
        loop {
            match &self.chunk.kind {
                b"IEND" if self.chunk.remaining == 0 => {
                    if self.chunk.finish(self.reader)? != self.reader.len() {
                        return Err(ImageDecodeError::InvalidImage);
                    }
                    return Ok(());
                }
                b"IDAT" if self.chunk.remaining == 0 => {}
                kind if kind[0] & 32 != 0 => self.chunk.skip(self.reader, self.input)?,
                _ => return Err(ImageDecodeError::InvalidImage),
            }
            self.chunk = Chunk::at(self.reader, self.chunk.finish(self.reader)?)?;
        }
    }
}

fn sample(row: &[u8], index: usize, depth: usize) -> u16 {
    match depth {
        16 => u16::from_be_bytes([row[index * 2], row[index * 2 + 1]]),
        8 => u16::from(row[index]),
        _ => u16::from(
            (row[index * depth / 8] >> (8 - depth - index * depth % 8)) & ((1 << depth) - 1),
        ),
    }
}

fn unfilter(row: &mut [u8], previous: &[u8], bpp: usize) -> Result<(), ImageDecodeError> {
    let filter = row[0];
    if filter > 4 {
        return Err(ImageDecodeError::InvalidImage);
    }
    for index in 1..row.len() {
        let left = if index > bpp { row[index - bpp] } else { 0 };
        let up = previous[index];
        let upper_left = if index > bpp {
            previous[index - bpp]
        } else {
            0
        };
        let predictor = match filter {
            0 => 0,
            1 => left,
            2 => up,
            3 => ((u16::from(left) + u16::from(up)) / 2) as u8,
            4 => {
                let a = i16::from(left);
                let b = i16::from(up);
                let c = i16::from(upper_left);
                let p = a + b - c;
                if (p - a).abs() <= (p - b).abs() && (p - a).abs() <= (p - c).abs() {
                    left
                } else if (p - b).abs() <= (p - c).abs() {
                    up
                } else {
                    upper_left
                }
            }
            _ => unreachable!(),
        };
        row[index] = row[index].wrapping_add(predictor);
    }
    Ok(())
}

fn exact<R: ReadAt>(
    reader: &R,
    mut offset: u32,
    mut output: &mut [u8],
) -> Result<(), ImageDecodeError> {
    while !output.is_empty() {
        let count = reader
            .read_at(offset, output)
            .map_err(|_| ImageDecodeError::InvalidImage)?;
        if count == 0 || count > output.len() {
            return Err(ImageDecodeError::InvalidImage);
        }
        offset = offset
            .checked_add(count as u32)
            .ok_or(ImageDecodeError::InvalidImage)?;
        output = &mut output[count..];
    }
    Ok(())
}

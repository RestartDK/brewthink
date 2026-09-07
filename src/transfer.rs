use crc32fast::Hasher;

use crate::image_decoder::ImageFormat;

pub const MAX_IMAGE_BYTES: usize = if cfg!(feature = "experimental-gray8") {
    56 * 1024
} else {
    96 * 1024
};
const MAX_IMAGE_STEM_BYTES: usize = 8;
const IMAGE_NAME_BYTES: usize = 12;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ImageName {
    bytes: [u8; IMAGE_NAME_BYTES],
    length: u8,
    format: ImageFormat,
}

impl ImageName {
    pub fn parse(value: &str) -> Result<Self, ImageNameError> {
        let (stem, extension) = value.rsplit_once('.').ok_or(ImageNameError)?;
        if stem.is_empty()
            || stem.len() > MAX_IMAGE_STEM_BYTES
            || is_reserved_fat_stem(stem)
            || !stem
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'~'))
        {
            return Err(ImageNameError);
        }
        let format = if extension.eq_ignore_ascii_case("jpg") {
            ImageFormat::Jpeg
        } else if extension.eq_ignore_ascii_case("png") {
            ImageFormat::Png
        } else {
            return Err(ImageNameError);
        };
        let mut bytes = [0; IMAGE_NAME_BYTES];
        let mut length = 0usize;
        for byte in stem
            .bytes()
            .chain(b".".iter().copied())
            .chain(extension.bytes())
        {
            bytes[length] = byte.to_ascii_uppercase();
            length += 1;
        }
        Ok(Self {
            bytes,
            length: length as u8,
            format,
        })
    }

    pub fn from_padded(bytes: [u8; IMAGE_NAME_BYTES]) -> Result<Self, ImageNameError> {
        let length = bytes
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(bytes.len());
        let value = core::str::from_utf8(&bytes[..length]).map_err(|_| ImageNameError)?;
        Self::parse(value)
    }

    pub fn write_padded(self, output: &mut [u8; IMAGE_NAME_BYTES]) {
        *output = self.bytes;
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..usize::from(self.length)])
            .expect("image names contain only ASCII")
    }

    pub const fn format(self) -> ImageFormat {
        self.format
    }
}

fn is_reserved_fat_stem(stem: &str) -> bool {
    ["CON", "PRN", "AUX", "NUL"]
        .iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
        || stem.get(..3).is_some_and(|prefix| {
            prefix.eq_ignore_ascii_case("COM") || prefix.eq_ignore_ascii_case("LPT")
        }) && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0'
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageNameError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UploadTarget {
    Image(ImageName),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UploadRequest {
    target: UploadTarget,
    format: ImageFormat,
    length: usize,
    crc32: u32,
}

impl UploadRequest {
    pub const fn image(name: ImageName, length: usize, crc32: u32) -> Self {
        Self {
            target: UploadTarget::Image(name),
            format: name.format(),
            length,
            crc32,
        }
    }

    pub const fn target(self) -> UploadTarget {
        self.target
    }

    pub const fn format(self) -> ImageFormat {
        self.format
    }

    pub const fn length(self) -> usize {
        self.length
    }

    pub const fn crc32(self) -> u32 {
        self.crc32
    }
}

pub trait UploadSink {
    type Error;

    fn begin(&mut self, request: UploadRequest) -> Result<(), Self::Error>;
    fn append(&mut self, request: UploadRequest, bytes: &[u8]) -> Result<(), Self::Error>;
    fn commit(&mut self, request: UploadRequest, scratch: &mut [u8]) -> Result<(), Self::Error>;
    fn abort(&mut self, request: UploadRequest) -> Result<(), Self::Error>;
}

#[derive(Debug)]
pub enum FileTransferError<E> {
    Busy,
    NoTransfer,
    EmptyFile,
    FileTooLarge,
    Overflow,
    Incomplete { expected: usize, actual: usize },
    ChecksumMismatch { expected: u32, actual: u32 },
    FormatMismatch,
    Store(E),
}

impl<E> FileTransferError<E> {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Busy => "busy",
            Self::NoTransfer => "no-transfer",
            Self::EmptyFile => "empty-file",
            Self::FileTooLarge => "file-too-large",
            Self::Overflow => "overflow",
            Self::Incomplete { .. } => "incomplete",
            Self::ChecksumMismatch { .. } => "checksum-mismatch",
            Self::FormatMismatch => "format-mismatch",
            Self::Store(_) => "storage",
        }
    }
}

struct ActiveTransfer {
    request: UploadRequest,
    received: usize,
    signature: [u8; 8],
    signature_length: usize,
    hasher: Hasher,
}

pub struct FileTransfer {
    active: Option<ActiveTransfer>,
}

impl FileTransfer {
    pub const fn new() -> Self {
        Self { active: None }
    }

    pub const fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub fn request(&self) -> Option<UploadRequest> {
        self.active.as_ref().map(|active| active.request)
    }

    pub fn received(&self) -> usize {
        self.active.as_ref().map_or(0, |active| active.received)
    }

    pub fn begin<S: UploadSink>(
        &mut self,
        request: UploadRequest,
        sink: &mut S,
    ) -> Result<(), FileTransferError<S::Error>> {
        if self.active.is_some() {
            return Err(FileTransferError::Busy);
        }
        if request.length == 0 {
            return Err(FileTransferError::EmptyFile);
        }
        if request.length > MAX_IMAGE_BYTES {
            return Err(FileTransferError::FileTooLarge);
        }
        sink.begin(request).map_err(FileTransferError::Store)?;
        self.active = Some(ActiveTransfer {
            request,
            received: 0,
            signature: [0; 8],
            signature_length: 0,
            hasher: Hasher::new(),
        });
        Ok(())
    }

    pub fn append<S: UploadSink>(
        &mut self,
        bytes: &[u8],
        sink: &mut S,
    ) -> Result<(), FileTransferError<S::Error>> {
        let active = self.active.as_mut().ok_or(FileTransferError::NoTransfer)?;
        if active
            .received
            .checked_add(bytes.len())
            .is_none_or(|length| length > active.request.length)
        {
            return Err(FileTransferError::Overflow);
        }
        sink.append(active.request, bytes)
            .map_err(FileTransferError::Store)?;
        let signature_remaining = active.signature.len() - active.signature_length;
        let copied = signature_remaining.min(bytes.len());
        active.signature[active.signature_length..active.signature_length + copied]
            .copy_from_slice(&bytes[..copied]);
        active.signature_length += copied;
        active.received += bytes.len();
        active.hasher.update(bytes);
        Ok(())
    }

    pub fn finish<S: UploadSink>(
        &mut self,
        sink: &mut S,
        scratch: &mut [u8],
    ) -> Result<UploadRequest, FileTransferError<S::Error>> {
        let Some(active) = self.active.take() else {
            return Err(FileTransferError::NoTransfer);
        };
        let validation = if active.received != active.request.length {
            Err(FileTransferError::Incomplete {
                expected: active.request.length,
                actual: active.received,
            })
        } else {
            let actual_crc = active.hasher.finalize();
            if actual_crc != active.request.crc32 {
                Err(FileTransferError::ChecksumMismatch {
                    expected: active.request.crc32,
                    actual: actual_crc,
                })
            } else if ImageFormat::detect(&active.signature[..active.signature_length])
                != Some(active.request.format)
            {
                Err(FileTransferError::FormatMismatch)
            } else {
                sink.commit(active.request, scratch)
                    .map_err(FileTransferError::Store)
            }
        };
        match validation {
            Ok(()) => Ok(active.request),
            Err(error) => {
                let _ = sink.abort(active.request);
                Err(error)
            }
        }
    }

    pub fn abort<S: UploadSink>(
        &mut self,
        sink: &mut S,
    ) -> Result<(), FileTransferError<S::Error>> {
        let Some(active) = self.active.take() else {
            return Ok(());
        };
        sink.abort(active.request).map_err(FileTransferError::Store)
    }
}

impl Default for FileTransfer {
    fn default() -> Self {
        Self::new()
    }
}

pub mod wifi {
    use super::{ImageName, UploadRequest};
    use crate::image_decoder::ImageFormat;

    pub const IMAGE_PATH: &str = "/api/files/images";

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum WifiUploadRequestError {
        UnsupportedContentType,
    }

    pub fn image_request(
        name: ImageName,
        content_type: &str,
        content_length: usize,
        crc32: u32,
    ) -> Result<UploadRequest, WifiUploadRequestError> {
        let format = match content_type {
            "image/jpeg" => ImageFormat::Jpeg,
            "image/png" => ImageFormat::Png,
            _ => return Err(WifiUploadRequestError::UnsupportedContentType),
        };
        if name.format() != format {
            return Err(WifiUploadRequestError::UnsupportedContentType);
        }
        Ok(UploadRequest::image(name, content_length, crc32))
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::vec::Vec;

    use super::{FileTransfer, FileTransferError, ImageName, UploadRequest, UploadSink};

    #[derive(Default)]
    struct MemorySink {
        bytes: Vec<u8>,
        committed: bool,
        aborted: bool,
    }

    impl UploadSink for MemorySink {
        type Error = ();

        fn begin(&mut self, _request: UploadRequest) -> Result<(), Self::Error> {
            self.bytes.clear();
            Ok(())
        }

        fn append(&mut self, _request: UploadRequest, bytes: &[u8]) -> Result<(), Self::Error> {
            self.bytes.extend_from_slice(bytes);
            Ok(())
        }

        fn commit(
            &mut self,
            _request: UploadRequest,
            _scratch: &mut [u8],
        ) -> Result<(), Self::Error> {
            self.committed = true;
            Ok(())
        }

        fn abort(&mut self, _request: UploadRequest) -> Result<(), Self::Error> {
            self.aborted = true;
            self.bytes.clear();
            Ok(())
        }
    }

    #[test]
    fn image_names_are_canonical_bounded_fat_names() {
        assert_eq!(ImageName::parse("nice.png").unwrap().as_str(), "NICE.PNG");
        assert_eq!(
            ImageName::parse("anothe.jpg").unwrap().as_str(),
            "ANOTHE.JPG"
        );
        assert!(ImageName::parse("too-long-name.png").is_err());
        assert!(ImageName::parse("bad.name.png").is_err());
        assert!(ImageName::parse("cover.jpeg").is_err());
        assert!(ImageName::parse("CON.JPG").is_err());
        assert!(ImageName::parse("lpt9.png").is_err());
    }

    #[test]
    fn commits_an_exact_chunked_upload() {
        let bytes = b"\xff\xd8jpeg";
        let request = UploadRequest::image(
            ImageName::parse("COVER.JPG").unwrap(),
            bytes.len(),
            crc32fast::hash(bytes),
        );
        let mut transfer = FileTransfer::new();
        let mut sink = MemorySink::default();
        transfer.begin(request, &mut sink).unwrap();
        transfer.append(&bytes[..3], &mut sink).unwrap();
        transfer.append(&bytes[3..], &mut sink).unwrap();
        assert_eq!(transfer.finish(&mut sink, &mut [0; 16]).unwrap(), request);
        assert!(sink.committed);
        assert!(!sink.aborted);
    }

    #[test]
    fn aborts_checksum_failures() {
        let bytes = b"\xff\xd8jpeg";
        let request = UploadRequest::image(ImageName::parse("COVER.JPG").unwrap(), bytes.len(), 0);
        let mut transfer = FileTransfer::new();
        let mut sink = MemorySink::default();
        transfer.begin(request, &mut sink).unwrap();
        transfer.append(bytes, &mut sink).unwrap();
        assert!(matches!(
            transfer.finish(&mut sink, &mut [0; 16]),
            Err(FileTransferError::ChecksumMismatch { .. })
        ));
        assert!(sink.aborted);
    }

    #[test]
    fn aborts_an_interrupted_upload_and_allows_retry() {
        let bytes = b"\x89PNG\r\n\x1a\nbody";
        let request = UploadRequest::image(
            ImageName::parse("ART.PNG").unwrap(),
            bytes.len(),
            crc32fast::hash(bytes),
        );
        let mut transfer = FileTransfer::new();
        let mut sink = MemorySink::default();

        transfer.begin(request, &mut sink).unwrap();
        transfer.append(&bytes[..5], &mut sink).unwrap();
        transfer.abort(&mut sink).unwrap();
        assert!(sink.aborted);
        assert!(!transfer.is_active());

        sink.aborted = false;
        transfer.begin(request, &mut sink).unwrap();
        transfer.append(bytes, &mut sink).unwrap();
        transfer.finish(&mut sink, &mut [0; 16]).unwrap();
        assert!(sink.committed);
        assert!(!sink.aborted);
    }

    #[test]
    fn aborts_format_mismatches() {
        let bytes = b"\xff\xd8jpeg";
        let request = UploadRequest::image(
            ImageName::parse("WRONG.PNG").unwrap(),
            bytes.len(),
            crc32fast::hash(bytes),
        );
        let mut transfer = FileTransfer::new();
        let mut sink = MemorySink::default();
        transfer.begin(request, &mut sink).unwrap();
        transfer.append(bytes, &mut sink).unwrap();
        assert!(matches!(
            transfer.finish(&mut sink, &mut [0; 16]),
            Err(FileTransferError::FormatMismatch)
        ));
        assert!(sink.aborted);
    }

    #[test]
    fn wifi_stub_maps_http_metadata_into_the_shared_request() {
        let name = ImageName::parse("ART.PNG").unwrap();
        let request = super::wifi::image_request(name, "image/png", 10, 20).unwrap();
        assert_eq!(request, UploadRequest::image(name, 10, 20));
        assert_eq!(super::wifi::IMAGE_PATH, "/api/files/images");
    }
}

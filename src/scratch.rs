use core::mem::{MaybeUninit, align_of, needs_drop, size_of};

#[derive(Clone, Copy, Eq, PartialEq)]
enum Contents {
    Bytes,
    Typed,
}

#[repr(C, align(8))]
pub(crate) struct Scratch<const N: usize> {
    storage: MaybeUninit<[u8; N]>,
    contents: Contents,
}

impl<const N: usize> Scratch<N> {
    pub const fn new() -> Self {
        Self {
            storage: MaybeUninit::new([0; N]),
            contents: Contents::Bytes,
        }
    }

    pub fn bytes(&mut self) -> &mut [u8; N] {
        if self.contents == Contents::Typed {
            // Typed values can leave padding and inactive enum payloads uninitialized.
            unsafe {
                self.storage.as_mut_ptr().write_bytes(0, 1);
            }
            self.contents = Contents::Bytes;
        }
        // SAFETY: construction or the transition above initializes every byte.
        unsafe { self.storage.assume_init_mut() }
    }

    /// # Safety
    /// The initializer must establish a valid T without reading uninitialized storage.
    pub unsafe fn initialize<T>(&mut self, initialize: unsafe fn(*mut T)) -> &mut T {
        const {
            assert!(!needs_drop::<T>());
            assert!(size_of::<T>() <= N);
            assert!(align_of::<T>() <= align_of::<Self>());
        }
        self.contents = Contents::Typed;
        let pointer = self.storage.as_mut_ptr().cast::<T>();
        // SAFETY: storage is aligned, fits T, and is exclusively borrowed. The caller
        // supplies its initializer. Bytes are inaccessible until reinitialized above.
        unsafe {
            initialize(pointer);
            &mut *pointer
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Scratch;
    use crate::{
        bounded_layout::BoundedPage, device_epub::DevicePublication, storage::BookCatalog,
        zip_stream::InflateWorkspace,
    };

    #[test]
    fn typed_scratch_reinitializes_every_byte_before_reuse() {
        let mut scratch = Scratch::<50_000>::new();
        scratch.bytes().fill(0xA5);
        // SAFETY: each domain initializer establishes every field in its type.
        unsafe {
            let page = scratch.initialize(BoundedPage::initialize_in_place);
            assert_eq!(page.page_count(), 0);
            assert_eq!(page.lines().count(), 0);
            assert_eq!(page.chapter_title(), "");
        }
        assert!(scratch.bytes().iter().all(|&byte| byte == 0));
        scratch.bytes()[123] = 17;
        assert_eq!(scratch.bytes()[123], 17);
        unsafe {
            let publication = scratch.initialize(DevicePublication::initialize_in_place);
            assert_eq!(publication.spine_len(), 0);
            assert_eq!(publication.cover_path(), None);
            assert_eq!(publication.title(), "");
            let catalog = scratch.initialize(BookCatalog::<16>::initialize_in_place);
            assert!(catalog.is_empty());
            assert!(!catalog.is_truncated());
            scratch.initialize(InflateWorkspace::initialize_in_place);
        }
        assert!(scratch.bytes().iter().all(|&byte| byte == 0));
    }
}

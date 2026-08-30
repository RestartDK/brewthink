#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TemporaryFilePhase {
    Preflight,
    Create,
    Readback,
}

impl TemporaryFilePhase {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Preflight => "preflight",
            Self::Create => "create",
            Self::Readback => "readback",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TemporaryFileTestError<E> {
    Store { phase: TemporaryFilePhase, error: E },
    TargetAlreadyExists,
    PayloadLength { expected: usize, actual: usize },
    PayloadMismatch,
    Cleanup(E),
    TargetRemains,
}

impl<E> TemporaryFileTestError<E> {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Store { phase, .. } => phase.name(),
            Self::TargetAlreadyExists => "target_already_exists",
            Self::PayloadLength { .. } => "payload_length",
            Self::PayloadMismatch => "payload_mismatch",
            Self::Cleanup(_) => "cleanup",
            Self::TargetRemains => "target_remains",
        }
    }
}

pub trait TemporaryFileStore {
    type Error;

    fn exists(&mut self, name: &str) -> Result<bool, Self::Error>;
    fn create(&mut self, name: &str, contents: &[u8]) -> Result<(), Self::Error>;
    fn read(&mut self, name: &str, output: &mut [u8]) -> Result<usize, Self::Error>;
    fn delete(&mut self, name: &str) -> Result<(), Self::Error>;
}

pub fn create_verify_delete<S>(
    store: &mut S,
    name: &str,
    payload: &[u8],
    readback: &mut [u8],
) -> Result<(), TemporaryFileTestError<S::Error>>
where
    S: TemporaryFileStore,
{
    if store
        .exists(name)
        .map_err(|error| TemporaryFileTestError::Store {
            phase: TemporaryFilePhase::Preflight,
            error,
        })?
    {
        return Err(TemporaryFileTestError::TargetAlreadyExists);
    }

    let operation = (|| {
        store
            .create(name, payload)
            .map_err(|error| TemporaryFileTestError::Store {
                phase: TemporaryFilePhase::Create,
                error,
            })?;
        let bytes_read =
            store
                .read(name, readback)
                .map_err(|error| TemporaryFileTestError::Store {
                    phase: TemporaryFilePhase::Readback,
                    error,
                })?;
        if bytes_read != payload.len() {
            return Err(TemporaryFileTestError::PayloadLength {
                expected: payload.len(),
                actual: bytes_read,
            });
        }
        if readback.get(..bytes_read) != Some(payload) {
            return Err(TemporaryFileTestError::PayloadMismatch);
        }
        Ok(())
    })();

    let cleanup = (|| {
        if store
            .exists(name)
            .map_err(TemporaryFileTestError::Cleanup)?
        {
            store
                .delete(name)
                .map_err(TemporaryFileTestError::Cleanup)?;
        }
        if store
            .exists(name)
            .map_err(TemporaryFileTestError::Cleanup)?
        {
            Err(TemporaryFileTestError::TargetRemains)
        } else {
            Ok(())
        }
    })();

    cleanup?;
    operation
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::vec::Vec;

    use super::{TemporaryFileStore, TemporaryFileTestError, create_verify_delete};

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum FakeError {
        Create,
        Delete,
    }

    struct FakeStore {
        file: Option<Vec<u8>>,
        create_error_after_file: bool,
        delete_error: bool,
        deleted: bool,
    }

    impl FakeStore {
        fn empty() -> Self {
            Self {
                file: None,
                create_error_after_file: false,
                delete_error: false,
                deleted: false,
            }
        }
    }

    impl TemporaryFileStore for FakeStore {
        type Error = FakeError;

        fn exists(&mut self, _name: &str) -> Result<bool, Self::Error> {
            Ok(self.file.is_some())
        }

        fn create(&mut self, _name: &str, contents: &[u8]) -> Result<(), Self::Error> {
            self.file = Some(contents.into());
            if self.create_error_after_file {
                Err(FakeError::Create)
            } else {
                Ok(())
            }
        }

        fn read(&mut self, _name: &str, output: &mut [u8]) -> Result<usize, Self::Error> {
            let contents = self.file.as_deref().unwrap_or_default();
            let count = contents.len().min(output.len());
            output[..count].copy_from_slice(&contents[..count]);
            Ok(count)
        }

        fn delete(&mut self, _name: &str) -> Result<(), Self::Error> {
            if self.delete_error {
                return Err(FakeError::Delete);
            }
            self.file = None;
            self.deleted = true;
            Ok(())
        }
    }

    #[test]
    fn creates_verifies_and_deletes_the_exact_payload() {
        let mut store = FakeStore::empty();
        let mut readback = [0; 4];

        assert_eq!(
            create_verify_delete(&mut store, "TEST.TMP", b"test", &mut readback),
            Ok(())
        );
        assert_eq!(readback, *b"test");
        assert!(store.deleted);
        assert!(store.file.is_none());
    }

    #[test]
    fn never_deletes_a_preexisting_target() {
        let mut store = FakeStore::empty();
        store.file = Some(b"mine".to_vec());
        let mut readback = [0; 4];

        assert_eq!(
            create_verify_delete(&mut store, "TEST.TMP", b"test", &mut readback),
            Err(TemporaryFileTestError::TargetAlreadyExists)
        );
        assert_eq!(store.file.as_deref(), Some(b"mine".as_slice()));
        assert!(!store.deleted);
    }

    #[test]
    fn cleans_up_a_file_left_by_a_failed_create() {
        let mut store = FakeStore::empty();
        store.create_error_after_file = true;
        let mut readback = [0; 4];

        assert_eq!(
            create_verify_delete(&mut store, "TEST.TMP", b"test", &mut readback),
            Err(TemporaryFileTestError::Store {
                phase: super::TemporaryFilePhase::Create,
                error: FakeError::Create,
            })
        );
        assert!(store.deleted);
        assert!(store.file.is_none());
    }

    #[test]
    fn reports_cleanup_failure_instead_of_hiding_a_remaining_file() {
        let mut store = FakeStore::empty();
        store.delete_error = true;
        let mut readback = [0; 4];

        assert_eq!(
            create_verify_delete(&mut store, "TEST.TMP", b"test", &mut readback),
            Err(TemporaryFileTestError::Cleanup(FakeError::Delete))
        );
        assert!(store.file.is_some());
    }
}

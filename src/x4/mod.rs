pub mod input;
pub mod shared_display;
pub mod shared_spi;

#[cfg(target_arch = "riscv32")]
mod board;
#[cfg(all(target_arch = "riscv32", feature = "device-reader"))]
mod reader_app;
#[cfg(target_arch = "riscv32")]
mod storage;

#[cfg(target_arch = "riscv32")]
pub use board::SharedSpiChipSelects;
#[cfg(target_arch = "riscv32")]
pub use input::{InputReadError, X4InputHardware, X4InputPeripherals};
pub use input::{X4ButtonDecodeError, decode_buttons};
#[cfg(all(target_arch = "riscv32", feature = "device-reader"))]
pub use reader_app::{reader_app_task, reader_input_task};
#[cfg(all(target_arch = "riscv32", feature = "sd-write-diagnostic"))]
pub use storage::X4FatBlockDevice;
#[cfg(all(target_arch = "riscv32", feature = "sd-card"))]
pub use storage::{X4FatBlockDeviceError, X4ReadOnlyFatBlockDevice};
#[cfg(target_arch = "riscv32")]
pub use storage::{X4SharedSpiPeripherals, X4StorageError, X4StorageHardware};

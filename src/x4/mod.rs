pub mod input;
pub mod shared_spi;
pub mod spec;

#[cfg(target_arch = "riscv32")]
mod board;
#[cfg(target_arch = "riscv32")]
mod display;
#[cfg(target_arch = "riscv32")]
mod storage;

#[cfg(target_arch = "riscv32")]
pub use board::SharedSpiChipSelects;
#[cfg(target_arch = "riscv32")]
pub use display::X4DisplayHardware;
#[cfg(target_arch = "riscv32")]
pub use input::{InputReadError, X4InputHardware, X4InputPeripherals};
pub use input::{X4ButtonDecodeError, decode_buttons};
#[cfg(all(target_arch = "riscv32", feature = "sd-write-diagnostic"))]
pub use storage::{X4FatBlockDevice, X4FatBlockDeviceError};
#[cfg(target_arch = "riscv32")]
pub use storage::{X4SharedSpiPeripherals, X4StorageError, X4StorageHardware};

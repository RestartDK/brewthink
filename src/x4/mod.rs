pub mod spec;

#[cfg(target_arch = "riscv32")]
mod board;
#[cfg(target_arch = "riscv32")]
mod display;

#[cfg(target_arch = "riscv32")]
pub use board::SharedSpiChipSelects;
#[cfg(target_arch = "riscv32")]
pub use display::X4DisplayHardware;

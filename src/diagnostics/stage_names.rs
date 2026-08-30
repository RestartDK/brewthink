pub const HEARTBEAT: &str = "heartbeat";
pub const INPUTS_RAW: &str = "inputs-raw";
pub const INPUTS_EVENTS: &str = "inputs-events";
pub const POWER_USB: &str = "power-usb";
pub const STORAGE_READONLY: &str = "storage-readonly";
pub const STORAGE_WRITE_TEST: &str = "storage-write-test";
pub const INTEGRATED_DEVICE: &str = "integrated-device";
pub const SLEEP_WAKE: &str = "sleep-wake";
pub const DISPLAY_RESET: &str = "display-reset";
pub const DISPLAY_INITIALIZE: &str = "display-initialize";
pub const DISPLAY_WRITE: &str = "display-write";
pub const DISPLAY_REFRESH: &str = "display-refresh";
pub const DISPLAY_BLACK: &str = "display-black";
pub const DISPLAY_CHECKERBOARD: &str = "display-checkerboard";
pub const DISPLAY_ORIENTATION: &str = "display-orientation";
pub const DISPLAY_IMAGE: &str = "display-image";

pub const ALL_STAGES: &[&str] = &[
    HEARTBEAT,
    INPUTS_RAW,
    INPUTS_EVENTS,
    POWER_USB,
    STORAGE_READONLY,
    STORAGE_WRITE_TEST,
    INTEGRATED_DEVICE,
    SLEEP_WAKE,
    DISPLAY_RESET,
    DISPLAY_INITIALIZE,
    DISPLAY_WRITE,
    DISPLAY_REFRESH,
    DISPLAY_BLACK,
    DISPLAY_CHECKERBOARD,
    DISPLAY_ORIENTATION,
    DISPLAY_IMAGE,
];

pub const DISPLAY_ROTATIONS: &[&str] = &["0", "90", "180", "270"];

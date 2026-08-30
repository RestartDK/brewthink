pub mod stage_names;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticStage {
    Heartbeat,
    Input(InputDiagnosticStage),
    StorageReadOnly,
    StorageWriteTest,
    IntegratedDevice,
    SleepWake,
    DisplayReset,
    Display(DisplayDiagnosticStage),
}

impl DiagnosticStage {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Heartbeat => stage_names::HEARTBEAT,
            Self::Input(stage) => stage.name(),
            Self::StorageReadOnly => stage_names::STORAGE_READONLY,
            Self::StorageWriteTest => stage_names::STORAGE_WRITE_TEST,
            Self::IntegratedDevice => stage_names::INTEGRATED_DEVICE,
            Self::SleepWake => stage_names::SLEEP_WAKE,
            Self::DisplayReset => stage_names::DISPLAY_RESET,
            Self::Display(stage) => stage.name(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputDiagnosticStage {
    Raw,
    Events,
    PowerUsb,
}

impl InputDiagnosticStage {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Raw => stage_names::INPUTS_RAW,
            Self::Events => stage_names::INPUTS_EVENTS,
            Self::PowerUsb => stage_names::POWER_USB,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnknownDiagnosticStage;

impl core::str::FromStr for DiagnosticStage {
    type Err = UnknownDiagnosticStage;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            stage_names::HEARTBEAT => Ok(Self::Heartbeat),
            stage_names::INPUTS_RAW => Ok(Self::Input(InputDiagnosticStage::Raw)),
            stage_names::INPUTS_EVENTS => Ok(Self::Input(InputDiagnosticStage::Events)),
            stage_names::POWER_USB => Ok(Self::Input(InputDiagnosticStage::PowerUsb)),
            stage_names::STORAGE_READONLY => Ok(Self::StorageReadOnly),
            stage_names::STORAGE_WRITE_TEST => Ok(Self::StorageWriteTest),
            stage_names::INTEGRATED_DEVICE => Ok(Self::IntegratedDevice),
            stage_names::SLEEP_WAKE => Ok(Self::SleepWake),
            stage_names::DISPLAY_RESET => Ok(Self::DisplayReset),
            stage_names::DISPLAY_INITIALIZE => {
                Ok(Self::Display(DisplayDiagnosticStage::Initialize))
            }
            stage_names::DISPLAY_WRITE => Ok(Self::Display(DisplayDiagnosticStage::Write)),
            stage_names::DISPLAY_REFRESH => Ok(Self::Display(DisplayDiagnosticStage::Refresh)),
            stage_names::DISPLAY_BLACK => Ok(Self::Display(DisplayDiagnosticStage::Black)),
            stage_names::DISPLAY_CHECKERBOARD => {
                Ok(Self::Display(DisplayDiagnosticStage::Checkerboard))
            }
            stage_names::DISPLAY_ORIENTATION => {
                Ok(Self::Display(DisplayDiagnosticStage::Orientation))
            }
            stage_names::DISPLAY_IMAGE => Ok(Self::Display(DisplayDiagnosticStage::Image)),
            _ => Err(UnknownDiagnosticStage),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisplayDiagnosticStage {
    Initialize,
    Write,
    Refresh,
    Black,
    Checkerboard,
    Orientation,
    Image,
}

impl DisplayDiagnosticStage {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Initialize => stage_names::DISPLAY_INITIALIZE,
            Self::Write => stage_names::DISPLAY_WRITE,
            Self::Refresh => stage_names::DISPLAY_REFRESH,
            Self::Black => stage_names::DISPLAY_BLACK,
            Self::Checkerboard => stage_names::DISPLAY_CHECKERBOARD,
            Self::Orientation => stage_names::DISPLAY_ORIENTATION,
            Self::Image => stage_names::DISPLAY_IMAGE,
        }
    }

    pub const fn uses_rotation(self) -> bool {
        matches!(self, Self::Orientation | Self::Image)
    }
}

#[cfg(test)]
mod tests {
    use super::{DiagnosticStage, DisplayDiagnosticStage, InputDiagnosticStage, stage_names};

    #[test]
    fn parses_every_supported_stage() {
        let cases = [
            ("heartbeat", DiagnosticStage::Heartbeat),
            (
                "inputs-raw",
                DiagnosticStage::Input(InputDiagnosticStage::Raw),
            ),
            (
                "inputs-events",
                DiagnosticStage::Input(InputDiagnosticStage::Events),
            ),
            (
                "power-usb",
                DiagnosticStage::Input(InputDiagnosticStage::PowerUsb),
            ),
            ("storage-readonly", DiagnosticStage::StorageReadOnly),
            ("storage-write-test", DiagnosticStage::StorageWriteTest),
            ("integrated-device", DiagnosticStage::IntegratedDevice),
            ("sleep-wake", DiagnosticStage::SleepWake),
            ("display-reset", DiagnosticStage::DisplayReset),
            (
                "display-initialize",
                DiagnosticStage::Display(DisplayDiagnosticStage::Initialize),
            ),
            (
                "display-write",
                DiagnosticStage::Display(DisplayDiagnosticStage::Write),
            ),
            (
                "display-refresh",
                DiagnosticStage::Display(DisplayDiagnosticStage::Refresh),
            ),
            (
                "display-black",
                DiagnosticStage::Display(DisplayDiagnosticStage::Black),
            ),
            (
                "display-checkerboard",
                DiagnosticStage::Display(DisplayDiagnosticStage::Checkerboard),
            ),
            (
                "display-orientation",
                DiagnosticStage::Display(DisplayDiagnosticStage::Orientation),
            ),
            (
                "display-image",
                DiagnosticStage::Display(DisplayDiagnosticStage::Image),
            ),
        ];

        for (name, expected) in cases {
            let parsed: DiagnosticStage = name.parse().unwrap();
            assert_eq!(parsed, expected);
            assert_eq!(parsed.name(), name);
        }
    }

    #[test]
    fn rejects_legacy_and_unknown_stage_names() {
        assert!("none".parse::<DiagnosticStage>().is_err());
        assert!("reset".parse::<DiagnosticStage>().is_err());
        assert!("unknown".parse::<DiagnosticStage>().is_err());
    }

    #[test]
    fn stage_name_list_matches_the_parser() {
        assert_eq!(stage_names::ALL_STAGES.len(), 16);
        for name in stage_names::ALL_STAGES {
            let parsed: DiagnosticStage = name.parse().unwrap();
            assert_eq!(parsed.name(), *name);
        }
    }

    #[test]
    fn rotation_is_limited_to_orientation_and_image() {
        for stage in [
            DisplayDiagnosticStage::Initialize,
            DisplayDiagnosticStage::Write,
            DisplayDiagnosticStage::Refresh,
            DisplayDiagnosticStage::Black,
            DisplayDiagnosticStage::Checkerboard,
        ] {
            assert!(!stage.uses_rotation());
        }
        assert!(DisplayDiagnosticStage::Orientation.uses_rotation());
        assert!(DisplayDiagnosticStage::Image.uses_rotation());
    }
}

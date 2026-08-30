#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticStage {
    Heartbeat,
    InputsRaw,
    InputsEvents,
    PowerUsb,
    StorageReadOnly,
    StorageWriteTest,
    IntegratedDevice,
    SleepWake,
    Display(DisplayDiagnosticStage),
}

impl DiagnosticStage {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Heartbeat => "heartbeat",
            Self::InputsRaw => "inputs-raw",
            Self::InputsEvents => "inputs-events",
            Self::PowerUsb => "power-usb",
            Self::StorageReadOnly => "storage-readonly",
            Self::StorageWriteTest => "storage-write-test",
            Self::IntegratedDevice => "integrated-device",
            Self::SleepWake => "sleep-wake",
            Self::Display(stage) => stage.name(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnknownDiagnosticStage;

impl core::str::FromStr for DiagnosticStage {
    type Err = UnknownDiagnosticStage;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "heartbeat" => Ok(Self::Heartbeat),
            "inputs-raw" => Ok(Self::InputsRaw),
            "inputs-events" => Ok(Self::InputsEvents),
            "power-usb" => Ok(Self::PowerUsb),
            "storage-readonly" => Ok(Self::StorageReadOnly),
            "storage-write-test" => Ok(Self::StorageWriteTest),
            "integrated-device" => Ok(Self::IntegratedDevice),
            "sleep-wake" => Ok(Self::SleepWake),
            "display-reset" => Ok(Self::Display(DisplayDiagnosticStage::Reset)),
            "display-initialize" => Ok(Self::Display(DisplayDiagnosticStage::Initialize)),
            "display-write" => Ok(Self::Display(DisplayDiagnosticStage::Write)),
            "display-refresh" => Ok(Self::Display(DisplayDiagnosticStage::Refresh)),
            "display-black" => Ok(Self::Display(DisplayDiagnosticStage::Black)),
            "display-checkerboard" => Ok(Self::Display(DisplayDiagnosticStage::Checkerboard)),
            "display-orientation" => Ok(Self::Display(DisplayDiagnosticStage::Orientation)),
            "display-image" => Ok(Self::Display(DisplayDiagnosticStage::Image)),
            _ => Err(UnknownDiagnosticStage),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisplayDiagnosticStage {
    Reset,
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
            Self::Reset => "display-reset",
            Self::Initialize => "display-initialize",
            Self::Write => "display-write",
            Self::Refresh => "display-refresh",
            Self::Black => "display-black",
            Self::Checkerboard => "display-checkerboard",
            Self::Orientation => "display-orientation",
            Self::Image => "display-image",
        }
    }

    pub const fn uses_rotation(self) -> bool {
        matches!(self, Self::Orientation | Self::Image)
    }
}

#[cfg(test)]
mod tests {
    use super::{DiagnosticStage, DisplayDiagnosticStage};

    #[test]
    fn parses_every_supported_stage() {
        let cases = [
            ("heartbeat", DiagnosticStage::Heartbeat),
            ("inputs-raw", DiagnosticStage::InputsRaw),
            ("inputs-events", DiagnosticStage::InputsEvents),
            ("power-usb", DiagnosticStage::PowerUsb),
            ("storage-readonly", DiagnosticStage::StorageReadOnly),
            ("storage-write-test", DiagnosticStage::StorageWriteTest),
            ("integrated-device", DiagnosticStage::IntegratedDevice),
            ("sleep-wake", DiagnosticStage::SleepWake),
            (
                "display-reset",
                DiagnosticStage::Display(DisplayDiagnosticStage::Reset),
            ),
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
    fn rotation_is_limited_to_orientation_and_image() {
        for stage in [
            DisplayDiagnosticStage::Reset,
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

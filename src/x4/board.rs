use esp_hal::{
    gpio::{Level, Output, OutputConfig},
    peripherals::{GPIO12, GPIO21},
};

pub struct SharedSpiChipSelects<'d> {
    display: Output<'d>,
    sd: Output<'d>,
}

impl<'d> SharedSpiChipSelects<'d> {
    pub fn deselected(display: GPIO21<'d>, sd: GPIO12<'d>) -> Self {
        Self {
            display: Output::new(display, Level::High, OutputConfig::default()),
            sd: Output::new(sd, Level::High, OutputConfig::default()),
        }
    }

    pub fn display_is_deselected(&self) -> bool {
        self.display.is_set_high()
    }

    pub fn sd_is_deselected(&self) -> bool {
        self.sd.is_set_high()
    }

    pub fn both_are_deselected(&self) -> bool {
        self.display_is_deselected() && self.sd_is_deselected()
    }

    pub fn into_parts(self) -> (Output<'d>, Output<'d>) {
        (self.display, self.sd)
    }
}

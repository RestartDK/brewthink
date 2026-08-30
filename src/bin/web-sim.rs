use brewthink::image::{Dither, MonochromeImage, RenderOptions, RgbImage, ScaleMode, Size};
use wasm_bindgen::prelude::*;

const WIDTH: usize = 480;
const HEIGHT: usize = 800;

#[wasm_bindgen]
#[derive(Clone, Copy)]
pub enum FitMode {
    Contain,
    Cover,
}

#[wasm_bindgen]
#[derive(Clone, Copy)]
pub enum DitherMode {
    Ordered,
    Threshold,
}

#[wasm_bindgen]
pub struct RenderedFrame {
    pixels: Vec<u8>,
    source_width: usize,
    source_height: usize,
    content_width: usize,
    content_height: usize,
    black_pixels: usize,
}

#[wasm_bindgen]
impl RenderedFrame {
    #[wasm_bindgen(getter)]
    pub fn width(&self) -> usize {
        WIDTH
    }

    #[wasm_bindgen(getter)]
    pub fn height(&self) -> usize {
        HEIGHT
    }

    #[wasm_bindgen(getter)]
    pub fn source_width(&self) -> usize {
        self.source_width
    }

    #[wasm_bindgen(getter)]
    pub fn source_height(&self) -> usize {
        self.source_height
    }

    #[wasm_bindgen(getter)]
    pub fn content_width(&self) -> usize {
        self.content_width
    }

    #[wasm_bindgen(getter)]
    pub fn content_height(&self) -> usize {
        self.content_height
    }

    #[wasm_bindgen(getter)]
    pub fn black_pixels(&self) -> usize {
        self.black_pixels
    }

    #[wasm_bindgen(getter)]
    pub fn payload_bytes(&self) -> usize {
        self.pixels.len()
    }

    pub fn pixels(&self) -> Vec<u8> {
        self.pixels.clone()
    }
}

#[wasm_bindgen]
pub fn render_image(
    encoded: &[u8],
    fit: FitMode,
    dither: DitherMode,
) -> Result<RenderedFrame, JsValue> {
    let decoded = image::load_from_memory(encoded).map_err(js_error)?;
    let rgb = decoded.into_rgb8();
    let source_size = Size::new(rgb.width() as usize, rgb.height() as usize).map_err(js_error)?;
    let source = RgbImage::new(source_size, rgb.as_raw()).map_err(js_error)?;
    let target_size = Size::new(WIDTH, HEIGHT).map_err(js_error)?;
    let mut pixels = vec![0xFF; WIDTH * HEIGHT / 8];
    let report = {
        let mut target = MonochromeImage::new(target_size, &mut pixels).map_err(js_error)?;
        brewthink::image::render(
            &source,
            &mut target,
            RenderOptions {
                scale: match fit {
                    FitMode::Contain => ScaleMode::Contain,
                    FitMode::Cover => ScaleMode::Cover,
                },
                dither: match dither {
                    DitherMode::Ordered => Dither::Ordered4x4,
                    DitherMode::Threshold => Dither::Threshold(128),
                },
            },
        )
    };
    let black_pixels = pixels
        .iter()
        .map(|byte| 8 - byte.count_ones() as usize)
        .sum();

    Ok(RenderedFrame {
        pixels,
        source_width: source_size.width(),
        source_height: source_size.height(),
        content_width: report.scaled.width(),
        content_height: report.scaled.height(),
        black_pixels,
    })
}

#[wasm_bindgen]
pub fn renderer_version() -> String {
    env!("CARGO_PKG_VERSION").into()
}

fn js_error(error: impl core::fmt::Debug) -> JsValue {
    JsValue::from_str(&format!("{error:?}"))
}

fn main() {}

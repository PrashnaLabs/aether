use std::path::Path;
use std::sync::Arc;

use gpui_kit::RenderImage;
use image::{ImageDecoder, ImageReader};

const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PIXELS: u64 = 32 * 1024 * 1024;
const MAX_DIMENSION: u32 = 16_384;

pub(crate) fn is_image(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| {
        matches!(
            ext.to_ascii_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "webp" | "gif" | "tif" | "tiff" | "bmp"
        )
    })
}

pub(super) fn fit_scale(image: [f32; 2], area: [f32; 2]) -> f32 {
    if image.iter().any(|v| *v <= 0.) {
        return 1.;
    }
    (area[0].max(1.) / image[0]).min(area[1].max(1.) / image[1]).min(1.)
}

pub(super) fn clamp_pan(pan: [f32; 2], image: [f32; 2], area: [f32; 2]) -> [f32; 2] {
    std::array::from_fn(|i| {
        let limit = ((image[i] - area[i]) / 2.).max(0.);
        pan[i].clamp(-limit, limit)
    })
}

pub(super) struct LoadedImage {
    pub pixels: Arc<RenderImage>,
    pub dimensions: [u32; 2],
}

pub(super) fn load(path: &Path) -> Result<LoadedImage, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("Couldn't open image: {e}"))?;
    if file.metadata().map_err(|e| e.to_string())?.len() > MAX_FILE_BYTES {
        return Err("This image exceeds the 64 MB file limit.".into());
    }
    let mut reader = ImageReader::new(std::io::BufReader::new(file))
        .with_guessed_format()
        .map_err(|e| format!("Couldn't read image: {e}"))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_PIXELS * 4);
    reader.limits(limits);
    let decoder = reader.into_decoder().map_err(|e| format!("Couldn't decode image: {e}"))?;
    let (width, height) = decoder.dimensions();
    if width > MAX_DIMENSION
        || height > MAX_DIMENSION
        || width == 0
        || height == 0
        || u64::from(width) * u64::from(height) > MAX_PIXELS
        || decoder.total_bytes() > MAX_PIXELS * 4
    {
        return Err("This image exceeds the preview size limit.".into());
    }
    let mut pixels = image::DynamicImage::from_decoder(decoder)
        .map_err(|e| format!("Couldn't decode image: {e}"))?
        .into_rgba8();
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    Ok(LoadedImage {
        pixels: Arc::new(RenderImage::new(vec![image::Frame::new(pixels)])),
        dimensions: [width, height],
    })
}

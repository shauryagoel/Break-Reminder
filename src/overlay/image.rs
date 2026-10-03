use std::{
    fs::File,
    io::{Cursor, Read},
    path::Path,
};

use eframe::egui::{ColorImage, Rect, Vec2, pos2, vec2};
use image::{ImageFormat, ImageReader, Limits};

use crate::config::ImageFit;

const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;
const MAX_DIMENSION: u32 = 4096;

pub(super) fn load(path: &Path, max_texture_side: usize) -> Result<ColorImage, String> {
    let file = File::open(path).map_err(|error| error.to_string())?;
    if file.metadata().map_err(|error| error.to_string())?.len() > MAX_FILE_BYTES {
        return Err("image exceeds 20 MiB".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err("image exceeds 20 MiB".into());
    }
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| error.to_string())?;
    if !matches!(
        reader.format(),
        Some(ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP)
    ) {
        return Err("image must be PNG, JPEG, or WebP".into());
    }
    let mut limits = Limits::default();
    let max_dimension = MAX_DIMENSION.min(max_texture_side.try_into().unwrap_or(MAX_DIMENSION));
    limits.max_image_width = Some(max_dimension);
    limits.max_image_height = Some(max_dimension);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader.decode().map_err(|error| error.to_string())?;
    let rgba = decoded.to_rgba8();
    Ok(ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    ))
}

pub(super) fn geometry(source: Vec2, bounds: Vec2, fit: ImageFit) -> (Vec2, Rect) {
    let scale_x = bounds.x / source.x;
    let scale_y = bounds.y / source.y;
    match fit {
        ImageFit::Contain => (
            source * scale_x.min(scale_y),
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        ),
        ImageFit::Cover => {
            let scaled = source * scale_x.max(scale_y);
            let visible = vec2(bounds.x / scaled.x, bounds.y / scaled.y);
            (bounds, Rect::from_center_size(pos2(0.5, 0.5), visible))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Cursor, path::PathBuf};

    use eframe::egui::{Rect, pos2, vec2};
    use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba};

    use super::{geometry, load};
    use crate::config::ImageFit;

    fn encoded(format: ImageFormat, width: u32, height: u32) -> Vec<u8> {
        let picture = DynamicImage::ImageRgba8(ImageBuffer::from_pixel(
            width,
            height,
            Rgba([60, 120, 180, 255]),
        ));
        let mut bytes = Cursor::new(Vec::new());
        picture.write_to(&mut bytes, format).unwrap();
        bytes.into_inner()
    }

    fn temporary_file(bytes: &[u8]) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "break-reminder-image-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn decodes_only_bounded_png_jpeg_and_webp() {
        for format in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::WebP] {
            let path = temporary_file(&encoded(format, 2, 3));
            let picture = load(&path, 4096).unwrap();
            assert_eq!(picture.size, [2, 3]);
            assert!(load(&path, 2).is_err());
            fs::remove_file(&path).unwrap();
        }
        let path = temporary_file(b"GIF89a\x01\x00\x01\x00");
        assert!(load(&path, 4096).is_err());
        fs::remove_file(&path).unwrap();

        let path = temporary_file(&encoded(ImageFormat::Png, 4097, 1));
        assert!(load(&path, 4096).is_err());
        fs::remove_file(&path).unwrap();

        let path = temporary_file(b"not an image");
        assert!(load(&path, 4096).is_err());
        fs::remove_file(&path).unwrap();
        assert!(load(&path, 4096).is_err());
    }

    #[test]
    fn rejects_files_over_twenty_mebibytes() {
        let path = temporary_file(&[]);
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(20 * 1024 * 1024 + 1)
            .unwrap();
        assert!(load(&path, 4096).unwrap_err().contains("20 MiB"));
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn contain_and_cover_keep_aspect_and_crop_from_center() {
        let (size, uv) = geometry(vec2(400.0, 200.0), vec2(300.0, 180.0), ImageFit::Contain);
        assert_eq!(size, vec2(300.0, 150.0));
        assert_eq!(uv, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)));

        let (size, uv) = geometry(vec2(400.0, 200.0), vec2(300.0, 180.0), ImageFit::Cover);
        assert_eq!(size, vec2(300.0, 180.0));
        assert!((uv.min.x - 1.0 / 12.0).abs() < 0.0001);
        assert!((uv.max.x - 11.0 / 12.0).abs() < 0.0001);
        assert_eq!(uv.min.y, 0.0);
        assert_eq!(uv.max.y, 1.0);
    }
}

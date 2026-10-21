#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormatKind {
    Jpeg,
    Png,
    Webp,
    Avif,
    Jxl,
}

impl ImageFormatKind {
    pub const EXTENSIONS: &'static [&'static str] = &["jpg", "jpeg", "png", "webp", "avif", "jxl"];

    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "png" => Some(Self::Png),
            "webp" => Some(Self::Webp),
            "avif" => Some(Self::Avif),
            "jxl" => Some(Self::Jxl),
            _ => None,
        }
    }

    pub const fn supports_alpha(self) -> bool {
        matches!(self, Self::Png | Self::Webp | Self::Avif | Self::Jxl)
    }

    pub const fn may_be_animated(self) -> bool {
        matches!(self, Self::Png)
    }

    pub const fn preview_format(self) -> image::ImageFormat {
        if self.supports_alpha() {
            image::ImageFormat::Png
        } else {
            image::ImageFormat::Jpeg
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ImageFormatKind as Format;

    #[test]
    fn recognizes_case_insensitive_tinify_extensions() {
        assert_eq!(Format::from_extension("JPG"), Some(Format::Jpeg));
        assert_eq!(Format::from_extension("jpeg"), Some(Format::Jpeg));
        assert_eq!(Format::from_extension("Png"), Some(Format::Png));
        assert_eq!(Format::from_extension("WEBP"), Some(Format::Webp));
        assert_eq!(Format::from_extension("avif"), Some(Format::Avif));
        assert_eq!(Format::from_extension("JXL"), Some(Format::Jxl));
    }

    #[test]
    fn rejects_formats_outside_the_tinify_image_scope() {
        assert_eq!(Format::from_extension("gif"), None);
        assert_eq!(Format::from_extension("svg"), None);
        assert_eq!(Format::from_extension("heic"), None);
        assert_eq!(Format::from_extension("mp4"), None);
    }

    #[test]
    fn exposes_alpha_and_animation_capabilities() {
        assert!(!Format::Jpeg.supports_alpha());
        assert!(Format::Png.supports_alpha());
        assert!(Format::Webp.supports_alpha());
        assert!(Format::Avif.supports_alpha());
        assert!(Format::Jxl.supports_alpha());
        assert!(Format::Png.may_be_animated());
        assert!(!Format::Webp.may_be_animated());
    }
}

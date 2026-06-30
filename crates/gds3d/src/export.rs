use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportFormat {
    Png,
    Svg,
    Pdf,
    Gltf,
}

impl ExportFormat {
    pub const ALL: [Self; 4] = [Self::Png, Self::Svg, Self::Pdf, Self::Gltf];

    pub fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Svg => "SVG",
            Self::Pdf => "PDF",
            Self::Gltf => "glTF",
        }
    }

    pub fn needs_image_size(self) -> bool {
        matches!(self, Self::Png | Self::Svg | Self::Pdf)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportQuality {
    Low,
    Standard,
    High,
}

impl ExportQuality {
    pub const ALL: [Self; 3] = [Self::Low, Self::Standard, Self::High];

    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Standard => "Standard",
            Self::High => "High",
        }
    }

    fn width(self) -> u32 {
        match self {
            Self::Low => 2000,
            Self::Standard => 4000,
            Self::High => 6000,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportSizePreset {
    Figure4x3,
    Figure3x2,
    Wide16x9,
    Square1x1,
}

impl ExportSizePreset {
    pub const ALL: [Self; 4] = [
        Self::Figure4x3,
        Self::Figure3x2,
        Self::Wide16x9,
        Self::Square1x1,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Figure4x3 => "Figure 4:3",
            Self::Figure3x2 => "Figure 3:2",
            Self::Wide16x9 => "Wide 16:9",
            Self::Square1x1 => "Square 1:1",
        }
    }

    fn ratio(self) -> (u32, u32) {
        match self {
            Self::Figure4x3 => (4, 3),
            Self::Figure3x2 => (3, 2),
            Self::Wide16x9 => (16, 9),
            Self::Square1x1 => (1, 1),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportSettings {
    pub format: ExportFormat,
    pub quality: ExportQuality,
    pub size_preset: ExportSizePreset,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            format: ExportFormat::Png,
            quality: ExportQuality::Standard,
            size_preset: ExportSizePreset::Figure4x3,
        }
    }
}

impl ExportSettings {
    pub fn image_size(self) -> Option<(u32, u32)> {
        if !self.format.needs_image_size() {
            return None;
        }

        let width = self.quality.width();
        let (ratio_w, ratio_h) = self.size_preset.ratio();
        let height = width * ratio_h / ratio_w;
        Some((width, height))
    }
}

use rust_i18n::t;
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

    pub fn label(self) -> String {
        match self {
            Self::Png => t!("export.format_png").to_string(),
            Self::Svg => t!("export.format_svg").to_string(),
            Self::Pdf => t!("export.format_pdf").to_string(),
            Self::Gltf => t!("export.format_gltf").to_string(),
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

    pub fn label(self) -> String {
        match self {
            Self::Low => t!("export.quality_low").to_string(),
            Self::Standard => t!("export.quality_standard").to_string(),
            Self::High => t!("export.quality_high").to_string(),
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

    pub fn label(self) -> String {
        match self {
            Self::Figure4x3 => t!("export.size_figure_4_3").to_string(),
            Self::Figure3x2 => t!("export.size_figure_3_2").to_string(),
            Self::Wide16x9 => t!("export.size_wide_16_9").to_string(),
            Self::Square1x1 => t!("export.size_square_1_1").to_string(),
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

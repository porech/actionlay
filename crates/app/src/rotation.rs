//! Manual orientation of decoded video; overlays use the oriented canvas.
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Rotation {
    #[default]
    #[value(name = "auto")]
    Automatic,
    #[value(name = "0")]
    None,
    #[value(name = "90")]
    Clockwise90,
    #[value(name = "180")]
    HalfTurn,
    #[value(name = "270")]
    CounterClockwise90,
}

impl Rotation {
    pub const ALL: [Self; 5] = [
        Self::Automatic,
        Self::None,
        Self::Clockwise90,
        Self::HalfTurn,
        Self::CounterClockwise90,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Automatic => "Automatic",
            Self::None => "0° (original)",
            Self::Clockwise90 => "90° clockwise",
            Self::HalfTurn => "180°",
            Self::CounterClockwise90 => "90° counterclockwise",
        }
    }

    pub fn turns(self) -> u32 {
        match self {
            Self::Automatic | Self::None => 0,
            Self::Clockwise90 => 1,
            Self::HalfTurn => 2,
            Self::CounterClockwise90 => 3,
        }
    }

    pub fn resolve(self, clockwise_degrees: u16) -> Self {
        if self != Self::Automatic {
            return self;
        }
        match clockwise_degrees {
            90 => Self::Clockwise90,
            180 => Self::HalfTurn,
            270 => Self::CounterClockwise90,
            _ => Self::None,
        }
    }

    pub fn dimensions(self, width: u32, height: u32) -> [u32; 2] {
        if self.turns() % 2 == 1 {
            [height, width]
        } else {
            [width, height]
        }
    }

    pub fn rgba(self, pixels: Vec<u8>, width: u32, height: u32) -> Vec<u8> {
        if matches!(self, Self::None | Self::Automatic) {
            return pixels;
        }
        let source =
            image::RgbaImage::from_raw(width, height, pixels).expect("decoded RGBA frame size");
        match self {
            Self::Clockwise90 => image::imageops::rotate90(&source),
            Self::HalfTurn => image::imageops::rotate180(&source),
            Self::CounterClockwise90 => image::imageops::rotate270(&source),
            Self::None | Self::Automatic => unreachable!(),
        }
        .into_raw()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asymmetric_frame_rotates_in_the_selected_direction() {
        // 3x2, numbered pixels; preserve all four channels.
        let pixels: Vec<u8> = (1..=6).flat_map(|n| [n, n + 10, n + 20, 255]).collect();
        for (rotation, dimensions, expected) in [
            (Rotation::None, [3, 2], vec![1, 2, 3, 4, 5, 6]),
            (Rotation::Clockwise90, [2, 3], vec![4, 1, 5, 2, 6, 3]),
            (Rotation::HalfTurn, [3, 2], vec![6, 5, 4, 3, 2, 1]),
            (Rotation::CounterClockwise90, [2, 3], vec![3, 6, 2, 5, 1, 4]),
        ] {
            assert_eq!(rotation.dimensions(3, 2), dimensions);
            let output = rotation.rgba(pixels.clone(), 3, 2);
            let expected: Vec<u8> = expected
                .into_iter()
                .flat_map(|n| [n, n + 10, n + 20, 255])
                .collect();
            assert_eq!(output, expected);
        }
    }
}

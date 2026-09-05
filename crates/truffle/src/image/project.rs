use anyhow::{ensure, Context, Result};
use image::{Rgba, RgbaImage};
use serde::Deserialize;
use std::{collections::HashSet, fs::File, io::BufReader, path::Path};

const MAX_PIXELS: u64 = 64 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectionMap {
    version: u32,
    source_size: [u32; 2],
    output_size: [u32; 2],
    #[serde(default = "white_palette")]
    palette: Vec<[u8; 4]>,
    rows: Vec<ProjectionRow>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectionRow {
    at: [u32; 2],
    pixels: Vec<[u32; 3]>,
}

fn white_palette() -> Vec<[u8; 4]> {
    vec![[255; 4]]
}

fn validate_size([width, height]: [u32; 2], name: &str) -> Result<()> {
    ensure!(
        width > 0 && height > 0,
        "{name} dimensions must be positive"
    );
    ensure!(
        u64::from(width) * u64::from(height) <= MAX_PIXELS,
        "{name} exceeds the 64 megapixel limit"
    );
    Ok(())
}

impl ProjectionMap {
    pub fn load(path: &Path) -> Result<Self> {
        let file =
            File::open(path).with_context(|| format!("Failed to read map {}", path.display()))?;
        let map: Self = serde_json::from_reader(BufReader::new(file))
            .with_context(|| format!("Invalid projection JSON in {}", path.display()))?;
        Ok(map)
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1,
            "Unsupported projection version {}; expected 1",
            self.version
        );
        validate_size(self.source_size, "source_size")?;
        validate_size(self.output_size, "output_size")?;
        ensure!(
            !self.palette.is_empty(),
            "palette must contain at least one RGBA multiplier"
        );
        let mut destinations = HashSet::new();
        for (index, row) in self.rows.iter().enumerate() {
            let [x, y] = row.at;
            ensure!(!row.pixels.is_empty(), "rows[{index}] must contain pixels");
            ensure!(
                y < self.output_size[1]
                    && u64::from(x) + row.pixels.len() as u64 <= u64::from(self.output_size[0]),
                "rows[{index}] extends outside output_size"
            );
            for (offset, &[source_x, source_y, shade]) in row.pixels.iter().enumerate() {
                ensure!(
                    source_x < self.source_size[0] && source_y < self.source_size[1],
                    "rows[{index}].pixels[{offset}] source coordinate [{source_x}, {source_y}] is outside source_size"
                );
                ensure!(
                    (shade as usize) < self.palette.len(),
                    "rows[{index}].pixels[{offset}] palette index {shade} does not exist"
                );
                ensure!(
                    destinations.insert((x + offset as u32, y)),
                    "rows[{index}].pixels[{offset}] overlaps another destination pixel"
                );
            }
        }
        Ok(())
    }

    pub fn project(&self, source: &RgbaImage) -> Result<RgbaImage> {
        self.validate()?;
        ensure!(
            [source.width(), source.height()] == self.source_size,
            "Source image is {}x{}; map requires {}x{}",
            source.width(),
            source.height(),
            self.source_size[0],
            self.source_size[1]
        );
        let mut output = RgbaImage::new(self.output_size[0], self.output_size[1]);
        for row in &self.rows {
            for (offset, &[source_x, source_y, shade]) in row.pixels.iter().enumerate() {
                let color = source.get_pixel(source_x, source_y);
                let tint = self.palette[shade as usize];
                let mut result = [0; 4];
                for channel in 0..4 {
                    result[channel] =
                        ((u16::from(color[channel]) * u16::from(tint[channel]) + 127) / 255) as u8;
                }
                if result[3] == 0 {
                    result = [0; 4];
                }
                output.put_pixel(row.at[0] + offset as u32, row.at[1], Rgba(result));
            }
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> ProjectionMap {
        serde_json::from_str(
            r#"{
            "version": 1, "source_size": [300, 2], "output_size": [4, 3],
            "palette": [[255,255,255,255], [128,64,255,128]],
            "rows": [{"at": [1,1], "pixels": [[299,1,0], [0,0,1]]}]
        }"#,
        )
        .unwrap()
    }

    #[test]
    fn projects_sparse_coordinates_beyond_byte_range_with_shading_and_alpha() {
        let mut source = RgbaImage::new(300, 2);
        source.put_pixel(299, 1, Rgba([12, 34, 56, 255]));
        source.put_pixel(0, 0, Rgba([201, 100, 80, 128]));
        let output = map().project(&source).unwrap();
        assert_eq!(output.dimensions(), (4, 3));
        assert_eq!(output.get_pixel(1, 1).0, [12, 34, 56, 255]);
        assert_eq!(output.get_pixel(2, 1).0, [101, 25, 80, 64]);
        assert_eq!(output.pixels().filter(|pixel| pixel[3] != 0).count(), 2);
    }

    #[test]
    fn transparent_sources_and_coverage_clear_hidden_rgb() {
        let mut map = map();
        map.palette[1][3] = 0;
        let mut source = RgbaImage::from_pixel(300, 2, Rgba([100, 200, 255, 255]));
        source.put_pixel(299, 1, Rgba([50, 60, 70, 0]));
        assert!(map
            .project(&source)
            .unwrap()
            .pixels()
            .all(|pixel| pixel.0 == [0; 4]));
    }

    #[test]
    fn white_source_reproduces_embedded_shading() {
        let map = map();
        let output = map
            .project(&RgbaImage::from_pixel(300, 2, Rgba([255; 4])))
            .unwrap();
        assert_eq!(output.get_pixel(2, 1).0, map.palette[1]);
    }

    #[test]
    fn omitted_palette_defaults_to_white_and_empty_map_is_transparent() {
        let map: ProjectionMap = serde_json::from_str(
            r#"{
            "version":1,"source_size":[1,1],"output_size":[2,2],"rows":[]
        }"#,
        )
        .unwrap();
        assert_eq!(map.palette, vec![[255; 4]]);
        assert!(map
            .project(&RgbaImage::new(1, 1))
            .unwrap()
            .pixels()
            .all(|pixel| pixel.0 == [0; 4]));
    }

    #[test]
    fn rejects_invalid_maps_before_rendering() {
        let cases: Vec<(&str, serde_json::Value)> = vec![
            ("Unsupported", serde_json::json!({"version":2})),
            ("positive", serde_json::json!({"output_size":[0,2]})),
            (
                "64 megapixel",
                serde_json::json!({"output_size":[4294967295u32,4294967295u32]}),
            ),
            ("positive", serde_json::json!({"source_size":[1,0]})),
            ("at least one", serde_json::json!({"palette":[]})),
            (
                "contain pixels",
                serde_json::json!({"rows":[{"at":[0,0],"pixels":[]}]}),
            ),
            (
                "outside output_size",
                serde_json::json!({"rows":[{"at":[3,2],"pixels":[[0,0,0],[0,0,0]]}]}),
            ),
            (
                "outside source_size",
                serde_json::json!({"rows":[{"at":[0,0],"pixels":[[300,0,0]]}]}),
            ),
            (
                "does not exist",
                serde_json::json!({"rows":[{"at":[0,0],"pixels":[[0,0,2]]}]}),
            ),
            (
                "overlaps",
                serde_json::json!({"rows":[{"at":[0,0],"pixels":[[0,0,0]]},{"at":[0,0],"pixels":[[1,0,0]]}]}),
            ),
        ];
        for (message, changes) in cases {
            let mut value = serde_json::json!({"version":1,"source_size":[300,2],"output_size":[4,3],"palette":[[255,255,255,255],[128,128,128,255]],"rows":[]});
            for (key, change) in changes.as_object().unwrap() {
                value[key] = change.clone();
            }
            let map: ProjectionMap = serde_json::from_value(value).unwrap();
            assert!(
                map.validate().unwrap_err().to_string().contains(message),
                "{message}"
            );
        }
    }

    #[test]
    fn rejects_wrong_source_dimensions_and_unknown_fields() {
        assert!(map()
            .project(&RgbaImage::new(2, 2))
            .unwrap_err()
            .to_string()
            .contains("map requires 300x2"));
        assert!(serde_json::from_str::<ProjectionMap>(
            r#"{
            "version":1,"source_size":[1,1],"output_size":[1,1],"rows":[],"shade":"external.png"
        }"#
        )
        .is_err());
    }
}

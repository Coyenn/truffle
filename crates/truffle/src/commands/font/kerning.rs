use std::collections::HashMap;

use super::raster::InkProfile;

#[derive(Debug, Clone)]
pub struct KerningClasses {
    pub left_class: Vec<u8>,
    pub right_class: Vec<u8>,
    pub left_count: u8,
    pub right_count: u8,
    pub matrix: Vec<f32>,
}

fn edge_signature(profile: &InkProfile, side: EdgeSide) -> u32 {
    let mut sum: u64 = 0;
    let mut count: u64 = 0;
    for (i, row) in profile.rows.iter().enumerate() {
        let Some((left, right)) = *row else {
            continue;
        };
        let v = match side {
            EdgeSide::Left => left,
            EdgeSide::Right => right,
        };
        sum += v as u64 + (i as u64 * 131);
        count += 1;
    }
    if count == 0 {
        return 0;
    }
    (sum / count) as u32
}

#[derive(Copy, Clone)]
enum EdgeSide {
    Left,
    Right,
}

fn assign_classes(signatures: &[u32], bucket_bits: u32) -> (Vec<u8>, u8) {
    let shift = 32u32.saturating_sub(bucket_bits.min(16));
    let mut map: HashMap<u32, u8> = HashMap::new();
    let mut classes = Vec::with_capacity(signatures.len());
    let mut next: u8 = 0;
    for &sig in signatures {
        let key = sig >> shift;
        let class = *map.entry(key).or_insert_with(|| {
            let c = next;
            next = next.saturating_add(1);
            c
        });
        classes.push(class);
    }
    (classes, next.max(1))
}

fn pair_kern_px(
    left: &InkProfile,
    right: &InkProfile,
    left_advance: f32,
    target_gap: f32,
) -> Option<f32> {
    let ly0 = left.ymin;
    let ly1 = left.ymin + left.rows.len() as i32;
    let ry0 = right.ymin;
    let ry1 = right.ymin + right.rows.len() as i32;
    let y0 = ly0.max(ry0);
    let y1 = ly1.min(ry1);
    if y1 <= y0 {
        return None;
    }

    let mut min_gap: Option<f32> = None;
    for by in y0..y1 {
        let li = (by - left.ymin) as usize;
        let ri = (by - right.ymin) as usize;
        let Some((_l_left, l_right)) = left.rows.get(li).and_then(|v| *v) else {
            continue;
        };
        let Some((r_left, _r_right)) = right.rows.get(ri).and_then(|v| *v) else {
            continue;
        };
        let gap = left_advance + (right.xmin as f32 + r_left as f32)
            - (left.xmin as f32 + l_right as f32 + 1.0);
        min_gap = Some(min_gap.map_or(gap, |g| g.min(gap)));
    }
    let min_gap = min_gap?;
    Some(-(min_gap - target_gap))
}

/// Build kerning classes from ink profiles and a target pixel gap between glyph ink.
pub fn build_kerning_classes(
    chars: &[char],
    profiles: &HashMap<char, InkProfile>,
    advances: &[f32],
    target_gap_px: u32,
) -> KerningClasses {
    let target_gap = target_gap_px as f32;

    let left_sigs: Vec<u32> = chars
        .iter()
        .map(|ch| {
            profiles
                .get(ch)
                .map(|p| edge_signature(p, EdgeSide::Right))
                .unwrap_or(0)
        })
        .collect();
    let right_sigs: Vec<u32> = chars
        .iter()
        .map(|ch| {
            profiles
                .get(ch)
                .map(|p| edge_signature(p, EdgeSide::Left))
                .unwrap_or(0)
        })
        .collect();

    let (left_class, left_count) = assign_classes(&left_sigs, 4);
    let (right_class, right_count) = assign_classes(&right_sigs, 4);

    let lc = left_count as usize;
    let rc = right_count as usize;
    let mut matrix = vec![0f32; lc * rc];

    let mut samples: HashMap<(u8, u8), Vec<f32>> = HashMap::new();
    for (i, &left_ch) in chars.iter().enumerate() {
        if left_ch == ' ' {
            continue;
        }
        let Some(lp) = profiles.get(&left_ch) else {
            continue;
        };
        let la = advances.get(i).copied().unwrap_or(0.0);
        let l_cls = left_class[i];
        for (j, &right_ch) in chars.iter().enumerate() {
            if right_ch == ' ' {
                continue;
            }
            let Some(rp) = profiles.get(&right_ch) else {
                continue;
            };
            let r_cls = right_class[j];
            if let Some(kern) = pair_kern_px(lp, rp, la, target_gap) {
                if kern.abs() >= 0.01 {
                    samples.entry((l_cls, r_cls)).or_default().push(kern);
                }
            }
        }
    }

    for ((l, r), values) in samples {
        if values.is_empty() {
            continue;
        }
        let sum: f32 = values.iter().sum();
        let avg = sum / values.len() as f32;
        matrix[l as usize * rc + r as usize] = avg;
    }

    KerningClasses {
        left_class,
        right_class,
        left_count,
        right_count,
        matrix,
    }
}

#[allow(dead_code)]
pub fn kerning_lookup(classes: &KerningClasses, left_index: usize, right_index: usize) -> f32 {
    let l = classes.left_class.get(left_index).copied().unwrap_or(0) as usize;
    let r = classes.right_class.get(right_index).copied().unwrap_or(0) as usize;
    let rc = classes.right_count as usize;
    classes
        .matrix
        .get(l * rc + r)
        .copied()
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_profiles_yield_zero_matrix() {
        let classes = build_kerning_classes(&['A', 'V'], &HashMap::new(), &[10.0, 10.0], 1);
        assert_eq!(classes.matrix.iter().all(|&v| v == 0.0), true);
    }
}

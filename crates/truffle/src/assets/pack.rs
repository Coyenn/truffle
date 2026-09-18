use anyhow::bail;

/// An axis-aligned rectangle in integer pixel coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    fn area(&self) -> u64 {
        self.w as u64 * self.h as u64
    }

    fn contains(&self, other: &Rect) -> bool {
        self.x <= other.x
            && self.y <= other.y
            && self.x + self.w >= other.x + other.w
            && self.y + self.h >= other.y + other.h
    }
}

/// A placed rectangle, expressed as the *inner* rect (padding already stripped).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    pub page: u32,
    pub rect: Rect,
}

/// Free space on an existing page, in allocation coordinates (padding included).
#[derive(Debug, Clone, Copy)]
pub struct SeedRect {
    pub page: u32,
    pub rect: Rect,
}

/// Pack `sizes` (inner dimensions) into square pages of `page_size`, leaving a
/// `padding` gutter between sprites.
///
/// Free space supplied via `seed` is filled first (stable incremental packing),
/// before new pages are opened. Returns placements indexed by input order; the
/// internal item order is chosen largest-first for density but never leaks out.
pub fn pack(
    sizes: &[(u32, u32)],
    padding: u32,
    page_size: u32,
    seed: &[SeedRect],
) -> anyhow::Result<Vec<Placed>> {
    if page_size == 0 {
        bail!("atlas size must be > 0");
    }

    let gutter = padding.saturating_mul(2);

    let mut items: Vec<(usize, u32, u32)> = sizes
        .iter()
        .enumerate()
        .map(|(i, &(w, h))| (i, w.saturating_add(gutter), h.saturating_add(gutter)))
        .collect();

    // Pack largest-first for density; ties broken by area then original index.
    items.sort_by(|a, b| {
        let (amax, bmax) = (a.1.max(a.2), b.1.max(b.2));
        bmax.cmp(&amax)
            .then_with(|| (b.1 as u64 * b.2 as u64).cmp(&(a.1 as u64 * a.2 as u64)))
            .then_with(|| a.0.cmp(&b.0))
    });

    let mut free: Vec<(u32, Rect)> = seed.iter().map(|s| (s.page, s.rect)).collect();

    let mut next_page = free.iter().map(|(p, _)| *p).max().map_or(0, |m| m + 1);

    let mut out = vec![
        Placed {
            page: 0,
            rect: Rect {
                x: 0,
                y: 0,
                w: 0,
                h: 0
            }
        };
        sizes.len()
    ];

    for (idx, alloc_w, alloc_h) in items {
        if alloc_w > page_size || alloc_h > page_size {
            bail!(
                "item {}x{} (with padding) exceeds atlas size {}x{}",
                sizes[idx].0,
                sizes[idx].1,
                page_size,
                page_size
            );
        }

        // Degenerate items consume no space but still need a stable slot.
        if alloc_w == 0 || alloc_h == 0 {
            out[idx] = Placed {
                page: 0,
                rect: Rect {
                    x: padding,
                    y: padding,
                    w: sizes[idx].0,
                    h: sizes[idx].1,
                },
            };
            continue;
        }

        let (page, rect) = match best_fit(&free, alloc_w, alloc_h) {
            Some(pos) => {
                let (page, rect) = free.swap_remove(pos);
                free.extend(
                    split(&rect, alloc_w, alloc_h)
                        .into_iter()
                        .map(|r| (page, r)),
                );
                prune(&mut free);
                (page, rect)
            }
            None => {
                let page = next_page;
                next_page += 1;
                let rect = Rect {
                    x: 0,
                    y: 0,
                    w: page_size,
                    h: page_size,
                };
                free.extend(
                    split(&rect, alloc_w, alloc_h)
                        .into_iter()
                        .map(|r| (page, r)),
                );
                prune(&mut free);
                (page, rect)
            }
        };

        out[idx] = Placed {
            page,
            rect: Rect {
                x: rect.x + padding,
                y: rect.y + padding,
                w: sizes[idx].0,
                h: sizes[idx].1,
            },
        };
    }

    Ok(out)
}

/// Compute the free rectangles remaining in a `page_size` square after the
/// (allocation-coordinate) rectangles in `fixed` are subtracted. Used to seed
/// incremental packing from previously placed sprites.
pub fn free_space(page_size: u32, fixed: &[Rect]) -> Vec<Rect> {
    let mut free = vec![Rect {
        x: 0,
        y: 0,
        w: page_size,
        h: page_size,
    }];

    for placed in fixed {
        let mut next = Vec::with_capacity(free.len() + 3);
        for region in &free {
            if region.contains(placed) {
                next.extend(subtract(region, placed));
            } else {
                next.push(*region);
            }
        }
        free = next;
        prune_rects(&mut free);
    }

    free
}

/// Best Short-Side Fit: prefer the free rect leaving the least leftover on its
/// shorter side, then its longer side, then its area. Stable on ties.
fn best_fit(free: &[(u32, Rect)], w: u32, h: u32) -> Option<usize> {
    let mut best: Option<(usize, u32, u32, u64)> = None;

    for (i, (_, r)) in free.iter().enumerate() {
        if r.w < w || r.h < h {
            continue;
        }
        let dw = r.w - w;
        let dh = r.h - h;
        let (short, long) = if dw <= dh { (dw, dh) } else { (dh, dw) };
        let area = r.area();

        let better = match best {
            None => true,
            Some((_, bs, bl, ba)) => {
                short < bs || (short == bs && (long < bl || (long == bl && area < ba)))
            }
        };

        if better {
            best = Some((i, short, long, area));
        }
    }

    best.map(|(i, _, _, _)| i)
}

/// Split a free rect after placing a `w x h` item at its top-left, keeping the
/// right and bottom strips (classic MaxRects decomposition).
fn split(rect: &Rect, w: u32, h: u32) -> Vec<Rect> {
    let mut out = Vec::with_capacity(2);
    if rect.w > w {
        out.push(Rect {
            x: rect.x + w,
            y: rect.y,
            w: rect.w - w,
            h,
        });
    }
    if rect.h > h {
        out.push(Rect {
            x: rect.x,
            y: rect.y + h,
            w: rect.w,
            h: rect.h - h,
        });
    }
    out
}

/// Subtract `placed` (fully contained in `region`) from `region`, yielding up to
/// four disjoint free rectangles.
fn subtract(region: &Rect, placed: &Rect) -> Vec<Rect> {
    let mut out = Vec::with_capacity(4);

    if placed.y > region.y {
        out.push(Rect {
            x: region.x,
            y: region.y,
            w: region.w,
            h: placed.y - region.y,
        });
    }
    let bottom = (region.y + region.h) as i64 - (placed.y + placed.h) as i64;
    if bottom > 0 {
        out.push(Rect {
            x: region.x,
            y: placed.y + placed.h,
            w: region.w,
            h: bottom as u32,
        });
    }
    if placed.x > region.x {
        out.push(Rect {
            x: region.x,
            y: placed.y,
            w: placed.x - region.x,
            h: placed.h,
        });
    }
    let right = (region.x + region.w) as i64 - (placed.x + placed.w) as i64;
    if right > 0 {
        out.push(Rect {
            x: placed.x + placed.w,
            y: placed.y,
            w: right as u32,
            h: placed.h,
        });
    }

    out
}

fn prune(free: &mut Vec<(u32, Rect)>) {
    let mut i = 0;
    while i < free.len() {
        let (page, rect) = free[i];
        let contained = free
            .iter()
            .enumerate()
            .any(|(j, (p, other))| i != j && *p == page && other.contains(&rect));
        if contained {
            free.swap_remove(i);
        } else {
            i += 1;
        }
    }
}

fn prune_rects(free: &mut Vec<Rect>) {
    let mut i = 0;
    while i < free.len() {
        let rect = free[i];
        let contained = free
            .iter()
            .enumerate()
            .any(|(j, other)| i != j && other.contains(&rect));
        if contained {
            free.swap_remove(i);
        } else {
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_item_places_at_padding_inset() {
        let placed = pack(&[(8, 16)], 1, 64, &[]).unwrap();
        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].page, 0);
        assert_eq!(
            placed[0].rect,
            Rect {
                x: 1,
                y: 1,
                w: 8,
                h: 16
            }
        );
    }

    #[test]
    fn oversized_item_errors() {
        assert!(pack(&[(65, 10)], 1, 64, &[]).is_err());
    }

    #[test]
    fn fills_seed_space_before_opening_new_pages() {
        // Page 0 has a 8x8 gutter of free space at the top-left (after a fixed
        // sprite), and page 1 exists. The 4x4 item should land in page 0.
        let seed = [SeedRect {
            page: 0,
            rect: Rect {
                x: 8,
                y: 0,
                w: 8,
                h: 8,
            },
        }];
        let placed = pack(&[(4, 4)], 0, 16, &seed).unwrap();
        assert_eq!(placed[0].page, 0);
        assert_eq!(
            placed[0].rect,
            Rect {
                x: 8,
                y: 0,
                w: 4,
                h: 4
            }
        );
    }

    #[test]
    fn opens_new_page_after_max_seed_page() {
        let seed = [SeedRect {
            page: 3,
            rect: Rect {
                x: 0,
                y: 0,
                w: 1,
                h: 1,
            },
        }];
        let placed = pack(&[(8, 8)], 0, 8, &seed).unwrap();
        assert_eq!(placed[0].page, 4);
    }

    #[test]
    fn deterministic_output() {
        let sizes: Vec<(u32, u32)> = (0..32).map(|i| (1 + (i % 7), 1 + (i % 5))).collect();
        let a = pack(&sizes, 2, 32, &[]).unwrap();
        let b = pack(&sizes, 2, 32, &[]).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn free_space_subtracts_fixed_rects() {
        let free = free_space(
            16,
            &[Rect {
                x: 4,
                y: 4,
                w: 8,
                h: 8,
            }],
        );
        // A 4x4 item fits in the corner regions left by the subtraction.
        assert!(free.iter().any(|r| r.contains(&Rect {
            x: 0,
            y: 0,
            w: 4,
            h: 4
        })));
        assert!(free.iter().any(|r| r.contains(&Rect {
            x: 12,
            y: 12,
            w: 4,
            h: 4
        })));
    }
}

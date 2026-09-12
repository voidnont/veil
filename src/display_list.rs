use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};

use crate::engine::{RenderBlock, TextRun};
use crate::style::{ComputedStyle, LayoutMode};

/// Only virtualize long vertical sequences. Small groups are cheaper and safer to paint normally.
pub const VIRTUALIZE_MIN_ITEMS: usize = 24;
/// Keep a generous band around the visible clip so fast wheel/touchpad scrolling never reveals blanks.
pub const VIRTUALIZE_OVERSCAN: f32 = 900.0;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DisplayDiff {
    pub changed: usize,
    pub reused: usize,
    pub start: usize,
    pub old_remove_count: usize,
    pub new_end: usize,
}

impl DisplayDiff {
    pub fn is_empty(self) -> bool {
        self.changed == 0
    }
}

#[derive(Debug, Clone)]
struct RetainedPaintItem {
    fingerprint: u64,
    height: f32,
    measured: bool,
}

/// Browser-side retained paint metadata. The engine owns DOM/style retention; this list owns
/// stable paint fingerprints and measured heights so egui can skip off-screen work without
/// losing scroll geometry.
#[derive(Debug, Clone, Default)]
pub struct RetainedDisplayList {
    items: Vec<RetainedPaintItem>,
    revision: u64,
}

impl RetainedDisplayList {
    pub fn reconcile(&mut self, blocks: &[RenderBlock], width: f32) -> DisplayDiff {
        let old_fingerprints: Vec<u64> = self.items.iter().map(|item| item.fingerprint).collect();
        let new_fingerprints = block_fingerprints(blocks);
        let diff = diff_fingerprints(&old_fingerprints, &new_fingerprints);

        // Reuse measured geometry even when an insertion shifts an unchanged block's index.
        let mut old_heights: HashMap<u64, VecDeque<(f32, bool)>> = HashMap::new();
        for item in self.items.drain(..) {
            old_heights
                .entry(item.fingerprint)
                .or_default()
                .push_back((item.height, item.measured));
        }

        self.items = blocks
            .iter()
            .zip(new_fingerprints)
            .map(|(block, fingerprint)| {
                let reused = old_heights
                    .get_mut(&fingerprint)
                    .and_then(VecDeque::pop_front);
                let (height, measured) =
                    reused.unwrap_or_else(|| (estimate_block_height(block, width).max(1.0), false));
                RetainedPaintItem {
                    fingerprint,
                    height,
                    measured,
                }
            })
            .collect();

        if !diff.is_empty() {
            self.revision = self.revision.saturating_add(1);
        }
        diff
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn height_for(&self, index: usize, block: &RenderBlock, width: f32) -> f32 {
        self.items
            .get(index)
            .map(|item| item.height)
            .unwrap_or_else(|| estimate_block_height(block, width))
            .max(1.0)
    }

    pub fn observe_height(&mut self, index: usize, height: f32) {
        let Some(item) = self.items.get_mut(index) else {
            return;
        };
        let height = height.clamp(1.0, 12_000.0);
        item.height = if item.measured {
            // Smooth tiny font/layout jitter so the virtual scroll geometry stays stable.
            item.height * 0.75 + height * 0.25
        } else {
            height
        };
        item.measured = true;
    }
}

pub fn block_fingerprint(block: &RenderBlock) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    if let Ok(bytes) = serde_json::to_vec(block) {
        bytes.hash(&mut hasher);
    }
    hasher.finish()
}

pub fn block_fingerprints(blocks: &[RenderBlock]) -> Vec<u64> {
    blocks.iter().map(block_fingerprint).collect()
}

/// Find a single contiguous replacement range by preserving the common prefix and suffix.
/// This is the same broad retained-list idea Gecko uses: unchanged display items stay retained,
/// and only the damaged middle span needs to cross the renderer IPC boundary.
pub fn diff_fingerprints(old: &[u64], new: &[u64]) -> DisplayDiff {
    let mut prefix = 0usize;
    let common = old.len().min(new.len());
    while prefix < common && old[prefix] == new[prefix] {
        prefix += 1;
    }

    let mut suffix = 0usize;
    while suffix < old.len().saturating_sub(prefix)
        && suffix < new.len().saturating_sub(prefix)
        && old[old.len() - 1 - suffix] == new[new.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let old_remove_count = old.len().saturating_sub(prefix + suffix);
    let new_end = new.len().saturating_sub(suffix);
    let inserted = new_end.saturating_sub(prefix);
    let changed = old_remove_count.max(inserted);
    DisplayDiff {
        changed,
        reused: prefix + suffix,
        start: prefix,
        old_remove_count,
        new_end,
    }
}

pub fn estimate_block_height(block: &RenderBlock, available_width: f32) -> f32 {
    let width = available_width.max(220.0);
    let raw = match block {
        RenderBlock::Heading { runs, style, .. } | RenderBlock::Paragraph { runs, style } => {
            estimate_text_height(runs, style, width)
        }
        RenderBlock::Container { children, style } => {
            let child_width = style
                .width
                .or(style.max_width)
                .unwrap_or(width)
                .min(width)
                .max(160.0);
            let body = match style.layout {
                LayoutMode::Block | LayoutMode::FlexColumn => {
                    let sum: f32 = children
                        .iter()
                        .map(|child| estimate_block_height(child, child_width))
                        .sum();
                    sum + style.gap.max(0.0) * children.len().saturating_sub(1) as f32
                }
                LayoutMode::FlexRow => {
                    if children.is_empty() {
                        1.0
                    } else if style.flex_wrap {
                        let columns = ((child_width / 220.0).floor() as usize).max(1);
                        let rows = children.len().div_ceil(columns);
                        let row_height = children
                            .iter()
                            .map(|child| estimate_block_height(child, child_width / columns as f32))
                            .fold(1.0_f32, f32::max);
                        rows as f32 * row_height
                            + rows.saturating_sub(1) as f32 * style.gap.max(0.0)
                    } else {
                        children
                            .iter()
                            .map(|child| {
                                estimate_block_height(child, child_width / children.len() as f32)
                            })
                            .fold(1.0_f32, f32::max)
                    }
                }
                LayoutMode::Grid => {
                    let columns = style
                        .grid_columns
                        .max(1)
                        .min(((child_width / 180.0).floor() as usize).max(1));
                    let rows = children.len().div_ceil(columns);
                    let row_height = children
                        .iter()
                        .map(|child| estimate_block_height(child, child_width / columns as f32))
                        .fold(1.0_f32, f32::max);
                    rows as f32 * row_height + rows.saturating_sub(1) as f32 * style.gap.max(0.0)
                }
            };
            body + box_vertical(style)
        }
        RenderBlock::Image {
            width,
            height,
            style,
            ..
        } => {
            height
                .or(style.height)
                .unwrap_or_else(|| {
                    width.or(style.width).unwrap_or(360.0).min(width_value(
                        width,
                        style,
                        available_width,
                    )) * 0.62
                })
                .clamp(24.0, 1800.0)
                + box_vertical(style)
        }
        RenderBlock::Canvas { height, style, .. } => {
            height.clamp(1.0, 1400.0) + box_vertical(style)
        }
        RenderBlock::Media {
            height,
            width,
            style,
            ..
        } => {
            height
                .or(style.height)
                .unwrap_or_else(|| width.or(style.width).unwrap_or(420.0) * 0.56 + 86.0)
                .clamp(60.0, 1800.0)
                + box_vertical(style)
        }
        RenderBlock::Form {
            controls, style, ..
        } => controls.len().max(1) as f32 * 46.0 + 18.0 + box_vertical(style),
        RenderBlock::Rule { style } => 18.0 + box_vertical(style),
        RenderBlock::Code { text, style } => {
            let lines = text.lines().count().max(1) as f32;
            lines.min(80.0) * (style.font_size.max(12.0) * 1.35) + box_vertical(style)
        }
        RenderBlock::Notice(text) => 42.0 + (text.len() as f32 / 90.0).floor() * 20.0,
    };
    raw.clamp(1.0, 12_000.0)
}

fn width_value(width: &Option<f32>, style: &ComputedStyle, available_width: f32) -> f32 {
    width
        .or(style.width)
        .unwrap_or(available_width)
        .min(available_width)
        .max(1.0)
}

fn estimate_text_height(runs: &[TextRun], style: &ComputedStyle, available_width: f32) -> f32 {
    let chars: usize = runs.iter().map(|run| run.text.chars().count()).sum();
    let font = runs
        .iter()
        .map(|run| run.size)
        .fold(style.font_size.max(12.0), f32::max)
        .max(10.0);
    let chars_per_line = (available_width / (font * 0.54)).max(8.0);
    let lines = ((chars.max(1) as f32 / chars_per_line).ceil()).clamp(1.0, 120.0);
    lines * font * 1.42 + box_vertical(style)
}

fn box_vertical(style: &ComputedStyle) -> f32 {
    style.margin.top.max(0.0)
        + style.margin.bottom.max(0.0)
        + style.padding.top.max(0.0)
        + style.padding.bottom.max(0.0)
        + style.border_width.max(0.0) * 2.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::ComputedStyle;

    fn paragraph(text: &str) -> RenderBlock {
        let style = ComputedStyle::default();
        RenderBlock::Paragraph {
            runs: vec![TextRun {
                text: text.to_owned(),
                href: None,
                size: 16.0,
                bold: false,
                italic: false,
                muted: false,
                color: None,
                font_family: Default::default(),
                font_name: None,
            }],
            style,
        }
    }

    #[test]
    fn diff_keeps_common_prefix_and_suffix() {
        let old = vec![1, 2, 3, 4];
        let new = vec![1, 9, 3, 4];
        let diff = diff_fingerprints(&old, &new);
        assert_eq!(diff.start, 1);
        assert_eq!(diff.old_remove_count, 1);
        assert_eq!(diff.new_end, 2);
        assert_eq!(diff.reused, 3);
    }

    #[test]
    fn retained_list_reuses_measured_height() {
        let mut list = RetainedDisplayList::default();
        let blocks = vec![paragraph("hello"), paragraph("world")];
        list.reconcile(&blocks, 800.0);
        list.observe_height(1, 123.0);
        let diff = list.reconcile(&blocks, 800.0);
        assert!(diff.is_empty());
        assert!((list.height_for(1, &blocks[1], 800.0) - 123.0).abs() < 0.01);
    }

    #[test]
    fn text_estimate_grows_with_content() {
        let short = paragraph("small");
        let long = paragraph(&"long content ".repeat(80));
        assert!(estimate_block_height(&long, 500.0) > estimate_block_height(&short, 500.0));
    }
}

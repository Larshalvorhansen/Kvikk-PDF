use crate::model::{
    DocumentLayout, LayoutRow, PageCrop, PageMetric, PlacedPage, ViewMode, BASE_PX_PER_POINT,
    GRID_GAP, GRID_MARGIN, PAGE_GAP, PAGE_MARGIN,
};

pub fn build_layout(
    pages: &[PageMetric],
    crops: &[Option<PageCrop>],
    crop_enabled: bool,
    mode: ViewMode,
    manual_zoom: f32,
    viewport_width: f32,
    viewport_height: f32,
) -> DocumentLayout {
    let viewport_width = viewport_width.max(1.0);
    let viewport_height = viewport_height.max(1.0);
    let usable_w = (viewport_width - PAGE_MARGIN * 2.0).max(64.0);
    let usable_h = (viewport_height - PAGE_MARGIN * 2.0).max(64.0);

    let mut rows = Vec::with_capacity(pages.len());
    let mut y = PAGE_MARGIN;
    let mut content_width = viewport_width;
    let mut pages_per_group = 1usize;

    let grid_spec = match mode {
        ViewMode::Spread => Some((2, 1)),
        ViewMode::Overview => Some(overview_grid_spec(pages, viewport_width, viewport_height)),
        _ => mode.requested_grid_rows().map(|requested_rows| {
            (
                columns_for_fixed_rows(
                    pages,
                    requested_rows,
                    viewport_width,
                    viewport_height,
                ),
                requested_rows,
            )
        }),
    };

    if let Some((cols, grid_rows)) = grid_spec {
        let cols = cols.max(1);
        let grid_rows = grid_rows.max(1);
        let grid_usable_w = (viewport_width - GRID_MARGIN * 2.0).max(16.0);
        let grid_usable_h = (viewport_height - GRID_MARGIN * 2.0).max(16.0);
        let min_cell = if mode == ViewMode::Overview { 2.0 } else { 16.0 };
        let cell_w = ((grid_usable_w - GRID_GAP * cols.saturating_sub(1) as f32) / cols as f32)
            .max(min_cell);
        let cell_h = ((grid_usable_h - GRID_GAP * grid_rows.saturating_sub(1) as f32)
            / grid_rows as f32)
            .max(min_cell);
        content_width = content_width.max(
            GRID_MARGIN * 2.0 + cols as f32 * cell_w + cols.saturating_sub(1) as f32 * GRID_GAP,
        );
        pages_per_group = if mode == ViewMode::Overview {
            pages.len().max(1)
        } else {
            cols * grid_rows
        };
        let mut group_start = 0usize;
        y = GRID_MARGIN;

        while group_start < pages.len() {
            let group_top = y;

            for row_index in 0..grid_rows {
                let row_y = group_top + row_index as f32 * (cell_h + GRID_GAP);
                let mut placed = Vec::with_capacity(cols);

                for col_index in 0..cols {
                    let page = group_start + row_index * cols + col_index;
                    let Some(metric) = pages.get(page).copied() else { break };
                    let crop = crop_for_page(crops, crop_enabled, page);
                    let raw_w = metric.width_pt * crop.width() * BASE_PX_PER_POINT;
                    let raw_h = metric.height_pt * crop.height() * BASE_PX_PER_POINT;
                    let scale = (cell_w / raw_w).min(cell_h / raw_h).clamp(0.001, 5.0);
                    let w = raw_w * scale;
                    let h = raw_h * scale;
                    let cell_x = GRID_MARGIN + col_index as f32 * (cell_w + GRID_GAP);
                    let x = cell_x + (cell_w - w) * 0.5;
                    let page_y = row_y + (cell_h - h) * 0.5;

                    placed.push(PlacedPage {
                        page,
                        crop,
                        x,
                        y: page_y,
                        w,
                        h,
                        scale,
                    });
                }

                if !placed.is_empty() {
                    rows.push(LayoutRow {
                        y: row_y,
                        h: cell_h,
                        pages: placed,
                    });
                }
            }

            // Each multi-page group occupies one viewport. Modes 4–8 choose
            // their column count dynamically but retain the requested row count.
            // Overview puts the entire document in one dynamically chosen grid.
            y += viewport_height;
            group_start += pages_per_group;
        }
    } else {
        for (page, metric) in pages.iter().copied().enumerate() {
            let crop = crop_for_page(crops, crop_enabled, page);
            let cropped_w_pt = metric.width_pt * crop.width();
            let cropped_h_pt = metric.height_pt * crop.height();
            let scale = match mode {
                ViewMode::Manual => manual_zoom.clamp(0.10, 20.0),
                ViewMode::FitWidth => {
                    (usable_w / (cropped_w_pt * BASE_PX_PER_POINT)).clamp(0.02, 5.0)
                }
                ViewMode::FitHeight => {
                    (usable_h / (cropped_h_pt * BASE_PX_PER_POINT)).clamp(0.02, 5.0)
                }
                _ => unreachable!("grid modes handled above"),
            };
            let w = cropped_w_pt * BASE_PX_PER_POINT * scale;
            let h = cropped_h_pt * BASE_PX_PER_POINT * scale;
            let x = ((viewport_width - w) * 0.5).max(PAGE_MARGIN);
            rows.push(LayoutRow {
                y,
                h,
                pages: vec![PlacedPage { page, crop, x, y, w, h, scale }],
            });
            content_width = content_width.max(x * 2.0 + w);
            y += h + PAGE_GAP;
        }
    }

    let trailing_margin = if mode.is_grid() { GRID_MARGIN } else { PAGE_MARGIN };
    let content_height = rows
        .last()
        .map(|row| row.y + row.h + trailing_margin)
        .unwrap_or(viewport_height);

    DocumentLayout {
        rows,
        content_width,
        content_height,
        pages_per_group,
    }
}

fn crop_for_page(crops: &[Option<PageCrop>], crop_enabled: bool, page: usize) -> PageCrop {
    if crop_enabled {
        crops.get(page).and_then(|crop| *crop).unwrap_or(PageCrop::FULL)
    } else {
        PageCrop::FULL
    }
}

fn average_page_ratio(pages: &[PageMetric]) -> f32 {
    if pages.is_empty() {
        return 0.707;
    }
    let sum: f32 = pages
        .iter()
        .map(|page| (page.width_pt / page.height_pt.max(1.0)).clamp(0.1, 10.0))
        .sum();
    (sum / pages.len() as f32).clamp(0.1, 10.0)
}

/// Given a requested number of rows, choose enough columns to use the current
/// window width naturally. This deliberately uses the PDF's original page boxes
/// rather than asynchronous crop results, so turning crop on cannot reshuffle groups.
fn columns_for_fixed_rows(
    pages: &[PageMetric],
    requested_rows: usize,
    viewport_width: f32,
    viewport_height: f32,
) -> usize {
    let rows = requested_rows.max(1);
    let count = pages.len().max(1);
    let usable_w = (viewport_width - GRID_MARGIN * 2.0).max(16.0);
    let usable_h = (viewport_height - GRID_MARGIN * 2.0).max(16.0);
    let cell_h = ((usable_h - GRID_GAP * rows.saturating_sub(1) as f32) / rows as f32).max(1.0);
    let ideal_page_w = cell_h * average_page_ratio(pages);
    let columns_from_window = ((usable_w + GRID_GAP) / (ideal_page_w + GRID_GAP))
        .round()
        .max(1.0) as usize;

    columns_from_window.min(count.div_ceil(rows).max(1)).max(1)
}

/// Choose a grid that fits the entire document into one viewport while maximizing
/// the approximate displayed page area. Crop results do not alter this grid, which
/// keeps mode 9 stable while crop analysis arrives in the background.
fn overview_grid_spec(pages: &[PageMetric], viewport_width: f32, viewport_height: f32) -> (usize, usize) {
    let count = pages.len().max(1);
    if count == 1 {
        return (1, 1);
    }

    let average_ratio = average_page_ratio(pages);
    let usable_w = (viewport_width - GRID_MARGIN * 2.0).max(16.0);
    let usable_h = (viewport_height - GRID_MARGIN * 2.0).max(16.0);
    let mut best = (1usize, count);
    let mut best_score = -1.0f32;

    for cols in 1..=count {
        let rows = count.div_ceil(cols);
        let cell_w = ((usable_w - GRID_GAP * cols.saturating_sub(1) as f32) / cols as f32).max(0.1);
        let cell_h = ((usable_h - GRID_GAP * rows.saturating_sub(1) as f32) / rows as f32).max(0.1);
        let scale = (cell_w / average_ratio).min(cell_h);
        let page_area = average_ratio * scale * scale;
        let occupancy = count as f32 / (cols * rows) as f32;
        let score = page_area * occupancy;

        if score > best_score {
            best_score = score;
            best = (cols, rows);
        }
    }

    best
}

#[cfg(test)]
mod tests {
    use super::{columns_for_fixed_rows, overview_grid_spec};
    use crate::model::PageMetric;

    fn portrait_pages(count: usize) -> Vec<PageMetric> {
        vec![PageMetric { width_pt: 595.0, height_pt: 842.0 }; count]
    }

    #[test]
    fn two_row_mode_chooses_columns_from_window_shape() {
        let pages = portrait_pages(100);
        let cols = columns_for_fixed_rows(&pages, 2, 1600.0, 1000.0);
        assert!((4..=6).contains(&cols));
    }

    #[test]
    fn overview_uses_five_by_two_for_ten_portrait_pages_on_a_wide_window() {
        let pages = portrait_pages(10);
        assert_eq!(overview_grid_spec(&pages, 1600.0, 1000.0), (5, 2));
    }

    #[test]
    fn overview_keeps_three_hundred_pages_close_to_one_screen() {
        let pages = portrait_pages(300);
        let (cols, rows) = overview_grid_spec(&pages, 1600.0, 1000.0);
        assert!(cols * rows >= 300);
        assert!(rows <= 14);
        assert!(cols >= 22);
    }
}

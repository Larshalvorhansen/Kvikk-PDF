use crate::model::{
    DocumentLayout, LayoutRow, PageMetric, PlacedPage, ViewMode, BASE_PX_PER_POINT, GRID_GAP,
    GRID_MARGIN, PAGE_GAP, PAGE_MARGIN,
};

pub fn build_layout(
    pages: &[PageMetric],
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

    if let Some((cols, grid_rows)) = mode.grid_spec() {
        let grid_usable_w = (viewport_width - GRID_MARGIN * 2.0).max(64.0);
        let grid_usable_h = (viewport_height - GRID_MARGIN * 2.0).max(64.0);
        let cell_w = ((grid_usable_w - GRID_GAP * cols.saturating_sub(1) as f32) / cols as f32).max(24.0);
        let cell_h = ((grid_usable_h - GRID_GAP * grid_rows.saturating_sub(1) as f32) / grid_rows as f32).max(24.0);
        let pages_per_group = cols * grid_rows;
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

                    let raw_w = metric.width_pt * BASE_PX_PER_POINT;
                    let raw_h = metric.height_pt * BASE_PX_PER_POINT;
                    let scale = (cell_w / raw_w)
                        .min(cell_h / raw_h)
                        .clamp(0.02, 5.0);
                    let w = raw_w * scale;
                    let h = raw_h * scale;
                    let cell_x = GRID_MARGIN + col_index as f32 * (cell_w + GRID_GAP);
                    let x = cell_x + (cell_w - w) * 0.5;
                    let page_y = row_y + (cell_h - h) * 0.5;

                    placed.push(PlacedPage {
                        page,
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

            // Every multi-page group occupies exactly one viewport worth of document
            // height. This makes Space/Shift+Space land on stable 2/3/6/10/21/40/160-page
            // boundaries instead of depending on individual page aspect ratios.
            y += viewport_height;
            group_start += pages_per_group;
        }
    } else {
        for (page, metric) in pages.iter().copied().enumerate() {
            let scale = match mode {
                ViewMode::Manual => manual_zoom.clamp(0.10, 20.0),
                ViewMode::FitWidth => {
                    (usable_w / (metric.width_pt * BASE_PX_PER_POINT)).clamp(0.02, 5.0)
                }
                ViewMode::FitHeight => {
                    (usable_h / (metric.height_pt * BASE_PX_PER_POINT)).clamp(0.02, 5.0)
                }
                _ => unreachable!("grid modes handled above"),
            };
            let w = metric.width_pt * BASE_PX_PER_POINT * scale;
            let h = metric.height_pt * BASE_PX_PER_POINT * scale;
            let x = ((viewport_width - w) * 0.5).max(PAGE_MARGIN);
            rows.push(LayoutRow {
                y,
                h,
                pages: vec![PlacedPage { page, x, y, w, h, scale }],
            });
            content_width = content_width.max(x * 2.0 + w);
            y += h + PAGE_GAP;
        }
    }

    let trailing_margin = if mode.grid_spec().is_some() { GRID_MARGIN } else { PAGE_MARGIN };
    let content_height = rows
        .last()
        .map(|row| row.y + row.h + trailing_margin)
        .unwrap_or(viewport_height);

    DocumentLayout {
        rows,
        content_width,
        content_height,
    }
}

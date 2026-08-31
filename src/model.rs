pub const SPEED_LEVELS: &[f32] = &[
    0.5, 1.0, 1.5, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 11.0, 14.0, 15.0,
    17.0, 20.0, 23.0, 27.0, 31.0, 41.0, 53.0, 67.0, 83.0, 91.0, 120.0, 150.0,
    190.0, 230.0, 270.0, 320.0, 380.0, 450.0, 550.0, 650.0, 800.0, 1000.0, 1250.0,
];

pub const DEFAULT_SPEED: f32 = 15.0;
pub const PAGE_TURN_REFERENCE_DISTANCE: f32 = 900.0;
pub const PAGE_TURN_MIN_SECONDS: f32 = 0.5;
pub const PAGE_TURN_MAX_SECONDS: f32 = 1800.0;
pub const MIN_NATIVE_TEXT_CHARS: usize = 48;
pub const BASE_PX_PER_POINT: f32 = 96.0 / 72.0;
pub const PAGE_GAP: f32 = 4.0;
pub const PAGE_MARGIN: f32 = 26.0;
pub const GRID_GAP: f32 = 2.0;
pub const GRID_MARGIN: f32 = 2.0;
pub const MAX_MANUAL_ZOOM: f32 = 20.0;
pub const MAX_RENDER_WIDTH: u32 = 8192;
pub const MIN_RENDER_WIDTH: u32 = 96;
pub const BITMAP_CACHE_BUDGET: usize = 320 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    Manual,
    FitWidth,
    FitHeight,
    Spread,
    Rows2,
    Rows3,
    Rows4,
    Rows5,
    Rows7,
    Overview,
}

impl ViewMode {
    /// Modes 4–8 specify the number of page rows and let the layout choose
    /// the column count from the current window shape and average page shape.
    pub fn requested_grid_rows(self) -> Option<usize> {
        match self {
            Self::Rows2 => Some(2),
            Self::Rows3 => Some(3),
            Self::Rows4 => Some(4),
            Self::Rows5 => Some(5),
            Self::Rows7 => Some(7),
            _ => None,
        }
    }

    pub fn is_grid(self) -> bool {
        matches!(
            self,
            Self::Spread
                | Self::Rows2
                | Self::Rows3
                | Self::Rows4
                | Self::Rows5
                | Self::Rows7
                | Self::Overview
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageCrop {
    /// Normalized left edge in rendered-page coordinates (0..1).
    pub left: f32,
    /// Normalized top edge in rendered-page coordinates (0..1).
    pub top: f32,
    /// Normalized right edge in rendered-page coordinates (0..1).
    pub right: f32,
    /// Normalized bottom edge in rendered-page coordinates (0..1).
    pub bottom: f32,
}

impl Default for PageCrop {
    fn default() -> Self {
        Self::FULL
    }
}

impl PageCrop {
    pub const FULL: Self = Self { left: 0.0, top: 0.0, right: 1.0, bottom: 1.0 };

    pub fn width(self) -> f32 {
        (self.right - self.left).clamp(0.01, 1.0)
    }

    pub fn height(self) -> f32 {
        (self.bottom - self.top).clamp(0.01, 1.0)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PageMetric {
    pub width_pt: f32,
    pub height_pt: f32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PdfBounds {
    pub left: f32,
    pub bottom: f32,
    pub right: f32,
    pub top: f32,
}

#[derive(Clone, Debug)]
pub struct Glyph {
    pub ch: char,
    pub bounds: Option<PdfBounds>,
}

#[derive(Clone, Debug)]
pub struct PageTextData {
    pub page: usize,
    pub text: String,
    pub glyphs: Vec<Glyph>,
}

#[derive(Clone, Debug)]
pub struct SearchHit {
    pub page: usize,
    pub snippet: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionPoint {
    pub page: usize,
    pub glyph: usize,
}

#[derive(Clone, Debug)]
pub enum LinkTarget {
    Page(usize),
    Url(String),
}

#[derive(Clone, Debug)]
pub struct DocumentInfo {
    pub title: String,
    pub pages: Vec<PageMetric>,
}

#[derive(Clone, Copy, Debug)]
pub struct PlacedPage {
    pub page: usize,
    pub crop: PageCrop,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub scale: f32,
}

#[derive(Clone, Debug, Default)]
pub struct LayoutRow {
    pub y: f32,
    pub h: f32,
    pub pages: Vec<PlacedPage>,
}

#[derive(Clone, Debug, Default)]
pub struct DocumentLayout {
    pub rows: Vec<LayoutRow>,
    pub content_width: f32,
    pub content_height: f32,
    /// Number of pages in one Space/Shift+Space navigation group for the
    /// current layout. This is calculated by layout.rs because modes 4–8
    /// have a dynamic number of columns.
    pub pages_per_group: usize,
}

impl DocumentLayout {
    pub fn first_visible_row(&self, y: f32) -> usize {
        if self.rows.is_empty() {
            return 0;
        }
        self.rows
            .partition_point(|row| row.y + row.h < y)
            .min(self.rows.len().saturating_sub(1))
    }

    pub fn row_for_page(&self, page: usize) -> Option<&LayoutRow> {
        self.rows.iter().find(|row| row.pages.iter().any(|p| p.page == page))
    }

    pub fn placed_page(&self, page: usize) -> Option<PlacedPage> {
        self.rows
            .iter()
            .flat_map(|row| row.pages.iter())
            .copied()
            .find(|p| p.page == page)
    }

    pub fn canonical_page(&self, page: usize) -> usize {
        let step = self.pages_per_group.max(1);
        if step > 1 { (page / step) * step } else { page }
    }

    pub fn canonical_page_at_y(&self, y: f32) -> usize {
        if self.rows.is_empty() {
            return 0;
        }
        let idx = self.first_visible_row(y);
        let row = &self.rows[idx];
        let page = row.pages.first().map(|p| p.page).unwrap_or(0);
        self.canonical_page(page)
    }
}

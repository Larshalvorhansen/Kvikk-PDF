use crate::{
    backend::{BackendCommand, BackendEvent, PdfBackend},
    layout::build_layout,
    model::{
        DocumentInfo, DocumentLayout, LinkTarget, PageTextData, PdfBounds, PlacedPage, SearchHit,
        SelectionPoint, ViewMode, BITMAP_CACHE_BUDGET, DEFAULT_SPEED, MAX_MANUAL_ZOOM,
        MAX_RENDER_WIDTH, MIN_NATIVE_TEXT_CHARS, MIN_RENDER_WIDTH, SPEED_LEVELS,
    },
};
use eframe::egui::{self, Color32, ColorImage, Key, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions, Vec2};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
};

const COMMANDS: &[(&str, &str)] = &[
    ("Ctrl/⌘ F or /", "Search inside the PDF"),
    ("Enter / Shift+Enter", "Next / previous search result"),
    ("Esc", "Close search"),
    ("g#", "Go to page #, e.g. g45"),
    ("Hold ?", "Show this command list"),
    ("S", "Show / hide menu"),
    ("I", "Invert PDF"),
    ("P / K", "Play / pause pacer"),
    ("J", "Slower pacer"),
    ("L", "Faster pacer"),
    ("Space", "Next page / page group"),
    ("Shift + Space", "Previous page / page group"),
    ("+", "Zoom in"),
    ("−", "Zoom out"),
    ("O", "Reset zoom to 100%"),
    ("Pinch / Ctrl-scroll", "Zoom around the pointer"),
    ("1", "Fit page width"),
    ("2", "Fit page height"),
    ("3", "2 pages (2×1)"),
    ("4", "3 pages (3×1)"),
    ("5", "6 pages (3×2)"),
    ("6", "10 pages (5×2)"),
    ("7", "21 pages (7×3)"),
    ("F", "Toggle fullscreen"),
    ("Ctrl/⌘ C", "Copy selected PDF text"),
];

#[derive(Clone, Copy)]
struct ScrollAnchor {
    page: usize,
    offset_y: f32,
    viewport_y: f32,
    offset_x: f32,
    viewport_x: f32,
}

struct BitmapEntry {
    page: usize,
    requested_width: u32,
    width: usize,
    height: usize,
    rgba: Vec<u8>,
    normal: Option<TextureHandle>,
    inverted: Option<TextureHandle>,
    last_used: u64,
}

impl BitmapEntry {
    fn byte_size(&self) -> usize {
        self.rgba.len()
    }
}

pub struct KvikkApp {
    backend: PdfBackend,
    doc_id: u64,
    document: Option<DocumentInfo>,
    view_mode: ViewMode,
    manual_zoom: f32,
    invert: bool,
    fullscreen: bool,
    show_menu: bool,
    is_playing: bool,
    speed_index: usize,
    scroll_x: f32,
    scroll_y: f32,
    layout: DocumentLayout,
    layout_dirty: bool,
    pending_anchor: Option<ScrollAnchor>,
    pending_goto: Option<usize>,
    current_page: usize,
    frame_no: u64,

    bitmaps: HashMap<(usize, u32), BitmapEntry>,
    render_in_flight: HashSet<(usize, u32)>,
    render_failed: HashSet<(usize, u32)>,
    render_generation: u64,
    render_debounce_frames: u8,

    native_text: Vec<Option<PageTextData>>,
    search_text: Vec<Option<String>>,
    text_requested: HashSet<usize>,
    ocr_queued: VecDeque<usize>,
    ocr_queued_set: HashSet<usize>,
    ocr_in_flight: HashSet<usize>,
    ocr_done: HashSet<usize>,
    ocr_available: bool,

    search_open: bool,
    search_focus_requested: bool,
    search_query: String,
    search_results: Vec<SearchHit>,
    search_result_index: Option<usize>,
    search_index_cursor: usize,

    selection_anchor: Option<SelectionPoint>,
    selection_focus: Option<SelectionPoint>,

    goto_active: bool,
    goto_buffer: String,
    goto_deadline: Option<f64>,
    question_down: bool,
    question_visible_until: f64,
    show_about: bool,
    logo_texture: Option<TextureHandle>,

    status: String,
    last_viewport: Vec2,
    startup_path: Option<PathBuf>,
}

impl KvikkApp {
    pub fn new(cc: &eframe::CreationContext<'_>, startup_path: Option<PathBuf>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::BLACK;
        visuals.window_fill = Color32::from_gray(18);
        cc.egui_ctx.set_visuals(visuals);

        let speed_index = SPEED_LEVELS
            .iter()
            .position(|speed| (*speed - DEFAULT_SPEED).abs() < f32::EPSILON)
            .unwrap_or(0);

        let backend = PdfBackend::new(cc.egui_ctx.clone());
        let render_generation = backend.render_generation();
        let logo_texture = image::load_from_memory(include_bytes!("../assets/logo.png"))
            .ok()
            .map(|image| {
                let rgba = image.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                cc.egui_ctx.load_texture(
                    "kvikk-logo",
                    ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()),
                    TextureOptions::LINEAR,
                )
            });

        Self {
            backend,
            doc_id: 0,
            document: None,
            view_mode: ViewMode::FitWidth,
            manual_zoom: 1.0,
            invert: false,
            fullscreen: false,
            show_menu: true,
            is_playing: false,
            speed_index,
            scroll_x: 0.0,
            scroll_y: 0.0,
            layout: DocumentLayout::default(),
            layout_dirty: true,
            pending_anchor: None,
            pending_goto: None,
            current_page: 0,
            frame_no: 0,
            bitmaps: HashMap::new(),
            render_in_flight: HashSet::new(),
            render_failed: HashSet::new(),
            render_generation,
            render_debounce_frames: 0,
            native_text: Vec::new(),
            search_text: Vec::new(),
            text_requested: HashSet::new(),
            ocr_queued: VecDeque::new(),
            ocr_queued_set: HashSet::new(),
            ocr_in_flight: HashSet::new(),
            ocr_done: HashSet::new(),
            ocr_available: true,
            search_open: false,
            search_focus_requested: false,
            search_query: String::new(),
            search_results: Vec::new(),
            search_result_index: None,
            search_index_cursor: 0,
            selection_anchor: None,
            selection_focus: None,
            goto_active: false,
            goto_buffer: String::new(),
            goto_deadline: None,
            question_down: false,
            question_visible_until: 0.0,
            show_about: false,
            logo_texture,
            status: "Drop a PDF here or open one from the menu.".into(),
            last_viewport: Vec2::ZERO,
            startup_path,
        }
    }

    fn page_count(&self) -> usize {
        self.document.as_ref().map(|doc| doc.pages.len()).unwrap_or(0)
    }

    fn speed(&self) -> f32 {
        SPEED_LEVELS[self.speed_index]
    }

    fn invalidate_render_requests(&mut self, debounce_frames: u8) {
        self.render_generation = self.backend.bump_render_generation();
        self.render_in_flight.clear();
        self.render_failed.clear();
        self.render_debounce_frames = debounce_frames;
    }

    fn open_path(&mut self, path: PathBuf) {
        if !is_pdf(&path) {
            self.status = "That file does not look like a PDF.".into();
            return;
        }

        self.doc_id = self.doc_id.wrapping_add(1).max(1);
        self.invalidate_render_requests(0);
        self.document = None;
        self.bitmaps.clear();
        self.render_in_flight.clear();
        self.render_failed.clear();
        self.native_text.clear();
        self.search_text.clear();
        self.text_requested.clear();
        self.ocr_queued.clear();
        self.ocr_queued_set.clear();
        self.ocr_in_flight.clear();
        self.ocr_done.clear();
        self.pending_goto = None;
        self.search_results.clear();
        self.search_result_index = None;
        self.search_index_cursor = 0;
        self.selection_anchor = None;
        self.selection_focus = None;
        self.scroll_x = 0.0;
        self.scroll_y = 0.0;
        self.current_page = 0;
        self.is_playing = false;
        self.view_mode = ViewMode::FitWidth;
        self.layout_dirty = true;
        self.status = format!("Opening {}…", path.file_name().and_then(|s| s.to_str()).unwrap_or("PDF"));
        self.backend.high(BackendCommand::Open { doc_id: self.doc_id, path });
    }

    fn poll_backend(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.backend.events.try_recv() {
            match event {
                BackendEvent::Opened { doc_id, info } if doc_id == self.doc_id => {
                    let count = info.pages.len();
                    self.native_text = vec![None; count];
                    self.search_text = vec![None; count];
                    self.current_page = 0;
                    self.scroll_x = 0.0;
                    self.scroll_y = 0.0;
                    self.layout_dirty = true;
                    self.status = format!("{} pages", count);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("{} — kvikk pdf", info.title)));
                    self.document = Some(info);
                }
                BackendEvent::Rendered {
                    doc_id,
                    page,
                    pixel_width,
                    generation,
                    width,
                    height,
                    rgba,
                } if doc_id == self.doc_id && generation == self.render_generation => {
                    self.render_in_flight.remove(&(page, pixel_width));
                    self.bitmaps.insert(
                        (page, pixel_width),
                        BitmapEntry {
                            page,
                            requested_width: pixel_width,
                            width,
                            height,
                            rgba,
                            normal: None,
                            inverted: None,
                            last_used: self.frame_no,
                        },
                    );
                    ctx.request_repaint();
                }
                BackendEvent::RenderFailed {
                    doc_id,
                    page,
                    pixel_width,
                    generation,
                    message,
                } if doc_id == self.doc_id && generation == self.render_generation => {
                    self.render_in_flight.remove(&(page, pixel_width));
                    self.render_failed.insert((page, pixel_width));
                    self.status = message;
                }
                BackendEvent::TextReady { doc_id, data } if doc_id == self.doc_id => {
                    let page = data.page;
                    self.text_requested.remove(&page);
                    if page < self.native_text.len() {
                        let sparse = useful_char_count(&data.text) < MIN_NATIVE_TEXT_CHARS;
                        self.search_text[page] = Some(data.text.clone());
                        self.native_text[page] = Some(data);
                        if sparse && self.search_open {
                            self.queue_ocr(page);
                        }
                        self.recompute_search();
                    }
                }
                BackendEvent::OcrReady { doc_id, page, text } if doc_id == self.doc_id => {
                    self.ocr_in_flight.remove(&page);
                    self.ocr_done.insert(page);
                    if page < self.search_text.len() && useful_char_count(&text) > 0 {
                        self.search_text[page] = Some(text);
                        self.recompute_search();
                    }
                }
                BackendEvent::OcrFailed { doc_id, page, message } if doc_id == self.doc_id => {
                    self.ocr_in_flight.remove(&page);
                    self.ocr_done.insert(page);
                    self.status = message;
                }
                BackendEvent::OcrUnavailable { message, .. } => {
                    self.ocr_available = false;
                    self.status = message;
                    self.ocr_queued.clear();
                    self.ocr_queued_set.clear();
                    self.ocr_in_flight.clear();
                }
                BackendEvent::LinkResolved { doc_id, target } if doc_id == self.doc_id => {
                    self.selection_anchor = None;
                    self.selection_focus = None;
                    match target {
                        LinkTarget::Page(page) => self.goto_page(page),
                        LinkTarget::Url(url) => {
                            let lower = url.to_ascii_lowercase();
                            if lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:") {
                                ctx.open_url(egui::OpenUrl::new_tab(url));
                            } else {
                                self.status = format!("Blocked unsupported link: {url}");
                            }
                        }
                    }
                }
                BackendEvent::Error { doc_id, message } if doc_id == 0 || doc_id == self.doc_id => {
                    self.status = message;
                }
                _ => {}
            }
        }
    }

    fn queue_text(&mut self, page: usize, high_priority: bool) {
        if page >= self.page_count()
            || self.native_text.get(page).and_then(|v| v.as_ref()).is_some()
            || !self.text_requested.insert(page)
        {
            return;
        }
        let cmd = BackendCommand::ExtractText { doc_id: self.doc_id, page };
        if high_priority {
            self.backend.high(cmd);
        } else {
            self.backend.low(cmd);
        }
    }

    fn queue_ocr(&mut self, page: usize) {
        if !self.ocr_available
            || page >= self.page_count()
            || self.ocr_queued_set.contains(&page)
            || self.ocr_in_flight.contains(&page)
            || self.ocr_done.contains(&page)
        {
            return;
        }
        self.ocr_queued.push_back(page);
        self.ocr_queued_set.insert(page);
    }

    fn pump_indexing(&mut self) {
        if !self.search_open || self.document.is_none() {
            return;
        }

        let count = self.page_count();
        let mut queued = 0;
        while self.search_index_cursor < count && queued < 6 {
            let page = self.search_index_cursor;
            self.search_index_cursor += 1;
            if self.native_text.get(page).and_then(|v| v.as_ref()).is_none() {
                self.queue_text(page, false);
                queued += 1;
            } else if self
                .native_text
                .get(page)
                .and_then(|v| v.as_ref())
                .is_some_and(|data| useful_char_count(&data.text) < MIN_NATIVE_TEXT_CHARS)
            {
                self.queue_ocr(page);
            }
        }

        while self.ocr_in_flight.len() < 1 {
            let Some(page) = self.ocr_queued.pop_front() else { break };
            self.ocr_queued_set.remove(&page);
            self.ocr_in_flight.insert(page);
            self.backend.low(BackendCommand::Ocr { doc_id: self.doc_id, page });
        }
    }

    fn recompute_search(&mut self) {
        self.search_results.clear();
        self.search_result_index = None;
        let needle = self.search_query.trim().to_lowercase();
        if needle.is_empty() {
            return;
        }

        for (page, text) in self.search_text.iter().enumerate() {
            let Some(text) = text else { continue };
            let lower = text.to_lowercase();
            for (byte_start, _) in lower.match_indices(&needle).take(500) {
                self.search_results.push(SearchHit {
                    page,
                    byte_start,
                    snippet: make_snippet(text, &lower, byte_start, needle.len()),
                });
                if self.search_results.len() >= 20_000 {
                    return;
                }
            }
        }
    }

    fn next_search_result(&mut self, backwards: bool) {
        if self.search_results.is_empty() {
            return;
        }
        let len = self.search_results.len();
        let next = match self.search_result_index {
            None => if backwards { len - 1 } else { 0 },
            Some(i) if backwards => (i + len - 1) % len,
            Some(i) => (i + 1) % len,
        };
        self.search_result_index = Some(next);
        let page = self.search_results[next].page;
        self.goto_page(page);
    }

    fn open_search(&mut self) {
        self.search_open = true;
        self.search_focus_requested = true;
        self.search_index_cursor = 0;
        for page in 0..self.page_count() {
            let sparse = self
                .native_text
                .get(page)
                .and_then(|v| v.as_ref())
                .is_some_and(|data| useful_char_count(&data.text) < MIN_NATIVE_TEXT_CHARS);
            if sparse {
                self.queue_ocr(page);
            }
        }
    }

    fn capture_anchor(&self, viewport: Rect, pointer: Option<Pos2>) -> Option<ScrollAnchor> {
        let document = self.document.as_ref()?;
        if document.pages.is_empty() {
            return None;
        }
        let target = pointer.unwrap_or_else(|| Pos2::new(viewport.center().x, viewport.top() + 10.0));
        let content_x = self.scroll_x + (target.x - viewport.left());
        let content_y = self.scroll_y + (target.y - viewport.top());

        // Anchor to the page at or immediately above the reading edge. In particular,
        // if the edge is sitting in the gap between pages we deliberately keep the
        // previous page instead of snapping forward during a layout-mode change.
        let reading_edge_page = self
            .layout
            .rows
            .iter()
            .take_while(|row| row.y <= content_y)
            .last()
            .and_then(|row| row.pages.first())
            .copied()
            .or_else(|| self.layout.rows.first().and_then(|row| row.pages.first()).copied());

        let candidate = if pointer.is_some() {
            self.layout
                .rows
                .iter()
                .flat_map(|row| row.pages.iter())
                .copied()
                .find(|placed| {
                    content_x >= placed.x
                        && content_x <= placed.x + placed.w
                        && content_y >= placed.y
                        && content_y <= placed.y + placed.h
                })
                .or(reading_edge_page)
        } else {
            reading_edge_page
        };
        let placed = candidate?;
        Some(ScrollAnchor {
            page: placed.page,
            offset_y: ((content_y - placed.y) / placed.h).clamp(0.0, 1.0),
            viewport_y: target.y - viewport.top(),
            offset_x: ((content_x - placed.x) / placed.w).clamp(0.0, 1.0),
            viewport_x: target.x - viewport.left(),
        })
    }

    fn apply_anchor(&mut self, anchor: ScrollAnchor, viewport: Rect) {
        let Some(placed) = self.layout.placed_page(anchor.page) else { return };
        let content_y = placed.y + placed.h * anchor.offset_y;
        let content_x = placed.x + placed.w * anchor.offset_x;
        self.scroll_y = content_y - anchor.viewport_y;
        self.scroll_x = content_x - anchor.viewport_x;
        self.clamp_scroll(viewport.size());
    }

    fn set_mode(&mut self, mode: ViewMode, viewport: Option<Rect>) {
        if self.view_mode == mode {
            return;
        }

        if let Some(viewport) = viewport {
            if mode.pages_per_view() > 1 {
                // Multi-page modes represent a complete viewport group. Preserve the
                // page the reader was on, but align the containing group to the top so
                // 3×2 / 5×2 / 7×3 never open halfway through their own grid.
                let page = self
                    .capture_anchor(viewport, None)
                    .map(|anchor| anchor.page)
                    .unwrap_or(self.current_page);
                self.pending_anchor = None;
                self.pending_goto = Some(mode.canonical_page(page));
            } else {
                self.pending_goto = None;
                self.pending_anchor = self.capture_anchor(viewport, None);
            }
        }

        self.view_mode = mode;
        self.invalidate_render_requests(5);
        self.layout_dirty = true;
    }

    fn effective_zoom(&self) -> f32 {
        self.layout
            .placed_page(self.current_page)
            .map(|p| p.scale)
            .unwrap_or(self.manual_zoom)
    }

    fn zoom_by(&mut self, factor: f32, viewport: Rect, pointer: Option<Pos2>) {
        let anchor = self.capture_anchor(viewport, pointer);
        let base = self.effective_zoom();
        self.manual_zoom = (base * factor).clamp(0.10, MAX_MANUAL_ZOOM);
        self.view_mode = ViewMode::Manual;
        self.pending_anchor = anchor;
        self.invalidate_render_requests(5);
        self.layout_dirty = true;
    }

    fn reset_zoom(&mut self, viewport: Option<Rect>) {
        if let Some(viewport) = viewport {
            self.pending_anchor = self.capture_anchor(viewport, None);
        }
        self.manual_zoom = 1.0;
        self.view_mode = ViewMode::Manual;
        self.invalidate_render_requests(5);
        self.layout_dirty = true;
    }

    fn goto_page(&mut self, page: usize) {
        let count = self.page_count();
        if count == 0 {
            return;
        }
        let page = page.min(count - 1);
        let canonical = self.view_mode.canonical_page(page);
        if let Some(row) = self.layout.row_for_page(canonical) {
            self.scroll_y = row.y;
            self.current_page = canonical;
            self.pending_goto = None;
        } else {
            self.current_page = canonical;
            self.pending_goto = Some(canonical);
            self.layout_dirty = true;
        }
    }

    fn next_page(&mut self, backwards: bool) {
        let count = self.page_count();
        if count == 0 {
            return;
        }
        let step = self.view_mode.pages_per_view();
        let target = if backwards {
            self.current_page.saturating_sub(step)
        } else {
            (self.current_page + step).min(count - 1)
        };
        self.goto_page(target);
    }

    fn speed_slower(&mut self) {
        self.speed_index = self.speed_index.saturating_sub(1);
    }

    fn speed_faster(&mut self) {
        self.speed_index = (self.speed_index + 1).min(SPEED_LEVELS.len() - 1);
    }

    fn toggle_invert(&mut self) {
        self.invert = !self.invert;
        // Keep only one GPU texture variant per cached bitmap. The source RGBA stays in
        // CPU memory, so toggling back only requires a texture upload, not a PDF rerender.
        for entry in self.bitmaps.values_mut() {
            if self.invert {
                entry.normal = None;
            } else {
                entry.inverted = None;
            }
        }
    }

    fn toggle_fullscreen(&mut self, ctx: &egui::Context) {
        self.fullscreen = !self.fullscreen;
        if self.fullscreen {
            self.show_menu = false;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
    }

    fn clamp_scroll(&mut self, viewport_size: Vec2) {
        let max_y = (self.layout.content_height - viewport_size.y).max(0.0);
        let max_x = (self.layout.content_width - viewport_size.x).max(0.0);
        self.scroll_y = self.scroll_y.clamp(0.0, max_y);
        self.scroll_x = self.scroll_x.clamp(0.0, max_x);
    }

    fn handle_global_input(&mut self, ctx: &egui::Context, viewport: Option<Rect>) {
        let now = ctx.input(|i| i.time);
        if self.goto_active && self.goto_deadline.is_some_and(|deadline| now >= deadline) {
            self.commit_goto();
        }

        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if let Some(path) = dropped
            .into_iter()
            .map(|file| file.path().to_path_buf())
            .find(|path| is_pdf(path))
        {
            self.open_path(path);
        }

        // Update the hold-to-help key before any early return (for example while the
        // search box is open), so `?` remains a global command.
        self.question_down = ctx.input(|i| i.key_down(Key::Questionmark));

        let (events, modifiers, space, escape, enter, command_f, command_c) = ctx.input(|i| {
            (
                i.events.clone(),
                i.modifiers,
                i.key_pressed(Key::Space),
                i.key_pressed(Key::Escape),
                i.key_pressed(Key::Enter),
                i.modifiers.command && i.key_pressed(Key::F),
                i.modifiers.command && i.key_pressed(Key::C),
            )
        });

        if command_f {
            self.open_search();
            return;
        }

        if self.search_open {
            if escape {
                self.search_open = false;
            } else if enter {
                self.next_search_result(modifiers.shift);
            }
            return;
        }

        if command_c {
            if let Some(text) = self.selected_text() {
                ctx.copy_text(text);
            }
        }

        if space {
            self.next_page(modifiers.shift);
        }

        for event in events {
            match event {
                egui::Event::Text(text) if !modifiers.command && !modifiers.ctrl && !modifiers.alt => {
                    for ch in text.chars() {
                        if ch == '?' {
                            self.question_down = true;
                            self.question_visible_until = now + 0.35;
                            continue;
                        }

                        if self.goto_active {
                            if ch.is_ascii_digit() {
                                self.goto_buffer.push(ch);
                                self.goto_deadline = Some(now + 0.45);
                                continue;
                            }
                            self.commit_goto();
                        }

                        match ch.to_ascii_lowercase() {
                            'g' => {
                                self.goto_active = true;
                                self.goto_buffer.clear();
                                self.goto_deadline = Some(now + 0.8);
                            }
                            's' => self.show_menu = !self.show_menu,
                            'i' => self.toggle_invert(),
                            'p' | 'k' => self.is_playing = !self.is_playing,
                            'j' => self.speed_slower(),
                            'l' => self.speed_faster(),
                            'f' => self.toggle_fullscreen(ctx),
                            'o' => self.reset_zoom(viewport),
                            '1' => self.set_mode(ViewMode::FitWidth, viewport),
                            '2' => self.set_mode(ViewMode::FitHeight, viewport),
                            '3' => self.set_mode(ViewMode::Spread, viewport),
                            '4' => self.set_mode(ViewMode::Grid3, viewport),
                            '5' => self.set_mode(ViewMode::Grid6, viewport),
                            '6' => self.set_mode(ViewMode::Grid10, viewport),
                            '7' => self.set_mode(ViewMode::Grid21, viewport),
                            '/' => self.open_search(),
                            '+' | '=' => {
                                if let Some(viewport) = viewport {
                                    self.zoom_by(1.12, viewport, None);
                                }
                            }
                            '-' | '−' => {
                                if let Some(viewport) = viewport {
                                    self.zoom_by(1.0 / 1.12, viewport, None);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }

    }

    fn commit_goto(&mut self) {
        if self.goto_active && !self.goto_buffer.is_empty() {
            if let Ok(page) = self.goto_buffer.parse::<usize>() {
                self.goto_page(page.saturating_sub(1));
            }
        }
        self.goto_active = false;
        self.goto_buffer.clear();
        self.goto_deadline = None;
    }

    fn selected_text(&self) -> Option<String> {
        let (start, end) = ordered_selection(self.selection_anchor?, self.selection_focus?);
        let mut out = String::new();
        for page in start.page..=end.page {
            let Some(data) = self.native_text.get(page).and_then(|d| d.as_ref()) else { continue };
            let from = if page == start.page { start.glyph } else { 0 };
            let to = if page == end.page {
                end.glyph.min(data.glyphs.len().saturating_sub(1))
            } else {
                data.glyphs.len().saturating_sub(1)
            };
            if from <= to && !data.glyphs.is_empty() {
                for glyph in &data.glyphs[from..=to] {
                    out.push(glyph.ch);
                }
                if page != end.page {
                    out.push('\n');
                }
            }
        }
        if out.is_empty() { None } else { Some(out) }
    }

    fn ensure_layout(&mut self, viewport: Rect) {
        if self.document.is_none() {
            self.layout = DocumentLayout::default();
            return;
        }

        let resized = (self.last_viewport.x - viewport.width()).abs() > 0.5
            || (self.last_viewport.y - viewport.height()).abs() > 0.5;
        if resized {
            if self.last_viewport != Vec2::ZERO && self.pending_anchor.is_none() {
                self.pending_anchor = self.capture_anchor(
                    Rect::from_min_size(viewport.min, self.last_viewport),
                    None,
                );
                // Resize storms behave like pinch zoom: reuse the current bitmap immediately
                // and stop PDFium from finishing obsolete sizes until the window settles.
                self.invalidate_render_requests(4);
            }
            self.last_viewport = viewport.size();
            self.layout_dirty = true;
        }

        if self.layout_dirty {
            let pages = &self.document.as_ref().expect("document checked above").pages;
            self.layout = build_layout(
                pages,
                self.view_mode,
                self.manual_zoom,
                viewport.width(),
                viewport.height(),
            );
            self.layout_dirty = false;
            if let Some(anchor) = self.pending_anchor.take() {
                self.apply_anchor(anchor, viewport);
            } else if let Some(page) = self.pending_goto.take() {
                if let Some(row) = self.layout.row_for_page(page) {
                    self.scroll_y = row.y;
                    self.current_page = page;
                }
            }
            self.clamp_scroll(viewport.size());
        }
    }

    fn desired_render_width(&self, displayed_width: f32, pixels_per_point: f32) -> u32 {
        let raw = (displayed_width * pixels_per_point * 1.08).ceil() as u32;
        let quantized = ((raw + 63) / 64) * 64;
        quantized.clamp(MIN_RENDER_WIDTH, MAX_RENDER_WIDTH)
    }

    fn best_bitmap_key(&self, page: usize, desired: u32) -> Option<(usize, u32)> {
        let mut wider: Option<u32> = None;
        let mut narrower: Option<u32> = None;
        for &(candidate_page, width) in self.bitmaps.keys() {
            if candidate_page != page {
                continue;
            }
            if width >= desired {
                if wider.is_none_or(|old| width < old) {
                    wider = Some(width);
                }
            } else if narrower.is_none_or(|old| width > old) {
                narrower = Some(width);
            }
        }
        wider.or(narrower).map(|width| (page, width))
    }

    fn request_render(&mut self, page: usize, pixel_width: u32) {
        let key = (page, pixel_width);
        if self.bitmaps.contains_key(&key)
            || self.render_failed.contains(&key)
            || !self.render_in_flight.insert(key)
        {
            return;
        }
        self.backend.high(BackendCommand::Render {
            doc_id: self.doc_id,
            page,
            pixel_width,
            generation: self.render_generation,
        });
    }

    fn texture_for(&mut self, ctx: &egui::Context, key: (usize, u32), inverted: bool) -> Option<TextureHandle> {
        let entry = self.bitmaps.get_mut(&key)?;
        entry.last_used = self.frame_no;
        if inverted {
            if entry.inverted.is_none() {
                let mut pixels = entry.rgba.clone();
                for rgba in pixels.chunks_exact_mut(4) {
                    rgba[0] = 255 - rgba[0];
                    rgba[1] = 255 - rgba[1];
                    rgba[2] = 255 - rgba[2];
                }
                let image = ColorImage::from_rgba_unmultiplied([entry.width, entry.height], &pixels);
                entry.inverted = Some(ctx.load_texture(
                    format!("pdf-{}-{}-inv", entry.page, entry.requested_width),
                    image,
                    TextureOptions::LINEAR,
                ));
            }
            entry.inverted.clone()
        } else {
            if entry.normal.is_none() {
                let image = ColorImage::from_rgba_unmultiplied([entry.width, entry.height], &entry.rgba);
                entry.normal = Some(ctx.load_texture(
                    format!("pdf-{}-{}", entry.page, entry.requested_width),
                    image,
                    TextureOptions::LINEAR,
                ));
            }
            entry.normal.clone()
        }
    }

    fn prune_bitmap_cache(&mut self, visible_pages: &HashSet<usize>) {
        if self.frame_no % 90 != 0 {
            return;
        }
        let mut bytes: usize = self.bitmaps.values().map(BitmapEntry::byte_size).sum();
        if bytes <= BITMAP_CACHE_BUDGET {
            return;
        }
        let mut victims: Vec<_> = self
            .bitmaps
            .iter()
            .filter(|(_, entry)| !visible_pages.contains(&entry.page))
            .map(|(key, entry)| (*key, entry.last_used, entry.byte_size()))
            .collect();
        victims.sort_by_key(|(_, used, _)| *used);
        for (key, _, size) in victims {
            if bytes <= BITMAP_CACHE_BUDGET {
                break;
            }
            self.bitmaps.remove(&key);
            bytes = bytes.saturating_sub(size);
        }
    }

    fn draw_document(&mut self, ui: &mut egui::Ui) -> Rect {
        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let viewport = response.rect;
        painter.rect_filled(viewport, 0.0, Color32::BLACK);
        self.ensure_layout(viewport);

        if self.document.is_none() {
            painter.text(
                viewport.center(),
                egui::Align2::CENTER_CENTER,
                "Drop a PDF here\n\nO = 100% zoom · ? = commands",
                egui::FontId::proportional(20.0),
                Color32::from_gray(150),
            );
            return viewport;
        }

        let zoom_delta = ui.ctx().input(|i| i.zoom_delta());
        if (zoom_delta - 1.0).abs() > 0.001 && response.hovered() {
            let pointer = ui.ctx().input(|i| i.pointer.hover_pos());
            self.zoom_by(zoom_delta, viewport, pointer);
            self.ensure_layout(viewport);
        } else {
            let scroll = ui.ctx().input(|i| i.smooth_scroll_delta());
            if response.hovered() && scroll != Vec2::ZERO {
                self.scroll_y -= scroll.y;
                self.scroll_x -= scroll.x;
            }
        }

        if self.is_playing {
            let dt = ui.ctx().input(|i| i.stable_dt).clamp(0.0, 0.1);
            self.scroll_y += self.speed() * dt;
            ui.ctx().request_repaint();
        }
        self.clamp_scroll(viewport.size());
        if self.is_playing {
            let max_y = (self.layout.content_height - viewport.height()).max(0.0);
            if self.scroll_y >= max_y - 0.01 {
                self.is_playing = false;
            }
        }

        self.current_page = self
            .layout
            .canonical_page_at_y(self.scroll_y + 12.0, self.view_mode)
            .min(self.page_count().saturating_sub(1));

        let preload = viewport.height() * 0.8;
        let visible_top = (self.scroll_y - preload).max(0.0);
        let visible_bottom = self.scroll_y + viewport.height() + preload;
        let first_row = self.layout.first_visible_row(visible_top);
        let pixels_per_point = ui.ctx().input(|i| i.pixels_per_point());
        let mut visible_pages = HashSet::new();
        let mut screen_pages = Vec::new();
        let mut paint_pages = Vec::new();
        for row in self.layout.rows.iter().skip(first_row) {
            if row.y > visible_bottom {
                break;
            }
            paint_pages.extend(row.pages.iter().copied());
        }

        for placed in paint_pages {
            visible_pages.insert(placed.page);
            let screen_rect = Rect::from_min_size(
                Pos2::new(
                    viewport.left() + placed.x - self.scroll_x,
                    viewport.top() + placed.y - self.scroll_y,
                ),
                Vec2::new(placed.w, placed.h),
            );
            screen_pages.push((placed, screen_rect));

            if screen_rect.intersects(viewport.expand(4.0)) {
                let paper = if self.invert { Color32::BLACK } else { Color32::WHITE };
                painter.rect_filled(screen_rect, 1.0, paper);
                let desired = self.desired_render_width(placed.w, pixels_per_point);
                let best = self.best_bitmap_key(placed.page, desired);
                let has_good_enough = best.is_some_and(|(_, width)| {
                    width as f32 >= desired as f32 * 0.90 && width as f32 <= desired as f32 * 1.55
                });
                if !has_good_enough && (best.is_none() || self.render_debounce_frames == 0) {
                    self.request_render(placed.page, desired);
                }
                if let Some(key) = best {
                    if let Some(texture) = self.texture_for(ui.ctx(), key, self.invert) {
                        painter.image(
                            texture.id(),
                            screen_rect,
                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                } else {
                    painter.text(
                        screen_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "rendering…",
                        egui::FontId::proportional(13.0),
                        if self.invert { Color32::LIGHT_GRAY } else { Color32::DARK_GRAY },
                    );
                }

                self.queue_text(placed.page, false);
                self.paint_selection(&painter, placed, screen_rect);

                if self
                    .search_result_index
                    .and_then(|i| self.search_results.get(i))
                    .is_some_and(|hit| hit.page == placed.page)
                {
                    painter.rect_stroke(
                        screen_rect.expand(3.0),
                        2.0,
                        Stroke::new(2.0, Color32::from_rgb(80, 150, 255)),
                        egui::StrokeKind::Outside,
                    );
                }
            }
        }

        self.handle_selection_input(ui, &response, viewport, &screen_pages);
        self.handle_link_click(ui, &response, &screen_pages);
        self.prune_bitmap_cache(&visible_pages);

        if self.goto_active {
            painter.text(
                viewport.right_top() + Vec2::new(-22.0, 22.0),
                egui::Align2::RIGHT_TOP,
                format!("g{}", self.goto_buffer),
                egui::FontId::monospace(18.0),
                Color32::WHITE,
            );
        }

        viewport
    }

    fn handle_selection_input(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        _viewport: Rect,
        screen_pages: &[(PlacedPage, Rect)],
    ) {
        let pointer_pos = ui.ctx().input(|i| i.pointer.interact_pos());
        let pressed = ui.ctx().input(|i| i.pointer.primary_pressed());
        let down = ui.ctx().input(|i| i.pointer.primary_down());

        if response.hovered() && pressed {
            if let Some(pos) = pointer_pos {
                if let Some(point) = self.hit_test_glyph(pos, screen_pages) {
                    self.selection_anchor = Some(point);
                    self.selection_focus = Some(point);
                } else {
                    self.selection_anchor = None;
                    self.selection_focus = None;
                }
            }
        } else if down && self.selection_anchor.is_some() {
            if let Some(pos) = pointer_pos {
                if let Some(point) = self.hit_test_glyph(pos, screen_pages) {
                    self.selection_focus = Some(point);
                }
            }
        }
    }

    fn handle_link_click(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        screen_pages: &[(PlacedPage, Rect)],
    ) {
        if !response.clicked() {
            return;
        }
        let Some(pos) = response.interact_pointer_pos() else { return };
        let Some(document) = self.document.as_ref() else { return };
        let Some((placed, rect)) = screen_pages.iter().find(|(_, rect)| rect.contains(pos)) else { return };
        let metric = document.pages[placed.page];
        let x_pt = ((pos.x - rect.left()) / rect.width()) * metric.width_pt;
        let y_pt = metric.height_pt - ((pos.y - rect.top()) / rect.height()) * metric.height_pt;

        self.backend.high(BackendCommand::ResolveLink {
            doc_id: self.doc_id,
            page: placed.page,
            x_pt,
            y_pt,
        });
        ui.ctx().request_repaint();
    }

    fn hit_test_glyph(&self, pos: Pos2, screen_pages: &[(PlacedPage, Rect)]) -> Option<SelectionPoint> {
        let document = self.document.as_ref()?;
        let (placed, screen_rect) = screen_pages.iter().find(|(_, rect)| rect.contains(pos))?;
        let data = self.native_text.get(placed.page)?.as_ref()?;
        if data.glyphs.is_empty() {
            return None;
        }
        let metric = document.pages[placed.page];
        let x_pt = ((pos.x - screen_rect.left()) / screen_rect.width()) * metric.width_pt;
        let y_pt = metric.height_pt - ((pos.y - screen_rect.top()) / screen_rect.height()) * metric.height_pt;

        let mut best: Option<(usize, f32)> = None;
        for (index, glyph) in data.glyphs.iter().enumerate() {
            let Some(bounds) = glyph.bounds else { continue };
            let dx = if x_pt < bounds.left {
                bounds.left - x_pt
            } else if x_pt > bounds.right {
                x_pt - bounds.right
            } else {
                0.0
            };
            let dy = if y_pt < bounds.bottom {
                bounds.bottom - y_pt
            } else if y_pt > bounds.top {
                y_pt - bounds.top
            } else {
                0.0
            };
            let distance = dx * dx + dy * dy;
            if best.is_none_or(|(_, old)| distance < old) {
                best = Some((index, distance));
                if distance == 0.0 {
                    break;
                }
            }
        }
        best.map(|(glyph, _)| SelectionPoint { page: placed.page, glyph })
    }

    fn paint_selection(&self, painter: &egui::Painter, placed: PlacedPage, screen_rect: Rect) {
        let (Some(a), Some(b)) = (self.selection_anchor, self.selection_focus) else { return };
        let (start, end) = ordered_selection(a, b);
        if placed.page < start.page || placed.page > end.page {
            return;
        }
        let Some(document) = self.document.as_ref() else { return };
        let Some(data) = self.native_text.get(placed.page).and_then(|v| v.as_ref()) else { return };
        if data.glyphs.is_empty() {
            return;
        }
        let from = if placed.page == start.page { start.glyph } else { 0 };
        let to = if placed.page == end.page {
            end.glyph.min(data.glyphs.len().saturating_sub(1))
        } else {
            data.glyphs.len().saturating_sub(1)
        };
        if from > to {
            return;
        }
        let metric = document.pages[placed.page];
        for glyph in &data.glyphs[from..=to] {
            let Some(bounds) = glyph.bounds else { continue };
            let rect = pdf_bounds_to_screen(bounds, metric.width_pt, metric.height_pt, screen_rect);
            painter.rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(60, 120, 255, 78));
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, viewport_hint: Option<Rect>) {
        egui::Panel::top("toolbar")
            .frame(egui::Frame::new().fill(Color32::from_gray(18)))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Open PDF").clicked() {
                        if let Some(path) = rfd::FileDialog::new().add_filter("PDF", &["pdf"]).pick_file() {
                            self.open_path(path);
                        }
                    }
                    ui.separator();
                    if ui.button(if self.is_playing { "Pause" } else { "Play" }).clicked() {
                        self.is_playing = !self.is_playing;
                    }
                    if ui.small_button("J slower").clicked() {
                        self.speed_slower();
                    }
                    ui.monospace(format!("{} px/s", pretty_speed(self.speed())));
                    if ui.small_button("L faster").clicked() {
                        self.speed_faster();
                    }
                    ui.separator();
                    if ui.selectable_label(self.view_mode == ViewMode::FitWidth, "1 Width").clicked() {
                        self.set_mode(ViewMode::FitWidth, viewport_hint);
                    }
                    if ui.selectable_label(self.view_mode == ViewMode::FitHeight, "2 Height").clicked() {
                        self.set_mode(ViewMode::FitHeight, viewport_hint);
                    }
                    if ui.selectable_label(self.view_mode == ViewMode::Spread, "3 2×1").clicked() {
                        self.set_mode(ViewMode::Spread, viewport_hint);
                    }
                    if ui.selectable_label(self.view_mode == ViewMode::Grid3, "4 3×1").clicked() {
                        self.set_mode(ViewMode::Grid3, viewport_hint);
                    }
                    if ui.selectable_label(self.view_mode == ViewMode::Grid6, "5 3×2").clicked() {
                        self.set_mode(ViewMode::Grid6, viewport_hint);
                    }
                    if ui.selectable_label(self.view_mode == ViewMode::Grid10, "6 5×2").clicked() {
                        self.set_mode(ViewMode::Grid10, viewport_hint);
                    }
                    if ui.selectable_label(self.view_mode == ViewMode::Grid21, "7 7×3").clicked() {
                        self.set_mode(ViewMode::Grid21, viewport_hint);
                    }
                    if ui.button("−").clicked() {
                        if let Some(rect) = viewport_hint {
                            self.zoom_by(1.0 / 1.12, rect, None);
                        }
                    }
                    ui.monospace(format!("{:.0}%", self.effective_zoom() * 100.0));
                    if ui.button("+").clicked() {
                        if let Some(rect) = viewport_hint {
                            self.zoom_by(1.12, rect, None);
                        }
                    }
                    if ui.button("O 100%").clicked() {
                        self.reset_zoom(viewport_hint);
                    }
                    ui.separator();
                    if ui.selectable_label(self.invert, "I Invert").clicked() {
                        self.toggle_invert();
                    }
                    if ui.button("Search").clicked() {
                        self.open_search();
                    }
                    if ui.button("F Fullscreen").clicked() {
                        self.toggle_fullscreen(ui.ctx());
                    }
                    if ui.button("About").clicked() {
                        self.show_about = true;
                    }
                    ui.separator();
                    let count = self.page_count();
                    if count > 0 {
                        let step = self.view_mode.pages_per_view();
                        if step > 1 {
                            let start = self.view_mode.canonical_page(self.current_page);
                            let end = (start + step).min(count);
                            ui.monospace(format!("Pages {}–{} / {}", start + 1, end, count));
                        } else {
                            ui.monospace(format!("Page {} / {}", self.current_page + 1, count));
                        }
                    }
                    if !self.status.is_empty() {
                        ui.separator();
                        ui.label(&self.status);
                    }
                });
            });
    }

    fn search_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("search")
            .frame(egui::Frame::new().fill(Color32::from_gray(24)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Find:");
                    let old = self.search_query.clone();
                    let response = ui.add_sized(
                        [360.0, 26.0],
                        egui::TextEdit::singleline(&mut self.search_query).hint_text("Search this PDF"),
                    );
                    if self.search_focus_requested {
                        response.request_focus();
                        self.search_focus_requested = false;
                    }
                    if self.search_query != old {
                        self.recompute_search();
                    }
                    if ui.button("↑").clicked() {
                        self.next_search_result(true);
                    }
                    if ui.button("↓").clicked() {
                        self.next_search_result(false);
                    }
                    let indexed = self.search_text.iter().filter(|v| v.is_some()).count();
                    let total = self.page_count();
                    match self.search_result_index {
                        Some(i) if !self.search_results.is_empty() => {
                            ui.monospace(format!("{} / {}", i + 1, self.search_results.len()));
                            if let Some(hit) = self.search_results.get(i) {
                                ui.label(format!("p{}  {}", hit.page + 1, hit.snippet));
                            }
                        }
                        _ => {
                            ui.monospace(format!("{} results", self.search_results.len()));
                        }
                    }
                    if indexed < total {
                        ui.label(format!("indexing {indexed}/{total}"));
                    }
                    if !self.ocr_in_flight.is_empty() || !self.ocr_queued.is_empty() {
                        ui.label("OCR…");
                    }
                    if ui.button("Esc Close").clicked() {
                        self.search_open = false;
                    }
                });
            });
    }

    fn show_about(&mut self, ctx: &egui::Context) {
        if !self.show_about {
            return;
        }

        let mut open = self.show_about;
        egui::Window::new("About kvikk pdf")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    if let Some(texture) = &self.logo_texture {
                        ui.add(egui::Image::new(texture).fit_to_exact_size(Vec2::splat(112.0)));
                        ui.add_space(8.0);
                    }
                    ui.heading("kvikk pdf");
                    ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                });
                ui.add_space(12.0);
                ui.label("This software is designed for rapid PDF navigation and flexible view customization. The goal is to make deep reading and skimming PDFs as frictionless as possible.");
                ui.add_space(8.0);
                ui.label("Written in Rust by Lars Halvor, in dialogue with an LLM.");
                ui.add_space(8.0);
                ui.strong("kvikk pdf is completely free and open source.");
                ui.add_space(8.0);
                ui.label("If you’d like to say thanks, the best thing you can do is visit my website or send me a message on any of my social platforms.");
                ui.add_space(8.0);
                ui.hyperlink_to("halvorhansen.no", "https://halvorhansen.no");
                ui.add_space(10.0);
                ui.small("Licensed under the MIT License.");
            });
        self.show_about = open;
    }

    fn show_commands(&self, ctx: &egui::Context, now: f64) {
        if !self.question_down && now >= self.question_visible_until {
            return;
        }
        egui::Window::new("Keyboard commands")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.heading("kvikk pdf commands");
                ui.add_space(6.0);
                egui::Grid::new("command-grid")
                    .num_columns(2)
                    .spacing([24.0, 5.0])
                    .show(ui, |ui| {
                        for (key, action) in COMMANDS {
                            ui.monospace(*key);
                            ui.label(*action);
                            ui.end_row();
                        }
                    });
            });
    }
}

impl eframe::App for KvikkApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.frame_no = self.frame_no.wrapping_add(1);
        if self.render_debounce_frames > 0 {
            self.render_debounce_frames -= 1;
        }
        let ctx = ui.ctx().clone();

        if let Some(path) = self.startup_path.take() {
            self.open_path(path);
        }
        self.poll_backend(&ctx);
        self.pump_indexing();

        let viewport_hint = if self.last_viewport != Vec2::ZERO {
            Some(Rect::from_min_size(Pos2::ZERO, self.last_viewport))
        } else {
            None
        };
        self.handle_global_input(&ctx, viewport_hint);

        if self.show_menu {
            self.toolbar(ui, viewport_hint);
        }
        if self.search_open {
            self.search_bar(ui);
        }

        let central = egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(Color32::BLACK))
            .show(ui, |ui| self.draw_document(ui));
        let viewport = central.inner;

        // Keyboard zoom/mode changes use the real viewer rectangle from the next frame.
        // Keeping anchors in document coordinates makes that one-frame handoff harmless.
        self.last_viewport = viewport.size();

        let now = ctx.input(|i| i.time);
        self.show_commands(&ctx, now);
        self.show_about(&ctx);

        if self.is_playing
            || self.question_down
            || now < self.question_visible_until
            || self.render_debounce_frames > 0
        {
            ctx.request_repaint();
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 1.0]
    }
}

fn useful_char_count(text: &str) -> usize {
    text.chars().filter(|ch| !ch.is_whitespace() && !ch.is_control()).count()
}

fn ordered_selection(a: SelectionPoint, b: SelectionPoint) -> (SelectionPoint, SelectionPoint) {
    if (a.page, a.glyph) <= (b.page, b.glyph) { (a, b) } else { (b, a) }
}

fn pdf_bounds_to_screen(bounds: PdfBounds, page_w: f32, page_h: f32, screen: Rect) -> Rect {
    let left = screen.left() + (bounds.left / page_w) * screen.width();
    let right = screen.left() + (bounds.right / page_w) * screen.width();
    let top = screen.top() + ((page_h - bounds.top) / page_h) * screen.height();
    let bottom = screen.top() + ((page_h - bounds.bottom) / page_h) * screen.height();
    Rect::from_min_max(Pos2::new(left, top), Pos2::new(right, bottom))
}

fn make_snippet(text: &str, lower: &str, byte_start: usize, needle_bytes: usize) -> String {
    let char_start = lower[..byte_start.min(lower.len())].chars().count();
    let needle_chars = lower[byte_start.min(lower.len())..(byte_start + needle_bytes).min(lower.len())]
        .chars()
        .count();
    let all: Vec<char> = text.chars().collect();
    let from = char_start.saturating_sub(34);
    let to = (char_start + needle_chars + 52).min(all.len());
    let mut snippet: String = all[from..to].iter().collect();
    snippet = snippet.split_whitespace().collect::<Vec<_>>().join(" ");
    if from > 0 {
        snippet.insert_str(0, "…");
    }
    if to < all.len() {
        snippet.push('…');
    }
    snippet
}

fn pretty_speed(speed: f32) -> String {
    if speed.fract() == 0.0 { format!("{speed:.0}") } else { format!("{speed:.1}") }
}

fn is_pdf(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
}

use crate::{
    backend::{BackendCommand, BackendEvent, PdfBackend},
    layout::build_layout,
    model::{
        DocumentInfo, DocumentLayout, LinkTarget, PageCrop, PageTextData, PdfBounds, PlacedPage,
        SearchHit, SelectionPoint, ViewMode, BITMAP_CACHE_BUDGET, DEFAULT_SPEED,
        MAX_MANUAL_ZOOM, MAX_RENDER_WIDTH, MIN_NATIVE_TEXT_CHARS, MIN_RENDER_WIDTH,
        PAGE_TURN_MAX_SECONDS, PAGE_TURN_MIN_SECONDS, PAGE_TURN_REFERENCE_DISTANCE, SPEED_LEVELS,
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
    ("⌘K", "Crop / restore empty page margins"),
    ("P", "Toggle continuous scroll / timed page turns"),
    ("K", "Play / pause pacer"),
    ("J", "Slower pacer"),
    ("L", "Faster pacer"),
    ("Space", "Next page / page group"),
    ("Shift + Space", "Top of current page/group, then previous"),
    ("+", "Zoom in"),
    ("−", "Zoom out"),
    ("O / ⌘O", "Open PDF"),
    ("⌘T", "New empty tab"),
    ("⌘W", "Close current tab"),
    ("⌘⇧Tab / ⌘⇧T", "Reopen previously closed tab"),
    ("0", "Reset zoom to 100%"),
    ("Pinch / Ctrl-scroll", "Zoom around the pointer"),
    ("1", "Fit page width"),
    ("2", "Fit page height"),
    ("3", "2 pages (2×1)"),
    ("4", "2 rows, automatic columns"),
    ("5", "3 rows, automatic columns"),
    ("6", "4 rows, automatic columns"),
    ("7", "5 rows, automatic columns"),
    ("8", "7 rows, automatic columns"),
    ("9", "Overview: fit the whole PDF"),
    ("⌘1–⌘8", "Switch to tab 1–8"),
    ("⌘9", "Switch to the last tab"),
    ("F", "Toggle fullscreen"),
    ("Ctrl/⌘ C", "Copy selected PDF text"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PacerMode {
    Continuous,
    PageTurn,
}

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


/// Per-tab state that is inexpensive enough to keep around while another tab is
/// active. Page bitmaps are deliberately not retained for inactive tabs; PDFium
/// keeps the parsed documents open and visible pages are rendered again on demand.
struct TabState {
    document: Option<DocumentInfo>,
    view_mode: ViewMode,
    manual_zoom: f32,
    invert: bool,
    crop_enabled: bool,
    crops: Vec<Option<PageCrop>>,
    scroll_x: f32,
    scroll_y: f32,
    layout: DocumentLayout,
    layout_dirty: bool,
    pending_anchor: Option<ScrollAnchor>,
    pending_goto: Option<usize>,
    current_page: usize,
    native_text: Vec<Option<PageTextData>>,
    search_text: Vec<Option<String>>,
    ocr_done: HashSet<usize>,
    ocr_available: bool,
    search_open: bool,
    search_query: String,
    search_results: Vec<SearchHit>,
    search_result_index: Option<usize>,
    search_index_cursor: usize,
    selection_anchor: Option<SelectionPoint>,
    selection_focus: Option<SelectionPoint>,
    status: String,
}

struct TabSlot {
    doc_id: u64,
    path: Option<PathBuf>,
    title: String,
    state: Option<TabState>,
}

pub struct KvikkApp {
    backend: PdfBackend,
    doc_id: u64,
    next_doc_id: u64,
    tabs: Vec<TabSlot>,
    closed_tabs: Vec<TabSlot>,
    active_tab: Option<usize>,
    document: Option<DocumentInfo>,
    view_mode: ViewMode,
    manual_zoom: f32,
    invert: bool,
    fullscreen: bool,
    show_menu: bool,
    is_playing: bool,
    pacer_mode: PacerMode,
    page_turn_elapsed: f32,
    speed_index: usize,
    crop_enabled: bool,
    crops: Vec<Option<PageCrop>>,
    crop_requested: HashSet<usize>,
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

    link_probe_id: u64,
    link_probe_signature: Option<(usize, i32, i32)>,
    hover_link_target: Option<LinkTarget>,

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

        crate::platform::register_context(&cc.egui_ctx);
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
            next_doc_id: 0,
            tabs: Vec::new(),
            closed_tabs: Vec::new(),
            active_tab: None,
            document: None,
            view_mode: ViewMode::FitWidth,
            manual_zoom: 1.0,
            invert: false,
            fullscreen: false,
            show_menu: true,
            is_playing: false,
            pacer_mode: PacerMode::Continuous,
            page_turn_elapsed: 0.0,
            speed_index,
            crop_enabled: false,
            crops: Vec::new(),
            crop_requested: HashSet::new(),
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
            link_probe_id: 0,
            link_probe_signature: None,
            hover_link_target: None,
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

    fn page_turn_seconds(&self) -> f32 {
        (PAGE_TURN_REFERENCE_DISTANCE / self.speed().max(0.1))
            .clamp(PAGE_TURN_MIN_SECONDS, PAGE_TURN_MAX_SECONDS)
    }

    fn invalidate_render_requests(&mut self, debounce_frames: u8) {
        self.render_generation = self.backend.bump_render_generation();
        self.render_in_flight.clear();
        self.render_failed.clear();
        self.render_debounce_frames = debounce_frames;
    }

    fn active_tab_title(&self) -> Option<&str> {
        self.active_tab
            .and_then(|index| self.tabs.get(index))
            .map(|tab| tab.title.as_str())
    }

    fn update_window_title(&self, ctx: &egui::Context) {
        let title = self
            .active_tab_title()
            .filter(|title| !title.trim().is_empty())
            .unwrap_or("kvikk pdf");
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(if title == "kvikk pdf" {
            "kvikk pdf".into()
        } else {
            format!("{title} — kvikk pdf")
        }));
    }

    fn capture_current_tab_state(&mut self) -> TabState {
        // Requests already sent to PDFium may still finish after the tab becomes
        // inactive. We intentionally forget their in-flight bookkeeping so the
        // page can simply be requested again if/when this tab becomes active.
        self.bitmaps.clear();
        self.render_in_flight.clear();
        self.render_failed.clear();
        self.crop_requested.clear();
        self.text_requested.clear();
        self.ocr_queued.clear();
        self.ocr_queued_set.clear();
        self.ocr_in_flight.clear();
        self.goto_active = false;
        self.goto_buffer.clear();
        self.goto_deadline = None;
        self.search_focus_requested = false;
        self.link_probe_signature = None;
        self.hover_link_target = None;

        TabState {
            document: self.document.take(),
            view_mode: self.view_mode,
            manual_zoom: self.manual_zoom,
            invert: self.invert,
            crop_enabled: self.crop_enabled,
            crops: std::mem::take(&mut self.crops),
            scroll_x: self.scroll_x,
            scroll_y: self.scroll_y,
            layout: std::mem::take(&mut self.layout),
            layout_dirty: self.layout_dirty,
            pending_anchor: self.pending_anchor.take(),
            pending_goto: self.pending_goto.take(),
            current_page: self.current_page,
            native_text: std::mem::take(&mut self.native_text),
            search_text: std::mem::take(&mut self.search_text),
            ocr_done: std::mem::take(&mut self.ocr_done),
            ocr_available: self.ocr_available,
            search_open: self.search_open,
            search_query: std::mem::take(&mut self.search_query),
            search_results: std::mem::take(&mut self.search_results),
            search_result_index: self.search_result_index.take(),
            search_index_cursor: self.search_index_cursor,
            selection_anchor: self.selection_anchor.take(),
            selection_focus: self.selection_focus.take(),
            status: std::mem::take(&mut self.status),
        }
    }

    fn save_active_tab(&mut self) {
        let Some(index) = self.active_tab else { return };
        let state = self.capture_current_tab_state();
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.state = Some(state);
        }
    }

    fn restore_tab_state(&mut self, state: TabState) {
        self.document = state.document;
        self.view_mode = state.view_mode;
        self.manual_zoom = state.manual_zoom;
        self.invert = state.invert;
        self.crop_enabled = state.crop_enabled;
        self.crops = state.crops;
        self.crop_requested.clear();
        self.scroll_x = state.scroll_x;
        self.scroll_y = state.scroll_y;
        self.layout = state.layout;
        self.layout_dirty = state.layout_dirty;
        self.pending_anchor = state.pending_anchor;
        self.pending_goto = state.pending_goto;
        self.current_page = state.current_page;
        self.native_text = state.native_text;
        self.search_text = state.search_text;
        self.ocr_done = state.ocr_done;
        self.ocr_available = state.ocr_available;
        self.search_open = state.search_open;
        self.search_query = state.search_query;
        self.search_results = state.search_results;
        self.search_result_index = state.search_result_index;
        self.search_index_cursor = state.search_index_cursor;
        self.selection_anchor = state.selection_anchor;
        self.selection_focus = state.selection_focus;
        self.status = state.status;

        self.bitmaps.clear();
        self.render_in_flight.clear();
        self.render_failed.clear();
        self.text_requested.clear();
        self.ocr_queued.clear();
        self.ocr_queued_set.clear();
        self.ocr_in_flight.clear();
        self.search_focus_requested = false;
        self.goto_active = false;
        self.goto_buffer.clear();
        self.goto_deadline = None;
        self.link_probe_signature = None;
        self.hover_link_target = None;
        self.is_playing = false;
        self.page_turn_elapsed = 0.0;
        self.render_generation = self.backend.bump_render_generation();
        self.render_debounce_frames = 0;
    }

    fn reset_current_for_new_tab(&mut self, doc_id: u64, path: &Path) {
        self.doc_id = doc_id;
        self.render_generation = self.backend.bump_render_generation();
        self.document = None;
        self.bitmaps.clear();
        self.render_in_flight.clear();
        self.render_failed.clear();
        self.crop_enabled = false;
        self.crops.clear();
        self.crop_requested.clear();
        self.native_text.clear();
        self.search_text.clear();
        self.text_requested.clear();
        self.ocr_queued.clear();
        self.ocr_queued_set.clear();
        self.ocr_in_flight.clear();
        self.ocr_done.clear();
        self.ocr_available = true;
        self.pending_anchor = None;
        self.pending_goto = None;
        self.search_open = false;
        self.search_focus_requested = false;
        self.search_query.clear();
        self.search_results.clear();
        self.search_result_index = None;
        self.search_index_cursor = 0;
        self.selection_anchor = None;
        self.selection_focus = None;
        self.goto_active = false;
        self.goto_buffer.clear();
        self.goto_deadline = None;
        self.link_probe_signature = None;
        self.hover_link_target = None;
        self.scroll_x = 0.0;
        self.scroll_y = 0.0;
        self.current_page = 0;
        self.is_playing = false;
        self.page_turn_elapsed = 0.0;
        self.view_mode = ViewMode::FitWidth;
        self.manual_zoom = 1.0;
        self.layout = DocumentLayout::default();
        self.layout_dirty = true;
        self.render_debounce_frames = 0;
        self.status = format!(
            "Opening {}…",
            path.file_name().and_then(|s| s.to_str()).unwrap_or("PDF")
        );
    }

    fn reset_current_blank(&mut self) {
        self.doc_id = 0;
        self.render_generation = self.backend.bump_render_generation();
        self.document = None;
        self.bitmaps.clear();
        self.render_in_flight.clear();
        self.render_failed.clear();
        self.crop_enabled = false;
        self.crops.clear();
        self.crop_requested.clear();
        self.native_text.clear();
        self.search_text.clear();
        self.text_requested.clear();
        self.ocr_queued.clear();
        self.ocr_queued_set.clear();
        self.ocr_in_flight.clear();
        self.ocr_done.clear();
        self.ocr_available = true;
        self.pending_anchor = None;
        self.pending_goto = None;
        self.search_open = false;
        self.search_focus_requested = false;
        self.search_query.clear();
        self.search_results.clear();
        self.search_result_index = None;
        self.search_index_cursor = 0;
        self.selection_anchor = None;
        self.selection_focus = None;
        self.goto_active = false;
        self.goto_buffer.clear();
        self.goto_deadline = None;
        self.link_probe_signature = None;
        self.hover_link_target = None;
        self.scroll_x = 0.0;
        self.scroll_y = 0.0;
        self.current_page = 0;
        self.is_playing = false;
        self.page_turn_elapsed = 0.0;
        self.view_mode = ViewMode::FitWidth;
        self.manual_zoom = 1.0;
        self.layout = DocumentLayout::default();
        self.layout_dirty = true;
        self.render_debounce_frames = 0;
        self.status = "Drop a PDF here or press O to open one.".into();
    }

    fn open_path(&mut self, path: PathBuf) {
        if !is_pdf(&path) {
            self.status = "That file does not look like a PDF.".into();
            return;
        }

        self.next_doc_id = self.next_doc_id.wrapping_add(1).max(1);
        let doc_id = self.next_doc_id;
        let fallback_title = path
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("PDF")
            .to_owned();

        // Opening a PDF into a deliberately-created empty tab should fill that tab
        // instead of leaving a useless blank tab behind. All other opens create a tab.
        let replace_blank = self
            .active_tab
            .filter(|&index| {
                self.tabs
                    .get(index)
                    .is_some_and(|tab| tab.doc_id == 0 && tab.path.is_none())
            });

        if let Some(index) = replace_blank {
            if let Some(tab) = self.tabs.get_mut(index) {
                tab.doc_id = doc_id;
                tab.path = Some(path.clone());
                tab.title = fallback_title;
                tab.state = None;
            }
        } else {
            self.save_active_tab();
            let index = self.tabs.len();
            self.tabs.push(TabSlot {
                doc_id,
                path: Some(path.clone()),
                title: fallback_title,
                state: None,
            });
            self.active_tab = Some(index);
        }

        self.reset_current_for_new_tab(doc_id, &path);
        self.backend.high(BackendCommand::Open { doc_id, path });
    }

    fn open_pdf_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new().add_filter("PDF", &["pdf"]).pick_file() {
            self.open_path(path);
        }
    }

    fn new_tab(&mut self, ctx: &egui::Context) {
        self.save_active_tab();
        let index = self.tabs.len();
        self.tabs.push(TabSlot {
            doc_id: 0,
            path: None,
            title: "New Tab".into(),
            state: None,
        });
        self.active_tab = Some(index);
        self.reset_current_blank();
        self.update_window_title(ctx);
        ctx.request_repaint();
    }

    fn close_current_tab(&mut self, ctx: &egui::Context) {
        let Some(index) = self.active_tab else { return };
        if index >= self.tabs.len() {
            return;
        }

        // Capture the active reader state before removing the slot. Closed PDF tabs
        // remain resident in PDFium for a small undo history, making reopen instant.
        self.save_active_tab();
        let closed = self.tabs.remove(index);
        if closed.doc_id != 0 {
            self.closed_tabs.push(closed);
            const CLOSED_TAB_HISTORY: usize = 12;
            if self.closed_tabs.len() > CLOSED_TAB_HISTORY {
                let forgotten = self.closed_tabs.remove(0);
                if forgotten.doc_id != 0 {
                    self.backend.high(BackendCommand::Close { doc_id: forgotten.doc_id });
                }
            }
        }

        if self.tabs.is_empty() {
            self.active_tab = None;
            self.reset_current_blank();
        } else {
            let next_index = index.min(self.tabs.len() - 1);
            let doc_id = self.tabs[next_index].doc_id;
            let state = self.tabs[next_index].state.take();
            self.active_tab = Some(next_index);
            self.doc_id = doc_id;
            if let Some(state) = state {
                self.restore_tab_state(state);
            } else {
                // This can only be a freshly-created empty tab.
                self.reset_current_blank();
            }
        }

        self.update_window_title(ctx);
        ctx.request_repaint();
    }

    fn reopen_closed_tab(&mut self, ctx: &egui::Context) {
        let Some(mut tab) = self.closed_tabs.pop() else { return };
        self.save_active_tab();

        let doc_id = tab.doc_id;
        let state = tab.state.take();
        let index = self.tabs.len();
        self.tabs.push(tab);
        self.active_tab = Some(index);
        self.doc_id = doc_id;

        if let Some(state) = state {
            self.restore_tab_state(state);
        } else if let Some(path) = self.tabs[index].path.clone() {
            // Defensive fallback for a tab closed during its initial open.
            self.reset_current_for_new_tab(doc_id, &path);
            self.backend.high(BackendCommand::Open { doc_id, path });
        } else {
            self.reset_current_blank();
        }

        self.update_window_title(ctx);
        ctx.request_repaint();
    }

    fn switch_tab(&mut self, ctx: &egui::Context, index: usize) {
        if self.active_tab == Some(index) || index >= self.tabs.len() {
            return;
        }

        self.save_active_tab();
        let doc_id = self.tabs[index].doc_id;
        let Some(state) = self.tabs[index].state.take() else {
            return;
        };
        self.active_tab = Some(index);
        self.doc_id = doc_id;
        self.restore_tab_state(state);
        self.update_window_title(ctx);
        ctx.request_repaint();
    }

    fn switch_tab_shortcut(&mut self, ctx: &egui::Context, number: usize) {
        if self.tabs.is_empty() {
            return;
        }
        let index = if number == 9 {
            self.tabs.len() - 1
        } else {
            number.saturating_sub(1)
        };
        if index < self.tabs.len() {
            self.switch_tab(ctx, index);
        }
    }


    fn poll_backend(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.backend.events.try_recv() {
            match event {
                BackendEvent::Opened { doc_id, info } => {
                    let count = info.pages.len();
                    if doc_id == self.doc_id {
                        if let Some(index) = self.active_tab {
                            if let Some(tab) = self.tabs.get_mut(index) {
                                tab.title = info.title.clone();
                            }
                        }
                        self.native_text = vec![None; count];
                        self.search_text = vec![None; count];
                        self.crops = vec![None; count];
                        self.crop_requested.clear();
                        self.current_page = 0;
                        self.scroll_x = 0.0;
                        self.scroll_y = 0.0;
                        self.layout_dirty = true;
                        self.status.clear();
                        self.document = Some(info);
                        self.update_window_title(ctx);
                    } else if let Some(tab) = self
                        .tabs
                        .iter_mut()
                        .chain(self.closed_tabs.iter_mut())
                        .find(|tab| tab.doc_id == doc_id)
                    {
                        tab.title = info.title.clone();
                        if let Some(state) = tab.state.as_mut() {
                            state.native_text = vec![None; count];
                            state.search_text = vec![None; count];
                            state.crops = vec![None; count];
                            state.current_page = 0;
                            state.scroll_x = 0.0;
                            state.scroll_y = 0.0;
                            state.layout_dirty = true;
                            state.status.clear();
                            state.document = Some(info);
                        }
                    }
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
                BackendEvent::OcrUnavailable { doc_id, message } if doc_id == 0 || doc_id == self.doc_id => {
                    self.ocr_available = false;
                    self.status = message;
                    self.ocr_queued.clear();
                    self.ocr_queued_set.clear();
                    self.ocr_in_flight.clear();
                }
                BackendEvent::CropReady { doc_id, page, crop } if doc_id == self.doc_id => {
                    self.crop_requested.remove(&page);
                    if page < self.crops.len() && self.crops[page] != Some(crop) {
                        if self.crop_enabled && self.last_viewport != Vec2::ZERO {
                            let viewport = Rect::from_min_size(Pos2::ZERO, self.last_viewport);
                            if self.view_mode.is_grid() {
                                self.pending_goto = Some(self.current_page);
                            } else if self.pending_anchor.is_none() {
                                self.pending_anchor = self.capture_anchor(viewport, None);
                            }
                        }
                        self.crops[page] = Some(crop);
                        if self.crop_enabled {
                            self.layout_dirty = true;
                        }
                        ctx.request_repaint();
                    }
                }
                BackendEvent::CropReady { doc_id, page, crop } => {
                    if let Some(tab) = self
                        .tabs
                        .iter_mut()
                        .chain(self.closed_tabs.iter_mut())
                        .find(|tab| tab.doc_id == doc_id)
                    {
                        if let Some(state) = tab.state.as_mut() {
                            if page < state.crops.len() {
                                state.crops[page] = Some(crop);
                                if state.crop_enabled {
                                    state.pending_goto = Some(state.current_page);
                                    state.layout_dirty = true;
                                }
                            }
                        }
                    }
                }
                BackendEvent::LinkResolved { doc_id, target } if doc_id == self.doc_id => {
                    self.follow_link_target(ctx, target);
                }
                BackendEvent::LinkProbed { doc_id, probe_id, target }
                    if doc_id == self.doc_id && probe_id == self.link_probe_id =>
                {
                    self.hover_link_target = target;
                    ctx.request_repaint();
                }
                BackendEvent::Error { doc_id, message } if doc_id == 0 || doc_id == self.doc_id => {
                    self.status = message;
                }
                BackendEvent::Error { doc_id, message } => {
                    if let Some(tab) = self
                        .tabs
                        .iter_mut()
                        .chain(self.closed_tabs.iter_mut())
                        .find(|tab| tab.doc_id == doc_id)
                    {
                        if let Some(state) = tab.state.as_mut() {
                            state.status = message;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn follow_link_target(&mut self, ctx: &egui::Context, target: LinkTarget) {
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

    fn queue_crop(&mut self, page: usize, high_priority: bool) {
        if !self.crop_enabled
            || page >= self.page_count()
            || self.crops.get(page).and_then(|crop| *crop).is_some()
            || !self.crop_requested.insert(page)
        {
            return;
        }
        let cmd = BackendCommand::AnalyzeCrop { doc_id: self.doc_id, page };
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
        self.page_turn_elapsed = 0.0;

        if let Some(viewport) = viewport {
            if mode.is_grid() {
                // The column count for modes 4–8 depends on the new viewport layout,
                // so preserve the actual reading page now and canonicalize it only
                // after the new layout has been built. This prevents a mode switch
                // from accidentally advancing to a neighboring page group.
                let page = self
                    .capture_anchor(viewport, None)
                    .map(|anchor| anchor.page)
                    .unwrap_or(self.current_page);
                self.pending_anchor = None;
                self.pending_goto = Some(page);
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
        self.page_turn_elapsed = 0.0;
        if count == 0 {
            return;
        }
        let page = page.min(count - 1);

        // When a layout change is pending, its group size may not exist yet
        // (modes 4–8 calculate columns from the viewport), so defer canonicalization.
        if self.layout_dirty {
            self.current_page = page;
            self.pending_goto = Some(page);
            return;
        }

        let canonical = self.layout.canonical_page(page);
        if let Some(row) = self.layout.row_for_page(canonical) {
            self.scroll_y = row.y;
            self.current_page = canonical;
            self.pending_goto = None;
        } else {
            self.current_page = page;
            self.pending_goto = Some(page);
            self.layout_dirty = true;
        }
    }

    fn next_page(&mut self, backwards: bool) {
        self.page_turn_elapsed = 0.0;
        let count = self.page_count();
        if count == 0 {
            return;
        }

        let current = self
            .layout
            .canonical_page_at_y(self.scroll_y + 1.0)
            .min(count.saturating_sub(1));
        let group_top = self
            .layout
            .row_for_page(current)
            .map(|row| {
                row.pages
                    .iter()
                    .map(|page| page.y)
                    .reduce(f32::min)
                    .unwrap_or(row.y)
            })
            .unwrap_or(0.0);

        if backwards {
            // Shift+Space first reveals the top of the current page/page group when
            // it has scrolled out of view. Only a second press from the top moves to
            // the previous page/group. This mirrors how readers naturally backtrack.
            if self.scroll_y > group_top + 2.0 {
                self.scroll_y = group_top;
                self.current_page = current;
                self.pending_goto = None;
                return;
            }
            if self.view_mode == ViewMode::Overview {
                return;
            }
            let step = self.layout.pages_per_group.max(1);
            let target = current.saturating_sub(step);
            if target < current {
                self.goto_page(target);
            }
        } else {
            if self.view_mode == ViewMode::Overview {
                return;
            }
            let step = self.layout.pages_per_group.max(1);
            let target = (current + step).min(count - 1);
            if self.layout.canonical_page(target) != current {
                self.goto_page(target);
            }
        }
    }

    fn speed_slower(&mut self) {
        self.speed_index = self.speed_index.saturating_sub(1);
        self.page_turn_elapsed = 0.0;
    }

    fn speed_faster(&mut self) {
        self.speed_index = (self.speed_index + 1).min(SPEED_LEVELS.len() - 1);
        self.page_turn_elapsed = 0.0;
    }

    fn toggle_crop(&mut self, viewport: Option<Rect>) {
        if self.document.is_none() {
            return;
        }

        if let Some(viewport) = viewport {
            if self.view_mode.is_grid() {
                self.pending_goto = Some(self.current_page);
            } else if self.pending_anchor.is_none() {
                self.pending_anchor = self.capture_anchor(viewport, None);
            }
        }

        self.crop_enabled = !self.crop_enabled;
        self.layout_dirty = true;
        self.page_turn_elapsed = 0.0;
        if self.crop_enabled {
            self.queue_crop(self.current_page, true);
            let next = self.current_page.saturating_add(1);
            if next < self.page_count() {
                self.queue_crop(next, true);
            }
        }
    }

    fn toggle_pacer_mode(&mut self) {
        self.pacer_mode = match self.pacer_mode {
            PacerMode::Continuous => PacerMode::PageTurn,
            PacerMode::PageTurn => PacerMode::Continuous,
        };
        self.page_turn_elapsed = 0.0;
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
        for path in dropped
            .into_iter()
            .map(|file| file.path().to_path_buf())
            .filter(|path| is_pdf(path))
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

        if ctx.input(|i| {
            i.modifiers.command
                && i.modifiers.shift
                && (i.key_pressed(Key::Tab) || i.key_pressed(Key::T))
        }) {
            self.reopen_closed_tab(ctx);
            return;
        }

        if ctx.input(|i| i.modifiers.command && !i.modifiers.shift && i.key_pressed(Key::T)) {
            self.new_tab(ctx);
            return;
        }

        if ctx.input(|i| i.modifiers.command && i.key_pressed(Key::W)) {
            self.close_current_tab(ctx);
            return;
        }

        let command_tab = ctx.input(|i| {
            if !i.modifiers.command {
                return None;
            }
            [
                (Key::Num1, 1usize),
                (Key::Num2, 2),
                (Key::Num3, 3),
                (Key::Num4, 4),
                (Key::Num5, 5),
                (Key::Num6, 6),
                (Key::Num7, 7),
                (Key::Num8, 8),
                (Key::Num9, 9),
            ]
            .into_iter()
            .find_map(|(key, number)| i.key_pressed(key).then_some(number))
        });
        if let Some(number) = command_tab {
            self.switch_tab_shortcut(ctx, number);
            return;
        }

        if ctx.input(|i| i.modifiers.command && i.key_pressed(Key::K)) {
            self.toggle_crop(viewport);
            return;
        }

        if ctx.input(|i| i.modifiers.command && i.key_pressed(Key::O)) {
            self.open_pdf_dialog();
            return;
        }

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
                            'p' => self.toggle_pacer_mode(),
                            'k' => {
                                self.is_playing = !self.is_playing;
                                if self.is_playing {
                                    self.page_turn_elapsed = 0.0;
                                }
                            },
                            'j' => self.speed_slower(),
                            'l' => self.speed_faster(),
                            'f' => self.toggle_fullscreen(ctx),
                            'o' => self.open_pdf_dialog(),
                            '0' => self.reset_zoom(viewport),
                            '1' => self.set_mode(ViewMode::FitWidth, viewport),
                            '2' => self.set_mode(ViewMode::FitHeight, viewport),
                            '3' => self.set_mode(ViewMode::Spread, viewport),
                            '4' => self.set_mode(ViewMode::Rows2, viewport),
                            '5' => self.set_mode(ViewMode::Rows3, viewport),
                            '6' => self.set_mode(ViewMode::Rows4, viewport),
                            '7' => self.set_mode(ViewMode::Rows5, viewport),
                            '8' => self.set_mode(ViewMode::Rows7, viewport),
                            '9' => self.set_mode(ViewMode::Overview, viewport),
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
                let old_viewport = Rect::from_min_size(viewport.min, self.last_viewport);
                if self.view_mode.is_grid() {
                    // Dynamic-column grids can change group size when the window width
                    // changes. Preserve the reading page, then align the new containing
                    // group after reflow instead of anchoring to an obsolete cell.
                    let page = self
                        .capture_anchor(old_viewport, None)
                        .map(|anchor| anchor.page)
                        .unwrap_or(self.current_page);
                    self.pending_goto = Some(page);
                } else {
                    self.pending_anchor = self.capture_anchor(old_viewport, None);
                }
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
                &self.crops,
                self.crop_enabled,
                self.view_mode,
                self.manual_zoom,
                viewport.width(),
                viewport.height(),
            );
            self.layout_dirty = false;
            if let Some(anchor) = self.pending_anchor.take() {
                self.apply_anchor(anchor, viewport);
            } else if let Some(page) = self.pending_goto.take() {
                let canonical = self.layout.canonical_page(page);
                if let Some(row) = self.layout.row_for_page(canonical) {
                    self.scroll_y = row.y;
                    self.current_page = canonical;
                }
            }
            self.clamp_scroll(viewport.size());
        }
    }

    fn dense_grid(&self) -> bool {
        self.view_mode == ViewMode::Overview || self.layout.pages_per_group >= 40
    }

    fn desired_render_width(&self, displayed_width: f32, pixels_per_point: f32) -> u32 {
        let raw = (displayed_width * pixels_per_point * 1.08).ceil() as u32;
        let dense = self.dense_grid();
        let quantum = if dense { 32 } else { 64 };
        let quantized = ((raw + quantum - 1) / quantum) * quantum;
        let minimum = if dense { 32 } else { MIN_RENDER_WIDTH };
        quantized.clamp(minimum, MAX_RENDER_WIDTH)
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
        let command = BackendCommand::Render {
            doc_id: self.doc_id,
            page,
            pixel_width,
            generation: self.render_generation,
        };
        if self.dense_grid() {
            self.backend.low(command);
        } else {
            self.backend.high(command);
        }
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

    fn paint_page_turn_indicator(&self, painter: &egui::Painter, viewport: Rect) {
        let duration = self.page_turn_seconds().max(0.001);
        let remaining = (1.0 - self.page_turn_elapsed / duration).clamp(0.0, 1.0);
        let width = 112.0;
        let height = 8.0;
        let margin = 16.0;
        let min = Pos2::new(viewport.right() - width - margin, viewport.bottom() - 34.0);
        let bar = Rect::from_min_size(min, Vec2::new(width, height));
        painter.rect_filled(bar, 4.0, Color32::from_rgba_unmultiplied(255, 255, 255, 42));
        let fill = Rect::from_min_size(bar.min, Vec2::new(width * remaining, height));
        painter.rect_filled(fill, 4.0, Color32::from_rgba_unmultiplied(255, 255, 255, 190));
        let remaining_seconds = (duration - self.page_turn_elapsed).max(0.0);
        let label = if self.is_playing {
            format!("{}", pretty_duration(remaining_seconds))
        } else {
            format!("paused · {}", pretty_duration(remaining_seconds))
        };
        painter.text(
            Pos2::new(bar.center().x, bar.top() - 4.0),
            egui::Align2::CENTER_BOTTOM,
            label,
            egui::FontId::monospace(12.0),
            Color32::from_gray(220),
        );
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
                "Drop a PDF here\n\nO = open PDF · 0 = 100% zoom · ? = commands",
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
                if self.pacer_mode == PacerMode::PageTurn {
                    self.page_turn_elapsed = 0.0;
                }
            }
        }

        if self.is_playing {
            let dt = ui.ctx().input(|i| i.stable_dt).clamp(0.0, 0.1);
            match self.pacer_mode {
                PacerMode::Continuous => {
                    self.scroll_y += self.speed() * dt;
                }
                PacerMode::PageTurn => {
                    let duration = self.page_turn_seconds();
                    self.page_turn_elapsed += dt;
                    if self.page_turn_elapsed >= duration {
                        self.page_turn_elapsed = 0.0;
                        let before_y = self.scroll_y;
                        let before_page = self.current_page;
                        self.next_page(false);
                        if (self.scroll_y - before_y).abs() < 0.01 && self.current_page == before_page {
                            self.is_playing = false;
                        }
                    }
                }
            }
            ui.ctx().request_repaint();
        }
        self.clamp_scroll(viewport.size());
        if self.is_playing && self.pacer_mode == PacerMode::Continuous {
            let max_y = (self.layout.content_height - viewport.height()).max(0.0);
            if self.scroll_y >= max_y - 0.01 {
                self.is_playing = false;
            }
        }

        self.current_page = self
            .layout
            .canonical_page_at_y(self.scroll_y + 12.0)
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

            if self.crop_enabled {
                let high_priority_crop = !self.dense_grid()
                    && placed.page == self.current_page
                    && screen_rect.intersects(viewport.expand(4.0));
                self.queue_crop(placed.page, high_priority_crop);
            }

            if screen_rect.intersects(viewport.expand(4.0)) {
                let paper = if self.invert { Color32::BLACK } else { Color32::WHITE };
                painter.rect_filled(screen_rect, 1.0, paper);
                let source_display_width = placed.w / placed.crop.width();
                let desired = self.desired_render_width(source_display_width, pixels_per_point);
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
                            Rect::from_min_max(
                                Pos2::new(placed.crop.left, placed.crop.top),
                                Pos2::new(placed.crop.right, placed.crop.bottom),
                            ),
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

                if !self.dense_grid() || self.search_open {
                    self.queue_text(placed.page, false);
                }
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

        self.handle_link_hover(ui, &response, &screen_pages);
        self.handle_selection_input(ui, &response, viewport, &screen_pages);
        self.handle_link_click(ui, &response, &screen_pages);
        self.prune_bitmap_cache(&visible_pages);

        if self.pacer_mode == PacerMode::PageTurn {
            self.paint_page_turn_indicator(&painter, viewport);
        }

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

    fn handle_link_hover(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        screen_pages: &[(PlacedPage, Rect)],
    ) {
        if !response.hovered() {
            self.link_probe_signature = None;
            self.hover_link_target = None;
            return;
        }

        let Some(pos) = ui.ctx().input(|i| i.pointer.hover_pos()) else {
            self.link_probe_signature = None;
            self.hover_link_target = None;
            return;
        };
        let Some(document) = self.document.as_ref() else { return };
        let Some((placed, rect)) = screen_pages.iter().find(|(_, rect)| rect.contains(pos)) else {
            self.link_probe_signature = None;
            self.hover_link_target = None;
            return;
        };

        let metric = document.pages[placed.page];
        let (x_pt, y_pt) = screen_point_to_pdf(pos, *rect, metric.width_pt, metric.height_pt, placed.crop);
        // Quantizing to roughly one PDF point prevents a flood of worker messages while
        // still making link hover feel immediate at normal reading scales.
        let signature = (placed.page, x_pt.round() as i32, y_pt.round() as i32);

        if self.link_probe_signature != Some(signature) {
            self.link_probe_signature = Some(signature);
            self.hover_link_target = None;
            self.link_probe_id = self.link_probe_id.wrapping_add(1).max(1);
            self.backend.high(BackendCommand::ProbeLink {
                doc_id: self.doc_id,
                probe_id: self.link_probe_id,
                page: placed.page,
                x_pt,
                y_pt,
            });
        }

        if let Some(target) = &self.hover_link_target {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            let text = match target {
                LinkTarget::Page(page) => format!("Go to page {}", page + 1),
                LinkTarget::Url(url) => url.clone(),
            };
            let _ = response.clone().on_hover_text_at_pointer(text);
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
        let (x_pt, y_pt) = screen_point_to_pdf(pos, *rect, metric.width_pt, metric.height_pt, placed.crop);
        let signature = (placed.page, x_pt.round() as i32, y_pt.round() as i32);

        if self.link_probe_signature == Some(signature) {
            if let Some(target) = self.hover_link_target.clone() {
                self.follow_link_target(ui.ctx(), target);
                return;
            }
        }

        // If the hover probe has not returned yet, resolve the click directly. This
        // keeps fast clicks reliable even when PDFium is busy rendering another page.
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
        let (x_pt, y_pt) = screen_point_to_pdf(pos, *screen_rect, metric.width_pt, metric.height_pt, placed.crop);

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
            if let Some(rect) = pdf_bounds_to_screen(
                bounds,
                metric.width_pt,
                metric.height_pt,
                placed.crop,
                screen_rect,
            ) {
                painter.rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(60, 120, 255, 78));
            }
        }
    }

    fn tabs_bar(&mut self, ui: &mut egui::Ui) {
        if self.tabs.is_empty() {
            return;
        }

        let active = self.active_tab;
        let mut clicked = None;
        let mut create_new = false;
        let mut reopen_closed = false;
        egui::Panel::top("tabs")
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_gray(12))
                    .inner_margin(egui::Margin::symmetric(6, 3)),
            )
            .show(ui, |ui| {
                egui::ScrollArea::horizontal()
                    .id_salt("pdf-tabs-scroll")
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for (index, tab) in self.tabs.iter().enumerate() {
                                let response = ui.selectable_label(
                                    active == Some(index),
                                    tab.title.as_str(),
                                );
                                let response = if let Some(path) = tab.path.as_ref() {
                                    response.on_hover_text(path.display().to_string())
                                } else {
                                    response.on_hover_text("Empty tab")
                                };
                                if response.clicked() {
                                    clicked = Some(index);
                                }
                            }
                            if ui.small_button("+").on_hover_text("New tab (⌘T)").clicked() {
                                create_new = true;
                            }
                            if !self.closed_tabs.is_empty()
                                && ui
                                    .small_button("↶")
                                    .on_hover_text("Reopen closed tab (⌘⇧T)")
                                    .clicked()
                            {
                                reopen_closed = true;
                            }
                        });
                    });
            });

        let ctx = ui.ctx().clone();
        if let Some(index) = clicked {
            self.switch_tab(&ctx, index);
        } else if create_new {
            self.new_tab(&ctx);
        } else if reopen_closed {
            self.reopen_closed_tab(&ctx);
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, viewport_hint: Option<Rect>) {
        egui::Panel::top("toolbar")
            .frame(egui::Frame::new().fill(Color32::from_gray(18)))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.button("O Open PDF").clicked() {
                        self.open_pdf_dialog();
                    }
                    ui.separator();
                    if ui.button(if self.is_playing { "K Pause" } else { "K Play" }).clicked() {
                        self.is_playing = !self.is_playing;
                        if self.is_playing {
                            self.page_turn_elapsed = 0.0;
                        }
                    }
                    let pacer_label = match self.pacer_mode {
                        PacerMode::Continuous => "P Scroll",
                        PacerMode::PageTurn => "P Pages",
                    };
                    if ui.selectable_label(self.pacer_mode == PacerMode::PageTurn, pacer_label).clicked() {
                        self.toggle_pacer_mode();
                    }
                    if ui.small_button("J slower").clicked() {
                        self.speed_slower();
                    }
                    match self.pacer_mode {
                        PacerMode::Continuous => {
                            ui.monospace(format!("{} px/s", pretty_speed(self.speed())));
                        }
                        PacerMode::PageTurn => {
                            ui.monospace(format!("{} / page", pretty_duration(self.page_turn_seconds())));
                        }
                    }
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
                    if ui.selectable_label(self.view_mode == ViewMode::Rows2, "4 2 rows").clicked() {
                        self.set_mode(ViewMode::Rows2, viewport_hint);
                    }
                    if ui.selectable_label(self.view_mode == ViewMode::Rows3, "5 3 rows").clicked() {
                        self.set_mode(ViewMode::Rows3, viewport_hint);
                    }
                    if ui.selectable_label(self.view_mode == ViewMode::Rows4, "6 4 rows").clicked() {
                        self.set_mode(ViewMode::Rows4, viewport_hint);
                    }
                    if ui.selectable_label(self.view_mode == ViewMode::Rows5, "7 5 rows").clicked() {
                        self.set_mode(ViewMode::Rows5, viewport_hint);
                    }
                    if ui.selectable_label(self.view_mode == ViewMode::Rows7, "8 7 rows").clicked() {
                        self.set_mode(ViewMode::Rows7, viewport_hint);
                    }
                    if ui.selectable_label(self.view_mode == ViewMode::Overview, "9 Overview").clicked() {
                        self.set_mode(ViewMode::Overview, viewport_hint);
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
                    if ui.button("0 100%").clicked() {
                        self.reset_zoom(viewport_hint);
                    }
                    ui.separator();
                    if ui.selectable_label(self.invert, "I Invert").clicked() {
                        self.toggle_invert();
                    }
                    if ui.selectable_label(self.crop_enabled, "⌘K Crop").clicked() {
                        self.toggle_crop(viewport_hint);
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
        for path in crate::platform::take_open_paths()
            .into_iter()
            .filter(|path| is_pdf(path))
        {
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
            self.tabs_bar(ui);
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

fn screen_point_to_pdf(
    pos: Pos2,
    screen: Rect,
    page_w: f32,
    page_h: f32,
    crop: PageCrop,
) -> (f32, f32) {
    let local_x = ((pos.x - screen.left()) / screen.width().max(0.001)).clamp(0.0, 1.0);
    let local_y = ((pos.y - screen.top()) / screen.height().max(0.001)).clamp(0.0, 1.0);
    let source_x = crop.left + local_x * crop.width();
    let source_y_from_top = crop.top + local_y * crop.height();
    (source_x * page_w, page_h - source_y_from_top * page_h)
}

fn pdf_bounds_to_screen(
    bounds: PdfBounds,
    page_w: f32,
    page_h: f32,
    crop: PageCrop,
    screen: Rect,
) -> Option<Rect> {
    let left_norm = bounds.left / page_w.max(0.001);
    let right_norm = bounds.right / page_w.max(0.001);
    let top_norm = (page_h - bounds.top) / page_h.max(0.001);
    let bottom_norm = (page_h - bounds.bottom) / page_h.max(0.001);

    let left = screen.left() + ((left_norm - crop.left) / crop.width()) * screen.width();
    let right = screen.left() + ((right_norm - crop.left) / crop.width()) * screen.width();
    let top = screen.top() + ((top_norm - crop.top) / crop.height()) * screen.height();
    let bottom = screen.top() + ((bottom_norm - crop.top) / crop.height()) * screen.height();

    let clipped_left = left.max(screen.left());
    let clipped_right = right.min(screen.right());
    let clipped_top = top.max(screen.top());
    let clipped_bottom = bottom.min(screen.bottom());
    if clipped_left >= clipped_right || clipped_top >= clipped_bottom {
        None
    } else {
        Some(Rect::from_min_max(
            Pos2::new(clipped_left, clipped_top),
            Pos2::new(clipped_right, clipped_bottom),
        ))
    }
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

fn pretty_duration(seconds: f32) -> String {
    if seconds >= 60.0 {
        let total = seconds.round().max(0.0) as u32;
        let minutes = total / 60;
        let rest = total % 60;
        format!("{minutes}:{rest:02}")
    } else if seconds >= 10.0 {
        format!("{seconds:.0}s")
    } else {
        format!("{seconds:.1}s")
    }
}

fn is_pdf(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
}

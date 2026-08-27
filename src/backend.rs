use crate::model::{DocumentInfo, Glyph, LinkTarget, PageMetric, PageTextData, PdfBounds};
use anyhow::{anyhow, Context, Result};
use crossbeam_channel::{select_biased, unbounded, Receiver, Sender};
use eframe::egui;
use image::{DynamicImage, ImageFormat, RgbaImage};
use leptess::LepTess;
use pdfium_render::prelude::*;
use std::{
    env,
    io::Cursor,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    thread,
};

#[derive(Debug)]
pub enum BackendCommand {
    Open { doc_id: u64, path: PathBuf },
    Render { doc_id: u64, page: usize, pixel_width: u32, generation: u64 },
    ExtractText { doc_id: u64, page: usize },
    Ocr { doc_id: u64, page: usize },
    ResolveLink { doc_id: u64, page: usize, x_pt: f32, y_pt: f32 },
    Shutdown,
}

#[derive(Debug)]
pub enum BackendEvent {
    Opened { doc_id: u64, info: DocumentInfo },
    Rendered {
        doc_id: u64,
        page: usize,
        pixel_width: u32,
        generation: u64,
        width: usize,
        height: usize,
        rgba: Vec<u8>,
    },
    RenderFailed {
        doc_id: u64,
        page: usize,
        pixel_width: u32,
        generation: u64,
        message: String,
    },
    TextReady { doc_id: u64, data: PageTextData },
    OcrReady { doc_id: u64, page: usize, text: String },
    OcrFailed { doc_id: u64, page: usize, message: String },
    OcrUnavailable { doc_id: u64, message: String },
    LinkResolved { doc_id: u64, target: LinkTarget },
    Error { doc_id: u64, message: String },
}

#[derive(Debug)]
struct OcrJob {
    doc_id: u64,
    page: usize,
    png: Vec<u8>,
}

#[derive(Clone)]
struct EventSink {
    tx: Sender<BackendEvent>,
    repaint: egui::Context,
}

impl EventSink {
    fn send(&self, event: BackendEvent) {
        let _ = self.tx.send(event);
        self.repaint.request_repaint();
    }
}

pub struct PdfBackend {
    high_tx: Sender<BackendCommand>,
    low_tx: Sender<BackendCommand>,
    pub events: Receiver<BackendEvent>,
    render_generation: Arc<AtomicU64>,
}

impl PdfBackend {
    pub fn new(repaint: egui::Context) -> Self {
        let (high_tx, high_rx) = unbounded();
        let (low_tx, low_rx) = unbounded();
        let (event_tx, event_rx) = unbounded();
        let (ocr_tx, ocr_rx) = unbounded();
        let sink = EventSink { tx: event_tx, repaint };
        let render_generation = Arc::new(AtomicU64::new(1));

        spawn_ocr_worker(ocr_rx, sink.clone());
        thread::Builder::new()
            .name("pdfium-renderer".into())
            .spawn({
                let render_generation = render_generation.clone();
                move || renderer_loop(high_rx, low_rx, sink, ocr_tx, render_generation)
            })
            .expect("failed to spawn PDF renderer thread");

        Self { high_tx, low_tx, events: event_rx, render_generation }
    }

    pub fn high(&self, command: BackendCommand) {
        let _ = self.high_tx.send(command);
    }

    pub fn low(&self, command: BackendCommand) {
        let _ = self.low_tx.send(command);
    }

    pub fn render_generation(&self) -> u64 {
        self.render_generation.load(Ordering::Relaxed)
    }

    pub fn bump_render_generation(&self) -> u64 {
        self.render_generation.fetch_add(1, Ordering::Relaxed).wrapping_add(1)
    }
}

impl Drop for PdfBackend {
    fn drop(&mut self) {
        let _ = self.high_tx.send(BackendCommand::Shutdown);
    }
}

fn bind_pdfium() -> Result<Pdfium> {
    if let Ok(path) = env::var("PDFIUM_LIBRARY_PATH") {
        if !path.trim().is_empty() {
            return Ok(Pdfium::new(
                Pdfium::bind_to_library(path).context("could not load PDFium from PDFIUM_LIBRARY_PATH")?,
            ));
        }
    }

    // When built inside the supplied Nix shell, remember the exact Nix-store
    // PDFium path at compile time. The release executable can then be launched
    // directly from Finder/Spotlight without first entering nix-shell.
    if let Some(path) = option_env!("PDFIUM_LIBRARY_PATH") {
        if !path.trim().is_empty() {
            if let Ok(bindings) = Pdfium::bind_to_library(path) {
                return Ok(Pdfium::new(bindings));
            }
        }
    }

    // Packaged releases place PDFium next to the executable. Search there
    // before the working directory, because Finder/Explorer can launch us
    // with an arbitrary current directory.
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            let local = Pdfium::pdfium_platform_library_name_at_path(dir);
            if let Ok(bindings) = Pdfium::bind_to_library(&local) {
                return Ok(Pdfium::new(bindings));
            }
        }
    }

    let local = Pdfium::pdfium_platform_library_name_at_path(".");
    if let Ok(bindings) = Pdfium::bind_to_library(&local) {
        return Ok(Pdfium::new(bindings));
    }

    Ok(Pdfium::new(
        Pdfium::bind_to_system_library().context(
            "PDFium was not found. Enter the supplied Nix shell or set PDFIUM_LIBRARY_PATH",
        )?,
    ))
}

fn renderer_loop(
    high_rx: Receiver<BackendCommand>,
    low_rx: Receiver<BackendCommand>,
    events: EventSink,
    ocr_tx: Sender<OcrJob>,
    render_generation: Arc<AtomicU64>,
) {
    let pdfium = match bind_pdfium() {
        Ok(pdfium) => pdfium,
        Err(error) => {
            events.send(BackendEvent::Error { doc_id: 0, message: error.to_string() });
            return;
        }
    };

    let mut document: Option<PdfDocument<'_>> = None;
    let mut active_doc_id = 0u64;

    loop {
        let command = select_biased! {
            recv(high_rx) -> msg => match msg { Ok(v) => v, Err(_) => break },
            recv(low_rx) -> msg => match msg { Ok(v) => v, Err(_) => break },
        };

        match command {
            BackendCommand::Shutdown => break,
            BackendCommand::Open { doc_id, path } => {
                document = None;
                active_doc_id = doc_id;
                match open_document(&pdfium, &path) {
                    Ok((doc, info)) => {
                        document = Some(doc);
                        events.send(BackendEvent::Opened { doc_id, info });
                    }
                    Err(error) => {
                        events.send(BackendEvent::Error {
                            doc_id,
                            message: format!("Could not open {}: {error}", path.display()),
                        });
                    }
                }
            }
            BackendCommand::Render { doc_id, page, pixel_width, generation } => {
                if doc_id != active_doc_id || generation != render_generation.load(Ordering::Relaxed) {
                    continue;
                }
                let Some(doc) = document.as_ref() else { continue };
                match render_page(doc, page, pixel_width) {
                    Ok((width, height, rgba)) => {
                        // A pinch/mode change may have happened while PDFium was rendering.
                        // Drop the stale bitmap before paying the UI upload cost.
                        if generation != render_generation.load(Ordering::Relaxed) {
                            continue;
                        }
                        events.send(BackendEvent::Rendered {
                            doc_id,
                            page,
                            pixel_width,
                            generation,
                            width,
                            height,
                            rgba,
                        });
                    }
                    Err(error) => {
                        events.send(BackendEvent::RenderFailed {
                            doc_id,
                            page,
                            pixel_width,
                            generation,
                            message: format!("Page {} render failed: {error}", page + 1),
                        });
                    }
                }
            }
            BackendCommand::ExtractText { doc_id, page } => {
                if doc_id != active_doc_id {
                    continue;
                }
                let Some(doc) = document.as_ref() else { continue };
                match extract_page_text(doc, page) {
                    Ok(data) => {
                        events.send(BackendEvent::TextReady { doc_id, data });
                    }
                    Err(error) => {
                        events.send(BackendEvent::Error {
                            doc_id,
                            message: format!("Page {} text extraction failed: {error}", page + 1),
                        });
                    }
                }
            }
            BackendCommand::Ocr { doc_id, page } => {
                if doc_id != active_doc_id {
                    continue;
                }
                let Some(doc) = document.as_ref() else { continue };
                match render_ocr_png(doc, page) {
                    Ok(png) => {
                        let _ = ocr_tx.send(OcrJob { doc_id, page, png });
                    }
                    Err(error) => {
                        events.send(BackendEvent::Error {
                            doc_id,
                            message: format!("Page {} OCR preparation failed: {error}", page + 1),
                        });
                    }
                }
            }
            BackendCommand::ResolveLink { doc_id, page, x_pt, y_pt } => {
                if doc_id != active_doc_id {
                    continue;
                }
                let Some(doc) = document.as_ref() else { continue };
                match resolve_link(doc, page, x_pt, y_pt) {
                    Ok(Some(target)) => events.send(BackendEvent::LinkResolved { doc_id, target }),
                    Ok(None) => {}
                    Err(error) => events.send(BackendEvent::Error {
                        doc_id,
                        message: format!("Could not follow PDF link: {error}"),
                    }),
                }
            }
        }
    }
}

fn open_document<'a>(pdfium: &'a Pdfium, path: &Path) -> Result<(PdfDocument<'a>, DocumentInfo)> {
    let document = pdfium
        .load_pdf_from_file(path, None)
        .with_context(|| format!("PDFium rejected {}", path.display()))?;

    // `page_sizes()` avoids fully loading every PdfPage just to build the scroll layout.
    // On book-sized documents this makes opening considerably cheaper.
    let page_sizes = document
        .pages()
        .page_sizes()
        .context("could not read PDF page sizes")?;
    let pages = page_sizes
        .into_iter()
        .map(|rect| PageMetric {
            width_pt: rect.width().value,
            height_pt: rect.height().value,
        })
        .collect();

    let embedded_title = document
        .metadata()
        .get(PdfDocumentMetadataTagType::Title)
        .map(|tag| tag.value().trim().to_owned())
        .filter(|title| !title.is_empty());

    let fallback = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("kvikk pdf")
        .to_owned();

    let info = DocumentInfo {
        path: path.to_path_buf(),
        title: embedded_title.unwrap_or(fallback),
        pages,
    };

    Ok((document, info))
}

fn get_page<'a>(document: &'a PdfDocument<'_>, page: usize) -> Result<PdfPage<'a>> {
    document
        .pages()
        .get(page as PdfPageIndex)
        .map_err(|error| anyhow!("invalid page {}: {error}", page + 1))
}

fn resolve_link(
    document: &PdfDocument<'_>,
    page_index: usize,
    x_pt: f32,
    y_pt: f32,
) -> Result<Option<LinkTarget>> {
    let page = get_page(document, page_index)?;
    let Some(link) = page
        .links()
        .link_at_point(PdfPoints::new(x_pt), PdfPoints::new(y_pt))
    else {
        return Ok(None);
    };

    // Some internal links carry their destination directly.
    if let Some(destination) = link.destination() {
        let page = destination.page_index()? as usize;
        return Ok(Some(LinkTarget::Page(page)));
    }

    // Others carry an action. URI actions are web links; local-destination
    // actions are internal PDF navigation (TOCs, footnotes, references, …).
    if let Some(action) = link.action() {
        if let Some(uri_action) = action.as_uri_action() {
            if let Ok(uri) = uri_action.uri() {
                if !uri.trim().is_empty() {
                    return Ok(Some(LinkTarget::Url(uri)));
                }
            }
        }
        if let Some(local_action) = action.as_local_destination_action() {
            let destination = local_action.destination()?;
            let page = destination.page_index()? as usize;
            return Ok(Some(LinkTarget::Page(page)));
        }
    }

    Ok(None)
}

fn render_config(pixel_width: u32) -> PdfRenderConfig {
    PdfRenderConfig::new()
        .set_target_width(pixel_width.clamp(64, 8192) as Pixels)
        .set_maximum_height(8192 as Pixels)
        .set_text_smoothing(true)
        .set_path_smoothing(true)
        .set_image_smoothing(true)
        .use_lcd_text_rendering(true)
        .render_annotations(true)
}

fn render_page(document: &PdfDocument<'_>, page: usize, pixel_width: u32) -> Result<(usize, usize, Vec<u8>)> {
    let page = get_page(document, page)?;
    let bitmap = page
        .render_with_config(&render_config(pixel_width))
        .context("PDFium rendering failed")?;
    let width = bitmap.width().max(1) as usize;
    let height = bitmap.height().max(1) as usize;
    Ok((width, height, bitmap.as_rgba_bytes()))
}

fn extract_page_text(document: &PdfDocument<'_>, page_index: usize) -> Result<PageTextData> {
    let page = get_page(document, page_index)?;
    let text_page = page.text().context("PDFium could not open the text layer")?;
    let chars = text_page.chars();
    let mut glyphs = Vec::with_capacity(chars.len().max(0) as usize);
    let mut text = String::with_capacity(chars.len().max(0) as usize);

    for ch in chars.iter() {
        let Some(unicode) = ch.unicode_char() else { continue };
        text.push(unicode);
        let bounds = ch.loose_bounds().ok().map(|rect| PdfBounds {
            left: rect.left().value,
            bottom: rect.bottom().value,
            right: rect.right().value,
            top: rect.top().value,
        });
        glyphs.push(Glyph { ch: unicode, bounds });
    }

    Ok(PageTextData {
        page: page_index,
        text,
        glyphs,
        is_ocr: false,
    })
}

fn render_ocr_png(document: &PdfDocument<'_>, page: usize) -> Result<Vec<u8>> {
    let page = get_page(document, page)?;
    let bitmap = page
        .render_with_config(
            &PdfRenderConfig::new()
                .set_target_width(2400 as Pixels)
                .set_maximum_height(3600 as Pixels)
                .set_text_smoothing(true)
                .set_path_smoothing(true)
                .set_image_smoothing(true)
                .use_lcd_text_rendering(false)
                .render_annotations(false),
        )
        .context("PDFium OCR render failed")?;

    let width = bitmap.width().max(1) as u32;
    let height = bitmap.height().max(1) as u32;
    let rgba = bitmap.as_rgba_bytes();
    let image = RgbaImage::from_raw(width, height, rgba).ok_or_else(|| anyhow!("invalid OCR bitmap"))?;
    let mut cursor = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut cursor, ImageFormat::Png)
        .context("could not encode OCR image")?;
    Ok(cursor.into_inner())
}

fn spawn_ocr_worker(rx: Receiver<OcrJob>, events: EventSink) {
    thread::Builder::new()
        .name("tesseract-ocr".into())
        .spawn(move || {
            let runtime_tessdata = env::var("TESSDATA_PREFIX").ok();
            let bundled_tessdata = env::current_exe().ok().and_then(|exe| {
                let dir = exe.parent()?;
                let beside_exe = dir.join("tessdata");
                if beside_exe.is_dir() {
                    return Some(beside_exe.to_string_lossy().into_owned());
                }
                // macOS app bundle: Contents/MacOS/kvikk -> Contents/Resources/tessdata
                let resources = dir.parent()?.join("Resources").join("tessdata");
                resources.is_dir().then(|| resources.to_string_lossy().into_owned())
            });
            let build_tessdata = option_env!("TESSDATA_PREFIX").map(|path| path.to_owned());
            let tessdata = runtime_tessdata.or(bundled_tessdata).or(build_tessdata);

            let mut engine = match LepTess::new(tessdata.as_deref(), "eng+nor") {
                Ok(engine) => engine,
                Err(error) => {
                    events.send(BackendEvent::OcrUnavailable {
                        doc_id: 0,
                        message: format!("Tesseract OCR unavailable: {error}"),
                    });
                    return;
                }
            };

            for job in rx {
                let result = engine
                    .set_image_from_mem(&job.png)
                    .map_err(|e| anyhow!(e.to_string()))
                    .and_then(|_| engine.get_utf8_text().map_err(|e| anyhow!(e.to_string())));

                match result {
                    Ok(text) => {
                        events.send(BackendEvent::OcrReady {
                            doc_id: job.doc_id,
                            page: job.page,
                            text,
                        });
                    }
                    Err(error) => {
                        events.send(BackendEvent::OcrFailed {
                            doc_id: job.doc_id,
                            page: job.page,
                            message: format!("OCR failed on page {}: {error}", job.page + 1),
                        });
                    }
                }
            }
        })
        .expect("failed to spawn OCR thread");
}

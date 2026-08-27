use std::{collections::VecDeque, path::PathBuf, sync::{Mutex, OnceLock}};

static OPEN_PATHS: OnceLock<Mutex<VecDeque<PathBuf>>> = OnceLock::new();
static REPAINT_CONTEXT: OnceLock<Mutex<Option<egui::Context>>> = OnceLock::new();

fn open_paths() -> &'static Mutex<VecDeque<PathBuf>> {
    OPEN_PATHS.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn repaint_context() -> &'static Mutex<Option<egui::Context>> {
    REPAINT_CONTEXT.get_or_init(|| Mutex::new(None))
}

pub fn register_context(ctx: &egui::Context) {
    if let Ok(mut slot) = repaint_context().lock() {
        *slot = Some(ctx.clone());
    }
}

pub fn enqueue_open(path: PathBuf) {
    if let Ok(mut queue) = open_paths().lock() {
        queue.push_back(path);
    }
    if let Ok(slot) = repaint_context().lock() {
        if let Some(ctx) = slot.as_ref() {
            ctx.request_repaint();
        }
    }
}

pub fn take_open_paths() -> Vec<PathBuf> {
    if let Ok(mut queue) = open_paths().lock() {
        queue.drain(..).collect()
    } else {
        Vec::new()
    }
}

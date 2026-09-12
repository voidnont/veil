use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::OpenOptions;
use std::hash::{Hash, Hasher};
use std::io::Write as IoWrite;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, Color32, FontId, RichText, ScrollArea, Sense, TextureHandle};
use url::Url;
use veil_engine::engine::{
    DocumentView, FormControl, FormControlKind, RenderBlock, TextRun, WebFontResource,
};
use veil_engine::image_loader::{ImageLoadRequest, ImageLoader};
use veil_engine::loader::{LoadRequest, NavigationMethod, PageLoader};
use veil_engine::media_loader::{MediaLoadRequest, MediaLoader, MediaProbe};
use veil_engine::net::{MultipartPart, PrivacyNetwork};
use veil_engine::privacy::{strip_tracking_parameters, PrivacyProfiles, SitePrivacy};
use veil_engine::renderer_protocol::DomEventRequest;
use veil_engine::runtime_interaction::{
    RuntimeInteractionKind, RuntimeInteractionLoader, RuntimeInteractionRequest,
};
use veil_engine::script::CanvasCommand;
use veil_engine::storage::SharedBrowserStorage;
use veil_engine::style::{ComputedStyle, FontKind, JustifyContent, LayoutMode, TextAlign};

mod shell_ui;

const HOME: &str = "veil://home";
const DEFAULT_GLASS_TRANSPARENCY: f32 = 0.12;
const COLLAPSED_DOCK_WIDTH: f32 = 54.0;
const EXPANDED_DOCK_WIDTH: f32 = 260.0;
const HOVER_REVEAL_DELAY: Duration = Duration::from_secs(1);
const MAX_IMAGE_LOADS: usize = 6;

fn main() {
    install_crash_logger();
    let safe_ui = safe_ui_requested();

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1360.0, 860.0])
        .with_min_inner_size([820.0, 560.0])
        .with_transparent(!safe_ui)
        .with_decorations(false);
    if let Some(icon) = load_native_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    let result = eframe::run_native(
        "Veil Browser — Veil Engine",
        options,
        Box::new(|cc| {
            configure_visuals(&cc.egui_ctx);
            #[cfg(target_os = "windows")]
            if !safe_ui {
                if window_vibrancy::apply_acrylic(cc, Some((18, 18, 24, 118))).is_err() {
                    let _ = window_vibrancy::apply_blur(cc, Some((18, 18, 24, 118)));
                }
            }
            Ok(Box::new(VeilApp::new(&cc.egui_ctx)))
        }),
    );

    if let Err(error) = result {
        log_startup_error(&format!("native window startup failed: {error}"));
        eprintln!("Veil Browser could not start: {error}");
        eprintln!("Try running: veil-browser.exe --safe-ui");
        std::process::exit(2);
    }
}

fn safe_ui_requested() -> bool {
    std::env::args().any(|arg| arg == "--safe-ui") || std::env::var_os("VEIL_SAFE_UI").is_some()
}

fn is_guarded_script_site(url: &str) -> bool {
    Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
        .map(|host| {
            host == "youtube.com"
                || host.ends_with(".youtube.com")
                || host == "youtu.be"
                || host.ends_with(".youtu.be")
        })
        .unwrap_or(false)
}

fn install_crash_logger() {
    std::panic::set_hook(Box::new(|panic_info| {
        let message = format!("Veil Browser panic: {panic_info}\n");
        log_startup_error(&message);
        eprintln!("{message}");
    }));
}

fn log_startup_error(message: &str) {
    let path = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("veil-browser-crash.log")))
        .unwrap_or_else(|| std::path::PathBuf::from("veil-browser-crash.log"));
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{message}");
    }
}

fn initial_glass_transparency() -> f32 {
    if safe_ui_requested() {
        return 0.0;
    }
    if std::env::var_os("VEIL_DISABLE_TRANSPARENCY").is_some() {
        return 0.0;
    }
    std::env::var("VEIL_GLASS_TRANSPARENCY")
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .map(|value| value.clamp(0.0, 0.40))
        .unwrap_or(DEFAULT_GLASS_TRANSPARENCY)
}

fn load_native_icon() -> Option<egui::IconData> {
    let image = image::load_from_memory(include_bytes!("../assets/veil-glass-icon.png"))
        .ok()?
        .to_rgba8();
    let width = image.width();
    let height = image.height();
    Some(egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    })
}

fn configure_visuals(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = Color32::from_rgba_unmultiplied(16, 17, 22, 118);
    visuals.window_fill = Color32::from_rgba_unmultiplied(18, 19, 25, 132);
    visuals.extreme_bg_color = Color32::from_rgba_unmultiplied(10, 11, 15, 96);
    visuals.faint_bg_color = Color32::from_rgba_unmultiplied(255, 255, 255, 12);
    visuals.widgets.noninteractive.bg_fill = Color32::from_rgba_unmultiplied(255, 255, 255, 10);
    visuals.widgets.inactive.bg_fill = Color32::from_rgba_unmultiplied(255, 255, 255, 13);
    visuals.widgets.hovered.bg_fill = Color32::from_rgba_unmultiplied(255, 255, 255, 24);
    visuals.widgets.active.bg_fill = Color32::from_rgba_unmultiplied(255, 255, 255, 34);
    visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(10);
    visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(10);
    visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(10);
    visuals.widgets.active.corner_radius = egui::CornerRadius::same(10);
    visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(132, 102, 222, 72);
    ctx.set_visuals(visuals);
    ctx.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 7.0);
        style.spacing.button_padding = egui::vec2(10.0, 7.0);
        style.spacing.interact_size = egui::vec2(40.0, 36.0);
    });
}

#[derive(Clone)]
struct HistoryEntry {
    url: String,
}

#[derive(Debug, Clone)]
struct PendingNavigation {
    url: String,
    method: NavigationMethod,
}

struct Tab {
    id: u64,
    address: String,
    page: DocumentView,
    back: Vec<HistoryEntry>,
    forward: Vec<HistoryEntry>,
    status: String,
    loading: bool,
    generation: u64,
    space: usize,
}

impl Tab {
    fn home(id: u64, space: usize) -> Self {
        Self {
            id,
            address: HOME.to_owned(),
            page: DocumentView::home(),
            back: Vec::new(),
            forward: Vec::new(),
            status: "Private session · Shield active · ephemeral partitioned storage".into(),
            loading: false,
            generation: 0,
            space,
        }
    }
}

#[derive(Clone)]
enum CachedImage {
    Loading,
    Ready {
        texture: TextureHandle,
        size: [usize; 2],
    },
    Failed(String),
}

#[derive(Clone)]
enum CachedMedia {
    Loading,
    Ready(MediaProbe),
    Failed(String),
}

struct VeilApp {
    tabs: Vec<Tab>,
    active_tab: usize,
    split_tab_id: Option<u64>,
    next_tab_id: u64,
    network: PrivacyNetwork,
    loader: PageLoader,
    image_loader: ImageLoader,
    media_loader: MediaLoader,
    runtime_loader: RuntimeInteractionLoader,
    next_runtime_request_id: u64,
    last_runtime_tick: Instant,
    runtime_inflight: HashSet<u64>,
    storage: SharedBrowserStorage,
    profiles: PrivacyProfiles,
    blocked_log: VecDeque<String>,
    worker_blocked_count: usize,
    show_privacy: bool,
    sidebar_pinned: bool,
    sidebar_hover_since: Option<Instant>,
    sidebar_hover_revealed: bool,
    window_controls_hover_since: Option<Instant>,
    window_controls_revealed: bool,
    active_space: usize,
    image_cache: HashMap<String, CachedImage>,
    image_load_queue: VecDeque<ImageLoadRequest>,
    image_loads_inflight: usize,
    media_cache: HashMap<String, CachedMedia>,
    web_font_registry: HashMap<String, Vec<u8>>,
    custom_filters: String,
    form_values: HashMap<(u64, u64, usize), String>,
    form_checks: HashMap<(u64, u64, usize), bool>,
    home_queries: HashMap<u64, String>,
    logo: Option<TextureHandle>,
    glass_transparency: f32,
}

impl VeilApp {
    fn new(ctx: &egui::Context) -> Self {
        let storage = SharedBrowserStorage::new();
        let network = PrivacyNetwork::new_with_storage(storage.clone());
        let custom_filters = network.blocker().custom_text().to_owned();
        Self {
            tabs: vec![Tab::home(1, 0)],
            active_tab: 0,
            split_tab_id: None,
            next_tab_id: 2,
            network,
            loader: PageLoader::new(),
            image_loader: ImageLoader::new(),
            media_loader: MediaLoader::new(),
            runtime_loader: RuntimeInteractionLoader::new(),
            next_runtime_request_id: 1,
            last_runtime_tick: Instant::now(),
            runtime_inflight: HashSet::new(),
            storage,
            profiles: PrivacyProfiles::new(),
            blocked_log: VecDeque::new(),
            worker_blocked_count: 0,
            show_privacy: false,
            sidebar_pinned: true,
            sidebar_hover_since: None,
            sidebar_hover_revealed: false,
            window_controls_hover_since: None,
            window_controls_revealed: false,
            active_space: 0,
            image_cache: HashMap::new(),
            image_load_queue: VecDeque::new(),
            image_loads_inflight: 0,
            media_cache: HashMap::new(),
            web_font_registry: HashMap::new(),
            custom_filters,
            form_values: HashMap::new(),
            form_checks: HashMap::new(),
            home_queries: HashMap::new(),
            logo: load_embedded_logo(ctx),
            glass_transparency: initial_glass_transparency(),
        }
    }

    fn glass_alpha(&self, extra_opacity: f32) -> u8 {
        let base_opacity = (1.0 - self.glass_transparency).clamp(0.60, 1.0);
        ((base_opacity + extra_opacity).clamp(0.60, 0.98) * 255.0) as u8
    }

    fn glass_frame(&self, extra_opacity: f32) -> egui::Frame {
        egui::Frame::default()
            .fill(Color32::from_rgba_unmultiplied(
                20,
                20,
                26,
                self.glass_alpha(extra_opacity),
            ))
            .stroke(egui::Stroke::new(
                1.0_f32,
                Color32::from_rgba_unmultiplied(255, 255, 255, 24),
            ))
            .corner_radius(14)
            .inner_margin(6)
    }

    fn new_tab(&mut self) {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        self.tabs.push(Tab::home(id, self.active_space));
        self.active_tab = self.tabs.len() - 1;
    }

    fn switch_space(&mut self, space: usize) {
        if self.active_space == space {
            return;
        }
        self.active_space = space;
        self.split_tab_id = None;
        if let Some(index) = self.tabs.iter().position(|tab| tab.space == space) {
            self.active_tab = index;
        } else {
            self.new_tab();
        }
    }

    fn activate_tab(&mut self, index: usize) {
        if index >= self.tabs.len() || index == self.active_tab {
            return;
        }
        let new_id = self.tabs[index].id;
        if self.split_tab_id == Some(new_id) {
            let old_primary = self.tabs[self.active_tab].id;
            self.split_tab_id = Some(old_primary);
        }
        self.active_tab = index;
        self.active_space = self.tabs[index].space;
    }

    fn close_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        if self.tabs.len() == 1 {
            let id = self.tabs[0].id;
            self.tabs[0] = Tab::home(id, self.active_space);
            self.active_tab = 0;
            self.split_tab_id = None;
            return;
        }

        let closed_id = self.tabs[index].id;
        self.tabs.remove(index);
        self.form_values.retain(|(tab, _, _), _| *tab != closed_id);
        self.form_checks.retain(|(tab, _, _), _| *tab != closed_id);
        self.home_queries.remove(&closed_id);
        if self.split_tab_id == Some(closed_id) {
            self.split_tab_id = None;
        }
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        } else if index < self.active_tab {
            self.active_tab -= 1;
        }
        self.active_space = self.tabs[self.active_tab].space;
    }

    fn toggle_split(&mut self) {
        if self.split_tab_id.is_some() {
            self.split_tab_id = None;
            return;
        }
        let primary_id = self.tabs[self.active_tab].id;
        if let Some(tab) = self.tabs.iter().find(|tab| tab.id != primary_id) {
            self.split_tab_id = Some(tab.id);
            return;
        }
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        self.tabs.push(Tab::home(id, self.active_space));
        self.split_tab_id = Some(id);
    }

    fn split_tab_index(&self) -> Option<usize> {
        let id = self.split_tab_id?;
        self.tabs
            .iter()
            .position(|tab| tab.id == id && tab.id != self.tabs[self.active_tab].id)
    }

    fn normalize_address(&self, input: &str) -> String {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return HOME.to_owned();
        }
        if trimmed.starts_with("veil://") {
            return trimmed.to_owned();
        }

        let candidate = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            trimmed.to_owned()
        } else if trimmed.contains(' ') || !trimmed.contains('.') {
            let query: String = url::form_urlencoded::byte_serialize(trimmed.as_bytes()).collect();
            format!("https://lite.duckduckgo.com/lite/?q={query}")
        } else {
            format!("https://{trimmed}")
        };

        Url::parse(&candidate)
            .map(strip_tracking_parameters)
            .map(|url| url.to_string())
            .unwrap_or(candidate)
    }

    fn navigate_active(&mut self, target: String, push_history: bool) {
        self.navigate_tab(self.active_tab, target, push_history);
    }

    fn navigate_tab(&mut self, tab_index: usize, target: String, push_history: bool) {
        self.navigate_tab_with_method(tab_index, target, push_history, NavigationMethod::Get);
    }

    fn navigate_tab_with_method(
        &mut self,
        tab_index: usize,
        target: String,
        push_history: bool,
        method: NavigationMethod,
    ) {
        if tab_index >= self.tabs.len() {
            return;
        }
        let target = self.normalize_address(&target);

        if push_history {
            let current = self.tabs[tab_index].page.url.clone();
            if current != target {
                self.tabs[tab_index]
                    .back
                    .push(HistoryEntry { url: current });
                self.tabs[tab_index].forward.clear();
            }
        }

        if target == HOME {
            let tab = &mut self.tabs[tab_index];
            tab.generation += 1;
            tab.loading = false;
            tab.address = HOME.into();
            tab.page = DocumentView::home();
            tab.status = "Private new tab".into();
            return;
        }

        let privacy = Url::parse(&target)
            .ok()
            .map(|url| self.profiles.for_url(&url))
            .unwrap_or_default();

        let (tab_id, generation) = {
            let tab = &mut self.tabs[tab_index];
            tab.generation += 1;
            tab.loading = true;
            tab.address = target.clone();
            tab.status = match &method {
                NavigationMethod::Get => format!("Loading {target} …"),
                NavigationMethod::PostForm(_) => format!("Submitting securely to {target} …"),
                NavigationMethod::PostMultipart(_) => format!("Uploading securely to {target} …"),
            };
            (tab.id, tab.generation)
        };

        self.loader.start(LoadRequest {
            tab_id,
            generation,
            url: target,
            privacy,
            custom_filters: self.custom_filters.clone(),
            storage: self.storage.clone(),
            method,
        });
    }

    fn poll_loader(&mut self, ctx: &egui::Context) {
        while let Some(result) = self.loader.try_recv() {
            let Some(index) = self.tabs.iter().position(|tab| tab.id == result.tab_id) else {
                continue;
            };
            if self.tabs[index].generation != result.generation {
                continue;
            }

            self.worker_blocked_count += result.blocked_count;
            for event in result.blocked_events {
                self.blocked_log.push_front(event);
            }
            while self.blocked_log.len() > 180 {
                self.blocked_log.pop_back();
            }

            let tab = &mut self.tabs[index];
            tab.loading = false;
            match result.result {
                Ok(view) => {
                    install_web_fonts(ctx, &view.web_fonts, &mut self.web_font_registry);
                    tab.address = view.url.clone();
                    tab.status = format!(
                        "{} ms · {} blocked · {} CSS · {} scripts · {}",
                        result.elapsed_ms,
                        result.blocked_count,
                        view.external_stylesheets,
                        view.external_scripts,
                        result.renderer_mode.label(),
                    );
                    tab.page = view;
                }
                Err(err) => {
                    let target = tab.address.clone();
                    tab.status = format!("Navigation failed: {err}");
                    tab.page = DocumentView::error(&target, &err);
                }
            }
        }
    }

    fn dispatch_runtime_event(&mut self, tab_index: usize, event: DomEventRequest) {
        if tab_index >= self.tabs.len() {
            return;
        }
        let tab = &self.tabs[tab_index];
        if tab.page.url == HOME {
            return;
        }
        let javascript_enabled = !is_guarded_script_site(&tab.page.url)
            && Url::parse(&tab.page.url)
                .ok()
                .map(|url| self.profiles.for_url(&url).javascript)
                .unwrap_or(false);
        if !javascript_enabled {
            return;
        }
        if self.runtime_inflight.contains(&tab.id) {
            return;
        }

        let request_id = self.next_runtime_request_id;
        self.next_runtime_request_id = self.next_runtime_request_id.saturating_add(1);
        self.runtime_inflight.insert(tab.id);
        self.runtime_loader.start(RuntimeInteractionRequest {
            request_id,
            tab_id: tab.id,
            generation: tab.generation,
            page_url: tab.page.url.clone(),
            session_id: format!("tab-{}-{}", tab.id, tab.generation),
            storage: self.storage.clone(),
            kind: RuntimeInteractionKind::Event(event),
        });
    }

    fn poll_runtime_loader(&mut self, ctx: &egui::Context) {
        while let Some(result) = self.runtime_loader.try_recv() {
            self.runtime_inflight.remove(&result.tab_id);
            let Some(index) = self.tabs.iter().position(|tab| tab.id == result.tab_id) else {
                continue;
            };
            if self.tabs[index].generation != result.generation {
                continue;
            }
            match result.result {
                Ok(mut view) => {
                    // Fonts are loaded by the navigation broker, not the engine process.
                    // Preserve them while replacing the live DOM/paint snapshot.
                    view.web_fonts = self.tabs[index].page.web_fonts.clone();
                    install_web_fonts(ctx, &view.web_fonts, &mut self.web_font_registry);
                    self.tabs[index].page = view;
                    self.tabs[index].status = result
                        .mode
                        .map(|mode| format!("Live page updated · {}", mode.label()))
                        .unwrap_or_else(|| "Live page updated".into());
                    ctx.request_repaint();
                }
                Err(error) => {
                    // Interaction failures should not destroy a successfully loaded page.
                    self.tabs[index].status = format!("Live interaction unavailable: {error}");
                }
            }
        }
    }

    fn pump_runtime_timers(&mut self) {
        let elapsed = self.last_runtime_tick.elapsed();
        if elapsed < Duration::from_millis(750) {
            return;
        }
        self.last_runtime_tick = Instant::now();
        let elapsed_ms = elapsed.as_millis().min(u64::MAX as u128) as u64;

        let mut indices = vec![self.active_tab];
        if let Some(split) = self.split_tab_index() {
            if split != self.active_tab {
                indices.push(split);
            }
        }
        for index in indices {
            if index >= self.tabs.len() || self.tabs[index].page.url == HOME {
                continue;
            }
            let tab = &self.tabs[index];
            let javascript_enabled = !is_guarded_script_site(&tab.page.url)
                && Url::parse(&tab.page.url)
                    .ok()
                    .map(|url| self.profiles.for_url(&url).javascript)
                    .unwrap_or(false);
            if !javascript_enabled {
                continue;
            }
            if self.runtime_inflight.contains(&tab.id) {
                continue;
            }
            let request_id = self.next_runtime_request_id;
            self.next_runtime_request_id = self.next_runtime_request_id.saturating_add(1);
            self.runtime_inflight.insert(tab.id);
            self.runtime_loader.start(RuntimeInteractionRequest {
                request_id,
                tab_id: tab.id,
                generation: tab.generation,
                page_url: tab.page.url.clone(),
                session_id: format!("tab-{}-{}", tab.id, tab.generation),
                storage: self.storage.clone(),
                kind: RuntimeInteractionKind::Tick(elapsed_ms),
            });
        }
    }

    fn poll_image_loader(&mut self, ctx: &egui::Context) {
        while let Some(result) = self.image_loader.try_recv() {
            self.image_loads_inflight = self.image_loads_inflight.saturating_sub(1);
            self.worker_blocked_count += result.blocked_count;
            for event in result.blocked_events {
                self.blocked_log.push_front(event);
            }
            while self.blocked_log.len() > 180 {
                self.blocked_log.pop_back();
            }

            let cached = match result.result {
                Ok(decoded) => {
                    let color_image =
                        egui::ColorImage::from_rgba_unmultiplied(decoded.size, &decoded.rgba);
                    let texture = ctx.load_texture(
                        decoded.final_url,
                        color_image,
                        egui::TextureOptions::LINEAR,
                    );
                    CachedImage::Ready {
                        texture,
                        size: decoded.size,
                    }
                }
                Err(err) => CachedImage::Failed(err),
            };
            self.image_cache.insert(result.key, cached);
        }
    }

    fn pump_image_queue(&mut self) {
        while self.image_loads_inflight < MAX_IMAGE_LOADS {
            let Some(request) = self.image_load_queue.pop_front() else {
                break;
            };
            self.image_loads_inflight += 1;
            self.image_loader.start(request);
        }
    }

    fn poll_media_loader(&mut self) {
        while let Some(result) = self.media_loader.try_recv() {
            self.worker_blocked_count += result.blocked_count;
            for event in result.blocked_events {
                self.blocked_log.push_front(event);
            }
            while self.blocked_log.len() > 180 {
                self.blocked_log.pop_back();
            }
            let cached = match result.result {
                Ok(probe) => CachedMedia::Ready(probe),
                Err(error) => CachedMedia::Failed(error),
            };
            self.media_cache.insert(result.key, cached);
        }
    }

    fn go_back(&mut self) {
        let index = self.active_tab;
        if let Some(entry) = self.tabs[index].back.pop() {
            let current = HistoryEntry {
                url: self.tabs[index].page.url.clone(),
            };
            self.tabs[index].forward.push(current);
            self.navigate_tab(index, entry.url, false);
        }
    }

    fn go_forward(&mut self) {
        let index = self.active_tab;
        if let Some(entry) = self.tabs[index].forward.pop() {
            let current = HistoryEntry {
                url: self.tabs[index].page.url.clone(),
            };
            self.tabs[index].back.push(current);
            self.navigate_tab(index, entry.url, false);
        }
    }

    fn reload(&mut self) {
        let url = self.tabs[self.active_tab].page.url.clone();
        self.navigate_active(url, false);
    }

    fn stop_loading(&mut self) {
        let tab = &mut self.tabs[self.active_tab];
        if tab.loading {
            tab.generation += 1;
            tab.loading = false;
            tab.status = "Loading stopped".into();
        }
    }

    fn current_privacy(&self) -> SitePrivacy {
        Url::parse(&self.tabs[self.active_tab].page.url)
            .ok()
            .map(|url| self.profiles.for_url(&url))
            .unwrap_or_else(|| self.profiles.default_settings())
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let ctrl = ctx.input(|i| i.modifiers.ctrl || i.modifiers.command);
        if ctrl && ctx.input(|i| i.key_pressed(egui::Key::T)) {
            self.new_tab();
        }
        if ctrl && ctx.input(|i| i.key_pressed(egui::Key::W)) {
            self.close_tab(self.active_tab);
        }
        if ctrl && ctx.input(|i| i.key_pressed(egui::Key::R)) {
            self.reload();
        }
        if ctrl && ctx.input(|i| i.modifiers.shift && i.key_pressed(egui::Key::S)) {
            self.toggle_split();
        }
        if ctx.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::ArrowLeft)) {
            self.go_back();
        }
        if ctx.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::ArrowRight)) {
            self.go_forward();
        }
        if ctrl && ctx.input(|i| i.key_pressed(egui::Key::L)) {
            ctx.memory_mut(|mem| mem.request_focus(egui::Id::new("veil_address_bar")));
        }
        if ctrl && ctx.input(|i| i.key_pressed(egui::Key::K)) {
            let tab_id = self.tabs[self.active_tab].id;
            let target = if self.tabs[self.active_tab].page.url == HOME {
                egui::Id::new(("veil_home_search", tab_id))
            } else {
                egui::Id::new("veil_address_bar")
            };
            ctx.memory_mut(|mem| mem.request_focus(target));
        }
    }

    fn update_hover_reveals(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let pointer = ctx.input(|i| i.pointer.hover_pos());

        if self.sidebar_pinned {
            self.sidebar_hover_since = None;
            self.sidebar_hover_revealed = true;
        } else {
            let over_left_edge = pointer.map(|pos| pos.x <= 14.0).unwrap_or(false);
            let over_open_sidebar = self.sidebar_hover_revealed
                && pointer
                    .map(|pos| pos.x <= EXPANDED_DOCK_WIDTH + 24.0)
                    .unwrap_or(false);
            if over_open_sidebar {
                self.sidebar_hover_since = None;
            } else if over_left_edge {
                let started = self.sidebar_hover_since.get_or_insert(now);
                if now.duration_since(*started) >= HOVER_REVEAL_DELAY {
                    self.sidebar_hover_revealed = true;
                    self.sidebar_hover_since = None;
                } else {
                    ctx.request_repaint_after(Duration::from_millis(40));
                }
            } else {
                self.sidebar_hover_since = None;
                self.sidebar_hover_revealed = false;
            }
        }

        let screen = ctx.screen_rect();
        let over_controls_hotspot = pointer
            .map(|pos| pos.y <= 46.0 && pos.x >= screen.right() - 166.0)
            .unwrap_or(false);
        if over_controls_hotspot {
            let started = self.window_controls_hover_since.get_or_insert(now);
            if self.window_controls_revealed || now.duration_since(*started) >= HOVER_REVEAL_DELAY {
                self.window_controls_revealed = true;
                self.window_controls_hover_since = None;
            } else {
                ctx.request_repaint_after(Duration::from_millis(40));
            }
        } else {
            self.window_controls_hover_since = None;
            self.window_controls_revealed = false;
        }
    }

    fn sidebar_expanded(&self, _ctx: &egui::Context) -> bool {
        true
    }

    fn render_sidebar(&mut self, ctx: &egui::Context) {
        let expanded = self.sidebar_expanded(ctx);
        let width = if expanded {
            EXPANDED_DOCK_WIDTH
        } else {
            COLLAPSED_DOCK_WIDTH
        };
        let height = (ctx.screen_rect().height() - 16.0).max(320.0);
        let frame = self.glass_frame(0.0);

        egui::Area::new(egui::Id::new("veil_vertical_command_center"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(8.0, 8.0))
            .show(ctx, |ui| {
                frame.show(ui, |ui| {
                    ui.set_min_size(egui::vec2(width, height));
                    ui.set_max_width(width);
                    ui.add_space(4.0);

                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        if let Some(logo_id) = self.logo.as_ref().map(|logo| logo.id()) {
                            let image = egui::Image::new((logo_id, egui::vec2(36.0, 36.0)));
                            if ui
                                .add(image.sense(Sense::click()))
                                .on_hover_text("Veil Browser")
                                .clicked()
                            {
                                let active = self.active_tab;
                                self.navigate_tab(active, HOME.into(), true);
                            }
                        } else if ui
                            .add_sized([36.0, 36.0], egui::Button::new(RichText::new("V").strong()))
                            .clicked()
                        {
                            let active = self.active_tab;
                            self.navigate_tab(active, HOME.into(), true);
                        }
                        if expanded {
                            ui.vertical(|ui| {
                                ui.label(RichText::new("Veil").strong().size(16.0));
                                ui.label(
                                    RichText::new("Private browser")
                                        .small()
                                        .color(Color32::from_gray(150)),
                                );
                            });
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let pin = if self.sidebar_pinned { "◆" } else { "◇" };
                                    if ui.button(pin).on_hover_text("Pin command center").clicked()
                                    {
                                        self.sidebar_pinned = !self.sidebar_pinned;
                                    }
                                },
                            );
                        }
                    });

                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(6.0);

                    let mut requested_space = None;
                    if expanded {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Spaces")
                                    .small()
                                    .color(Color32::from_gray(145)),
                            );
                            ui.add_space(4.0);
                            for (index, label) in ["Personal", "Work", "Focus"].iter().enumerate() {
                                let selected = self.active_space == index;
                                let text = if selected {
                                    format!("● {}", &label[..1])
                                } else {
                                    format!("○ {}", &label[..1])
                                };
                                if ui.small_button(text).on_hover_text(*label).clicked() {
                                    requested_space = Some(index);
                                }
                            }
                        });
                    } else {
                        ui.vertical_centered(|ui| {
                            for index in 0..3 {
                                let glyph = if self.active_space == index {
                                    "●"
                                } else {
                                    "○"
                                };
                                if ui
                                    .add_sized([34.0, 24.0], egui::Button::new(glyph).frame(false))
                                    .on_hover_text(["Personal", "Work", "Focus"][index])
                                    .clicked()
                                {
                                    requested_space = Some(index);
                                }
                            }
                        });
                    }
                    if let Some(space) = requested_space {
                        self.switch_space(space);
                    }

                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(4.0);

                    let available_for_tabs = (height - 300.0).max(150.0);
                    let mut choose_tab = None;
                    let mut close_tab = None;
                    ScrollArea::vertical()
                        .id_salt("veil_vertical_tabs")
                        .max_height(available_for_tabs)
                        .show(ui, |ui| {
                            for index in 0..self.tabs.len() {
                                if self.tabs[index].space != self.active_space {
                                    continue;
                                }
                                let title = truncate_title(&self.tabs[index].page.title, 28);
                                let loading = self.tabs[index].loading;
                                let active = index == self.active_tab;
                                let fill = if active {
                                    Color32::from_rgba_unmultiplied(210, 118, 108, 24)
                                } else {
                                    Color32::TRANSPARENT
                                };
                                let stroke = if active {
                                    egui::Stroke::new(
                                        1.0_f32,
                                        Color32::from_rgba_unmultiplied(255, 255, 255, 22),
                                    )
                                } else {
                                    egui::Stroke::NONE
                                };
                                egui::Frame::default()
                                    .fill(fill)
                                    .stroke(stroke)
                                    .corner_radius(11)
                                    .inner_margin(3)
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            let response = self.render_tab_icon(ctx, ui, index);
                                            if response.clicked() {
                                                choose_tab = Some(index);
                                            }
                                            if expanded {
                                                let label = if loading {
                                                    format!("◌ {title}")
                                                } else {
                                                    title
                                                };
                                                if ui
                                                    .add_sized(
                                                        [144.0, 34.0],
                                                        egui::Button::new(label).frame(false),
                                                    )
                                                    .clicked()
                                                {
                                                    choose_tab = Some(index);
                                                }
                                                if ui
                                                    .small_button("×")
                                                    .on_hover_text("Close tab · Ctrl+W")
                                                    .clicked()
                                                {
                                                    close_tab = Some(index);
                                                }
                                            }
                                        });
                                    });
                                ui.add_space(2.0);
                            }
                        });

                    if let Some(index) = choose_tab {
                        self.activate_tab(index);
                    }
                    if let Some(index) = close_tab {
                        self.close_tab(index);
                    }

                    ui.separator();
                    ui.add_space(4.0);
                    sidebar_action(ui, expanded, "+", "New tab", "Ctrl+T", || self.new_tab());
                    sidebar_action(ui, expanded, "◫", "Split view", "Ctrl+Shift+S", || {
                        self.toggle_split()
                    });
                    sidebar_action(ui, expanded, "◈", "Privacy Shield", "", || {
                        self.show_privacy = !self.show_privacy
                    });

                    if expanded {
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(&self.tabs[self.active_tab].status)
                                .small()
                                .color(Color32::GRAY),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("Subtle glass · private by default · no telemetry")
                                .small()
                                .color(Color32::from_gray(150)),
                        );
                    }
                });
            });
    }

    fn render_tab_icon(
        &mut self,
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        tab_index: usize,
    ) -> egui::Response {
        let icon_url = self.tabs[tab_index].page.icon_url.clone();
        let top_level = Url::parse(&self.tabs[tab_index].page.url).ok();
        let privacy = top_level
            .as_ref()
            .map(|url| self.profiles.for_url(url))
            .unwrap_or_default();

        if let (Some(icon_url), Some(top_level)) = (icon_url, top_level) {
            if !self.image_cache.contains_key(&icon_url) {
                if let Ok(url) = Url::parse(&icon_url) {
                    self.image_cache
                        .insert(icon_url.clone(), CachedImage::Loading);
                    self.image_load_queue.push_back(ImageLoadRequest {
                        key: icon_url.clone(),
                        top_level,
                        url,
                        privacy,
                        custom_filters: self.custom_filters.clone(),
                        storage: self.storage.clone(),
                    });
                    ctx.request_repaint_after(Duration::from_millis(40));
                }
            }

            if let Some(CachedImage::Ready { texture, .. }) = self.image_cache.get(&icon_url) {
                return ui
                    .add(
                        egui::Image::new((texture.id(), egui::vec2(30.0, 30.0)))
                            .sense(Sense::click()),
                    )
                    .on_hover_text(&self.tabs[tab_index].page.title);
            }
        }

        let monogram = tab_monogram(&self.tabs[tab_index]);
        ui.add_sized(
            [38.0, 38.0],
            egui::Button::new(RichText::new(monogram).strong()),
        )
        .on_hover_text(&self.tabs[tab_index].page.title)
    }

    fn render_address_pill(&mut self, ctx: &egui::Context) {
        let screen = ctx.screen_rect();
        let width = (screen.width() - 220.0).clamp(440.0, 760.0);
        let x = screen.center().x - width / 2.0;
        let frame = self.glass_frame(0.05);

        egui::Area::new(egui::Id::new("veil_floating_url_bar"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(x, 14.0))
            .show(ctx, |ui| {
                frame.show(ui, |ui| {
                    ui.set_min_width(width);
                    ui.add_space(1.0);
                    let mut navigation: Option<String> = None;
                    let can_back = !self.tabs[self.active_tab].back.is_empty();
                    let can_forward = !self.tabs[self.active_tab].forward.is_empty();
                    let loading = self.tabs[self.active_tab].loading;
                    let privacy = self.current_privacy();

                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(can_back, egui::Button::new("←").frame(false))
                            .on_hover_text("Back · Alt+Left")
                            .clicked()
                        {
                            self.go_back();
                        }
                        if ui
                            .add_enabled(can_forward, egui::Button::new("→").frame(false))
                            .on_hover_text("Forward · Alt+Right")
                            .clicked()
                        {
                            self.go_forward();
                        }
                        if loading {
                            if ui.button("×").on_hover_text("Stop loading").clicked() {
                                self.stop_loading();
                            }
                        } else if ui.button("↻").on_hover_text("Reload · Ctrl+R").clicked() {
                            self.reload();
                        }

                        ui.label(security_icon(&self.tabs[self.active_tab].address));
                        let text_width = (ui.available_width() - 92.0).max(180.0);
                        let response = {
                            let address = &mut self.tabs[self.active_tab].address;
                            ui.add_sized(
                                [text_width, 32.0],
                                egui::TextEdit::singleline(address)
                                    .id(egui::Id::new("veil_address_bar"))
                                    .hint_text("Search or enter address")
                                    .frame(false),
                            )
                        };
                        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            navigation = Some(self.tabs[self.active_tab].address.clone());
                        }

                        let shield = if privacy.shields { "◈" } else { "◇" };
                        if ui.button(shield).on_hover_text("Privacy Shield").clicked() {
                            self.show_privacy = !self.show_privacy;
                        }
                    });
                    ui.add_space(4.0);
                    if let Some(target) = navigation {
                        self.navigate_active(target, true);
                    }
                });
            });
    }

    fn render_privacy_panel(&mut self, ctx: &egui::Context) {
        if !self.show_privacy {
            return;
        }
        let screen = ctx.screen_rect();
        let width = 372.0_f32.min((screen.width() - 40.0).max(280.0));
        let x = (screen.right() - width - 12.0).max(12.0);
        let y = 66.0;
        let max_height = (screen.bottom() - y - 14.0).max(260.0);
        let frame = self.glass_frame(0.06);
        let mut reload_after_change = false;

        egui::Area::new(egui::Id::new("veil_privacy_panel"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(x, y))
            .show(ctx, |ui| {
                frame.show(ui, |ui| {
                    ui.set_min_width(width);
                    ui.set_max_width(width);
                    ui.horizontal(|ui| {
                        ui.heading("Privacy Shield");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("×").clicked() {
                                self.show_privacy = false;
                            }
                        });
                    });
                    ui.label("Filtering happens before requests leave Veil Browser.");
                    ui.separator();

                    ScrollArea::vertical().max_height(max_height - 70.0).show(ui, |ui| {
                        let current_url = Url::parse(&self.tabs[self.active_tab].page.url).ok();
                        let current_host = current_url.as_ref().and_then(|url| url.host_str()).map(str::to_owned);
                        if let Some(host) = current_host.as_deref() {
                            ui.strong(format!("Site: {host}"));
                            let mut settings = self.profiles.for_host(host);
                            let before = settings;
                            ui.checkbox(&mut settings.shields, "Block ads & trackers");
                            ui.checkbox(&mut settings.block_third_party, "Strict third-party blocking");
                            ui.checkbox(&mut settings.load_images, "Load images");
                            ui.checkbox(&mut settings.javascript, "Run JavaScript VM");
                            if settings != before {
                                self.profiles.set_for_host(host, settings);
                                reload_after_change = true;
                            }
                            if ui.button("Reset site privacy").clicked() {
                                self.profiles.clear_for_host(host);
                                reload_after_change = true;
                            }
                            if let Some(url) = current_url.as_ref() {
                                if ui.button("Clear this site's in-memory data").clicked() {
                                    self.storage.clear_site(url);
                                    reload_after_change = true;
                                }
                            }
                        } else {
                            ui.label("Per-site controls are available on HTTP/HTTPS pages.");
                        }

                        ui.separator();
                        ui.strong("Glass shell");
                        ui.add(egui::Slider::new(&mut self.glass_transparency, 0.00..=0.40).text("Glass transparency"));
                        ui.label(RichText::new("Default: 12% transparent chrome. Lower values are more opaque and calmer.").small().color(Color32::GRAY));

                        ui.separator();
                        ui.strong("Fingerprint reduction");
                        ui.label("Fixed Veil Browser user agent · fixed language · no client hints");
                        ui.label("GPC: 1 · DNT: 1 · no Referer");
                        ui.label("Cookies/localStorage are partitioned and memory-only.");
                        ui.label("Tracking query parameters are stripped on navigation.");

                        ui.separator();
                        let stats = self.network.blocker().stats();
                        ui.strong("Ad/tracker filter engine");
                        ui.label(format!(
                            "{} network · {} exceptions · {} cosmetic · {} ignored",
                            stats.network_rules, stats.exception_rules, stats.cosmetic_rules, stats.ignored_rules
                        ));
                        ui.collapsing("Custom filter list", |ui| {
                            ui.add(egui::TextEdit::multiline(&mut self.custom_filters).desired_rows(7).code_editor());
                            if ui.button("Apply filters").clicked() {
                                self.network.blocker_mut().replace_custom_filters(self.custom_filters.clone());
                                reload_after_change = true;
                            }
                        });

                        ui.separator();
                        let page = &self.tabs[self.active_tab].page;
                        ui.strong("Current page");
                        ui.label(format!("Linked CSS: {}", page.external_stylesheets));
                        ui.label(format!("External scripts: {}", page.external_scripts));
                        ui.label(format!("Cosmetic elements hidden: {}", page.cosmetic_hidden));
                        ui.label(format!(
                            "JS discovered: {} · executed: {} · errors: {}",
                            page.script_report.discovered, page.script_report.executed, page.script_report.errors
                        ));
                        ui.label(format!(
                            "Live DOM nodes: {} · event listeners: {} · cookie writes: {}",
                            page.script_report.live_node_count,
                            page.script_report.event_listener_count,
                            page.script_report.cookie_writes.len(),
                        ));
                        if let Some(error) = &page.script_report.last_error {
                            ui.label(RichText::new(format!("Last JS error: {error}")).small().color(Color32::GRAY));
                        }

                        ui.separator();
                        ui.label(format!("Blocked this session: {}", self.worker_blocked_count));
                        ui.horizontal(|ui| {
                            if ui.button("Clear image cache").clicked() {
                                self.image_cache.clear();
                            }
                            if ui.button("Clear all session data").clicked() {
                                self.storage.clear_all();
                                self.image_cache.clear();
                                reload_after_change = true;
                            }
                        });
                        ui.strong("Recent blocks");
                        ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                            if self.blocked_log.is_empty() {
                                ui.label(RichText::new("Nothing blocked yet.").color(Color32::GRAY));
                            }
                            for line in &self.blocked_log {
                                ui.label(RichText::new(line).small().monospace());
                            }
                        });
                    });
                });
            });

        if reload_after_change {
            self.image_cache.clear();
            self.reload();
        }
    }

    fn render_content(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(Color32::from_rgba_unmultiplied(18, 19, 22, 246)))
            .show(ctx, |ui| {
                let primary_index = self.active_tab;
                if let Some(split_index) = self.split_tab_index() {
                    ui.columns(2, |columns| {
                        self.render_page_for_tab(ctx, &mut columns[0], primary_index, true);
                        self.render_page_for_tab(ctx, &mut columns[1], split_index, true);
                    });
                } else {
                    self.render_page_for_tab(ctx, ui, primary_index, false);
                }
            });
    }

    fn render_home(&mut self, ui: &mut egui::Ui, tab_index: usize, split: bool) {
        let tab_id = self.tabs[tab_index].id;
        let mut navigation: Option<String> = None;
        let available = ui.available_size();
        let top_space = if split {
            42.0
        } else {
            (available.y * 0.13).clamp(56.0, 130.0)
        };

        ScrollArea::vertical()
            .id_salt(("veil_home", tab_id))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(top_space);
                ui.vertical_centered(|ui| {
                    if let Some(logo) = &self.logo {
                        let size = if split { 58.0 } else { 78.0 };
                        ui.image((logo.id(), egui::vec2(size, size)));
                    }
                    ui.add_space(12.0);
                    ui.label(RichText::new("Veil Browser").strong().size(if split {
                        28.0
                    } else {
                        36.0
                    }));
                    ui.label(
                        RichText::new("A quieter web.")
                            .size(15.0)
                            .color(Color32::from_gray(160)),
                    );
                    ui.add_space(if split { 20.0 } else { 28.0 });

                    let search_width = ui
                        .available_width()
                        .min(if split { 520.0 } else { 680.0 })
                        .max(280.0);
                    self.glass_frame(0.04).show(ui, |ui| {
                        ui.set_min_width(search_width);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("⌕").size(20.0).color(Color32::from_gray(175)));
                            let query = self.home_queries.entry(tab_id).or_default();
                            let response = ui.add_sized(
                                [(search_width - 56.0).max(220.0), 38.0],
                                egui::TextEdit::singleline(query)
                                    .id(egui::Id::new(("veil_home_search", tab_id)))
                                    .hint_text("Search privately or enter a URL")
                                    .frame(false),
                            );
                            if response.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                            {
                                navigation = Some(query.clone());
                            }
                        });
                    });

                    ui.add_space(22.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
                        for (label, target) in [
                            ("▶  YouTube", "https://youtube.com"),
                            ("⌘  GitHub", "https://github.com"),
                            ("W  Wikipedia", "https://wikipedia.org"),
                            ("＋  New tab", HOME),
                        ] {
                            if ui
                                .add_sized([132.0, 46.0], egui::Button::new(label))
                                .clicked()
                            {
                                if target == HOME {
                                    self.new_tab();
                                } else {
                                    navigation = Some(target.into());
                                }
                            }
                        }
                    });

                    ui.add_space(28.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new("◈ Shield active")
                                .small()
                                .color(Color32::from_gray(165)),
                        );
                        ui.separator();
                        ui.label(
                            RichText::new("No telemetry")
                                .small()
                                .color(Color32::from_gray(165)),
                        );
                        ui.separator();
                        ui.label(
                            RichText::new("Session data stays in memory")
                                .small()
                                .color(Color32::from_gray(165)),
                        );
                    });
                });
            });

        if let Some(target) = navigation {
            self.navigate_tab(tab_index, target, true);
        }
    }

    fn render_page_for_tab(
        &mut self,
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        tab_index: usize,
        split: bool,
    ) {
        if tab_index >= self.tabs.len() {
            return;
        }
        if self.tabs[tab_index].page.url == HOME {
            self.render_home(ui, tab_index, split);
            return;
        }
        let page = self.tabs[tab_index].page.clone();
        let base_url = Url::parse(&page.url).ok();
        let privacy = base_url
            .as_ref()
            .map(|url| self.profiles.for_url(url))
            .unwrap_or_default();
        let mut navigation = None;

        if split {
            let active = tab_index == self.active_tab;
            let title = truncate_title(&page.title, 34);
            let chip = if active {
                format!("● {title}")
            } else {
                title
            };
            if ui
                .add(egui::Button::new(chip).frame(false))
                .on_hover_text("Click to make this the active pane")
                .clicked()
            {
                self.activate_tab(tab_index);
            }
            ui.separator();
        }

        ScrollArea::vertical()
            .id_salt(("page", self.tabs[tab_index].id))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(if split { 8.0 } else { 4.0 });
                ui.horizontal(|ui| {
                    ui.add_space(if split { 8.0 } else { 6.0 });
                    ui.vertical(|ui| {
                        ui.set_max_width((ui.available_width() - 20.0).max(280.0).min(1260.0));
                        for block in &page.blocks {
                            self.render_block(
                                ctx,
                                ui,
                                tab_index,
                                block,
                                base_url.as_ref(),
                                privacy,
                                &mut navigation,
                            );
                        }
                        ui.add_space(56.0);
                    });
                });
            });

        if let Some(target) = navigation {
            self.navigate_tab_with_method(tab_index, target.url, true, target.method);
        }
    }

    fn render_block(
        &mut self,
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        tab_index: usize,
        block: &RenderBlock,
        base_url: Option<&Url>,
        privacy: SitePrivacy,
        navigation: &mut Option<PendingNavigation>,
    ) {
        match block {
            RenderBlock::Heading { runs, style, .. } | RenderBlock::Paragraph { runs, style } => {
                with_box(ui, style, |ui| {
                    render_runs(
                        ui,
                        runs,
                        base_url,
                        navigation,
                        style.text_align,
                        &self.web_font_registry,
                    )
                });
            }
            RenderBlock::Container { children, style } => {
                with_box(ui, style, |ui| match style.layout {
                    LayoutMode::Block => {
                        for child in children {
                            self.render_block(
                                ctx, ui, tab_index, child, base_url, privacy, navigation,
                            );
                        }
                    }
                    LayoutMode::FlexColumn => {
                        let mut ordered: Vec<&RenderBlock> = children.iter().collect();
                        ordered.sort_by_key(|child| block_order(child));
                        for (index, child) in ordered.iter().enumerate() {
                            self.render_block(
                                ctx, ui, tab_index, child, base_url, privacy, navigation,
                            );
                            if index + 1 < ordered.len() {
                                ui.add_space(style.gap);
                            }
                        }
                    }
                    LayoutMode::FlexRow => {
                        let mut ordered: Vec<&RenderBlock> = children.iter().collect();
                        ordered.sort_by_key(|child| block_order(child));
                        let mut render_children = |ui: &mut egui::Ui| {
                            for (index, child) in ordered.iter().enumerate() {
                                self.render_block(
                                    ctx, ui, tab_index, child, base_url, privacy, navigation,
                                );
                                if index + 1 < ordered.len() {
                                    ui.add_space(style.gap);
                                }
                            }
                        };
                        if style.flex_wrap {
                            ui.horizontal_wrapped(|ui| render_children(ui));
                        } else if style.justify_content == JustifyContent::Center {
                            ui.horizontal_centered(|ui| render_children(ui));
                        } else {
                            ui.horizontal(|ui| render_children(ui));
                        }
                    }
                    LayoutMode::Grid => {
                        let responsive_cap =
                            ((ui.available_width() / 180.0).floor() as usize).max(1);
                        let columns = style.grid_columns.max(1).min(responsive_cap.max(1));
                        let grid_id = format!("vv_grid_{:p}", children.as_ptr());
                        egui::Grid::new(grid_id)
                            .num_columns(columns)
                            .spacing([style.gap.max(4.0), style.gap.max(4.0)])
                            .show(ui, |ui| {
                                for (index, child) in children.iter().enumerate() {
                                    self.render_block(
                                        ctx, ui, tab_index, child, base_url, privacy, navigation,
                                    );
                                    if (index + 1) % columns == 0 {
                                        ui.end_row();
                                    }
                                }
                            });
                    }
                });
            }
            RenderBlock::Image {
                src,
                alt,
                width,
                height,
                style,
            } => {
                with_box(ui, style, |ui| {
                    self.render_image(ctx, ui, base_url, src, alt, *width, *height, privacy)
                });
            }
            RenderBlock::Canvas {
                width,
                height,
                commands,
                style,
                ..
            } => {
                with_box(ui, style, |ui| render_canvas(ui, *width, *height, commands));
            }
            RenderBlock::Media {
                kind,
                src,
                poster,
                controls,
                muted,
                autoplay,
                width,
                height,
                style,
            } => {
                with_box(ui, style, |ui| {
                    if let Some(poster) = poster {
                        self.render_image(
                            ctx,
                            ui,
                            base_url,
                            poster,
                            "Video poster",
                            *width,
                            *height,
                            privacy,
                        );
                    }
                    ui.label(
                        RichText::new(format!("{} element", kind.to_ascii_uppercase())).strong(),
                    );
                    if src.is_empty() {
                        ui.label(RichText::new("No supported media source was found.").small());
                        return;
                    }
                    let resolved = resolve_href(base_url, src);
                    ui.label(RichText::new(&resolved).small().color(Color32::GRAY));
                    ui.label(
                        RichText::new(format!(
                            "controls: {controls} · muted: {muted} · autoplay: {autoplay}"
                        ))
                        .small()
                        .color(Color32::GRAY),
                    );
                    let Some(top_level) = base_url else {
                        return;
                    };
                    let key = format!("{}|{}", top_level, resolved);
                    match self.media_cache.get(&key).cloned() {
                        Some(CachedMedia::Loading) => {
                            ui.spinner();
                            ui.label("Loading media through Privacy Shield…");
                        }
                        Some(CachedMedia::Ready(probe)) => {
                            ui.label(format!(
                                "{} · {:.1} MiB",
                                probe.format,
                                probe.byte_len as f64 / (1024.0 * 1024.0)
                            ));
                            if let Some(duration) = probe.duration_seconds {
                                ui.label(format!("Duration: {:.2} s", duration));
                            }
                            if !probe.content_type.is_empty() {
                                ui.label(
                                    RichText::new(probe.content_type)
                                        .small()
                                        .color(Color32::GRAY),
                                );
                            }
                            ui.label(RichText::new("0.5 has the privacy-filtered media fetch/probe pipeline; full cross-codec audio/video playback is still being built.").small());
                        }
                        Some(CachedMedia::Failed(error)) => {
                            ui.label(RichText::new(error).small().color(Color32::LIGHT_RED));
                        }
                        None => {
                            if ui.button("Load media").clicked() {
                                if let Ok(url) = Url::parse(&resolved) {
                                    self.media_cache.insert(key.clone(), CachedMedia::Loading);
                                    self.media_loader.start(MediaLoadRequest {
                                        key,
                                        top_level: top_level.clone(),
                                        url,
                                        privacy,
                                        custom_filters: self.custom_filters.clone(),
                                        storage: self.storage.clone(),
                                    });
                                }
                            }
                        }
                    }
                });
            }
            RenderBlock::Form {
                action,
                method,
                enctype,
                controls,
                style,
            } => {
                let mut submit = None;
                with_box(ui, style, |ui| {
                    submit = self
                        .render_form(ui, tab_index, base_url, action, method, enctype, controls);
                });
                if submit.is_some() {
                    *navigation = submit;
                }
            }
            RenderBlock::Rule { style } => with_box(ui, style, |ui| {
                ui.separator();
            }),
            RenderBlock::Code { text, style } => with_box(ui, style, |ui| {
                ui.label(RichText::new(text).monospace().size(style.font_size));
            }),
            RenderBlock::Notice(text) => {
                self.glass_frame(0.10).show(ui, |ui| {
                    ui.label(RichText::new(text).strong());
                });
                ui.add_space(10.0);
            }
        }
    }

    fn render_form(
        &mut self,
        ui: &mut egui::Ui,
        tab_index: usize,
        base_url: Option<&Url>,
        action: &str,
        method: &str,
        enctype: &str,
        controls: &[FormControl],
    ) -> Option<PendingNavigation> {
        let tab_id = self.tabs[tab_index].id;
        let generation = self.tabs[tab_index].generation;
        let mut submitted_by = None;
        let mut runtime_events = Vec::new();

        ui.vertical(|ui| {
            for control in controls {
                let key = (tab_id, generation, control.node_id);
                match control.kind {
                    FormControlKind::Hidden => {
                        self.form_values
                            .entry(key)
                            .or_insert_with(|| control.value.clone());
                    }
                    FormControlKind::Checkbox => {
                        let value = self.form_checks.entry(key).or_insert(control.checked);
                        let label = if control.label.is_empty() {
                            control.name.as_str()
                        } else {
                            control.label.as_str()
                        };
                        let response = ui.checkbox(value, label);
                        if response.changed() {
                            runtime_events.push(DomEventRequest {
                                node_id: control.node_id,
                                event_type: "change".into(),
                                value: Some(if *value { "true" } else { "false" }.into()),
                            });
                        }
                    }
                    FormControlKind::Submit => {
                        let label = if control.label.trim().is_empty() {
                            "Submit"
                        } else {
                            control.label.as_str()
                        };
                        if ui.button(label).clicked() {
                            runtime_events.push(DomEventRequest {
                                node_id: control.node_id,
                                event_type: "click".into(),
                                value: None,
                            });
                            submitted_by = Some(control.node_id);
                        }
                    }
                    FormControlKind::Button => {
                        let label = if control.label.trim().is_empty() {
                            "Button"
                        } else {
                            control.label.as_str()
                        };
                        if ui.button(label).clicked() {
                            runtime_events.push(DomEventRequest {
                                node_id: control.node_id,
                                event_type: "click".into(),
                                value: None,
                            });
                        }
                    }
                    FormControlKind::File => {
                        let value = self.form_values.entry(key).or_default();
                        ui.horizontal(|ui| {
                            ui.label("File");
                            ui.add(
                                egui::TextEdit::singleline(value)
                                    .desired_width(360.0)
                                    .hint_text("Local file path"),
                            );
                        });
                        ui.label(
                            RichText::new(
                                "Veil Browser reads this file only when you submit this form.",
                            )
                            .small()
                            .color(Color32::GRAY),
                        );
                    }
                    FormControlKind::Password
                    | FormControlKind::Text
                    | FormControlKind::Search
                    | FormControlKind::Email
                    | FormControlKind::Url => {
                        let value = self
                            .form_values
                            .entry(key)
                            .or_insert_with(|| control.value.clone());
                        let mut edit = egui::TextEdit::singleline(value).desired_width(420.0);
                        if !control.placeholder.is_empty() {
                            edit = edit.hint_text(&control.placeholder);
                        }
                        if control.kind == FormControlKind::Password {
                            edit = edit.password(true);
                        }
                        let response = ui.add(edit);
                        if response.changed() {
                            runtime_events.push(DomEventRequest {
                                node_id: control.node_id,
                                event_type: "input".into(),
                                value: Some(value.clone()),
                            });
                        }
                        if response.lost_focus() {
                            runtime_events.push(DomEventRequest {
                                node_id: control.node_id,
                                event_type: "change".into(),
                                value: Some(value.clone()),
                            });
                        }
                        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            submitted_by = controls
                                .iter()
                                .find(|c| c.kind == FormControlKind::Submit)
                                .map(|c| c.node_id)
                                .or(Some(control.node_id));
                        }
                    }
                }
            }
        });

        for event in runtime_events {
            self.dispatch_runtime_event(tab_index, event);
        }

        let submit_id = submitted_by?;
        let method = method.trim().to_ascii_lowercase();
        if method != "get" && method != "post" {
            self.tabs[tab_index].status = format!("Unsupported form method: {method}");
            return None;
        }

        let target = if action.trim().is_empty() {
            base_url.map(Url::to_string).unwrap_or_else(|| HOME.into())
        } else {
            resolve_href(base_url, action)
        };
        let mut url = Url::parse(&target).ok()?;
        let mut pairs = Vec::new();
        let mut file_inputs: Vec<(String, String)> = Vec::new();
        for control in controls {
            if control.name.is_empty() {
                continue;
            }
            let key = (tab_id, generation, control.node_id);
            match control.kind {
                FormControlKind::Submit => {
                    if control.node_id == submit_id && !control.value.is_empty() {
                        pairs.push((control.name.clone(), control.value.clone()));
                    }
                }
                FormControlKind::Button => {}
                FormControlKind::Checkbox => {
                    if *self.form_checks.get(&key).unwrap_or(&control.checked) {
                        pairs.push((
                            control.name.clone(),
                            if control.value.is_empty() {
                                "on".into()
                            } else {
                                control.value.clone()
                            },
                        ));
                    }
                }
                FormControlKind::File => {
                    let path = self.form_values.get(&key).cloned().unwrap_or_default();
                    if !path.trim().is_empty() {
                        file_inputs.push((control.name.clone(), path));
                    }
                }
                _ => {
                    let value = self
                        .form_values
                        .get(&key)
                        .cloned()
                        .unwrap_or_else(|| control.value.clone());
                    pairs.push((control.name.clone(), value));
                }
            }
        }

        let wants_multipart = method == "post"
            && (enctype.eq_ignore_ascii_case("multipart/form-data") || !file_inputs.is_empty());
        if wants_multipart {
            let mut parts = Vec::new();
            for (name, value) in &pairs {
                parts.push(MultipartPart {
                    name: name.clone(),
                    filename: None,
                    content_type: None,
                    data: value.as_bytes().to_vec(),
                });
            }
            for (name, path) in file_inputs {
                let file_path = Path::new(path.trim());
                let bytes = match std::fs::read(file_path) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        self.tabs[tab_index].status =
                            format!("Could not read upload file: {error}");
                        return None;
                    }
                };
                if bytes.len() > 32 * 1024 * 1024 {
                    self.tabs[tab_index].status =
                        "Upload file exceeds Veil Browser's 32 MiB safety limit".into();
                    return None;
                }
                let filename = file_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("upload.bin")
                    .to_owned();
                parts.push(MultipartPart {
                    name,
                    filename: Some(filename),
                    content_type: Some(guess_mime(file_path).to_owned()),
                    data: bytes,
                });
            }
            Some(PendingNavigation {
                url: url.to_string(),
                method: NavigationMethod::PostMultipart(parts),
            })
        } else if method == "post" {
            for (name, path) in file_inputs {
                let filename = Path::new(path.trim())
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("")
                    .to_owned();
                pairs.push((name, filename));
            }
            let mut serializer = url::form_urlencoded::Serializer::new(String::new());
            for (name, value) in &pairs {
                serializer.append_pair(name, value);
            }
            Some(PendingNavigation {
                url: url.to_string(),
                method: NavigationMethod::PostForm(serializer.finish()),
            })
        } else {
            for (name, path) in file_inputs {
                let filename = Path::new(path.trim())
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("")
                    .to_owned();
                pairs.push((name, filename));
            }
            url.query_pairs_mut().clear().extend_pairs(pairs);
            Some(PendingNavigation {
                url: url.to_string(),
                method: NavigationMethod::Get,
            })
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_image(
        &mut self,
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        base_url: Option<&Url>,
        src: &str,
        alt: &str,
        requested_width: Option<f32>,
        requested_height: Option<f32>,
        privacy: SitePrivacy,
    ) {
        if !privacy.load_images {
            image_placeholder(ui, alt, "Images disabled for this site");
            return;
        }
        let Some(top_level) = base_url else {
            image_placeholder(ui, alt, "Image has no valid page origin");
            return;
        };
        let resolved = resolve_href(Some(top_level), src);
        let Ok(image_url) = Url::parse(&resolved) else {
            image_placeholder(ui, alt, "Invalid image URL");
            return;
        };
        if !matches!(image_url.scheme(), "http" | "https") {
            image_placeholder(ui, alt, "Unsupported image scheme");
            return;
        }

        let cache_key = image_url.to_string();
        if !self.image_cache.contains_key(&cache_key) {
            self.image_cache
                .insert(cache_key.clone(), CachedImage::Loading);
            self.image_load_queue.push_back(ImageLoadRequest {
                key: cache_key.clone(),
                top_level: top_level.clone(),
                url: image_url,
                privacy,
                custom_filters: self.custom_filters.clone(),
                storage: self.storage.clone(),
            });
            ctx.request_repaint_after(Duration::from_millis(40));
        }

        match self.image_cache.get(&cache_key).cloned() {
            Some(CachedImage::Ready { texture, size }) => {
                let natural_w = size[0].max(1) as f32;
                let natural_h = size[1].max(1) as f32;
                let mut width = requested_width.unwrap_or(natural_w);
                let mut height = requested_height.unwrap_or_else(|| natural_h * width / natural_w);
                let max_width = ui.available_width().max(80.0).min(1200.0);
                if width > max_width {
                    let scale = max_width / width;
                    width *= scale;
                    height *= scale;
                }
                width = width.clamp(24.0, 1600.0);
                height = height.clamp(24.0, 1400.0);
                ui.image((texture.id(), egui::vec2(width, height)));
                if !alt.trim().is_empty() {
                    ui.label(RichText::new(alt).small().color(Color32::GRAY));
                }
            }
            Some(CachedImage::Loading) => {
                image_placeholder(ui, alt, "Loading image…");
                ctx.request_repaint_after(Duration::from_millis(40));
            }
            Some(CachedImage::Failed(reason)) => image_placeholder(ui, alt, &reason),
            None => image_placeholder(ui, alt, "Image unavailable"),
        }
    }
}

impl eframe::App for VeilApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.update_hover_reveals(ctx);
        self.poll_loader(ctx);
        self.poll_runtime_loader(ctx);
        self.pump_runtime_timers();
        self.poll_image_loader(ctx);
        self.pump_image_queue();
        self.poll_media_loader();
        self.handle_shortcuts(ctx);
        if self.tabs.iter().any(|tab| tab.loading) {
            ctx.request_repaint_after(Duration::from_millis(40));
        }

        shell_ui::render_content(self, ctx);
        shell_ui::render_sidebar(self, ctx);
        shell_ui::render_address_pill(self, ctx);
        shell_ui::render_window_chrome(self, ctx);
        self.render_privacy_panel(ctx);
    }
}

fn sidebar_action(
    ui: &mut egui::Ui,
    expanded: bool,
    icon: &str,
    label: &str,
    shortcut: &str,
    action: impl FnOnce(),
) {
    let clicked = if expanded {
        let text = if shortcut.is_empty() {
            label.to_owned()
        } else {
            format!("{label}    {shortcut}")
        };
        ui.add_sized(
            [216.0, 34.0],
            egui::Button::new(format!("{icon}  {text}")).frame(false),
        )
        .clicked()
    } else {
        ui.add_sized([38.0, 36.0], egui::Button::new(icon))
            .on_hover_text(label)
            .clicked()
    };
    if clicked {
        action();
    }
}

fn render_runs(
    ui: &mut egui::Ui,
    runs: &[TextRun],
    base_url: Option<&Url>,
    navigation: &mut Option<PendingNavigation>,
    align: TextAlign,
    registered_web_fonts: &HashMap<String, Vec<u8>>,
) {
    let render = |ui: &mut egui::Ui, navigation: &mut Option<PendingNavigation>| {
        ui.horizontal_wrapped(|ui| {
            for run in runs {
                let mut text = RichText::new(&run.text).size(run.size);
                if run.bold {
                    text = text.strong();
                }
                if run.italic {
                    text = text.italics();
                }
                if let Some(name) = &run.font_name {
                    let family_key = name.trim().to_ascii_lowercase();
                    if registered_web_fonts.contains_key(&family_key) {
                        text = text.family(egui::FontFamily::Name(Arc::from(family_key.as_str())));
                    } else if run.font_family == FontKind::Mono {
                        text = text.monospace();
                    }
                } else if run.font_family == FontKind::Mono {
                    text = text.monospace();
                }
                if run.muted {
                    text = text.color(Color32::GRAY);
                } else if let Some(color) = run.color {
                    text = text.color(to_color32(color));
                }
                if let Some(href) = &run.href {
                    if ui.link(text).clicked() {
                        *navigation = Some(PendingNavigation {
                            url: resolve_href(base_url, href),
                            method: NavigationMethod::Get,
                        });
                    }
                } else {
                    ui.label(text);
                }
            }
        });
    };
    match align {
        TextAlign::Center => {
            ui.vertical_centered(|ui| render(ui, navigation));
        }
        TextAlign::Right => {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                render(ui, navigation)
            });
        }
        TextAlign::Left => render(ui, navigation),
    }
}

fn with_box(ui: &mut egui::Ui, style: &ComputedStyle, add_contents: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(style.margin.top.max(0.0));
    ui.horizontal(|ui| {
        ui.add_space(style.margin.left.max(0.0));
        let mut frame = egui::Frame::default();
        if let Some(background) = style.background {
            frame = frame.fill(to_color32(background));
        }
        if style.border_width > 0.0 {
            frame = frame.stroke(egui::Stroke::new(
                style.border_width,
                ui.visuals().widgets.noninteractive.bg_stroke.color,
            ));
        }
        frame.show(ui, |ui| {
            ui.add_space(style.padding.top.max(0.0));
            ui.horizontal(|ui| {
                ui.add_space(style.padding.left.max(0.0));
                ui.vertical(|ui| {
                    if let Some(min_width) = style.min_width {
                        ui.set_min_width(min_width.max(1.0));
                    }
                    if let Some(width) = style.width.or(style.max_width) {
                        ui.set_max_width(width.max(32.0));
                    }
                    add_contents(ui);
                });
                ui.add_space(style.padding.right.max(0.0));
            });
            ui.add_space(style.padding.bottom.max(0.0));
        });
        ui.add_space(style.margin.right.max(0.0));
    });
    ui.add_space(style.margin.bottom.max(0.0));
}

fn block_order(block: &RenderBlock) -> i32 {
    match block {
        RenderBlock::Heading { style, .. }
        | RenderBlock::Paragraph { style, .. }
        | RenderBlock::Container { style, .. }
        | RenderBlock::Image { style, .. }
        | RenderBlock::Canvas { style, .. }
        | RenderBlock::Media { style, .. }
        | RenderBlock::Form { style, .. }
        | RenderBlock::Rule { style }
        | RenderBlock::Code { style, .. } => style.order,
        RenderBlock::Notice(_) => 0,
    }
}

fn guess_mime(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "txt" | "log" | "md" => "text/plain",
        "html" | "htm" => "text/html",
        "json" => "application/json",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" | "oga" => "audio/ogg",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    }
}

fn install_web_fonts(
    ctx: &egui::Context,
    fonts: &[WebFontResource],
    registry: &mut HashMap<String, Vec<u8>>,
) {
    for font in fonts {
        let bytes = font.bytes.as_slice();
        let supported = bytes.starts_with(&[0x00, 0x01, 0x00, 0x00])
            || bytes.starts_with(b"OTTO")
            || bytes.starts_with(b"true")
            || bytes.starts_with(b"typ1");
        if !supported {
            continue;
        }
        let family = font.family.trim().to_ascii_lowercase();
        if family.is_empty() {
            continue;
        }
        registry.insert(family, font.bytes.clone());
    }

    if registry.is_empty() {
        return;
    }

    // Rebuild from every font Veil has actually registered. This keeps named
    // families valid across tabs/split panes and prevents egui from panicking
    // when a site merely requests an uninstalled font such as Helvetica Neue.
    let mut definitions = egui::FontDefinitions::default();
    for (family, bytes) in registry.iter() {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        family.hash(&mut hasher);
        bytes.len().hash(&mut hasher);
        let key = format!("veil_webfont_{:x}", hasher.finish());
        definitions.font_data.insert(
            key.clone(),
            Arc::new(egui::FontData::from_owned(bytes.clone())),
        );
        definitions
            .families
            .entry(egui::FontFamily::Name(Arc::from(family.as_str())))
            .or_default()
            .insert(0, key);
    }
    ctx.set_fonts(definitions);
}

fn render_canvas(ui: &mut egui::Ui, width: f32, height: f32, commands: &[CanvasCommand]) {
    let width = width.clamp(1.0, ui.available_width().max(1.0));
    let height = height.clamp(1.0, 1400.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
    let painter = ui.painter_at(rect);

    for command in commands {
        let fill = parse_css_color(&command.fill).unwrap_or(Color32::WHITE);
        let stroke = parse_css_color(&command.stroke).unwrap_or(Color32::WHITE);
        let min = rect.min + egui::vec2(command.x, command.y);
        match command.op.as_str() {
            "fillRect" => {
                painter.rect_filled(
                    egui::Rect::from_min_size(min, egui::vec2(command.w, command.h)),
                    0.0,
                    fill,
                );
            }
            "strokeRect" => {
                let r = egui::Rect::from_min_size(min, egui::vec2(command.w, command.h));
                painter.line_segment(
                    [r.left_top(), r.right_top()],
                    egui::Stroke::new(1.0_f32, stroke),
                );
                painter.line_segment(
                    [r.right_top(), r.right_bottom()],
                    egui::Stroke::new(1.0_f32, stroke),
                );
                painter.line_segment(
                    [r.right_bottom(), r.left_bottom()],
                    egui::Stroke::new(1.0_f32, stroke),
                );
                painter.line_segment(
                    [r.left_bottom(), r.left_top()],
                    egui::Stroke::new(1.0_f32, stroke),
                );
            }
            "fillText" => {
                painter.text(
                    min,
                    Align2::LEFT_TOP,
                    &command.text,
                    FontId::proportional(command.font_size.max(8.0)),
                    fill,
                );
            }
            "strokeText" => {
                painter.text(
                    min,
                    Align2::LEFT_TOP,
                    &command.text,
                    FontId::proportional(command.font_size.max(8.0)),
                    stroke,
                );
            }
            "line" => {
                let end = rect.min + egui::vec2(command.x2, command.y2);
                painter.line_segment(
                    [min, end],
                    egui::Stroke::new(command.line_width.max(0.5), stroke),
                );
            }
            "fillArc" => {
                painter.circle_filled(min, command.radius.max(0.0), fill);
            }
            "strokeArc" => {
                painter.circle_stroke(
                    min,
                    command.radius.max(0.0),
                    egui::Stroke::new(command.line_width.max(0.5), stroke),
                );
            }
            "clearRect" => {}
            _ => {}
        }
    }
}

fn image_placeholder(ui: &mut egui::Ui, alt: &str, reason: &str) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        let label = if alt.trim().is_empty() { "Image" } else { alt };
        ui.label(RichText::new(label).strong());
        ui.label(RichText::new(reason).small().color(Color32::GRAY));
    });
}

fn security_icon(address: &str) -> &'static str {
    if address.starts_with("https://") {
        "⌾"
    } else if address.starts_with("http://") {
        "!"
    } else {
        "V"
    }
}

fn resolve_href(base: Option<&Url>, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") || href.starts_with("veil://") {
        return href.to_owned();
    }
    if let Some(base) = base {
        if let Ok(joined) = base.join(href) {
            return joined.to_string();
        }
    }
    href.to_owned()
}

fn to_color32(rgba: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3])
}

fn parse_css_color(value: &str) -> Option<Color32> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix('#') {
        return match hex.len() {
            3 => {
                let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
                let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
                let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
                Some(Color32::from_rgb(r, g, b))
            }
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                Some(Color32::from_rgb(r, g, b))
            }
            _ => None,
        };
    }
    match value.to_ascii_lowercase().as_str() {
        "black" => Some(Color32::BLACK),
        "white" => Some(Color32::WHITE),
        "red" => Some(Color32::from_rgb(255, 0, 0)),
        "green" => Some(Color32::from_rgb(0, 128, 0)),
        "blue" => Some(Color32::from_rgb(0, 0, 255)),
        _ => None,
    }
}

fn truncate_title(title: &str, max_chars: usize) -> String {
    let mut chars = title.chars();
    let prefix: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{prefix}…")
    } else if prefix.is_empty() {
        "New tab".into()
    } else {
        prefix
    }
}

fn tab_monogram(tab: &Tab) -> String {
    if tab.page.url == HOME {
        return "V".into();
    }
    if let Ok(url) = Url::parse(&tab.page.url) {
        if let Some(host) = url.host_str() {
            return host
                .trim_start_matches("www.")
                .chars()
                .next()
                .map(|c| c.to_ascii_uppercase().to_string())
                .unwrap_or_else(|| "•".into());
        }
    }
    tab.page
        .title
        .chars()
        .next()
        .map(|c| c.to_string())
        .unwrap_or_else(|| "•".into())
}

fn load_embedded_logo(ctx: &egui::Context) -> Option<TextureHandle> {
    let image = image::load_from_memory(include_bytes!("../assets/veil-glass-icon.png"))
        .ok()?
        .to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
    Some(ctx.load_texture("veil-glass-logo", color, egui::TextureOptions::LINEAR))
}

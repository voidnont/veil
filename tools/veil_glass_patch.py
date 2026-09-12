from pathlib import Path


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected one match, found {count}")
    return text.replace(old, new, 1)


cargo = Path("Cargo.toml").read_text()
cargo = replace_once(
    cargo,
    'httpdate = "1.0"\n\n\n[profile.release]',
    'httpdate = "1.0"\n\n[target.\'cfg(target_os = "windows")\'.dependencies]\nwindow-vibrancy = "0.8"\n\n[profile.release]',
    "Cargo target dependency",
)
Path("Cargo.toml").write_text(cargo)

main = Path("src/main.rs").read_text()
replacements = [
    (
        "use std::collections::{HashMap, VecDeque};",
        "use std::collections::{HashMap, HashSet, VecDeque};",
        "collections import",
    ),
    (
        "const EXPANDED_DOCK_WIDTH: f32 = 260.0;\n",
        "const EXPANDED_DOCK_WIDTH: f32 = 260.0;\nconst HOVER_REVEAL_DELAY: Duration = Duration::from_secs(1);\nconst MAX_IMAGE_LOADS: usize = 6;\n",
        "UI constants",
    ),
    (
        "        .with_min_inner_size([820.0, 560.0])\n        .with_transparent(!safe_ui);",
        "        .with_min_inner_size([820.0, 560.0])\n        .with_transparent(!safe_ui)\n        .with_decorations(false);",
        "borderless viewport",
    ),
    (
        '''        Box::new(|cc| {
            configure_visuals(&cc.egui_ctx);
            Ok(Box::new(VeilApp::new(&cc.egui_ctx)))
        }),''',
        '''        Box::new(|cc| {
            configure_visuals(&cc.egui_ctx);
            #[cfg(target_os = "windows")]
            if !safe_ui {
                if window_vibrancy::apply_acrylic(cc, Some((18, 18, 24, 118))).is_err() {
                    let _ = window_vibrancy::apply_blur(cc, Some((18, 18, 24, 118)));
                }
            }
            Ok(Box::new(VeilApp::new(&cc.egui_ctx)))
        }),''',
        "acrylic setup",
    ),
    (
        '''    visuals.panel_fill = Color32::from_rgba_unmultiplied(20, 21, 24, 246);
    visuals.window_fill = Color32::from_rgba_unmultiplied(24, 25, 29, 232);
    visuals.extreme_bg_color = Color32::from_rgba_unmultiplied(16, 17, 20, 240);''',
        '''    visuals.panel_fill = Color32::from_rgba_unmultiplied(16, 17, 22, 118);
    visuals.window_fill = Color32::from_rgba_unmultiplied(18, 19, 25, 132);
    visuals.extreme_bg_color = Color32::from_rgba_unmultiplied(10, 11, 15, 96);''',
        "glass visuals",
    ),
    (
        '''    runtime_loader: RuntimeInteractionLoader,
    next_runtime_request_id: u64,
    last_runtime_tick: Instant,
    storage:''',
        '''    runtime_loader: RuntimeInteractionLoader,
    next_runtime_request_id: u64,
    last_runtime_tick: Instant,
    runtime_inflight: HashSet<u64>,
    storage:''',
        "runtime inflight field",
    ),
    (
        '''    show_privacy: bool,
    sidebar_pinned: bool,
    active_space: usize,
    image_cache: HashMap<String, CachedImage>,''',
        '''    show_privacy: bool,
    sidebar_pinned: bool,
    sidebar_hover_since: Option<Instant>,
    sidebar_hover_revealed: bool,
    window_controls_hover_since: Option<Instant>,
    window_controls_revealed: bool,
    active_space: usize,
    image_cache: HashMap<String, CachedImage>,
    image_load_queue: VecDeque<ImageLoadRequest>,
    image_loads_inflight: usize,''',
        "hover and image queue fields",
    ),
    (
        '''            runtime_loader: RuntimeInteractionLoader::new(),
            next_runtime_request_id: 1,
            last_runtime_tick: Instant::now(),
            storage,''',
        '''            runtime_loader: RuntimeInteractionLoader::new(),
            next_runtime_request_id: 1,
            last_runtime_tick: Instant::now(),
            runtime_inflight: HashSet::new(),
            storage,''',
        "runtime init",
    ),
    (
        '''            show_privacy: false,
            sidebar_pinned: false,
            active_space: 0,
            image_cache: HashMap::new(),''',
        '''            show_privacy: false,
            sidebar_pinned: false,
            sidebar_hover_since: None,
            sidebar_hover_revealed: false,
            window_controls_hover_since: None,
            window_controls_revealed: false,
            active_space: 0,
            image_cache: HashMap::new(),
            image_load_queue: VecDeque::new(),
            image_loads_inflight: 0,''',
        "hover and image queue init",
    ),
    (
        '''        if !javascript_enabled { return; }

        let request_id = self.next_runtime_request_id;''',
        '''        if !javascript_enabled { return; }
        if self.runtime_inflight.contains(&tab.id) { return; }

        let request_id = self.next_runtime_request_id;''',
        "event inflight guard",
    ),
    (
        '''        self.next_runtime_request_id = self.next_runtime_request_id.saturating_add(1);
        self.runtime_loader.start(RuntimeInteractionRequest {''',
        '''        self.next_runtime_request_id = self.next_runtime_request_id.saturating_add(1);
        self.runtime_inflight.insert(tab.id);
        self.runtime_loader.start(RuntimeInteractionRequest {''',
        "event inflight insert",
    ),
    (
        '''        while let Some(result) = self.runtime_loader.try_recv() {
            let Some(index) = self.tabs.iter().position(|tab| tab.id == result.tab_id) else { continue; };''',
        '''        while let Some(result) = self.runtime_loader.try_recv() {
            self.runtime_inflight.remove(&result.tab_id);
            let Some(index) = self.tabs.iter().position(|tab| tab.id == result.tab_id) else { continue; };''',
        "runtime inflight remove",
    ),
    (
        '''            if !javascript_enabled { continue; }
            let request_id = self.next_runtime_request_id;''',
        '''            if !javascript_enabled { continue; }
            if self.runtime_inflight.contains(&tab.id) { continue; }
            let request_id = self.next_runtime_request_id;''',
        "tick inflight guard",
    ),
    (
        '''            self.next_runtime_request_id = self.next_runtime_request_id.saturating_add(1);
            self.runtime_loader.start(RuntimeInteractionRequest {''',
        '''            self.next_runtime_request_id = self.next_runtime_request_id.saturating_add(1);
            self.runtime_inflight.insert(tab.id);
            self.runtime_loader.start(RuntimeInteractionRequest {''',
        "tick inflight insert",
    ),
    (
        '''    fn poll_image_loader(&mut self, ctx: &egui::Context) {
        while let Some(result) = self.image_loader.try_recv() {
            self.worker_blocked_count += result.blocked_count;''',
        '''    fn poll_image_loader(&mut self, ctx: &egui::Context) {
        while let Some(result) = self.image_loader.try_recv() {
            self.image_loads_inflight = self.image_loads_inflight.saturating_sub(1);
            self.worker_blocked_count += result.blocked_count;''',
        "image inflight decrement",
    ),
    (
        "    fn poll_media_loader(&mut self) {",
        '''    fn pump_image_queue(&mut self) {
        while self.image_loads_inflight < MAX_IMAGE_LOADS {
            let Some(request) = self.image_load_queue.pop_front() else { break; };
            self.image_loads_inflight += 1;
            self.image_loader.start(request);
        }
    }

    fn poll_media_loader(&mut self) {''',
        "image queue pump",
    ),
    (
        '''    fn sidebar_expanded(&self, ctx: &egui::Context) -> bool {
        if self.sidebar_pinned {
            return true;
        }
        ctx.input(|i| i.pointer.hover_pos())
            .map(|pos| pos.x <= EXPANDED_DOCK_WIDTH + 20.0)
            .unwrap_or(false)
    }''',
        '''    fn update_hover_reveals(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let pointer = ctx.input(|i| i.pointer.hover_pos());

        if self.sidebar_pinned {
            self.sidebar_hover_since = None;
            self.sidebar_hover_revealed = true;
        } else {
            let over_left_edge = pointer.map(|pos| pos.x <= 14.0).unwrap_or(false);
            let over_open_sidebar = self.sidebar_hover_revealed
                && pointer.map(|pos| pos.x <= EXPANDED_DOCK_WIDTH + 24.0).unwrap_or(false);
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
        self.sidebar_pinned || self.sidebar_hover_revealed
    }''',
        "delayed hover logic",
    ),
    (
        '''                    self.image_loader.start(ImageLoadRequest {
                        key: icon_url.clone(),
                        top_level,
                        url,
                        privacy,
                        custom_filters: self.custom_filters.clone(),
                        storage: self.storage.clone(),
                    });''',
        '''                    self.image_load_queue.push_back(ImageLoadRequest {
                        key: icon_url.clone(),
                        top_level,
                        url,
                        privacy,
                        custom_filters: self.custom_filters.clone(),
                        storage: self.storage.clone(),
                    });''',
        "favicon queue",
    ),
    (
        '''            self.image_loader.start(ImageLoadRequest {
                key: cache_key.clone(),
                top_level: top_level.clone(),
                url: image_url,
                privacy,
                custom_filters: self.custom_filters.clone(),
                storage: self.storage.clone(),
            });''',
        '''            self.image_load_queue.push_back(ImageLoadRequest {
                key: cache_key.clone(),
                top_level: top_level.clone(),
                url: image_url,
                privacy,
                custom_filters: self.custom_filters.clone(),
                storage: self.storage.clone(),
            });''',
        "page image queue",
    ),
    (
        '''    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_loader(ctx);
        self.poll_runtime_loader(ctx);
        self.pump_runtime_timers();
        self.poll_image_loader(ctx);
        self.poll_media_loader();
        self.handle_shortcuts(ctx);''',
        '''    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.update_hover_reveals(ctx);
        self.poll_loader(ctx);
        self.poll_runtime_loader(ctx);
        self.pump_runtime_timers();
        self.poll_image_loader(ctx);
        self.pump_image_queue();
        self.poll_media_loader();
        self.handle_shortcuts(ctx);''',
        "update pumps",
    ),
    (
        '''        shell_ui::render_address_pill(self, ctx);
        self.render_privacy_panel(ctx);''',
        '''        shell_ui::render_address_pill(self, ctx);
        shell_ui::render_window_chrome(self, ctx);
        self.render_privacy_panel(ctx);''',
        "window chrome render",
    ),
]
for old, new, label in replacements:
    main = replace_once(main, old, new, label)
Path("src/main.rs").write_text(main)

loader = Path("src/loader.rs").read_text()
loader = replace_once(
    loader,
    "const MAX_SCRIPTS_PER_PAGE: usize = 24;\nconst MAX_WEB_FONTS_PER_PAGE: usize = 8;",
    "const MAX_SCRIPTS_PER_PAGE: usize = 12;\nconst MAX_WEB_FONTS_PER_PAGE: usize = 8;\nconst MAX_TOTAL_STYLESHEET_BYTES: usize = 6 * 1024 * 1024;\nconst MAX_TOTAL_SCRIPT_BYTES: usize = 8 * 1024 * 1024;\nconst HEAVY_DOCUMENT_BYTES: usize = 1024 * 1024;",
    "loader budgets",
)
loader = replace_once(
    loader,
    "            let result = load_document(&mut network, &request);",
    '''            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                load_document(&mut network, &request)
            }))
            .unwrap_or_else(|_| Err("Veil recovered from an internal page-load panic.".into()));''',
    "loader panic containment",
)
loader = replace_once(
    loader,
    '''    let mut external_css = Vec::new();
    let mut web_fonts = Vec::new();
    let mut seen_fonts = HashSet::new();''',
    '''    let mut external_css = Vec::new();
    let mut external_css_bytes = 0usize;
    let mut web_fonts = Vec::new();
    let mut seen_fonts = HashSet::new();''',
    "css byte counter",
)
loader = replace_once(
    loader,
    '''        if let Ok(resource) = network.get_stylesheet(&final_url, &stylesheet, request.privacy) {
            if web_fonts.len() < MAX_WEB_FONTS_PER_PAGE {''',
    '''        if let Ok(resource) = network.get_stylesheet(&final_url, &stylesheet, request.privacy) {
            if external_css_bytes.saturating_add(resource.body.len()) > MAX_TOTAL_STYLESHEET_BYTES { break; }
            external_css_bytes = external_css_bytes.saturating_add(resource.body.len());
            if web_fonts.len() < MAX_WEB_FONTS_PER_PAGE {''',
    "css total budget",
)
loader = replace_once(
    loader,
    '''    let script_urls = discovery.discover_external_scripts(&response.body, &final_url);
    let external_script_count = script_urls.len().min(MAX_SCRIPTS_PER_PAGE);
    let mut external_scripts = Vec::new();
    if request.privacy.javascript {
        for script in script_urls.into_iter().take(MAX_SCRIPTS_PER_PAGE) {
            if let Ok(resource) = network.get_script(&final_url, &script, request.privacy) {
                external_scripts.push(resource.body);
            }
        }
    }''',
    '''    let script_urls = discovery.discover_external_scripts(&response.body, &final_url);
    let heavy_document = response.body.len() >= HEAVY_DOCUMENT_BYTES || script_urls.len() > 16;
    let script_limit = if heavy_document { 4 } else { MAX_SCRIPTS_PER_PAGE };
    let script_byte_limit = if heavy_document { 2 * 1024 * 1024 } else { MAX_TOTAL_SCRIPT_BYTES };
    let mut external_scripts = Vec::new();
    let mut external_script_bytes = 0usize;
    if request.privacy.javascript {
        for script in script_urls.into_iter().take(script_limit) {
            if let Ok(resource) = network.get_script(&final_url, &script, request.privacy) {
                if external_script_bytes.saturating_add(resource.body.len()) > script_byte_limit { break; }
                external_script_bytes = external_script_bytes.saturating_add(resource.body.len());
                external_scripts.push(resource.body);
            }
        }
    }
    let external_script_count = external_scripts.len();''',
    "heavy script budget",
)
Path("src/loader.rs").write_text(loader)

host = Path("src/renderer_host.rs").read_text()
host = replace_once(
    host,
    '''            Err(process_error) => {
                let view = render_in_process(request).map_err(|fallback_error| {''',
    '''            Err(process_error) => {
                if !in_process_fallback_is_safe(&request) {
                    return Err(format!(
                        "isolated Veil Engine failed on a heavyweight page; main browser process was kept protected: {process_error}"
                    ));
                }
                let view = render_in_process(request).map_err(|fallback_error| {''',
    "safe renderer fallback",
)
host = replace_once(
    host,
    "fn render_in_process(request: RenderRequest) -> Result<DocumentView, String> {",
    '''fn in_process_fallback_is_safe(request: &RenderRequest) -> bool {
    let total = request
        .html
        .len()
        .saturating_add(request.external_css.iter().map(String::len).sum::<usize>())
        .saturating_add(request.external_scripts.iter().map(String::len).sum::<usize>());
    total <= 3 * 1024 * 1024 && request.external_scripts.len() <= 6
}

fn render_in_process(request: RenderRequest) -> Result<DocumentView, String> {''',
    "fallback safety helper",
)
Path("src/renderer_host.rs").write_text(host)

shell = Path("src/shell_ui.rs").read_text()
shell = replace_once(
    shell,
    '''const SHELL_BG: Color32 = Color32::from_rgb(10, 11, 14);
const SIDEBAR_BG: Color32 = Color32::from_rgba_premultiplied(18, 19, 24, 248);
const TOOLBAR_BG: Color32 = Color32::from_rgba_premultiplied(20, 21, 27, 250);
const PAGE_BG: Color32 = Color32::from_rgb(15, 16, 20);
const SURFACE: Color32 = Color32::from_rgba_premultiplied(10, 10, 10, 10);
const SURFACE_HOVER: Color32 = Color32::from_rgba_premultiplied(18, 18, 18, 18);
const BORDER: Color32 = Color32::from_rgba_premultiplied(22, 22, 22, 22);''',
    '''const SHELL_BG: Color32 = Color32::TRANSPARENT;
const SIDEBAR_BG: Color32 = Color32::from_rgba_premultiplied(11, 12, 16, 150);
const TOOLBAR_BG: Color32 = Color32::from_rgba_premultiplied(13, 14, 19, 164);
const PAGE_BG: Color32 = Color32::from_rgba_premultiplied(9, 10, 13, 134);
const SURFACE: Color32 = Color32::from_rgba_premultiplied(14, 14, 16, 22);
const SURFACE_HOVER: Color32 = Color32::from_rgba_premultiplied(22, 22, 25, 36);
const BORDER: Color32 = Color32::from_rgba_premultiplied(38, 38, 42, 48);''',
    "glass constants",
)
shell = replace_once(shell, "    let sidebar = sidebar_width(app, ctx);", "    let sidebar = COLLAPSED_DOCK_WIDTH;", "stable content inset")
shell = replace_once(
    shell,
    '''    let sidebar = sidebar_width(app, ctx);
    let left = sidebar + 28.0;
    let available = (screen.width() - left - 20.0).max(360.0);
    let width = available.min(980.0);
    let x = left + (available - width) * 0.5;''',
    '''    let width = (screen.width() - 180.0).clamp(420.0, 980.0);
    let x = screen.center().x - width * 0.5;''',
    "centered address pill",
)
chrome = r'''
pub(crate) fn render_window_chrome(app: &mut VeilApp, ctx: &egui::Context) {
    let screen = ctx.screen_rect();

    egui::Area::new(egui::Id::new("veil_window_drag_strip"))
        .order(egui::Order::Background)
        .fixed_pos(egui::pos2(0.0, 0.0))
        .show(ctx, |ui| {
            let response = ui.allocate_response(egui::vec2(screen.width(), 9.0), Sense::click_and_drag());
            if response.drag_started() {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
        });

    if !app.window_controls_revealed {
        return;
    }

    let width = 126.0;
    let x = (screen.right() - width - 10.0).max(0.0);
    egui::Area::new(egui::Id::new("veil_window_controls"))
        .order(egui::Order::Tooltip)
        .fixed_pos(egui::pos2(x, 8.0))
        .show(ctx, |ui| {
            egui::Frame::default()
                .fill(Color32::from_rgba_unmultiplied(17, 18, 24, 172))
                .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 34)))
                .corner_radius(13)
                .inner_margin(3)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui.add_sized([34.0, 30.0], egui::Button::new("—").frame(false)).on_hover_text("Minimize").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }
                        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                        if ui.add_sized([34.0, 30.0], egui::Button::new(if maximized { "❐" } else { "□" }).frame(false)).on_hover_text(if maximized { "Restore" } else { "Maximize" }).clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                        }
                        if ui.add_sized([34.0, 30.0], egui::Button::new("×").frame(false)).on_hover_text("Close").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                });
        });
}

'''
shell = replace_once(
    shell,
    "pub(crate) fn render_address_pill(app: &mut VeilApp, ctx: &egui::Context) {",
    chrome + "pub(crate) fn render_address_pill(app: &mut VeilApp, ctx: &egui::Context) {",
    "window chrome function",
)
Path("src/shell_ui.rs").write_text(shell)

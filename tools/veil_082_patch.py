from pathlib import Path


def replace_once(text, old, new, label):
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected one match, found {count}")
    return text.replace(old, new, 1)


# Cargo: version bump and make release panics recoverable by worker containment.
cargo_path = Path("Cargo.toml")
cargo = cargo_path.read_text()
cargo = replace_once(cargo, 'version = "0.8.1"', 'version = "0.8.2"', "cargo version")
cargo = replace_once(cargo, 'panic = "abort"', 'panic = "unwind"', "release panic strategy")
cargo_path.write_text(cargo)

# Main shell/runtime behavior.
main_path = Path("src/main.rs")
main = main_path.read_text()
main = replace_once(
    main,
    '''fn safe_ui_requested() -> bool {
    std::env::args().any(|arg| arg == "--safe-ui") || std::env::var_os("VEIL_SAFE_UI").is_some()
}
''',
    '''fn safe_ui_requested() -> bool {
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
''',
    "guarded site helper",
)
main = replace_once(main, "            sidebar_pinned: false,", "            sidebar_pinned: true,", "sidebar pinned default")
old_js = '''            let javascript_enabled = Url::parse(&tab.page.url)
                .ok()
                .map(|url| self.profiles.for_url(&url).javascript)
                .unwrap_or(false);'''
new_js = '''            let javascript_enabled = !is_guarded_script_site(&tab.page.url)
                && Url::parse(&tab.page.url)
                    .ok()
                    .map(|url| self.profiles.for_url(&url).javascript)
                    .unwrap_or(false);'''
count = main.count(old_js)
if count != 2:
    raise SystemExit(f"runtime javascript guards: expected two matches, found {count}")
main = main.replace(old_js, new_js)
main = replace_once(
    main,
    '''    fn sidebar_expanded(&self, _ctx: &egui::Context) -> bool {
        self.sidebar_pinned || self.sidebar_hover_revealed
    }''',
    '''    fn sidebar_expanded(&self, _ctx: &egui::Context) -> bool {
        true
    }''',
    "permanent sidebar",
)
main_path.write_text(main)

# Shell: fixed rail, properly centered address bar, and visible glass on both rails.
shell_path = Path("src/shell_ui.rs")
shell = shell_path.read_text()
shell = replace_once(
    shell,
    '''fn sidebar_progress(app: &VeilApp, ctx: &egui::Context) -> f32 {
    ctx.animate_bool_with_time(
        egui::Id::new("veil_sidebar_animation_v2"),
        app.sidebar_expanded(ctx),
        0.16,
    )
}''',
    '''fn sidebar_progress(_app: &VeilApp, _ctx: &egui::Context) -> f32 {
    1.0
}''',
    "fixed sidebar progress",
)
shell = replace_once(
    shell,
    "    let sidebar = COLLAPSED_DOCK_WIDTH;",
    "    let sidebar = EXPANDED_DOCK_WIDTH + 8.0;",
    "content sidebar inset",
)
shell = replace_once(
    shell,
    '''    let t = sidebar_progress(app, ctx);
    let expanded = t > 0.48;
    let width = COLLAPSED_DOCK_WIDTH + (EXPANDED_DOCK_WIDTH - COLLAPSED_DOCK_WIDTH) * t;
    let height = (ctx.screen_rect().height() - 16.0).max(320.0);''',
    '''    let expanded = true;
    let width = EXPANDED_DOCK_WIDTH;
    let height = (ctx.screen_rect().height() - 16.0).max(320.0);''',
    "fixed sidebar width",
)
shell = replace_once(
    shell,
    ".fill(SIDEBAR_BG)",
    ".fill(Color32::from_rgba_unmultiplied(18, 20, 28, 118))",
    "sidebar glass fill",
)
shell = replace_once(
    shell,
    '''                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let pin = if app.sidebar_pinned { "◆" } else { "◇" };
                                    if ui
                                        .add_sized(
                                            [28.0, 28.0],
                                            egui::Button::new(pin).frame(false),
                                        )
                                        .on_hover_text("Keep sidebar open")
                                        .clicked()
                                    {
                                        app.sidebar_pinned = !app.sidebar_pinned;
                                    }
                                },
                            );''',
    '''                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new("◈")
                                            .size(13.0)
                                            .color(Color32::from_rgba_unmultiplied(210, 205, 235, 190)),
                                    )
                                    .on_hover_text("Sidebar fixed open");
                                },
                            );''',
    "remove sidebar pin toggle",
)
shell = replace_once(
    shell,
    '''pub(crate) fn render_address_pill(app: &mut VeilApp, ctx: &egui::Context) {
    let screen = ctx.screen_rect();
    let width = (screen.width() - 180.0).clamp(420.0, 980.0);
    let x = screen.center().x - width * 0.5;
    let address_id = egui::Id::new("veil_address_bar");
    let focused = ctx.memory(|memory| memory.has_focus(address_id));

    egui::Area::new(egui::Id::new("veil_toolbar_v2"))''',
    '''pub(crate) fn render_address_pill(app: &mut VeilApp, ctx: &egui::Context) {
    let screen = ctx.screen_rect();
    let sidebar_right = 8.0 + EXPANDED_DOCK_WIDTH;
    let content_left = sidebar_right + 16.0;
    let controls_reserve = 154.0;
    let content_right = (screen.right() - controls_reserve).max(content_left + 420.0);
    let available = (content_right - content_left).max(420.0);
    let width = (available - 24.0).clamp(420.0, 980.0).min(available);
    let x = content_left + (available - width) * 0.5;
    let address_id = egui::Id::new("veil_address_bar");
    let focused = ctx.memory(|memory| memory.has_focus(address_id));

    let top_rail_rect = egui::Rect::from_min_max(
        egui::pos2(sidebar_right + 8.0, 8.0),
        egui::pos2(screen.right() - 8.0, 68.0),
    );
    ctx.layer_painter(egui::LayerId::new(
        egui::Order::Middle,
        egui::Id::new("veil_top_glass_rail"),
    ))
    .rect_filled(
        top_rail_rect,
        16.0,
        Color32::from_rgba_unmultiplied(18, 20, 28, 104),
    );

    egui::Area::new(egui::Id::new("veil_toolbar_v2"))''',
    "adaptive centered address bar",
)
shell = replace_once(
    shell,
    ".fill(TOOLBAR_BG)",
    ".fill(Color32::from_rgba_unmultiplied(18, 20, 28, 132))",
    "address glass fill",
)
shell_path.write_text(shell)

# Loader: treat YouTube as a guarded heavy site. Remove inline executable payloads,
# cap the HTML snapshot, disable runtime JS for the snapshot, and reduce CSS budgets.
loader_path = Path("src/loader.rs")
loader = loader_path.read_text()
loader = replace_once(
    loader,
    "const HEAVY_DOCUMENT_BYTES: usize = 1024 * 1024;",
    "const HEAVY_DOCUMENT_BYTES: usize = 1024 * 1024;\nconst GUARDED_RENDER_HTML_BYTES: usize = 2 * 1024 * 1024;",
    "guarded html budget",
)
loader = replace_once(
    loader,
    '''    let final_url = response.final_url.clone();

    // Resource discovery remains deliberately bounded. Full HTML/CSS/JS parsing
    // and render-tree creation happens inside veil-engine when available.
    let discovery = Engine::default();
    let stylesheet_urls = discovery.discover_stylesheets(&response.body, &final_url);''',
    '''    let final_url = response.final_url.clone();
    let guarded_site = is_guarded_heavy_site(&final_url);
    let mut render_privacy = request.privacy;
    let mut render_html = response.body;
    if guarded_site {
        render_privacy.javascript = false;
        render_html = prepare_guarded_html(&render_html);
    }

    // Resource discovery remains deliberately bounded. Full HTML/CSS/JS parsing
    // and render-tree creation happens inside veil-engine when available.
    let discovery = Engine::default();
    let stylesheet_urls = discovery.discover_stylesheets(&render_html, &final_url);''',
    "guarded render setup",
)
loader = replace_once(
    loader,
    '''    let mut web_fonts = Vec::new();
    let mut seen_fonts = HashSet::new();
    for stylesheet in stylesheet_urls.into_iter().take(MAX_STYLESHEETS_PER_PAGE) {
        if let Ok(resource) = network.get_stylesheet(&final_url, &stylesheet, request.privacy) {
            if external_css_bytes.saturating_add(resource.body.len()) > MAX_TOTAL_STYLESHEET_BYTES {
                break;
            }
            external_css_bytes = external_css_bytes.saturating_add(resource.body.len());
            if web_fonts.len() < MAX_WEB_FONTS_PER_PAGE {''',
    '''    let mut web_fonts = Vec::new();
    let mut seen_fonts = HashSet::new();
    let stylesheet_limit = if guarded_site { 8 } else { MAX_STYLESHEETS_PER_PAGE };
    let stylesheet_byte_limit = if guarded_site {
        2 * 1024 * 1024
    } else {
        MAX_TOTAL_STYLESHEET_BYTES
    };
    for stylesheet in stylesheet_urls.into_iter().take(stylesheet_limit) {
        if let Ok(resource) = network.get_stylesheet(&final_url, &stylesheet, render_privacy) {
            if external_css_bytes.saturating_add(resource.body.len()) > stylesheet_byte_limit {
                break;
            }
            external_css_bytes = external_css_bytes.saturating_add(resource.body.len());
            if !guarded_site && web_fonts.len() < MAX_WEB_FONTS_PER_PAGE {''',
    "guarded stylesheet budgets",
)
loader = replace_once(
    loader,
    '''    let script_urls = discovery.discover_external_scripts(&response.body, &final_url);
    let heavy_document = response.body.len() >= HEAVY_DOCUMENT_BYTES || script_urls.len() > 16;''',
    '''    let script_urls = if render_privacy.javascript {
        discovery.discover_external_scripts(&render_html, &final_url)
    } else {
        Vec::new()
    };
    let heavy_document = render_html.len() >= HEAVY_DOCUMENT_BYTES || script_urls.len() > 16;''',
    "guarded script discovery",
)
loader = replace_once(
    loader,
    "    if request.privacy.javascript {",
    "    if render_privacy.javascript {",
    "render javascript flag",
)
loader = replace_once(
    loader,
    '''        html: response.body,
        privacy: request.privacy,''',
    '''        html: render_html,
        privacy: render_privacy,''',
    "render request guarded content",
)
loader += '''

fn is_guarded_heavy_site(url: &Url) -> bool {
    url.host_str()
        .map(str::to_ascii_lowercase)
        .map(|host| {
            host == "youtube.com"
                || host.ends_with(".youtube.com")
                || host == "youtu.be"
                || host.ends_with(".youtu.be")
        })
        .unwrap_or(false)
}

fn prepare_guarded_html(html: &str) -> String {
    cap_render_html(strip_script_blocks(html))
}

fn strip_script_blocks(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len().min(GUARDED_RENDER_HTML_BYTES));
    let mut cursor = 0usize;

    while cursor < html.len() {
        let Some(relative_start) = lower[cursor..].find("<script") else {
            out.push_str(&html[cursor..]);
            break;
        };
        let start = cursor + relative_start;
        out.push_str(&html[cursor..start]);

        let Some(relative_close) = lower[start..].find("</script") else {
            break;
        };
        let close_start = start + relative_close;
        let Some(relative_end) = lower[close_start..].find('>') else {
            break;
        };
        cursor = close_start + relative_end + 1;
    }

    out
}

fn cap_render_html(html: String) -> String {
    if html.len() <= GUARDED_RENDER_HTML_BYTES {
        return html;
    }

    let mut end = GUARDED_RENDER_HTML_BYTES.min(html.len());
    while end > 0 && !html.is_char_boundary(end) {
        end -= 1;
    }
    if let Some(boundary) = html[..end].rfind('>') {
        end = boundary + 1;
    }

    let mut capped = String::with_capacity(end + 32);
    capped.push_str(&html[..end]);
    capped.push_str("\n</body></html>");
    capped
}

#[cfg(test)]
mod guarded_tests {
    use super::*;

    #[test]
    fn youtube_is_guarded() {
        assert!(is_guarded_heavy_site(&Url::parse("https://www.youtube.com/").unwrap()));
        assert!(is_guarded_heavy_site(&Url::parse("https://music.youtube.com/").unwrap()));
        assert!(!is_guarded_heavy_site(&Url::parse("https://example.com/").unwrap()));
    }

    #[test]
    fn guarded_html_drops_scripts() {
        let html = "<html><body><h1>keep</h1><script>throw new Error('boom')</script><p>also keep</p></body></html>";
        let safe = prepare_guarded_html(html);
        assert!(safe.contains("keep"));
        assert!(safe.contains("also keep"));
        assert!(!safe.to_ascii_lowercase().contains("<script"));
        assert!(!safe.contains("boom"));
    }
}
'''
loader_path.write_text(loader)

# Image worker: with unwind-enabled release builds, contain decoder/worker panics
# and reject absurd pixel counts before RGBA expansion.
image_path = Path("src/image_loader.rs")
image = image_path.read_text()
image = replace_once(
    image,
    '''            let result = network
                .get_image(&request.top_level, &request.url, request.privacy)
                .and_then(|response| {
                    let decoded = image::load_from_memory(&response.bytes)
                        .map_err(|e| format!("Image decode failed: {e}"))?;
                    let rgba = decoded.to_rgba8();
                    Ok(DecodedImage {
                        final_url: response.final_url.to_string(),
                        size: [rgba.width() as usize, rgba.height() as usize],
                        rgba: rgba.into_raw(),
                    })
                });''',
    '''            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                network
                    .get_image(&request.top_level, &request.url, request.privacy)
                    .and_then(|response| {
                        let decoded = image::load_from_memory(&response.bytes)
                            .map_err(|e| format!("Image decode failed: {e}"))?;
                        let width = decoded.width() as usize;
                        let height = decoded.height() as usize;
                        let pixels = width
                            .checked_mul(height)
                            .ok_or_else(|| "Image dimensions overflowed Veil's safety limit.".to_owned())?;
                        if pixels > 16_000_000 {
                            return Err("Image exceeds Veil's 16 megapixel safety limit.".into());
                        }
                        let rgba = decoded.to_rgba8();
                        Ok(DecodedImage {
                            final_url: response.final_url.to_string(),
                            size: [rgba.width() as usize, rgba.height() as usize],
                            rgba: rgba.into_raw(),
                        })
                    })
            }))
            .unwrap_or_else(|_| Err("Veil recovered from an image worker panic.".into()));''',
    "image panic containment",
)
image_path.write_text(image)

# Keep the network identity in sync with the build version.
net_path = Path("src/net.rs")
net = net_path.read_text()
net = replace_once(
    net,
    'Mozilla/5.0 (Veil; privacy) VeilBrowser/0.8.0 VeilEngine/0.8.0',
    'Mozilla/5.0 (Veil; privacy) VeilBrowser/0.8.2 VeilEngine/0.8.2',
    "user agent version",
)
net_path.write_text(net)

from pathlib import Path


def replace_once(text, old, new, label):
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected one match, found {count}")
    return text.replace(old, new, 1)


# Version bump.
cargo_path = Path("Cargo.toml")
cargo = cargo_path.read_text()
cargo = replace_once(cargo, 'version = "0.8.2"', 'version = "0.8.3"', "cargo version")
cargo_path.write_text(cargo)

net_path = Path("src/net.rs")
net = net_path.read_text()
net = net.replace("VeilBrowser/0.8.2 VeilEngine/0.8.2", "VeilBrowser/0.8.3 VeilEngine/0.8.3")
net_path.write_text(net)

workflow_path = Path(".github/workflows/veil.yml")
workflow = workflow_path.read_text()
workflow = replace_once(workflow, "AppVersion=0.8.2", "AppVersion=0.8.3", "installer version")
workflow_path.write_text(workflow)

# Main app: reveal the entire top chrome after a one-second top-edge hover,
# not just the window buttons. Keep Ctrl+L/Ctrl+K able to reveal it immediately.
main_path = Path("src/main.rs")
main = main_path.read_text()
main = replace_once(
    main,
    '''        if ctrl && ctx.input(|i| i.key_pressed(egui::Key::L)) {
            ctx.memory_mut(|mem| mem.request_focus(egui::Id::new("veil_address_bar")));
        }''',
    '''        if ctrl && ctx.input(|i| i.key_pressed(egui::Key::L)) {
            self.window_controls_revealed = true;
            self.window_controls_hover_since = None;
            ctx.memory_mut(|mem| mem.request_focus(egui::Id::new("veil_address_bar")));
        }''',
    "ctrl-l top chrome reveal",
)
main = replace_once(
    main,
    '''        if ctrl && ctx.input(|i| i.key_pressed(egui::Key::K)) {
            let tab_id = self.tabs[self.active_tab].id;
            let target = if self.tabs[self.active_tab].page.url == HOME {
                egui::Id::new(("veil_home_search", tab_id))
            } else {
                egui::Id::new("veil_address_bar")
            };
            ctx.memory_mut(|mem| mem.request_focus(target));
        }''',
    '''        if ctrl && ctx.input(|i| i.key_pressed(egui::Key::K)) {
            let tab_id = self.tabs[self.active_tab].id;
            let target = if self.tabs[self.active_tab].page.url == HOME {
                egui::Id::new(("veil_home_search", tab_id))
            } else {
                self.window_controls_revealed = true;
                self.window_controls_hover_since = None;
                egui::Id::new("veil_address_bar")
            };
            ctx.memory_mut(|mem| mem.request_focus(target));
        }''',
    "ctrl-k top chrome reveal",
)
main = replace_once(
    main,
    '''        let screen = ctx.screen_rect();
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
        }''',
    '''        let over_top_edge = pointer.map(|pos| pos.y <= 12.0).unwrap_or(false);
        let over_open_top = self.window_controls_revealed
            && pointer.map(|pos| pos.y <= 76.0).unwrap_or(false);
        if over_open_top {
            self.window_controls_hover_since = None;
        } else if over_top_edge {
            let started = self.window_controls_hover_since.get_or_insert(now);
            if now.duration_since(*started) >= HOVER_REVEAL_DELAY {
                self.window_controls_revealed = true;
                self.window_controls_hover_since = None;
            } else {
                ctx.request_repaint_after(Duration::from_millis(40));
            }
        } else {
            self.window_controls_hover_since = None;
            self.window_controls_revealed = false;
        }''',
    "whole top chrome hover reveal",
)

# Add icons to the settings/privacy controls and sections.
settings_replacements = [
    ('ui.heading("Privacy Shield");', 'ui.heading("◈  Privacy Shield");'),
    ('ui.checkbox(&mut settings.shields, "Block ads & trackers");', 'ui.checkbox(&mut settings.shields, "◈  Block ads & trackers");'),
    ('ui.checkbox(&mut settings.block_third_party, "Strict third-party blocking");', 'ui.checkbox(&mut settings.block_third_party, "⊘  Strict third-party blocking");'),
    ('ui.checkbox(&mut settings.load_images, "Load images");', 'ui.checkbox(&mut settings.load_images, "▧  Load images");'),
    ('ui.checkbox(&mut settings.javascript, "Run JavaScript VM");', 'ui.checkbox(&mut settings.javascript, "⌘  Run JavaScript VM");'),
    ('ui.button("Reset site privacy")', 'ui.button("↺  Reset site privacy")'),
    ('ui.button("Clear this site\'s in-memory data")', 'ui.button("⌫  Clear this site\'s in-memory data")'),
    ('ui.strong("Glass shell");', 'ui.strong("◫  Glass shell");'),
    ('.text("Glass transparency")', '.text("◐  Glass transparency")'),
    ('ui.strong("Fingerprint reduction");', 'ui.strong("◎  Fingerprint reduction");'),
    ('ui.strong("Ad/tracker filter engine");', 'ui.strong("≡  Ad/tracker filter engine");'),
    ('ui.collapsing("Custom filter list", |ui| {', 'ui.collapsing("✎  Custom filter list", |ui| {'),
    ('ui.button("Apply filters")', 'ui.button("✓  Apply filters")'),
    ('ui.strong("Current page");', 'ui.strong("▤  Current page");'),
    ('ui.button("Clear image cache")', 'ui.button("▧  Clear image cache")'),
    ('ui.button("Clear all session data")', 'ui.button("⌫  Clear all session data")'),
    ('ui.strong("Recent blocks");', 'ui.strong("☷  Recent blocks");'),
]
for old, new in settings_replacements:
    if old not in main:
        raise SystemExit(f"settings icon target missing: {old}")
    main = main.replace(old, new, 1)
main_path.write_text(main)

# Shell: the address/search rail is part of the top chrome reveal.
shell_path = Path("src/shell_ui.rs")
shell = shell_path.read_text()
shell = replace_once(
    shell,
    '''pub(crate) fn render_address_pill(app: &mut VeilApp, ctx: &egui::Context) {
    let screen = ctx.screen_rect();''',
    '''pub(crate) fn render_address_pill(app: &mut VeilApp, ctx: &egui::Context) {
    if !app.window_controls_revealed {
        return;
    }
    let screen = ctx.screen_rect();''',
    "hide whole top chrome until reveal",
)
shell_path.write_text(shell)

# Loader: recover a bounded static YouTube feed from ytInitialData before stripping
# executable scripts. The recovered cards are normal Veil HTML/images and do not
# execute YouTube runtime code.
loader_path = Path("src/loader.rs")
loader = loader_path.read_text()
loader = replace_once(
    loader,
    'use url::Url;\n',
    'use serde_json::Value;\nuse url::Url;\n',
    "serde json import",
)
loader = replace_once(
    loader,
    '''fn prepare_guarded_html(html: &str) -> String {
    cap_render_html(strip_script_blocks(html))
}''',
    '''fn prepare_guarded_html(html: &str) -> String {
    let gallery = youtube_static_gallery(html);
    let safe = cap_render_html(strip_script_blocks(html));
    if gallery.is_empty() {
        safe
    } else {
        inject_before_body_end(safe, &gallery)
    }
}

#[derive(Debug, Clone)]
struct YoutubeCard {
    video_id: String,
    title: String,
    thumbnail: String,
}

fn youtube_static_gallery(html: &str) -> String {
    let Some(raw_json) = extract_yt_initial_data(html) else {
        return String::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(raw_json) else {
        return String::new();
    };

    let mut cards = Vec::new();
    let mut seen = HashSet::new();
    collect_youtube_cards(&value, &mut cards, &mut seen);
    if cards.is_empty() {
        return String::new();
    }

    let mut out = String::from(
        r#"<section id="veil-youtube-static" style="padding:24px 18px 40px 18px">
<h2 style="font-size:24px;margin:0 0 8px 0">YouTube</h2>
<p style="color:#aaa;margin:0 0 18px 0">Veil safe view · static feed recovered without running YouTube's full JavaScript app.</p>
<div style="display:flex;flex-wrap:wrap;gap:18px">"#,
    );
    for card in cards.into_iter().take(24) {
        let title = escape_html(&card.title);
        let thumb = escape_html_attr(&card.thumbnail);
        let href = format!("https://www.youtube.com/watch?v={}", card.video_id);
        out.push_str(&format!(
            r#"<div style="width:300px;max-width:100%;padding:8px"><img src="{thumb}" alt="{title}" width="300" style="width:300px;max-width:100%;border-radius:12px"/><p style="margin:8px 0 0 0;font-size:15px"><a href="{href}">{title}</a></p></div>"#,
        ));
    }
    out.push_str("</div></section>");
    out
}

fn collect_youtube_cards(value: &Value, cards: &mut Vec<YoutubeCard>, seen: &mut HashSet<String>) {
    if cards.len() >= 24 {
        return;
    }
    match value {
        Value::Object(map) => {
            for key in ["videoRenderer", "gridVideoRenderer", "compactVideoRenderer"] {
                if let Some(renderer) = map.get(key) {
                    if let Some(card) = youtube_card_from_renderer(renderer) {
                        if seen.insert(card.video_id.clone()) {
                            cards.push(card);
                            if cards.len() >= 24 {
                                return;
                            }
                        }
                    }
                }
            }
            for child in map.values() {
                collect_youtube_cards(child, cards, seen);
                if cards.len() >= 24 {
                    return;
                }
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_youtube_cards(child, cards, seen);
                if cards.len() >= 24 {
                    return;
                }
            }
        }
        _ => {}
    }
}

fn youtube_card_from_renderer(renderer: &Value) -> Option<YoutubeCard> {
    let video_id = renderer.get("videoId")?.as_str()?.trim();
    if video_id.is_empty() {
        return None;
    }
    let title_value = renderer.get("title")?;
    let title = title_value
        .get("runs")
        .and_then(Value::as_array)
        .and_then(|runs| runs.first())
        .and_then(|run| run.get("text"))
        .and_then(Value::as_str)
        .or_else(|| title_value.get("simpleText").and_then(Value::as_str))?
        .trim();
    if title.is_empty() {
        return None;
    }
    let thumbnails = renderer
        .get("thumbnail")?
        .get("thumbnails")?
        .as_array()?;
    let thumbnail = thumbnails
        .iter()
        .rev()
        .filter_map(|entry| entry.get("url").and_then(Value::as_str))
        .find(|url| url.starts_with("https://") || url.starts_with("http://"))?;

    Some(YoutubeCard {
        video_id: video_id.to_owned(),
        title: title.to_owned(),
        thumbnail: thumbnail.to_owned(),
    })
}

fn extract_yt_initial_data(html: &str) -> Option<&str> {
    for marker in [
        "var ytInitialData = ",
        "window[\"ytInitialData\"] = ",
        "ytInitialData = ",
    ] {
        let Some(marker_pos) = html.find(marker) else {
            continue;
        };
        let after = marker_pos + marker.len();
        let Some(relative_start) = html[after..].find('{') else {
            continue;
        };
        let start = after + relative_start;
        if let Some(end) = balanced_json_object_end(html, start) {
            return Some(&html[start..end]);
        }
    }
    None
}

fn balanced_json_object_end(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if bytes.get(start).copied()? != b'{' {
        return None;
    }
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, byte) in bytes[start..].iter().copied().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(start + offset + 1);
                }
            }
            _ => {}
        }
    }
    None
}

fn inject_before_body_end(mut html: String, fragment: &str) -> String {
    let lower = html.to_ascii_lowercase();
    if let Some(pos) = lower.rfind("</body>") {
        html.insert_str(pos, fragment);
    } else {
        html.push_str(fragment);
    }
    html
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn escape_html_attr(value: &str) -> String {
    escape_html(value)
}''',
    "youtube static recovery",
)

# Extend guarded tests with real ytInitialData-shaped thumbnail recovery.
loader = replace_once(
    loader,
    '''    fn guarded_html_drops_scripts() {
        let html = "<html><body><h1>keep</h1><script>throw new Error('boom')</script><p>also keep</p></body></html>";
        let safe = prepare_guarded_html(html);
        assert!(safe.contains("keep"));
        assert!(safe.contains("also keep"));
        assert!(!safe.to_ascii_lowercase().contains("<script"));
        assert!(!safe.contains("boom"));
    }
}''',
    '''    fn guarded_html_drops_scripts() {
        let html = "<html><body><h1>keep</h1><script>throw new Error('boom')</script><p>also keep</p></body></html>";
        let safe = prepare_guarded_html(html);
        assert!(safe.contains("keep"));
        assert!(safe.contains("also keep"));
        assert!(!safe.to_ascii_lowercase().contains("<script"));
        assert!(!safe.contains("boom"));
    }

    #[test]
    fn guarded_youtube_recovers_static_thumbnail_cards() {
        let html = r#"<html><body><script>var ytInitialData = {"contents":[{"videoRenderer":{"videoId":"abc123","title":{"runs":[{"text":"Example video"}]},"thumbnail":{"thumbnails":[{"url":"https://i.ytimg.com/vi/abc123/hqdefault.jpg","width":480,"height":360}]}}}]};</script></body></html>"#;
        let safe = prepare_guarded_html(html);
        assert!(safe.contains("veil-youtube-static"));
        assert!(safe.contains("Example video"));
        assert!(safe.contains("https://i.ytimg.com/vi/abc123/hqdefault.jpg"));
        assert!(safe.contains("https://www.youtube.com/watch?v=abc123"));
        assert!(!safe.to_ascii_lowercase().contains("<script"));
    }

    #[test]
    fn balanced_json_handles_braces_inside_strings() {
        let text = r#"prefix {"title":"a } brace","nested":{"ok":true}} suffix"#;
        let start = text.find('{').unwrap();
        let end = balanced_json_object_end(text, start).unwrap();
        assert_eq!(&text[start..end], r#"{"title":"a } brace","nested":{"ok":true}}"#);
    }
}''',
    "youtube recovery tests",
)
loader_path.write_text(loader)

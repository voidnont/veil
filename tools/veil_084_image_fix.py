from pathlib import Path


def replace_once(text, old, new, label):
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected one match, found {count}")
    return text.replace(old, new, 1)

# Version.
path = Path("Cargo.toml")
text = path.read_text()
text = replace_once(text, 'version = "0.8.3"', 'version = "0.8.4"', "package version")
path.write_text(text)

# Never advertise AVIF when this build has no AVIF decoder.
path = Path("src/net.rs")
text = path.read_text()
text = replace_once(
    text,
    'const MAX_UPLOAD_BYTES: usize = 32 * 1024 * 1024;\n',
    'const MAX_UPLOAD_BYTES: usize = 32 * 1024 * 1024;\n'
    'const IMAGE_ACCEPT: &str = "image/webp,image/png,image/jpeg,image/gif,image/x-icon,*/*;q=0.1";\n',
    "image accept constant",
)
text = replace_once(
    text,
    '                "Mozilla/5.0 (Veil; privacy) VeilBrowser/0.8.3 VeilEngine/0.8.3",\n',
    '                "Mozilla/5.0 (Veil; privacy) VeilBrowser/0.8.4 VeilEngine/0.8.4",\n',
    "user agent version",
)
text = replace_once(
    text,
    '            "image/avif,image/webp,image/png,image/jpeg,image/gif,image/x-icon,*/*;q=0.2",\n',
    '            IMAGE_ACCEPT,\n',
    "image accept use",
)
text = replace_once(
    text,
    '''    #[test]
    fn multipart_encoder_emits_file_and_text_parts() {''',
    '''    #[test]
    fn image_accept_only_advertises_compiled_decoders() {
        assert!(!IMAGE_ACCEPT.contains("avif"));
        assert!(IMAGE_ACCEPT.contains("image/webp"));
        assert!(IMAGE_ACCEPT.contains("image/png"));
        assert!(IMAGE_ACCEPT.contains("image/jpeg"));
    }

    #[test]
    fn multipart_encoder_emits_file_and_text_parts() {''',
    "image accept regression test",
)
path.write_text(text)

# Fix generic DOM image discovery and rendering.
path = Path("src/engine.rs")
text = path.read_text()
text = replace_once(
    text,
    '        "img" => {\n            let src = image_source(&el.attrs)?;\n',
    '        "img" => {\n            let src = image_source(dom, idx)?;\n',
    "DOM-aware image source",
)
text = replace_once(
    text,
    '            | "figure"\n            | "figcaption"\n',
    '            | "figure"\n            | "figcaption"\n            | "picture"\n',
    "picture block tag",
)

old_inline = '''            NodeKind::Element(_) => collect_runs(
                dom,
                child,
                parent_style,
                None,
                sheet,
                blocker,
                page_url,
                cosmetic_enabled,
                recover_visibility,
                hidden,
                &mut pending_runs,
            ),
'''
new_inline = '''            NodeKind::Element(_) => {
                collect_runs(
                    dom,
                    child,
                    parent_style,
                    None,
                    sheet,
                    blocker,
                    page_url,
                    cosmetic_enabled,
                    recover_visibility,
                    hidden,
                    &mut pending_runs,
                );
                let mut embedded = Vec::new();
                collect_embedded_blocks(
                    dom,
                    child,
                    parent_style,
                    sheet,
                    blocker,
                    page_url,
                    cosmetic_enabled,
                    recover_visibility,
                    hidden,
                    &mut embedded,
                );
                if !embedded.is_empty() {
                    flush_pending(&mut pending_runs, parent_style, &mut blocks);
                    blocks.extend(embedded);
                }
            }
'''
text = replace_once(text, old_inline, new_inline, "inline image traversal")

old_source = '''fn image_source(attrs: &std::collections::HashMap<String, String>) -> Option<String> {
    if let Some(src) = attrs.get("src").map(|s| s.trim()).filter(|s| !s.is_empty()) {
        return Some(src.to_owned());
    }
    attrs.get("srcset").and_then(|set| {
        set.split(',')
            .next()
            .and_then(|candidate| candidate.split_whitespace().next())
            .map(str::to_owned)
    })
}
'''
new_source = '''fn image_source(dom: &Dom, idx: usize) -> Option<String> {
    let NodeKind::Element(el) = &dom.nodes[idx].kind else {
        return None;
    };

    // Static recovery for common lazy-loading libraries. These attributes contain
    // the real image URL even when the site's JavaScript/IntersectionObserver has
    // not run yet.
    for key in [
        "data-src",
        "data-original",
        "data-lazy-src",
        "data-original-src",
        "data-image-src",
    ] {
        if let Some(src) = nonempty_attr(&el.attrs, key) {
            return Some(src.to_owned());
        }
    }
    if let Some(set) = nonempty_attr(&el.attrs, "data-srcset") {
        if let Some(src) = best_srcset_candidate(set) {
            return Some(src);
        }
    }

    // Respect <picture> by selecting a source Veil can actually decode. AVIF and
    // other unsupported formats are deliberately skipped so the <img> fallback
    // remains usable.
    if let Some(parent) = dom.nodes[idx].parent {
        if let NodeKind::Element(parent_el) = &dom.nodes[parent].kind {
            if parent_el.tag == "picture" {
                for &sibling in &dom.nodes[parent].children {
                    let NodeKind::Element(source) = &dom.nodes[sibling].kind else {
                        continue;
                    };
                    if source.tag != "source" || !supported_picture_type(source.attrs.get("type")) {
                        continue;
                    }
                    if let Some(set) = nonempty_attr(&source.attrs, "srcset")
                        .or_else(|| nonempty_attr(&source.attrs, "data-srcset"))
                    {
                        if let Some(src) = best_srcset_candidate(set) {
                            return Some(src);
                        }
                    }
                    if let Some(src) = nonempty_attr(&source.attrs, "src") {
                        return Some(src.to_owned());
                    }
                }
            }
        }
    }

    if let Some(src) = nonempty_attr(&el.attrs, "src") {
        return Some(src.to_owned());
    }
    nonempty_attr(&el.attrs, "srcset").and_then(best_srcset_candidate)
}

fn nonempty_attr<'a>(
    attrs: &'a std::collections::HashMap<String, String>,
    key: &str,
) -> Option<&'a str> {
    attrs.get(key).map(|value| value.trim()).filter(|value| !value.is_empty())
}

fn supported_picture_type(kind: Option<&String>) -> bool {
    let Some(kind) = kind.map(|value| value.trim().to_ascii_lowercase()) else {
        return true;
    };
    matches!(
        kind.as_str(),
        "image/webp"
            | "image/png"
            | "image/jpeg"
            | "image/jpg"
            | "image/gif"
            | "image/x-icon"
            | "image/vnd.microsoft.icon"
    )
}

fn best_srcset_candidate(set: &str) -> Option<String> {
    let mut best: Option<(f32, String)> = None;
    for raw in set.split(',') {
        let mut parts = raw.split_whitespace();
        let Some(url) = parts.next().map(str::trim).filter(|url| !url.is_empty()) else {
            continue;
        };
        let descriptor = parts.next().unwrap_or_default();
        let score = if let Some(width) = descriptor.strip_suffix('w') {
            width.parse::<f32>().unwrap_or(1.0)
        } else if let Some(scale) = descriptor.strip_suffix('x') {
            scale.parse::<f32>().unwrap_or(1.0) * 10_000.0
        } else {
            1.0
        };
        if best
            .as_ref()
            .map(|(best_score, _)| score >= *best_score)
            .unwrap_or(true)
        {
            best = Some((score, url.to_owned()));
        }
    }
    best.map(|(_, url)| url)
}

#[allow(clippy::too_many_arguments)]
fn collect_embedded_blocks(
    dom: &Dom,
    idx: usize,
    parent_style: &ComputedStyle,
    sheet: &StyleSheet,
    blocker: &Blocker,
    page_url: Option<&Url>,
    cosmetic_enabled: bool,
    recover_visibility: bool,
    hidden: &mut usize,
    out: &mut Vec<RenderBlock>,
) {
    for &child in &dom.nodes[idx].children {
        let NodeKind::Element(el) = &dom.nodes[child].kind else {
            continue;
        };
        if is_ignored_tag(&el.tag) {
            continue;
        }
        if is_block_tag(&el.tag) {
            if let Some(block) = build_block(
                dom,
                child,
                parent_style,
                sheet,
                blocker,
                page_url,
                cosmetic_enabled,
                recover_visibility,
                hidden,
            ) {
                out.push(block);
            }
        } else {
            let style = sheet.compute_node(dom, child, parent_style);
            if !style.display_none || recover_visibility {
                collect_embedded_blocks(
                    dom,
                    child,
                    &style,
                    sheet,
                    blocker,
                    page_url,
                    cosmetic_enabled,
                    recover_visibility,
                    hidden,
                    out,
                );
            }
        }
    }
}
'''
text = replace_once(text, old_source, new_source, "modern image source resolver")

text = replace_once(
    text,
    '''    #[test]
    fn parses_simple_get_form() {''',
    '''    #[test]
    fn renders_images_inside_inline_links() {
        let engine = Engine::default();
        let blocker = Blocker::default();
        let view = engine.parse(
            "https://example.com/",
            "<div><a href='/story'><img src='/photo.jpg' alt='Photo'></a></div>",
            &blocker,
            SitePrivacy::default(),
        );
        let rendered = format!("{:?}", view.blocks);
        assert!(rendered.contains("Image"));
        assert!(rendered.contains("/photo.jpg"));
    }

    #[test]
    fn resolves_lazy_and_picture_images() {
        let engine = Engine::default();
        let blocker = Blocker::default();
        let lazy = engine.parse(
            "https://example.com/",
            "<img src='placeholder.gif' data-src='/real-photo.jpg'>",
            &blocker,
            SitePrivacy::default(),
        );
        assert!(format!("{:?}", lazy.blocks).contains("/real-photo.jpg"));

        let picture = engine.parse(
            "https://example.com/",
            "<picture><source type='image/avif' srcset='/photo.avif 1200w'><source type='image/webp' srcset='/small.webp 320w, /large.webp 1280w'><img src='/fallback.jpg'></picture>",
            &blocker,
            SitePrivacy::default(),
        );
        let rendered = format!("{:?}", picture.blocks);
        assert!(rendered.contains("/large.webp"));
        assert!(!rendered.contains("/photo.avif"));
    }

    #[test]
    fn parses_simple_get_form() {''',
    "image regression tests",
)
path.write_text(text)

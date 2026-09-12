from pathlib import Path


def replace_once(text, old, new, label):
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected one match, found {count}")
    return text.replace(old, new, 1)


path = Path("src/loader.rs")
text = path.read_text()

old_collect = r'''        Value::Object(map) => {
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
            for child in map.values() {'''
new_collect = r'''        Value::Object(map) => {
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
            if let Some(renderer) = map.get("lockupViewModel") {
                if let Some(card) = youtube_card_from_lockup(renderer) {
                    if seen.insert(card.video_id.clone()) {
                        cards.push(card);
                        if cards.len() >= 24 {
                            return;
                        }
                    }
                }
            }
            if let Some(renderer) = map.get("shortsLockupViewModel") {
                if let Some(card) = youtube_card_from_shorts_lockup(renderer) {
                    if seen.insert(card.video_id.clone()) {
                        cards.push(card);
                        if cards.len() >= 24 {
                            return;
                        }
                    }
                }
            }
            for child in map.values() {'''
text = replace_once(text, old_collect, new_collect, "YouTube collector")

old_renderer = r'''    let thumbnails = renderer.get("thumbnail")?.get("thumbnails")?.as_array()?;
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

fn extract_yt_initial_data'''
new_renderer = r'''    let thumbnail = thumbnail_from_array(
        renderer
            .get("thumbnail")
            .and_then(|thumbnail| thumbnail.get("thumbnails")),
    )
    .unwrap_or_else(|| canonical_youtube_thumbnail(video_id));

    Some(YoutubeCard {
        video_id: video_id.to_owned(),
        title: title.to_owned(),
        thumbnail,
    })
}

fn youtube_card_from_lockup(renderer: &Value) -> Option<YoutubeCard> {
    if renderer.get("contentType").and_then(Value::as_str) != Some("LOCKUP_CONTENT_TYPE_VIDEO") {
        return None;
    }
    let video_id = renderer.get("contentId")?.as_str()?.trim();
    if video_id.is_empty() {
        return None;
    }
    let title = renderer
        .pointer("/metadata/lockupMetadataViewModel/title/content")
        .and_then(Value::as_str)?
        .trim();
    if title.is_empty() {
        return None;
    }
    let thumbnail = thumbnail_from_array(
        renderer.pointer("/contentImage/thumbnailViewModel/image/sources"),
    )
    .or_else(|| {
        thumbnail_from_array(renderer.pointer(
            "/contentImage/collectionThumbnailViewModel/primaryThumbnail/thumbnailViewModel/image/sources",
        ))
    })
    .unwrap_or_else(|| canonical_youtube_thumbnail(video_id));

    Some(YoutubeCard {
        video_id: video_id.to_owned(),
        title: title.to_owned(),
        thumbnail,
    })
}

fn youtube_card_from_shorts_lockup(renderer: &Value) -> Option<YoutubeCard> {
    let endpoint_video_id = renderer
        .pointer("/onTap/innertubeCommand/reelWatchEndpoint/videoId")
        .and_then(Value::as_str);
    let entity_video_id = renderer
        .get("entityId")
        .and_then(Value::as_str)
        .and_then(|value| value.strip_prefix("shorts-shelf-item-"));
    let video_id = endpoint_video_id.or(entity_video_id)?.trim();
    if video_id.is_empty() {
        return None;
    }
    let title = renderer
        .pointer("/overlayMetadata/primaryText/content")
        .and_then(Value::as_str)
        .or_else(|| renderer.get("accessibilityText").and_then(Value::as_str))?
        .trim();
    if title.is_empty() {
        return None;
    }
    let thumbnail = thumbnail_from_array(renderer.pointer("/thumbnail/sources"))
        .or_else(|| {
            thumbnail_from_array(
                renderer.pointer("/thumbnailViewModel/thumbnailViewModel/image/sources"),
            )
        })
        .unwrap_or_else(|| canonical_youtube_thumbnail(video_id));

    Some(YoutubeCard {
        video_id: video_id.to_owned(),
        title: title.to_owned(),
        thumbnail,
    })
}

fn thumbnail_from_array(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_array)
        .and_then(|items| {
            items
                .iter()
                .rev()
                .filter_map(|entry| entry.get("url").and_then(Value::as_str))
                .find(|url| url.starts_with("https://") || url.starts_with("http://"))
        })
        .map(str::to_owned)
}

fn canonical_youtube_thumbnail(video_id: &str) -> String {
    format!("https://i.ytimg.com/vi/{video_id}/hqdefault.jpg")
}

fn extract_yt_initial_data'''
text = replace_once(text, old_renderer, new_renderer, "YouTube renderer helpers")

old_test_anchor = r'''    #[test]
    fn balanced_json_handles_braces_inside_strings() {'''
new_tests = r'''    #[test]
    fn guarded_youtube_recovers_current_lockup_view_model() {
        let html = r#"<html><body><script>var ytInitialData = {"contents":[{"lockupViewModel":{"contentId":"modern123","contentType":"LOCKUP_CONTENT_TYPE_VIDEO","contentImage":{"thumbnailViewModel":{"image":{"sources":[{"url":"https://i.ytimg.com/vi/modern123/hqdefault.jpg"}]}}},"metadata":{"lockupMetadataViewModel":{"title":{"content":"Modern video"}}}}}]};</script></body></html>"#;
        let safe = prepare_guarded_html(html);
        assert!(safe.contains("Modern video"));
        assert!(safe.contains("https://i.ytimg.com/vi/modern123/hqdefault.jpg"));
        assert!(safe.contains("watch?v=modern123"));
    }

    #[test]
    fn guarded_youtube_recovers_shorts_lockup_view_model() {
        let html = r#"<html><body><script>var ytInitialData = {"contents":[{"shortsLockupViewModel":{"entityId":"shorts-shelf-item-short123","thumbnail":{"sources":[{"url":"https://i.ytimg.com/vi/short123/oar2.jpg"}]},"overlayMetadata":{"primaryText":{"content":"Example Short"}},"onTap":{"innertubeCommand":{"reelWatchEndpoint":{"videoId":"short123"}}}}}]};</script></body></html>"#;
        let safe = prepare_guarded_html(html);
        assert!(safe.contains("Example Short"));
        assert!(safe.contains("https://i.ytimg.com/vi/short123/oar2.jpg"));
        assert!(safe.contains("watch?v=short123"));
    }

    #[test]
    fn renderer_uses_canonical_thumbnail_when_sources_are_missing() {
        let renderer: Value = serde_json::from_str(
            r#"{"videoId":"fallback123","title":{"simpleText":"Fallback thumbnail"}}"#,
        )
        .unwrap();
        let card = youtube_card_from_renderer(&renderer).unwrap();
        assert_eq!(
            card.thumbnail,
            "https://i.ytimg.com/vi/fallback123/hqdefault.jpg"
        );
    }

    #[test]
    fn balanced_json_handles_braces_inside_strings() {'''
text = replace_once(text, old_test_anchor, new_tests, "YouTube current-model tests")

path.write_text(text)

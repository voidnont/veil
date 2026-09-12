from pathlib import Path
import re


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one match, found {count}: {old[:120]!r}")
    p.write_text(text.replace(old, new, 1))


replace_once("Cargo.toml", 'version = "0.8.8"', 'version = "0.8.9"')

path = Path("src/main.rs")
text = path.read_text()

# Cheap immutable page snapshots: rendering clones an Arc instead of the entire DOM paint tree.
text = text.replace("    page: DocumentView,\n", "    page: Arc<DocumentView>,\n", 1)
text = text.replace(
    "            page: DocumentView::home(),\n",
    "            page: Arc::new(DocumentView::home()),\n",
    1,
)
for old, new in [
    ("            tab.page = DocumentView::home();", "            tab.page = Arc::new(DocumentView::home());"),
    ("                    tab.page = view;", "                    tab.page = Arc::new(view);"),
    ("                    tab.page = DocumentView::error(&target, &err);", "                    tab.page = Arc::new(DocumentView::error(&target, &err));"),
    ("                        self.tabs[index].page = view;", "                        self.tabs[index].page = Arc::new(view);"),
]:
    if old not in text:
        raise SystemExit(f"missing page Arc assignment: {old}")
    text = text.replace(old, new)

mut_page = "                        let page = &mut self.tabs[index].page;"
count = text.count(mut_page)
if count < 2:
    raise SystemExit(f"expected at least two mutable page bindings, found {count}")
text = text.replace(
    mut_page,
    "                        let page = Arc::make_mut(&mut self.tabs[index].page);",
)

# Bounded queues/caches.
text = text.replace(
    "const MAX_IMAGE_LOADS: usize = 6;\n",
    "const MAX_IMAGE_LOADS: usize = 6;\nconst MAX_IMAGE_CACHE_ENTRIES: usize = 192;\nconst MAX_PENDING_RUNTIME_EVENTS: usize = 12;\n",
    1,
)
text = text.replace(
    "    runtime_inflight: HashSet<u64>,\n",
    "    runtime_inflight: HashSet<u64>,\n    pending_runtime_events: HashMap<u64, VecDeque<DomEventRequest>>,\n    pending_runtime_elapsed: HashMap<u64, u64>,\n",
    1,
)
text = text.replace(
    "    image_cache: HashMap<String, CachedImage>,\n",
    "    image_cache: HashMap<String, CachedImage>,\n    image_cache_touch: HashMap<String, u64>,\n    image_cache_clock: u64,\n",
    1,
)
text = text.replace(
    "            runtime_inflight: HashSet::new(),\n",
    "            runtime_inflight: HashSet::new(),\n            pending_runtime_events: HashMap::new(),\n            pending_runtime_elapsed: HashMap::new(),\n",
    1,
)
text = text.replace(
    "            image_cache: HashMap::new(),\n",
    "            image_cache: HashMap::new(),\n            image_cache_touch: HashMap::new(),\n            image_cache_clock: 0,\n",
    1,
)

# Preserve input under load. Continuous input is coalesced to the latest value; discrete events stay ordered.
old = '''        if self.runtime_inflight.contains(&tab.id) {
            return;
        }

        let request_id = self.next_runtime_request_id;'''
new = '''        let tab_id = tab.id;
        if self.runtime_inflight.contains(&tab_id) {
            let queue = self.pending_runtime_events.entry(tab_id).or_default();
            let coalescible = matches!(
                event.event_type.as_str(),
                "input" | "change" | "mousemove" | "pointermove" | "scroll"
            );
            if coalescible {
                if let Some(existing) = queue.iter_mut().rev().find(|existing| {
                    existing.node_id == event.node_id && existing.event_type == event.event_type
                }) {
                    *existing = event;
                    return;
                }
            }
            if queue.len() >= MAX_PENDING_RUNTIME_EVENTS {
                if coalescible {
                    queue.pop_front();
                } else {
                    return;
                }
            }
            queue.push_back(event);
            return;
        }

        let request_id = self.next_runtime_request_id;'''
if old not in text:
    raise SystemExit("runtime inflight dispatch block not found")
text = text.replace(old, new, 1)

# Busy runtime sessions accumulate elapsed time instead of silently losing timer/rAF time.
old = '''            if !javascript_enabled || self.runtime_inflight.contains(&tab.id) {
                continue;
            }
            let request_id = self.next_runtime_request_id;'''
new = '''            if !javascript_enabled {
                continue;
            }
            if self.runtime_inflight.contains(&tab.id) {
                let pending = self.pending_runtime_elapsed.entry(tab.id).or_default();
                *pending = pending.saturating_add(elapsed_ms).min(1_000);
                continue;
            }
            let tick_elapsed = elapsed_ms
                .saturating_add(self.pending_runtime_elapsed.remove(&tab.id).unwrap_or(0))
                .min(1_000);
            let request_id = self.next_runtime_request_id;'''
if old not in text:
    raise SystemExit("runtime timer inflight block not found")
text = text.replace(old, new, 1)
text = text.replace(
    "                kind: RuntimeInteractionKind::Tick(elapsed_ms),\n",
    "                kind: RuntimeInteractionKind::Tick(tick_elapsed),\n",
    1,
)

# Remember the completed tab ID and flush exactly one queued interaction afterward.
text = text.replace(
    "            processed += 1;\n            self.runtime_inflight.remove(&result.tab_id);\n",
    "            processed += 1;\n            let completed_tab_id = result.tab_id;\n            self.runtime_inflight.remove(&completed_tab_id);\n",
    1,
)
old = '''            if self.tabs[index].generation != result.generation {
                continue;
            }
            match result.result {'''
new = '''            if self.tabs[index].generation != result.generation {
                self.pending_runtime_events.remove(&completed_tab_id);
                self.pending_runtime_elapsed.remove(&completed_tab_id);
                continue;
            }
            match result.result {'''
if old not in text:
    raise SystemExit("runtime stale generation block not found")
text = text.replace(old, new, 1)
old = '''                Err(error) => {
                    // Interaction failures should not destroy a successfully loaded page.
                    self.tabs[index].status = format!("Live interaction unavailable: {error}");
                }
            }
        }
        if processed == MAX_RUNTIME_RESULTS_PER_FRAME {'''
new = '''                Err(error) => {
                    // Interaction failures should not destroy a successfully loaded page.
                    self.tabs[index].status = format!("Live interaction unavailable: {error}");
                }
            }
            self.flush_pending_runtime(completed_tab_id);
        }
        if processed == MAX_RUNTIME_RESULTS_PER_FRAME {'''
if old not in text:
    raise SystemExit("runtime poll tail not found")
text = text.replace(old, new, 1)

marker = '''    fn visible_runtime_indices(&self) -> Vec<usize> {'''
helper = '''    fn flush_pending_runtime(&mut self, tab_id: u64) {
        if self.runtime_inflight.contains(&tab_id) {
            return;
        }
        let Some(index) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            self.pending_runtime_events.remove(&tab_id);
            self.pending_runtime_elapsed.remove(&tab_id);
            return;
        };

        let next_event = self
            .pending_runtime_events
            .get_mut(&tab_id)
            .and_then(VecDeque::pop_front);
        if self
            .pending_runtime_events
            .get(&tab_id)
            .is_some_and(VecDeque::is_empty)
        {
            self.pending_runtime_events.remove(&tab_id);
        }
        if let Some(event) = next_event {
            self.dispatch_runtime_event(index, event);
            return;
        }

        let elapsed_ms = self.pending_runtime_elapsed.remove(&tab_id).unwrap_or(0).min(1_000);
        if elapsed_ms == 0 || self.tabs[index].page.url == HOME {
            return;
        }
        let javascript_enabled = !is_guarded_script_site(&self.tabs[index].page.url)
            && Url::parse(&self.tabs[index].page.url)
                .ok()
                .map(|url| self.profiles.for_url(&url).javascript)
                .unwrap_or(false);
        if !javascript_enabled {
            return;
        }
        let request_id = self.next_runtime_request_id;
        self.next_runtime_request_id = self.next_runtime_request_id.saturating_add(1);
        self.runtime_inflight.insert(tab_id);
        let tab = &self.tabs[index];
        self.runtime_loader.start(RuntimeInteractionRequest {
            request_id,
            tab_id,
            generation: tab.generation,
            page_url: tab.page.url.clone(),
            session_id: format!("tab-{}-{}", tab.id, tab.generation),
            storage: self.storage.clone(),
            kind: RuntimeInteractionKind::Tick(elapsed_ms),
        });
    }

'''
if marker not in text:
    raise SystemExit("visible_runtime_indices marker not found")
text = text.replace(marker, helper + marker, 1)

# Bounded GPU texture cache. Long browsing sessions no longer keep every decoded texture forever.
marker = '''    fn poll_image_loader(&mut self, ctx: &egui::Context) {'''
helper = '''    fn touch_image_cache(&mut self, key: &str) {
        self.image_cache_clock = self.image_cache_clock.saturating_add(1);
        self.image_cache_touch
            .insert(key.to_owned(), self.image_cache_clock);
    }

    fn evict_image_cache(&mut self) {
        while self.image_cache.len() > MAX_IMAGE_CACHE_ENTRIES {
            let candidate = self
                .image_cache
                .iter()
                .filter(|(_, entry)| !matches!(entry, &CachedImage::Loading))
                .min_by_key(|(key, _)| self.image_cache_touch.get(*key).copied().unwrap_or(0))
                .map(|(key, _)| key.clone());
            let Some(key) = candidate else {
                break;
            };
            self.image_cache.remove(&key);
            self.image_cache_touch.remove(&key);
        }
    }

'''
if marker not in text:
    raise SystemExit("poll_image_loader marker not found")
text = text.replace(marker, helper + marker, 1)
text = text.replace(
    "            self.image_cache.insert(result.key, cached);\n",
    "            let key = result.key;\n            self.image_cache.insert(key.clone(), cached);\n            self.touch_image_cache(&key);\n            self.evict_image_cache();\n",
    1,
)

old = '''        if !self.image_cache.contains_key(&cache_key) {
            self.image_cache
                .insert(cache_key.clone(), CachedImage::Loading);'''
new = '''        if !self.image_cache.contains_key(&cache_key) {
            self.image_cache
                .insert(cache_key.clone(), CachedImage::Loading);'''
# Keep insertion logic intact, then touch immediately before reading the cache.
if old not in text:
    raise SystemExit("image cache insertion block not found")
needle = '''            ctx.request_repaint_after(Duration::from_millis(40));
        }

        match self.image_cache.get(&cache_key).cloned() {'''
replacement = '''            ctx.request_repaint_after(Duration::from_millis(40));
        }
        self.touch_image_cache(&cache_key);

        match self.image_cache.get(&cache_key).cloned() {'''
if needle not in text:
    raise SystemExit("image cache render match not found")
text = text.replace(needle, replacement, 1)

path.write_text(text)
print("Veil 0.8.9 smoothness patch applied")

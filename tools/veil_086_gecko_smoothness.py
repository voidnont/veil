from pathlib import Path
import re

def replace_once(text, old, new, label):
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected one match, found {count}")
    return text.replace(old, new, 1)

# --- Version ---
path = Path("Cargo.toml")
text = path.read_text()
text = replace_once(text, 'version = "0.8.5"', 'version = "0.8.6"', "package version")
path.write_text(text)

# --- Shared HTTP connection pool, Gecko/Necko-style connection reuse ---
path = Path("src/net.rs")
text = path.read_text()
text = replace_once(text, "use std::sync::{Arc, Mutex};", "use std::sync::{Arc, Mutex, OnceLock};", "net OnceLock import")
text = replace_once(text, "    HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, CACHE_CONTROL, CONTENT_TYPE, COOKIE, DNT,\n", "    HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, CONTENT_TYPE, COOKIE, DNT,\n", "remove forced no-cache import")
insert_marker = '''const IMAGE_ACCEPT: &str =
    "image/webp,image/png,image/jpeg,image/gif,image/svg+xml,image/bmp,image/x-icon,*/*;q=0.1";

'''
shared_client = r'''const IMAGE_ACCEPT: &str =
    "image/webp,image/png,image/jpeg,image/gif,image/svg+xml,image/bmp,image/x-icon,*/*;q=0.1";

static SHARED_HTTP_CLIENT: OnceLock<Client> = OnceLock::new();

fn build_shared_http_client() -> Client {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static(
            "Mozilla/5.0 (Veil; privacy) VeilBrowser/0.8.6 VeilEngine/0.8.6",
        ),
    );
    headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.7"));
    headers.insert(DNT, HeaderValue::from_static("1"));
    headers.insert("sec-gpc", HeaderValue::from_static("1"));

    // Like Gecko's Necko layer, all browser resource brokers share the same
    // underlying HTTP client so TLS sessions and keep-alive connections can be
    // reused across documents, images, stylesheets, fonts and scripts.
    Client::builder()
        .default_headers(headers)
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .pool_idle_timeout(Some(Duration::from_secs(90)))
        .pool_max_idle_per_host(8)
        .tcp_keepalive(Some(Duration::from_secs(60)))
        .tcp_nodelay(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("failed to construct shared HTTPS client")
}

fn shared_http_client() -> Client {
    SHARED_HTTP_CLIENT.get_or_init(build_shared_http_client).clone()
}

'''
text = replace_once(text, insert_marker, shared_client, "shared HTTP client insertion")
old_ctor = r'''        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static(
                "Mozilla/5.0 (Veil; privacy) VeilBrowser/0.8.5 VeilEngine/0.8.5",
            ),
        );
        headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.7"));
        headers.insert(DNT, HeaderValue::from_static("1"));
        headers.insert("sec-gpc", HeaderValue::from_static("1"));
        headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));

        let client = Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("failed to construct HTTPS client");
'''
text = replace_once(text, old_ctor, "        let client = shared_http_client();\n", "reuse shared HTTP client")
path.write_text(text)

# --- Image decode worker pool instead of spawning one OS thread per image ---
path = Path("src/image_loader.rs")
text = path.read_text()
text = replace_once(text, "use std::io::Cursor;\n", "", "remove production Cursor import")
text = replace_once(text, "use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};\nuse std::thread;\n", "use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};\nuse std::sync::{Arc, Mutex};\nuse std::thread;\n", "image worker sync imports")
old_loader = r'''pub struct ImageLoader {
    sender: Sender<ImageLoadResult>,
    receiver: Receiver<ImageLoadResult>,
}

impl ImageLoader {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self { sender, receiver }
    }

    pub fn start(&self, request: ImageLoadRequest) {
        let sender = self.sender.clone();
        thread::spawn(move || {
            let mut network = PrivacyNetwork::new_with_storage(request.storage.clone());
            if !request.custom_filters.trim().is_empty() {
                network
                    .blocker_mut()
                    .replace_custom_filters(request.custom_filters.clone());
            }

            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if request.url.scheme() == "data" {
                    let (content_type, bytes) = decode_data_image(request.url.as_str())?;
                    let decoded = decode_image_payload(&bytes, &content_type)?;
                    return Ok(DecodedImage {
                        final_url: request.url.to_string(),
                        size: decoded.size,
                        rgba: decoded.rgba,
                    });
                }

                let response =
                    network.get_image(&request.top_level, &request.url, request.privacy)?;
                let decoded = decode_image_payload(&response.bytes, &response.content_type)?;
                Ok(DecodedImage {
                    final_url: response.final_url.to_string(),
                    size: decoded.size,
                    rgba: decoded.rgba,
                })
            }))
            .unwrap_or_else(|_| Err("Veil recovered from an image worker panic.".into()));

            let blocked_count = network.blocked_count();
            let blocked_events = network.take_blocked_events();
            let _ = sender.send(ImageLoadResult {
                key: request.key,
                result,
                blocked_count,
                blocked_events,
            });
        });
    }

    pub fn try_recv(&self) -> Option<ImageLoadResult> {
        match self.receiver.try_recv() {
            Ok(value) => Some(value),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
        }
    }
}
'''
new_loader = r'''pub struct ImageLoader {
    request_sender: Sender<ImageLoadRequest>,
    receiver: Receiver<ImageLoadResult>,
}

impl ImageLoader {
    pub fn new() -> Self {
        let (request_sender, request_receiver) = mpsc::channel::<ImageLoadRequest>();
        let (result_sender, receiver) = mpsc::channel::<ImageLoadResult>();
        let shared_receiver = Arc::new(Mutex::new(request_receiver));

        // Gecko uses bounded task pools instead of creating a new native thread
        // for every decode. Keep enough workers for parallelism while reserving
        // CPU for the UI/render threads.
        for worker in 0..image_worker_count() {
            let requests = shared_receiver.clone();
            let sender = result_sender.clone();
            let _ = thread::Builder::new()
                .name(format!("veil-image-{worker}"))
                .spawn(move || loop {
                    let request = {
                        let Ok(receiver) = requests.lock() else {
                            break;
                        };
                        match receiver.recv() {
                            Ok(request) => request,
                            Err(_) => break,
                        }
                    };
                    let result = process_image_request(request);
                    if sender.send(result).is_err() {
                        break;
                    }
                });
        }

        Self {
            request_sender,
            receiver,
        }
    }

    pub fn start(&self, request: ImageLoadRequest) {
        let _ = self.request_sender.send(request);
    }

    pub fn try_recv(&self) -> Option<ImageLoadResult> {
        match self.receiver.try_recv() {
            Ok(value) => Some(value),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
        }
    }
}

fn image_worker_count() -> usize {
    let cores = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(4);
    (cores / 2).clamp(2, 6)
}

fn process_image_request(request: ImageLoadRequest) -> ImageLoadResult {
    let mut network = PrivacyNetwork::new_with_storage(request.storage.clone());
    if !request.custom_filters.trim().is_empty() {
        network
            .blocker_mut()
            .replace_custom_filters(request.custom_filters.clone());
    }

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if request.url.scheme() == "data" {
            let (content_type, bytes) = decode_data_image(request.url.as_str())?;
            let decoded = decode_image_payload(&bytes, &content_type)?;
            return Ok(DecodedImage {
                final_url: request.url.to_string(),
                size: decoded.size,
                rgba: decoded.rgba,
            });
        }

        let response = network.get_image(&request.top_level, &request.url, request.privacy)?;
        let decoded = decode_image_payload(&response.bytes, &response.content_type)?;
        Ok(DecodedImage {
            final_url: response.final_url.to_string(),
            size: decoded.size,
            rgba: decoded.rgba,
        })
    }))
    .unwrap_or_else(|_| Err("Veil recovered from an image worker panic.".into()));

    let blocked_count = network.blocked_count();
    let blocked_events = network.take_blocked_events();
    ImageLoadResult {
        key: request.key,
        result,
        blocked_count,
        blocked_events,
    }
}
'''
text = replace_once(text, old_loader, new_loader, "image loader worker pool")
text = replace_once(text, "#[cfg(test)]\nmod tests {\n    use super::*;\n", "#[cfg(test)]\nmod tests {\n    use super::*;\n    use std::io::Cursor;\n\n    #[test]\n    fn worker_pool_is_bounded() {\n        assert!((2..=6).contains(&image_worker_count()));\n    }\n", "image worker pool test")
path.write_text(text)

# --- Cancel superseded navigation work between expensive stages ---
path = Path("src/loader.rs")
text = path.read_text()
text = replace_once(text, "use std::collections::HashSet;\nuse std::sync::mpsc::{self, Receiver, Sender, TryRecvError};\n", "use std::collections::{HashMap, HashSet};\nuse std::sync::mpsc::{self, Receiver, Sender, TryRecvError};\nuse std::sync::{Arc, Mutex};\n", "loader cancellation imports")
text = replace_once(text, '''pub struct PageLoader {
    sender: Sender<LoadResult>,
    receiver: Receiver<LoadResult>,
}
''', '''pub struct PageLoader {
    sender: Sender<LoadResult>,
    receiver: Receiver<LoadResult>,
    latest_generation: Arc<Mutex<HashMap<u64, u64>>>,
}
''', "PageLoader generation field")
text = replace_once(text, '''        let (sender, receiver) = mpsc::channel();
        Self { sender, receiver }
''', '''        let (sender, receiver) = mpsc::channel();
        Self {
            sender,
            receiver,
            latest_generation: Arc::new(Mutex::new(HashMap::new())),
        }
''', "PageLoader init")
text = replace_once(text, '''    pub fn start(&self, request: LoadRequest) {
        let sender = self.sender.clone();
        thread::spawn(move || {
''', '''    pub fn start(&self, request: LoadRequest) {
        if let Ok(mut latest) = self.latest_generation.lock() {
            latest.insert(request.tab_id, request.generation);
        }
        let sender = self.sender.clone();
        let latest_generation = self.latest_generation.clone();
        thread::spawn(move || {
''', "PageLoader start generation")
text = replace_once(text, "                load_document(&mut network, &request)\n", "                load_document(&mut network, &request, &latest_generation)\n", "load_document generation argument")
text = replace_once(text, '''fn load_document(
    network: &mut PrivacyNetwork,
    request: &LoadRequest,
) -> Result<(DocumentView, RendererMode), String> {
''', '''fn load_document(
    network: &mut PrivacyNetwork,
    request: &LoadRequest,
    latest_generation: &Arc<Mutex<HashMap<u64, u64>>>,
) -> Result<(DocumentView, RendererMode), String> {
''', "load_document signature")
text = replace_once(text, '''    let final_url = response.final_url.clone();
''', '''    ensure_navigation_current(latest_generation, request)?;
    let final_url = response.final_url.clone();
''', "cancel after document fetch")
text = replace_once(text, '''    for stylesheet in stylesheet_urls.into_iter().take(stylesheet_limit) {
        if let Ok(resource) = network.get_stylesheet(&final_url, &stylesheet, render_privacy) {
''', '''    for stylesheet in stylesheet_urls.into_iter().take(stylesheet_limit) {
        ensure_navigation_current(latest_generation, request)?;
        if let Ok(resource) = network.get_stylesheet(&final_url, &stylesheet, render_privacy) {
''', "cancel stylesheet loop")
text = replace_once(text, '''                for source in discovery.discover_web_fonts(&resource.body, &resource.final_url) {
                    if web_fonts.len() >= MAX_WEB_FONTS_PER_PAGE {
''', '''                for source in discovery.discover_web_fonts(&resource.body, &resource.final_url) {
                    ensure_navigation_current(latest_generation, request)?;
                    if web_fonts.len() >= MAX_WEB_FONTS_PER_PAGE {
''', "cancel font loop")
text = replace_once(text, '''        for script in script_urls.into_iter().take(script_limit) {
            if let Ok(resource) = network.get_script(&final_url, &script, request.privacy) {
''', '''        for script in script_urls.into_iter().take(script_limit) {
            ensure_navigation_current(latest_generation, request)?;
            if let Ok(resource) = network.get_script(&final_url, &script, request.privacy) {
''', "cancel script loop")
text = replace_once(text, '''    let (mut view, mode) = RendererHost::default().render(render_request)?;
''', '''    ensure_navigation_current(latest_generation, request)?;
    let (mut view, mode) = RendererHost::default().render(render_request)?;
''', "cancel before render")
helper_marker = "\nfn is_guarded_heavy_site(url: &Url) -> bool {\n"
helper = r'''
fn ensure_navigation_current(
    latest_generation: &Arc<Mutex<HashMap<u64, u64>>>,
    request: &LoadRequest,
) -> Result<(), String> {
    let current = latest_generation
        .lock()
        .ok()
        .and_then(|latest| latest.get(&request.tab_id).copied())
        .unwrap_or(request.generation);
    if current == request.generation {
        Ok(())
    } else {
        Err("Navigation superseded by a newer request.".into())
    }
}

'''
text = replace_once(text, helper_marker, "\n" + helper + "fn is_guarded_heavy_site(url: &Url) -> bool {\n", "navigation helper")
path.write_text(text)

# --- Refresh-driver style rAF/timer scheduling ---
path = Path("src/script.rs")
text = path.read_text()
text = replace_once(text, '''    pub event_listener_count: usize,
}
''', '''    pub event_listener_count: usize,
    pub pending_timer_count: usize,
    pub pending_animation_frame_count: usize,
}
''', "ScriptReport pending work fields")
text = replace_once(text, '''    pub fn tick(&mut self, _elapsed_ms: u64, storage: &ScriptStorageSnapshot) -> ScriptReport {
        if let Err(err) = self.context.eval(Source::from_bytes("__vvRunTimers();")) {
            self.errors += 1;
            self.last_error = Some(format!("Timer event error: {err}"));
        }
        let _ = self.context.run_jobs();
        self.snapshot(storage)
    }
''', '''    pub fn tick(&mut self, elapsed_ms: u64, storage: &ScriptStorageSnapshot) -> ScriptReport {
        let elapsed_ms = elapsed_ms.min(1_000);
        let source = format!("__vvAdvanceClock({elapsed_ms});__vvRunAnimationFrames();");
        if let Err(err) = self.context.eval(Source::from_bytes(source.as_str())) {
            self.errors += 1;
            self.last_error = Some(format!("Refresh tick error: {err}"));
        }
        let _ = self.context.run_jobs();
        self.snapshot(storage)
    }
''', "runtime refresh tick")
text = replace_once(text, '''        report.event_listener_count =
            eval_usize(&mut self.context, "__vvListenerCount").unwrap_or_default();
        report
''', '''        report.event_listener_count =
            eval_usize(&mut self.context, "__vvListenerCount").unwrap_or_default();
        report.pending_timer_count =
            eval_usize(&mut self.context, "__vvTimers.length").unwrap_or_default();
        report.pending_animation_frame_count =
            eval_usize(&mut self.context, "__vvAnimationFrames.length").unwrap_or_default();
        report
''', "snapshot pending work counts")
text = replace_once(text, "globalThis.__vvTimers = [];\n", "globalThis.__vvTimers = [];\nglobalThis.__vvAnimationFrames = [];\nglobalThis.__vvClock = 0;\nglobalThis.__vvNextTimerId = 1;\n", "animation queues")
old_timers = r'''globalThis.setTimeout=(fn,_ms=0,...args)=>{if(typeof fn==='function'&&__vvTimers.length<192){__vvTimers.push(()=>fn(...args));return __vvTimers.length;}return 0;};
globalThis.clearTimeout=_id=>{};globalThis.requestAnimationFrame=fn=>setTimeout(()=>fn(0),0);globalThis.cancelAnimationFrame=clearTimeout;
globalThis.__vvRunTimers=()=>{let guard=0;while(__vvTimers.length&&guard++<192){const fn=__vvTimers.shift();try{fn();}catch(e){__vvConsole.push('timer error: '+e);}}};
'''
new_timers = r'''globalThis.setTimeout=(fn,ms=0,...args)=>{
  if(typeof fn!=='function'||__vvTimers.length>=192)return 0;
  const id=__vvNextTimerId++, delay=Math.max(0,Number(ms)||0);
  __vvTimers.push({id,due:__vvClock+delay,interval:0,fn:()=>fn(...args)});return id;
};
globalThis.setInterval=(fn,ms=0,...args)=>{
  if(typeof fn!=='function'||__vvTimers.length>=192)return 0;
  const id=__vvNextTimerId++, delay=Math.max(4,Number(ms)||0);
  __vvTimers.push({id,due:__vvClock+delay,interval:delay,fn:()=>fn(...args)});return id;
};
globalThis.clearTimeout=id=>{
  id=Number(id)||0;for(let i=__vvTimers.length-1;i>=0;i--)if(__vvTimers[i].id===id)__vvTimers.splice(i,1);
};
globalThis.clearInterval=globalThis.clearTimeout;
globalThis.requestAnimationFrame=fn=>{
  if(typeof fn!=='function'||__vvAnimationFrames.length>=192)return 0;
  const id=__vvNextTimerId++;__vvAnimationFrames.push({id,fn});return id;
};
globalThis.cancelAnimationFrame=id=>{
  id=Number(id)||0;for(let i=__vvAnimationFrames.length-1;i>=0;i--)if(__vvAnimationFrames[i].id===id)__vvAnimationFrames.splice(i,1);
};
globalThis.performance=Object.freeze({now:()=>__vvClock,timeOrigin:0});
globalThis.__vvAdvanceClock=elapsed=>{
  __vvClock+=Math.max(0,Number(elapsed)||0);
  const pending=__vvTimers.splice(0,__vvTimers.length);let guard=0;
  for(const timer of pending){
    if(timer.due>__vvClock||guard>=192){__vvTimers.push(timer);continue;}
    guard++;try{timer.fn();}catch(e){__vvConsole.push('timer error: '+e);}
    if(timer.interval>0&&__vvTimers.length<192){timer.due=__vvClock+timer.interval;__vvTimers.push(timer);}
  }
};
globalThis.__vvRunAnimationFrames=()=>{
  const batch=__vvAnimationFrames.splice(0,192);
  for(const frame of batch){try{frame.fn(__vvClock);}catch(e){__vvConsole.push('animation frame error: '+e);}}
};
'''
text = replace_once(text, old_timers, new_timers, "refresh driver JS timers")
text = text.replace("VeilBrowser/0.8.0 VeilEngine/0.8.0", "VeilBrowser/0.8.6 VeilEngine/0.8.6")
test_marker = r'''    #[test]
    fn javascript_can_be_disabled_per_site() {
'''
new_tests = r'''    #[test]
    fn animation_frames_wait_for_refresh_tick() {
        let dom = Dom::parse("<script>requestAnimationFrame(()=>{document.title='Frame'})</script>");
        let (mut runtime, initial) = LiveJavascriptRuntime::new(
            &dom,
            &[],
            0,
            &ScriptStorageSnapshot::default(),
        );
        assert_eq!(initial.pending_animation_frame_count, 1);
        assert_ne!(initial.title_override.as_deref(), Some("Frame"));

        let refreshed = runtime.tick(16, &ScriptStorageSnapshot::default());
        assert_eq!(refreshed.title_override.as_deref(), Some("Frame"));
        assert_eq!(refreshed.pending_animation_frame_count, 0);
    }

    #[test]
    fn timeout_respects_elapsed_refresh_time() {
        let dom = Dom::parse("<script>setTimeout(()=>{document.title='Later'},100)</script>");
        let (mut runtime, initial) = LiveJavascriptRuntime::new(
            &dom,
            &[],
            0,
            &ScriptStorageSnapshot::default(),
        );
        assert_eq!(initial.pending_timer_count, 1);
        let early = runtime.tick(50, &ScriptStorageSnapshot::default());
        assert_ne!(early.title_override.as_deref(), Some("Later"));
        let due = runtime.tick(50, &ScriptStorageSnapshot::default());
        assert_eq!(due.title_override.as_deref(), Some("Later"));
    }

    #[test]
    fn javascript_can_be_disabled_per_site() {
'''
text = replace_once(text, test_marker, new_tests, "script refresh tests")
path.write_text(text)

# --- Main-thread frame budgets, input priority, adaptive refresh pacing, font dedupe ---
path = Path("src/main.rs")
text = path.read_text()
text = replace_once(text, '''const MAX_IMAGE_LOADS: usize = 6;
''', '''const MAX_IMAGE_LOADS: usize = 6;
const MAX_PAGE_RESULTS_PER_FRAME: usize = 1;
const MAX_RUNTIME_RESULTS_PER_FRAME: usize = 1;
const MAX_IMAGE_RESULTS_PER_FRAME: usize = 2;
const MAX_MEDIA_RESULTS_PER_FRAME: usize = 1;
const ACTIVE_REFRESH_INTERVAL: Duration = Duration::from_millis(16);
const TIMER_REFRESH_INTERVAL: Duration = Duration::from_millis(50);
''', "main frame budget constants")
text = replace_once(text, '''    fn poll_loader(&mut self, ctx: &egui::Context) {
        while let Some(result) = self.loader.try_recv() {
''', '''    fn poll_loader(&mut self, ctx: &egui::Context) {
        let mut processed = 0usize;
        while processed < MAX_PAGE_RESULTS_PER_FRAME {
            let Some(result) = self.loader.try_recv() else {
                break;
            };
            processed += 1;
''', "bounded page result polling")
text = replace_once(text, '''            }
        }
    }

    fn dispatch_runtime_event''', '''            }
        }
        if processed == MAX_PAGE_RESULTS_PER_FRAME {
            ctx.request_repaint();
        }
    }

    fn dispatch_runtime_event''', "page polling continuation")
text = replace_once(text, '''    fn poll_runtime_loader(&mut self, ctx: &egui::Context) {
        while let Some(result) = self.runtime_loader.try_recv() {
''', '''    fn poll_runtime_loader(&mut self, ctx: &egui::Context) {
        let mut processed = 0usize;
        while processed < MAX_RUNTIME_RESULTS_PER_FRAME {
            let Some(result) = self.runtime_loader.try_recv() else {
                break;
            };
            processed += 1;
''', "bounded runtime polling")
text = replace_once(text, '''            }
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
''', '''            }
        }
        if processed == MAX_RUNTIME_RESULTS_PER_FRAME {
            ctx.request_repaint();
        }
    }

    fn visible_runtime_indices(&self) -> Vec<usize> {
        let mut indices = vec![self.active_tab];
        if let Some(split) = self.split_tab_index() {
            if split != self.active_tab {
                indices.push(split);
            }
        }
        indices
    }

    fn pending_refresh_interval(&self) -> Option<Duration> {
        let mut has_timer = false;
        for index in self.visible_runtime_indices() {
            let Some(tab) = self.tabs.get(index) else {
                continue;
            };
            if tab.page.url == HOME {
                continue;
            }
            if tab.page.script_report.pending_animation_frame_count > 0 {
                return Some(ACTIVE_REFRESH_INTERVAL);
            }
            has_timer |= tab.page.script_report.pending_timer_count > 0;
        }
        has_timer.then_some(TIMER_REFRESH_INTERVAL)
    }

    fn pump_runtime_timers(&mut self) {
        let Some(interval) = self.pending_refresh_interval() else {
            return;
        };
        let elapsed = self.last_runtime_tick.elapsed();
        if elapsed < interval {
            return;
        }
        self.last_runtime_tick = Instant::now();
        let elapsed_ms = elapsed.as_millis().min(1_000) as u64;

        for index in self.visible_runtime_indices() {
            if index >= self.tabs.len() || self.tabs[index].page.url == HOME {
                continue;
            }
            let tab = &self.tabs[index];
            if tab.page.script_report.pending_animation_frame_count == 0
                && tab.page.script_report.pending_timer_count == 0
            {
                continue;
            }
            let javascript_enabled = !is_guarded_script_site(&tab.page.url)
                && Url::parse(&tab.page.url)
                    .ok()
                    .map(|url| self.profiles.for_url(&url).javascript)
                    .unwrap_or(false);
            if !javascript_enabled || self.runtime_inflight.contains(&tab.id) {
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
''', "refresh-driver runtime pump")
text = replace_once(text, '''    fn poll_image_loader(&mut self, ctx: &egui::Context) {
        while let Some(result) = self.image_loader.try_recv() {
''', '''    fn poll_image_loader(&mut self, ctx: &egui::Context) {
        let mut processed = 0usize;
        while processed < MAX_IMAGE_RESULTS_PER_FRAME {
            let Some(result) = self.image_loader.try_recv() else {
                break;
            };
            processed += 1;
''', "bounded image polling")
text = replace_once(text, '''            self.image_cache.insert(result.key, cached);
        }
    }

    fn pump_image_queue(&mut self) {
        while self.image_loads_inflight < MAX_IMAGE_LOADS {
''', '''            self.image_cache.insert(result.key, cached);
        }
        if processed == MAX_IMAGE_RESULTS_PER_FRAME {
            ctx.request_repaint();
        }
    }

    fn pump_image_queue(&mut self) {
        let concurrency = image_decode_concurrency();
        while self.image_loads_inflight < concurrency {
''', "adaptive image queue")
text = replace_once(text, '''    fn poll_media_loader(&mut self) {
        while let Some(result) = self.media_loader.try_recv() {
''', '''    fn poll_media_loader(&mut self, ctx: &egui::Context) {
        let mut processed = 0usize;
        while processed < MAX_MEDIA_RESULTS_PER_FRAME {
            let Some(result) = self.media_loader.try_recv() else {
                break;
            };
            processed += 1;
''', "bounded media polling")
text = replace_once(text, '''            self.media_cache.insert(result.key, cached);
        }
    }

    fn go_back''', '''            self.media_cache.insert(result.key, cached);
        }
        if processed == MAX_MEDIA_RESULTS_PER_FRAME {
            ctx.request_repaint();
        }
    }

    fn go_back''', "media polling continuation")
old_update = r'''    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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
'''
new_update = r'''    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.update_hover_reveals(ctx);

        // Gecko's task scheduler prioritizes user-visible/input work over
        // background completion. Handle input first, then drain bounded worker
        // batches so a burst of decoded images cannot monopolize one frame.
        self.handle_shortcuts(ctx);
        self.poll_loader(ctx);
        self.poll_runtime_loader(ctx);
        self.pump_runtime_timers();
        self.poll_image_loader(ctx);
        self.pump_image_queue();
        self.poll_media_loader(ctx);

        let refresh = self.pending_refresh_interval();
        let background_active = self.tabs.iter().any(|tab| tab.loading)
            || self.image_loads_inflight > 0
            || !self.image_load_queue.is_empty()
            || !self.runtime_inflight.is_empty();
        if let Some(interval) = refresh {
            ctx.request_repaint_after(interval);
        } else if background_active {
            ctx.request_repaint_after(Duration::from_millis(32));
        }

        shell_ui::render_content(self, ctx);
'''
text = replace_once(text, old_update, new_update, "input-first update loop")
text = replace_once(text, '''    for font in fonts {
        let bytes = font.bytes.as_slice();
        let supported = bytes.starts_with(&[0x00, 0x01, 0x00, 0x00])
''', '''    let mut changed = false;
    for font in fonts {
        let bytes = font.bytes.as_slice();
        let supported = bytes.starts_with(&[0x00, 0x01, 0x00, 0x00])
''', "font change tracking start")
text = replace_once(text, '''        registry.insert(family, font.bytes.clone());
    }

    if registry.is_empty() {
        return;
    }
''', '''        let identical = registry
            .get(&family)
            .is_some_and(|existing| existing.as_slice() == bytes);
        if !identical {
            registry.insert(family, font.bytes.clone());
            changed = true;
        }
    }

    if registry.is_empty() || !changed {
        return;
    }
''', "font rebuild dedupe")
helper_marker = "\nfn sidebar_action(\n"
helper = r'''
fn image_decode_concurrency() -> usize {
    let cores = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(4);
    (cores / 2).clamp(2, MAX_IMAGE_LOADS)
}

'''
text = replace_once(text, helper_marker, "\n" + helper + "fn sidebar_action(\n", "image concurrency helper")
path.write_text(text)

print("Veil 0.8.6 Gecko smoothness patch applied.")

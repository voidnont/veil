use std::collections::HashMap;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};

use url::Url;

use crate::blocker::Blocker;
use crate::engine::{DocumentView, Engine};
use crate::privacy::site_key_for_url;
use crate::renderer_protocol::{DomEventRequest, RenderRequest, RendererCommand, RendererReply};

const MAX_SITE_RENDERERS: usize = 8;
const MAX_RENDER_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RendererMode {
    SiteSandboxedProcess,
    SiteProcess,
    InProcessFallback,
}

impl RendererMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::SiteSandboxedProcess => "site-isolated sandboxed engine",
            Self::SiteProcess => "site-isolated engine",
            Self::InProcessFallback => "in-process engine fallback",
        }
    }
}

pub struct RuntimeHostUpdate {
    pub view: DocumentView,
    pub mode: RendererMode,
    pub default_prevented: bool,
}

#[derive(Default)]
pub struct RendererHost;

struct SiteRenderer {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    mode: RendererMode,
}

impl Drop for SiteRenderer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

type SharedRenderer = Arc<Mutex<SiteRenderer>>;
static SITE_POOL: OnceLock<Mutex<HashMap<String, SharedRenderer>>> = OnceLock::new();

fn pool() -> &'static Mutex<HashMap<String, SharedRenderer>> {
    SITE_POOL.get_or_init(|| Mutex::new(HashMap::new()))
}

impl RendererHost {
    pub fn render(&self, request: RenderRequest) -> Result<(DocumentView, RendererMode), String> {
        match self.render_site_process(&request) {
            Ok(result) => Ok(result),
            Err(process_error) => {
                if !in_process_fallback_is_safe(&request) {
                    return Err(format!(
                        "isolated Veil Engine failed on a heavyweight page; main browser process was kept protected: {process_error}"
                    ));
                }
                let view = render_in_process(request).map_err(|fallback_error| {
                    format!(
                        "engine process failed: {process_error}; fallback failed: {fallback_error}"
                    )
                })?;
                Ok((view, RendererMode::InProcessFallback))
            }
        }
    }

    pub fn dispatch_event(
        &self,
        page_url: &str,
        session_id: &str,
        event: DomEventRequest,
    ) -> Result<RuntimeHostUpdate, String> {
        let renderer = self.renderer_for_url(page_url, true)?;
        let (reply, mode) = send_command(
            &renderer,
            &RendererCommand::Event {
                session_id: session_id.to_owned(),
                event,
            },
        )?;
        match reply {
            RendererReply::Runtime(result) => result.map(|update| RuntimeHostUpdate {
                view: update.view,
                mode,
                default_prevented: update.default_prevented,
            }),
            RendererReply::Render(_) => Err("unexpected render reply for runtime event".into()),
        }
    }

    pub fn tick(
        &self,
        page_url: &str,
        session_id: &str,
        elapsed_ms: u64,
    ) -> Result<RuntimeHostUpdate, String> {
        let renderer = self.renderer_for_url(page_url, true)?;
        let (reply, mode) = send_command(
            &renderer,
            &RendererCommand::Tick {
                session_id: session_id.to_owned(),
                elapsed_ms,
            },
        )?;
        match reply {
            RendererReply::Runtime(result) => result.map(|update| RuntimeHostUpdate {
                view: update.view,
                mode,
                default_prevented: update.default_prevented,
            }),
            RendererReply::Render(_) => Err("unexpected render reply for runtime tick".into()),
        }
    }

    fn render_site_process(
        &self,
        request: &RenderRequest,
    ) -> Result<(DocumentView, RendererMode), String> {
        let renderer = self.renderer_for_url(&request.url, true)?;
        match send_command(&renderer, &RendererCommand::Render(request.clone())) {
            Ok((RendererReply::Render(result), mode)) => result.map(|view| (view, mode)),
            Ok((RendererReply::Runtime(_), _)) => {
                Err("unexpected runtime reply for render request".into())
            }
            Err(first_error) => {
                let site = site_for_url(&request.url);
                remove_site_renderer(&site);
                let renderer = self.renderer_for_url(&request.url, false)?;
                match send_command(&renderer, &RendererCommand::Render(request.clone())) {
                    Ok((RendererReply::Render(result), mode)) => result.map(|view| (view, mode)),
                    Ok((RendererReply::Runtime(_), _)) => {
                        Err("unexpected runtime reply after engine restart".into())
                    }
                    Err(second) => Err(format!("{first_error}; engine restart failed: {second}")),
                }
            }
        }
    }

    fn renderer_for_url(
        &self,
        page_url: &str,
        allow_sandbox: bool,
    ) -> Result<SharedRenderer, String> {
        let path = renderer_path()?;
        if !path.exists() {
            return Err(format!(
                "Veil Engine executable not found at {}",
                path.display()
            ));
        }
        let site = site_for_url(page_url);
        acquire_site_renderer(&site, &path, allow_sandbox)
    }
}

fn site_for_url(page_url: &str) -> String {
    Url::parse(page_url)
        .ok()
        .map(|url| site_key_for_url(&url))
        .filter(|site| !site.is_empty())
        .unwrap_or_else(|| "opaque".into())
}

fn acquire_site_renderer(
    site: &str,
    path: &Path,
    allow_sandbox: bool,
) -> Result<SharedRenderer, String> {
    let mut guard = pool()
        .lock()
        .map_err(|_| "engine pool lock poisoned".to_owned())?;
    if let Some(renderer) = guard.get(site) {
        return Ok(renderer.clone());
    }

    if guard.len() >= MAX_SITE_RENDERERS {
        if let Some(key) = guard.keys().next().cloned() {
            guard.remove(&key);
        }
    }

    let renderer = Arc::new(Mutex::new(spawn_site_renderer(path, allow_sandbox)?));
    guard.insert(site.to_owned(), renderer.clone());
    Ok(renderer)
}

fn remove_site_renderer(site: &str) {
    if let Ok(mut guard) = pool().lock() {
        guard.remove(site);
    }
}

fn send_command(
    renderer: &SharedRenderer,
    command: &RendererCommand,
) -> Result<(RendererReply, RendererMode), String> {
    let mut renderer = renderer
        .lock()
        .map_err(|_| "site engine lock poisoned".to_owned())?;
    serde_json::to_writer(&mut renderer.stdin, command)
        .map_err(|e| format!("engine request encode failed: {e}"))?;
    renderer
        .stdin
        .write_all(b"\n")
        .map_err(|e| format!("engine request write failed: {e}"))?;
    renderer
        .stdin
        .flush()
        .map_err(|e| format!("engine stdin flush failed: {e}"))?;

    let mut line = String::new();
    let read = renderer
        .stdout
        .read_line(&mut line)
        .map_err(|e| format!("engine response read failed: {e}"))?;
    if read == 0 {
        return Err("Veil Engine exited before producing a response".into());
    }
    if line.len() > MAX_RENDER_RESPONSE_BYTES {
        return Err("Veil Engine response exceeded 32 MiB safety limit".into());
    }
    let reply: RendererReply = serde_json::from_str(line.trim_end())
        .map_err(|e| format!("engine response decode failed: {e}"))?;
    Ok((reply, renderer.mode))
}

fn spawn_site_renderer(path: &Path, allow_sandbox: bool) -> Result<SiteRenderer, String> {
    #[cfg(not(target_os = "linux"))]
    let _ = allow_sandbox;

    #[cfg(target_os = "linux")]
    let (mut command, mode) = if allow_sandbox {
        if let Some(bwrap) = find_in_path("bwrap") {
            (
                linux_bwrap_command(&bwrap, path),
                RendererMode::SiteSandboxedProcess,
            )
        } else {
            (Command::new(path), RendererMode::SiteProcess)
        }
    } else {
        (Command::new(path), RendererMode::SiteProcess)
    };

    #[cfg(not(target_os = "linux"))]
    let (mut command, mode) = (Command::new(path), RendererMode::SiteProcess);

    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env_clear()
        .env("VEIL_ENGINE_SANDBOX", "1")
        .env("VEIL_ENGINE_SITE_PROCESS", "1");

    let mut child = command
        .spawn()
        .map_err(|e| format!("failed to start Veil Engine: {e}"))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "engine stdin unavailable".to_owned())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "engine stdout unavailable".to_owned())?;

    Ok(SiteRenderer {
        child,
        stdin: BufWriter::new(stdin),
        stdout: BufReader::new(stdout),
        mode,
    })
}

#[cfg(target_os = "linux")]
fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

#[cfg(target_os = "linux")]
fn linux_bwrap_command(bwrap: &Path, renderer: &Path) -> Command {
    let mut command = Command::new(bwrap);
    command
        .arg("--die-with-parent")
        .arg("--new-session")
        .arg("--unshare-net")
        .arg("--unshare-pid")
        .arg("--unshare-ipc")
        .arg("--unshare-uts")
        .arg("--ro-bind")
        .arg("/")
        .arg("/")
        .arg("--dir")
        .arg("/app")
        .arg("--ro-bind")
        .arg(renderer)
        .arg("/app/veil-engine")
        .arg("--tmpfs")
        .arg("/tmp")
        .arg("--tmpfs")
        .arg("/home")
        .arg("--tmpfs")
        .arg("/root")
        .arg("--proc")
        .arg("/proc")
        .arg("--dev")
        .arg("/dev")
        .arg("--")
        .arg("/app/veil-engine");
    command
}

fn renderer_path() -> Result<PathBuf, String> {
    let exe =
        std::env::current_exe().map_err(|e| format!("could not locate browser executable: {e}"))?;
    let dir = exe
        .parent()
        .ok_or_else(|| "browser executable has no parent directory".to_owned())?;
    #[cfg(windows)]
    let name = "veil-engine.exe";
    #[cfg(not(windows))]
    let name = "veil-engine";
    Ok(dir.join(name))
}

fn in_process_fallback_is_safe(request: &RenderRequest) -> bool {
    let total = request
        .html
        .len()
        .saturating_add(request.external_css.iter().map(String::len).sum::<usize>())
        .saturating_add(
            request
                .external_scripts
                .iter()
                .map(String::len)
                .sum::<usize>(),
        );
    total <= 3 * 1024 * 1024 && request.external_scripts.len() <= 6
}

fn render_in_process(request: RenderRequest) -> Result<DocumentView, String> {
    let mut blocker = Blocker::default();
    if !request.custom_filters.trim().is_empty() {
        blocker.replace_custom_filters(request.custom_filters.clone());
    }
    let engine = Engine::default();
    Ok(engine.parse_with_resources(
        &request.url,
        &request.html,
        &blocker,
        request.privacy,
        &request.external_css,
        &request.external_scripts,
        request.external_script_count,
        &request.storage,
    ))
}

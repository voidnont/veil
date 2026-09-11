use std::collections::HashMap;
use std::io::{self, BufRead, Write};

use veil_engine::blocker::Blocker;
use veil_engine::dom::Dom;
use veil_engine::engine::{DocumentView, Engine};
use veil_engine::renderer_protocol::{
    RendererCommand, RendererReply, RenderRequest, RuntimeUpdate,
};
use veil_engine::script::{JavascriptSandbox, LiveJavascriptRuntime, ScriptReport};

const MAX_REQUEST_LINE: usize = 24 * 1024 * 1024;
const MAX_SESSIONS: usize = 32;

struct LiveSession {
    request: RenderRequest,
    runtime: Option<LiveJavascriptRuntime>,
    report: ScriptReport,
}

fn main() {
    let stdin = io::stdin();
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    let mut sessions: HashMap<String, LiveSession> = HashMap::new();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => break,
        };
        if line.is_empty() { continue; }

        let reply = if line.len() > MAX_REQUEST_LINE {
            RendererReply::Render(Err("engine request exceeded 24 MiB safety limit".into()))
        } else {
            match serde_json::from_str::<RendererCommand>(&line) {
                Ok(RendererCommand::Render(request)) => {
                    let session_id = request.session_id.clone();
                    let (session, result) = create_session(request);
                    if sessions.len() >= MAX_SESSIONS && !sessions.contains_key(&session_id) {
                        if let Some(key) = sessions.keys().next().cloned() { sessions.remove(&key); }
                    }
                    sessions.insert(session_id, session);
                    RendererReply::Render(result)
                }
                Ok(RendererCommand::Event { session_id, event }) => {
                    let result = sessions
                        .get_mut(&session_id)
                        .ok_or_else(|| "live page session not found".to_owned())
                        .and_then(|session| {
                            let Some(runtime) = session.runtime.as_mut() else {
                                return Err("JavaScript is disabled for this page".into());
                            };
                            let (default_prevented, report) = runtime.dispatch_event(
                                event.node_id,
                                &event.event_type,
                                event.value.as_deref(),
                                &session.report.storage,
                            );
                            session.report = report;
                            Ok(RuntimeUpdate {
                                view: render_session(session),
                                default_prevented,
                            })
                        });
                    RendererReply::Runtime(result)
                }
                Ok(RendererCommand::Tick { session_id, elapsed_ms }) => {
                    let result = sessions
                        .get_mut(&session_id)
                        .ok_or_else(|| "live page session not found".to_owned())
                        .and_then(|session| {
                            let Some(runtime) = session.runtime.as_mut() else {
                                return Err("JavaScript is disabled for this page".into());
                            };
                            session.report = runtime.tick(elapsed_ms, &session.report.storage);
                            Ok(RuntimeUpdate {
                                view: render_session(session),
                                default_prevented: false,
                            })
                        });
                    RendererReply::Runtime(result)
                }
                Err(err) => RendererReply::Render(Err(format!("invalid engine request: {err}"))),
            }
        };

        if serde_json::to_writer(&mut stdout, &reply).is_err() { break; }
        if stdout.write_all(b"\n").is_err() || stdout.flush().is_err() { break; }
    }
}

fn create_session(request: RenderRequest) -> (LiveSession, Result<DocumentView, String>) {
    let dom = Dom::parse(&request.html);
    let (runtime, report) = if request.privacy.javascript {
        let (runtime, report) = LiveJavascriptRuntime::new(
            &dom,
            &request.external_scripts,
            request.external_script_count,
            &request.storage,
        );
        (Some(runtime), report)
    } else {
        let report = JavascriptSandbox::default().run(
            &dom,
            false,
            &request.external_scripts,
            request.external_script_count,
            &request.storage,
        );
        (None, report)
    };

    let session = LiveSession { request, runtime, report };
    let view = render_session(&session);
    (session, Ok(view))
}

fn render_session(session: &LiveSession) -> DocumentView {
    let mut blocker = Blocker::default();
    if !session.request.custom_filters.trim().is_empty() {
        blocker.replace_custom_filters(session.request.custom_filters.clone());
    }
    Engine::default().render_with_script_report(
        &session.request.url,
        &session.request.html,
        &blocker,
        session.request.privacy,
        &session.request.external_css,
        session.request.external_script_count,
        session.report.clone(),
    )
}

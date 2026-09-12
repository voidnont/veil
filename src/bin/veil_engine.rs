use std::collections::HashMap;
use std::io::{self, BufRead, Write};

use veil_engine::blocker::Blocker;
use veil_engine::dom::Dom;
use veil_engine::engine::{paint_fingerprint, DocumentView, InvalidationKind, RetainedDocument};
use veil_engine::renderer_protocol::{
    RenderRequest, RendererCommand, RendererReply, RuntimeDamage, RuntimeUpdate,
};
use veil_engine::script::{JavascriptSandbox, LiveJavascriptRuntime, ScriptReport};

const MAX_REQUEST_LINE: usize = 24 * 1024 * 1024;
const MAX_SESSIONS: usize = 32;

struct LiveSession {
    runtime: Option<LiveJavascriptRuntime>,
    report: ScriptReport,
    retained: RetainedDocument,
    blocker: Blocker,
    last_view: DocumentView,
    last_paint_fingerprint: u64,
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
        if line.is_empty() {
            continue;
        }

        let reply = if line.len() > MAX_REQUEST_LINE {
            RendererReply::Render(Err("engine request exceeded 24 MiB safety limit".into()))
        } else {
            match serde_json::from_str::<RendererCommand>(&line) {
                Ok(RendererCommand::Render(request)) => {
                    let session_id = request.session_id.clone();
                    let (session, result) = create_session(request);
                    if sessions.len() >= MAX_SESSIONS && !sessions.contains_key(&session_id) {
                        if let Some(key) = sessions.keys().next().cloned() {
                            sessions.remove(&key);
                        }
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
                            Ok(update_session(session, default_prevented))
                        });
                    RendererReply::Runtime(result)
                }
                Ok(RendererCommand::Tick {
                    session_id,
                    elapsed_ms,
                }) => {
                    let result = sessions
                        .get_mut(&session_id)
                        .ok_or_else(|| "live page session not found".to_owned())
                        .and_then(|session| {
                            let Some(runtime) = session.runtime.as_mut() else {
                                return Err("JavaScript is disabled for this page".into());
                            };
                            session.report = runtime.tick(elapsed_ms, &session.report.storage);
                            Ok(update_session(session, false))
                        });
                    RendererReply::Runtime(result)
                }
                Err(err) => RendererReply::Render(Err(format!("invalid engine request: {err}"))),
            }
        };

        if serde_json::to_writer(&mut stdout, &reply).is_err() {
            break;
        }
        if stdout.write_all(b"\n").is_err() || stdout.flush().is_err() {
            break;
        }
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

    let mut blocker = Blocker::default();
    if !request.custom_filters.trim().is_empty() {
        blocker.replace_custom_filters(request.custom_filters.clone());
    }
    let retained = RetainedDocument::new(
        &request.url,
        &request.html,
        request.privacy,
        &request.external_css,
        request.external_script_count,
        &report,
    );
    let mut view = retained.render(&blocker, &report);
    view.external_stylesheets = request.external_css.len();
    let fingerprint = paint_fingerprint(&view);
    let session = LiveSession {
        runtime,
        report,
        retained,
        blocker,
        last_view: view.clone(),
        last_paint_fingerprint: fingerprint,
    };
    (session, Ok(view))
}

fn update_session(session: &mut LiveSession, default_prevented: bool) -> RuntimeUpdate {
    let requested_damage = session.retained.update_from_report(&session.report);
    let old_title = session.last_view.title.clone();
    let old_icon = session.last_view.icon_url.clone();

    let mut candidate = if requested_damage.needs_layout() {
        session.retained.render(&session.blocker, &session.report)
    } else {
        let mut view = session.last_view.clone();
        session
            .retained
            .update_cached_view(&mut view, &session.report, requested_damage);
        view
    };
    candidate.external_stylesheets = session.last_view.external_stylesheets;
    candidate.external_scripts = session.last_view.external_scripts;
    candidate.web_fonts = session.last_view.web_fonts.clone();

    let fingerprint = paint_fingerprint(&candidate);
    let paint_changed = fingerprint != session.last_paint_fingerprint;
    let metadata_changed = candidate.title != old_title || candidate.icon_url != old_icon;
    let actual_damage = if paint_changed {
        if requested_damage == InvalidationKind::Layout {
            RuntimeDamage::Layout
        } else {
            RuntimeDamage::Paint
        }
    } else if metadata_changed {
        RuntimeDamage::Metadata
    } else {
        RuntimeDamage::None
    };

    let view = if actual_damage == RuntimeDamage::None {
        session.last_view.script_report = session.report.clone();
        None
    } else {
        session.last_paint_fingerprint = fingerprint;
        session.last_view = candidate.clone();
        Some(candidate)
    };

    RuntimeUpdate {
        view,
        script_report: session.report.clone(),
        damage: actual_damage,
        default_prevented,
    }
}

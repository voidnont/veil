use std::collections::HashMap;
use std::io::{self, BufRead, Write};

use veil_engine::blocker::Blocker;
use veil_engine::display_list::{block_fingerprints, diff_fingerprints};
use veil_engine::dom::Dom;
use veil_engine::engine::{DocumentView, InvalidationKind, RetainedDocument};
use veil_engine::renderer_protocol::{
    DisplayListPatch, RenderRequest, RendererCommand, RendererReply, RuntimeDamage, RuntimeUpdate,
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
    last_paint_fingerprints: Vec<u64>,
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
    let fingerprints = block_fingerprints(&view.blocks);
    let session = LiveSession {
        runtime,
        report,
        retained,
        blocker,
        last_view: view.clone(),
        last_paint_fingerprints: fingerprints,
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

    let fingerprints = block_fingerprints(&candidate.blocks);
    let diff = diff_fingerprints(&session.last_paint_fingerprints, &fingerprints);
    let paint_changed = !diff.is_empty();
    let metadata_changed = candidate.title != old_title || candidate.icon_url != old_icon;
    let actual_damage = if paint_changed {
        match requested_damage {
            InvalidationKind::Layout => RuntimeDamage::Layout,
            InvalidationKind::LayoutSubtree => RuntimeDamage::LayoutSubtree,
            _ => RuntimeDamage::Paint,
        }
    } else if metadata_changed {
        RuntimeDamage::Metadata
    } else {
        RuntimeDamage::None
    };

    let mut full_view = None;
    let mut patch = None;
    if actual_damage != RuntimeDamage::None {
        let inserted = diff.new_end.saturating_sub(diff.start);
        let localized = actual_damage != RuntimeDamage::Layout
            && diff.changed > 0
            && diff.changed <= candidate.blocks.len().max(1).div_ceil(2)
            && inserted <= 96;
        if localized {
            patch = Some(DisplayListPatch {
                start: diff.start,
                remove_count: diff.old_remove_count,
                blocks: candidate.blocks[diff.start..diff.new_end].to_vec(),
            });
        } else if paint_changed {
            full_view = Some(candidate.clone());
        }
        session.last_paint_fingerprints = fingerprints;
        session.last_view = candidate.clone();
    } else {
        session.last_view.script_report = session.report.clone();
    }

    RuntimeUpdate {
        view: full_view,
        patch,
        script_report: session.report.clone(),
        damage: actual_damage,
        title: candidate.title,
        icon_url: candidate.icon_url,
        cosmetic_hidden: candidate.cosmetic_hidden,
        external_stylesheets: candidate.external_stylesheets,
        external_scripts: candidate.external_scripts,
        reused_blocks: diff.reused,
        default_prevented,
    }
}

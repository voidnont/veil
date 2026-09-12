from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected exactly one match, found {count}: {old[:120]!r}")
    p.write_text(text.replace(old, new, 1))


# 1) Stop allocating a JSON Vec just to fingerprint retained paint items.
replace_once(
    "src/display_list.rs",
    "use std::hash::{Hash, Hasher};\n",
    "use std::hash::Hasher;\nuse std::io::{self, Write};\n",
)
replace_once(
    "src/display_list.rs",
    '''pub fn block_fingerprint(block: &RenderBlock) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    if let Ok(bytes) = serde_json::to_vec(block) {
        bytes.hash(&mut hasher);
    }
    hasher.finish()
}''',
    '''struct HashWriter<'a, H: Hasher>(&'a mut H);

impl<H: Hasher> Write for HashWriter<'_, H> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.write(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub fn block_fingerprint(block: &RenderBlock) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let _ = serde_json::to_writer(HashWriter(&mut hasher), block);
    hasher.finish()
}''',
)
replace_once(
    "src/display_list.rs",
    '''        let height = height.clamp(1.0, 12_000.0);
        item.height = if item.measured {
            // Smooth tiny font/layout jitter so the virtual scroll geometry stays stable.
            item.height * 0.75 + height * 0.25
        } else {
            height
        };
        item.measured = true;''',
    '''        let height = height.clamp(1.0, 12_000.0);
        if item.measured && (item.height - height).abs() < 0.5 {
            return;
        }
        item.height = if item.measured {
            // Smooth meaningful geometry changes while ignoring sub-pixel frame jitter.
            item.height * 0.75 + height * 0.25
        } else {
            height
        };
        item.measured = true;''',
)

# 2) Reconcile the browser-side retained list only when navigation/runtime damage changed it.
#    Small pages also skip allocating the height vector entirely.
replace_once(
    "src/main.rs",
    '''        self.display_lists
            .entry(tab_id)
            .or_default()
            .reconcile(&page.blocks, page_width);
        let heights: Vec<f32> = page
            .blocks
            .iter()
            .enumerate()
            .map(|(index, block)| {
                self.display_lists
                    .get(&tab_id)
                    .map(|list| list.height_for(index, block, page_width))
                    .unwrap_or_else(|| estimate_block_height(block, page_width))
            })
            .collect();
        let virtualize = page.blocks.len() >= VIRTUALIZE_MIN_ITEMS;''',
    '''        if !self.display_lists.contains_key(&tab_id) {
            let mut display_list = RetainedDisplayList::default();
            display_list.reconcile(&page.blocks, page_width);
            self.display_lists.insert(tab_id, display_list);
        }
        let virtualize = page.blocks.len() >= VIRTUALIZE_MIN_ITEMS;
        let heights: Vec<f32> = if virtualize {
            page.blocks
                .iter()
                .enumerate()
                .map(|(index, block)| {
                    self.display_lists
                        .get(&tab_id)
                        .map(|list| list.height_for(index, block, page_width))
                        .unwrap_or_else(|| estimate_block_height(block, page_width))
                })
                .collect()
        } else {
            Vec::new()
        };''',
)
replace_once(
    "src/main.rs",
    '''                            let estimated =
                                heights.get(block_index).copied().unwrap_or_else(|| {
                                    estimate_block_height(block, ui.available_width())
                                });''',
    '''                            let estimated = if virtualize {
                                heights.get(block_index).copied().unwrap_or_else(|| {
                                    estimate_block_height(block, ui.available_width())
                                })
                            } else {
                                1.0
                            };''',
)

# 3) Runtime ticks with no visual damage should not clone/hash the entire page.
replace_once(
    "src/bin/veil_engine.rs",
    '''fn update_session(session: &mut LiveSession, default_prevented: bool) -> RuntimeUpdate {
    let requested_damage = session.retained.update_from_report(&session.report);
    let old_title = session.last_view.title.clone();
    let old_icon = session.last_view.icon_url.clone();
''',
    '''fn update_session(session: &mut LiveSession, default_prevented: bool) -> RuntimeUpdate {
    let requested_damage = session.retained.update_from_report(&session.report);

    if requested_damage == InvalidationKind::None {
        session.last_view.script_report = session.report.clone();
        return RuntimeUpdate {
            view: None,
            patch: None,
            script_report: session.report.clone(),
            damage: RuntimeDamage::None,
            title: session.last_view.title.clone(),
            icon_url: session.last_view.icon_url.clone(),
            cosmetic_hidden: session.last_view.cosmetic_hidden,
            external_stylesheets: session.last_view.external_stylesheets,
            external_scripts: session.last_view.external_scripts,
            reused_blocks: session.last_view.blocks.len(),
            default_prevented,
        };
    }

    if requested_damage == InvalidationKind::Metadata {
        session.retained.update_cached_view(
            &mut session.last_view,
            &session.report,
            InvalidationKind::Metadata,
        );
        return RuntimeUpdate {
            view: None,
            patch: None,
            script_report: session.report.clone(),
            damage: RuntimeDamage::Metadata,
            title: session.last_view.title.clone(),
            icon_url: session.last_view.icon_url.clone(),
            cosmetic_hidden: session.last_view.cosmetic_hidden,
            external_stylesheets: session.last_view.external_stylesheets,
            external_scripts: session.last_view.external_scripts,
            reused_blocks: session.last_view.blocks.len(),
            default_prevented,
        };
    }

    let old_title = session.last_view.title.clone();
    let old_icon = session.last_view.icon_url.clone();
''',
)

# 4) A persistent runtime worker replaces one OS thread spawn per tick/event.
p = Path("src/runtime_interaction.rs")
text = p.read_text()
start = text.index("pub struct RuntimeInteractionLoader {")
end = text.index("    pub fn try_recv(&self) -> Option<RuntimeInteractionResult> {", start)
prefix = text[:start]
suffix = text[end:]
replacement = r'''pub struct RuntimeInteractionLoader {
    request_sender: Sender<RuntimeInteractionRequest>,
    receiver: Receiver<RuntimeInteractionResult>,
}

impl RuntimeInteractionLoader {
    pub fn new() -> Self {
        let (request_sender, request_receiver) = mpsc::channel::<RuntimeInteractionRequest>();
        let (result_sender, receiver) = mpsc::channel::<RuntimeInteractionResult>();
        thread::spawn(move || {
            let host = RendererHost::default();
            while let Ok(request) = request_receiver.recv() {
                let result = process_request(&host, request);
                if result_sender.send(result).is_err() {
                    break;
                }
            }
        });
        Self {
            request_sender,
            receiver,
        }
    }

    pub fn start(&self, request: RuntimeInteractionRequest) {
        let _ = self.request_sender.send(request);
    }

'''
# suffix already contains try_recv + closing impl.
p.write_text(prefix + replacement + suffix)

# Insert request processing helper before the loader struct.
replace_once(
    "src/runtime_interaction.rs",
    "pub struct RuntimeInteractionLoader {",
    r'''fn process_request(
    host: &RendererHost,
    request: RuntimeInteractionRequest,
) -> RuntimeInteractionResult {
    let update = match request.kind {
        RuntimeInteractionKind::Event(event) => {
            host.dispatch_event(&request.page_url, &request.session_id, event)
        }
        RuntimeInteractionKind::Tick(elapsed) => {
            host.tick(&request.page_url, &request.session_id, elapsed)
        }
    };
    let (mode, default_prevented, result) = match update {
        Ok(update) => {
            if let Ok(url) = Url::parse(&request.page_url) {
                request
                    .storage
                    .apply_script_snapshot(&url, &update.script_report.storage);
                for cookie in &update.script_report.cookie_writes {
                    request.storage.store_set_cookie(&url, &url, cookie);
                }
            }
            (
                Some(update.mode),
                update.default_prevented,
                Ok(RuntimePageUpdate {
                    view: update.view,
                    patch: update.patch,
                    script_report: update.script_report,
                    damage: update.damage,
                    title: update.title,
                    icon_url: update.icon_url,
                    cosmetic_hidden: update.cosmetic_hidden,
                    external_stylesheets: update.external_stylesheets,
                    external_scripts: update.external_scripts,
                    reused_blocks: update.reused_blocks,
                }),
            )
        }
        Err(error) => (None, false, Err(error)),
    };
    RuntimeInteractionResult {
        request_id: request.request_id,
        tab_id: request.tab_id,
        generation: request.generation,
        mode,
        default_prevented,
        result,
    }
}

pub struct RuntimeInteractionLoader {''',
)

print("Veil 0.8.8 hot-path optimizations applied")

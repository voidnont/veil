use eframe::egui::{self, Color32, RichText, ScrollArea, Sense};
use url::Url;
use veil_engine::engine::RenderBlock;
use veil_engine::privacy::SitePrivacy;
use veil_engine::style::{ComputedStyle, JustifyContent, LayoutMode};

use crate::{PendingNavigation, VeilApp, COLLAPSED_DOCK_WIDTH, EXPANDED_DOCK_WIDTH, HOME};

const SHELL_BG: Color32 = Color32::from_rgb(10, 11, 14);
const SIDEBAR_BG: Color32 = Color32::from_rgba_premultiplied(18, 19, 24, 248);
const TOOLBAR_BG: Color32 = Color32::from_rgba_premultiplied(20, 21, 27, 250);
const PAGE_BG: Color32 = Color32::from_rgb(15, 16, 20);
const SURFACE: Color32 = Color32::from_rgba_premultiplied(10, 10, 10, 10);
const SURFACE_HOVER: Color32 = Color32::from_rgba_premultiplied(18, 18, 18, 18);
const BORDER: Color32 = Color32::from_rgba_premultiplied(22, 22, 22, 22);
const TEXT_MUTED: Color32 = Color32::from_gray(145);
const ACCENT: Color32 = Color32::from_rgb(150, 121, 234);
const ACTIVE: Color32 = Color32::from_rgba_premultiplied(20, 16, 31, 34);
const TOOLBAR_HEIGHT: f32 = 58.0;

fn sidebar_progress(app: &VeilApp, ctx: &egui::Context) -> f32 {
    ctx.animate_bool_with_time(
        egui::Id::new("veil_sidebar_animation_v2"),
        app.sidebar_expanded(ctx),
        0.16,
    )
}

fn sidebar_width(app: &VeilApp, ctx: &egui::Context) -> f32 {
    let t = sidebar_progress(app, ctx);
    COLLAPSED_DOCK_WIDTH + (EXPANDED_DOCK_WIDTH - COLLAPSED_DOCK_WIDTH) * t
}

fn workspace_name(index: usize) -> &'static str {
    match index {
        1 => "Work",
        2 => "Focus",
        _ => "Personal",
    }
}

fn workspace_glyph(index: usize) -> &'static str {
    match index {
        1 => "◇",
        2 => "✦",
        _ => "●",
    }
}

pub(crate) fn render_content(app: &mut VeilApp, ctx: &egui::Context) {
    let sidebar = sidebar_width(app, ctx);
    egui::CentralPanel::default()
        .frame(egui::Frame::default().fill(SHELL_BG))
        .show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add_space(sidebar + 12.0);
                ui.vertical(|ui| {
                    ui.add_space(TOOLBAR_HEIGHT + 10.0);
                    let width = ui.available_width().max(320.0);
                    let height = ui.available_height().max(320.0);
                    egui::Frame::default()
                        .fill(PAGE_BG)
                        .stroke(egui::Stroke::new(1.0_f32, BORDER))
                        .corner_radius(16)
                        .inner_margin(0)
                        .show(ui, |ui| {
                            ui.set_min_size(egui::vec2(width, height));
                            let primary = app.active_tab;
                            if let Some(split) = app.split_tab_index() {
                                ui.columns(2, |columns| {
                                    render_page(app, ctx, &mut columns[0], primary, true);
                                    render_page(app, ctx, &mut columns[1], split, true);
                                });
                            } else {
                                render_page(app, ctx, ui, primary, false);
                            }
                        });
                });
            });
        });
}

pub(crate) fn render_sidebar(app: &mut VeilApp, ctx: &egui::Context) {
    let t = sidebar_progress(app, ctx);
    let expanded = t > 0.48;
    let width = COLLAPSED_DOCK_WIDTH + (EXPANDED_DOCK_WIDTH - COLLAPSED_DOCK_WIDTH) * t;
    let height = (ctx.screen_rect().height() - 16.0).max(320.0);

    egui::Area::new(egui::Id::new("veil_sidebar_v2"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(8.0, 8.0))
        .show(ctx, |ui| {
            egui::Frame::default()
                .fill(SIDEBAR_BG)
                .stroke(egui::Stroke::new(1.0_f32, BORDER))
                .corner_radius(16)
                .inner_margin(7)
                .show(ui, |ui| {
                    ui.set_min_size(egui::vec2(width, height));
                    ui.set_max_width(width);

                    ui.horizontal(|ui| {
                        let logo_size = 32.0;
                        if let Some(logo_id) = app.logo.as_ref().map(|logo| logo.id()) {
                            let logo = egui::Image::new((logo_id, egui::vec2(logo_size, logo_size)))
                                .sense(Sense::click());
                            if ui.add(logo).on_hover_text("Veil home").clicked() {
                                let active = app.active_tab;
                                app.navigate_tab(active, HOME.into(), true);
                            }
                        } else if ui
                            .add_sized([logo_size, logo_size], egui::Button::new("V").frame(false))
                            .clicked()
                        {
                            let active = app.active_tab;
                            app.navigate_tab(active, HOME.into(), true);
                        }

                        if expanded {
                            ui.add_space(5.0);
                            ui.vertical(|ui| {
                                ui.label(RichText::new("Veil").strong().size(14.0));
                                ui.label(
                                    RichText::new(workspace_name(app.active_space))
                                        .small()
                                        .color(TEXT_MUTED),
                                );
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let pin = if app.sidebar_pinned { "◆" } else { "◇" };
                                if ui
                                    .add_sized([28.0, 28.0], egui::Button::new(pin).frame(false))
                                    .on_hover_text("Keep sidebar open")
                                    .clicked()
                                {
                                    app.sidebar_pinned = !app.sidebar_pinned;
                                }
                            });
                        }
                    });

                    ui.add_space(10.0);
                    let mut requested_space = None;
                    if expanded {
                        ui.horizontal(|ui| {
                            for index in 0..3 {
                                let active = app.active_space == index;
                                let response = egui::Frame::default()
                                    .fill(if active { ACTIVE } else { Color32::TRANSPARENT })
                                    .corner_radius(9)
                                    .inner_margin(1)
                                    .show(ui, |ui| {
                                        ui.add_sized(
                                            [66.0, 28.0],
                                            egui::Button::new(
                                                RichText::new(format!(
                                                    "{} {}",
                                                    workspace_glyph(index),
                                                    workspace_name(index)
                                                ))
                                                .size(11.5)
                                                .color(if active {
                                                    Color32::WHITE
                                                } else {
                                                    Color32::from_gray(180)
                                                }),
                                            )
                                            .frame(false),
                                        )
                                    })
                                    .inner;
                                if response.clicked() {
                                    requested_space = Some(index);
                                }
                            }
                        });
                    } else {
                        ui.vertical_centered(|ui| {
                            for index in 0..3 {
                                let active = app.active_space == index;
                                if ui
                                    .add_sized(
                                        [34.0, 28.0],
                                        egui::Button::new(
                                            RichText::new(workspace_glyph(index)).color(if active {
                                                ACCENT
                                            } else {
                                                Color32::from_gray(155)
                                            }),
                                        )
                                        .frame(false),
                                    )
                                    .on_hover_text(workspace_name(index))
                                    .clicked()
                                {
                                    requested_space = Some(index);
                                }
                            }
                        });
                    }
                    if let Some(space) = requested_space {
                        app.switch_space(space);
                    }

                    ui.add_space(8.0);
                    if expanded {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Tabs").small().color(TEXT_MUTED));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui
                                    .add_sized([28.0, 28.0], egui::Button::new("+").frame(false))
                                    .on_hover_text("New tab · Ctrl+T")
                                    .clicked()
                                {
                                    app.new_tab();
                                }
                            });
                        });
                    }

                    let reserved = if expanded { 190.0 } else { 165.0 };
                    let tabs_height = (height - reserved).max(150.0);
                    let mut choose_tab = None;
                    let mut close_tab = None;
                    ScrollArea::vertical()
                        .id_salt("veil_tabs_v2")
                        .max_height(tabs_height)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for index in 0..app.tabs.len() {
                                if app.tabs[index].space != app.active_space {
                                    continue;
                                }
                                let active = index == app.active_tab;
                                let loading = app.tabs[index].loading;
                                let title = crate::truncate_title(&app.tabs[index].page.title, 27);
                                egui::Frame::default()
                                    .fill(if active {
                                        SURFACE_HOVER
                                    } else {
                                        Color32::TRANSPARENT
                                    })
                                    .stroke(if active {
                                        egui::Stroke::new(1.0_f32, BORDER)
                                    } else {
                                        egui::Stroke::NONE
                                    })
                                    .corner_radius(11)
                                    .inner_margin(egui::Margin::symmetric(4, 2))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            if active {
                                                let (rect, _) = ui.allocate_exact_size(
                                                    egui::vec2(2.0, 22.0),
                                                    Sense::hover(),
                                                );
                                                ui.painter().rect_filled(rect, 2.0, ACCENT);
                                            }
                                            let icon = app.render_tab_icon(ctx, ui, index);
                                            if icon.clicked() {
                                                choose_tab = Some(index);
                                            }
                                            if expanded {
                                                let label = if loading {
                                                    format!("◌  {title}")
                                                } else {
                                                    title
                                                };
                                                if ui
                                                    .add_sized(
                                                        [(width - 100.0).max(92.0), 32.0],
                                                        egui::Button::new(
                                                            RichText::new(label)
                                                                .size(12.5)
                                                                .color(if active {
                                                                    Color32::WHITE
                                                                } else {
                                                                    Color32::from_gray(195)
                                                                }),
                                                        )
                                                        .frame(false),
                                                    )
                                                    .clicked()
                                                {
                                                    choose_tab = Some(index);
                                                }
                                                if ui
                                                    .add_sized(
                                                        [24.0, 24.0],
                                                        egui::Button::new("×").frame(false),
                                                    )
                                                    .on_hover_text("Close tab · Ctrl+W")
                                                    .clicked()
                                                {
                                                    close_tab = Some(index);
                                                }
                                            }
                                        });
                                    });
                                ui.add_space(3.0);
                            }
                        });
                    if let Some(index) = choose_tab {
                        app.activate_tab(index);
                    }
                    if let Some(index) = close_tab {
                        app.close_tab(index);
                    }

                    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                        if expanded {
                            let status = crate::truncate_title(&app.tabs[app.active_tab].status, 48);
                            ui.label(
                                RichText::new(status)
                                    .small()
                                    .color(Color32::from_gray(112)),
                            );
                            ui.add_space(4.0);
                        }
                        ui.horizontal(|ui| {
                            if ui
                                .add_sized([34.0, 32.0], egui::Button::new("+").frame(false))
                                .on_hover_text("New tab · Ctrl+T")
                                .clicked()
                            {
                                app.new_tab();
                            }
                            if ui
                                .add_sized([34.0, 32.0], egui::Button::new("◫").frame(false))
                                .on_hover_text("Split view · Ctrl+Shift+S")
                                .clicked()
                            {
                                app.toggle_split();
                            }
                            if ui
                                .add_sized([34.0, 32.0], egui::Button::new("◈").frame(false))
                                .on_hover_text("Privacy Shield")
                                .clicked()
                            {
                                app.show_privacy = !app.show_privacy;
                            }
                        });
                    });
                });
        });
}

pub(crate) fn render_address_pill(app: &mut VeilApp, ctx: &egui::Context) {
    let screen = ctx.screen_rect();
    let sidebar = sidebar_width(app, ctx);
    let left = sidebar + 28.0;
    let available = (screen.width() - left - 20.0).max(360.0);
    let width = available.min(980.0);
    let x = left + (available - width) * 0.5;
    let address_id = egui::Id::new("veil_address_bar");
    let focused = ctx.memory(|memory| memory.has_focus(address_id));

    egui::Area::new(egui::Id::new("veil_toolbar_v2"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(x, 14.0))
        .show(ctx, |ui| {
            egui::Frame::default()
                .fill(TOOLBAR_BG)
                .stroke(egui::Stroke::new(
                    1.0_f32,
                    if focused {
                        Color32::from_rgba_unmultiplied(176, 155, 240, 80)
                    } else {
                        BORDER
                    },
                ))
                .corner_radius(14)
                .inner_margin(4)
                .show(ui, |ui| {
                    ui.set_min_width(width);
                    let mut navigation = None;
                    let can_back = !app.tabs[app.active_tab].back.is_empty();
                    let can_forward = !app.tabs[app.active_tab].forward.is_empty();
                    let loading = app.tabs[app.active_tab].loading;
                    let privacy = app.current_privacy();
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(can_back, egui::Button::new("←").frame(false))
                            .on_hover_text("Back · Alt+Left")
                            .clicked()
                        {
                            app.go_back();
                        }
                        if ui
                            .add_enabled(can_forward, egui::Button::new("→").frame(false))
                            .on_hover_text("Forward · Alt+Right")
                            .clicked()
                        {
                            app.go_forward();
                        }
                        if loading {
                            if ui
                                .add_sized([28.0, 28.0], egui::Button::new("×").frame(false))
                                .on_hover_text("Stop loading")
                                .clicked()
                            {
                                app.stop_loading();
                            }
                        } else if ui
                            .add_sized([28.0, 28.0], egui::Button::new("↻").frame(false))
                            .on_hover_text("Reload · Ctrl+R")
                            .clicked()
                        {
                            app.reload();
                        }

                        let secure = crate::security_icon(&app.tabs[app.active_tab].address);
                        ui.label(RichText::new(secure).color(Color32::from_gray(150)));
                        let text_width = (ui.available_width() - 96.0).max(180.0);
                        let response = {
                            let address = &mut app.tabs[app.active_tab].address;
                            ui.add_sized(
                                [text_width, 38.0],
                                egui::TextEdit::singleline(address)
                                    .id(address_id)
                                    .hint_text("Search or enter address")
                                    .frame(false),
                            )
                        };
                        if response.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        {
                            navigation = Some(app.tabs[app.active_tab].address.clone());
                        }

                        let shield = if privacy.shields { "◈" } else { "◇" };
                        if ui
                            .add_sized(
                                [30.0, 30.0],
                                egui::Button::new(RichText::new(shield).color(ACCENT)).frame(false),
                            )
                            .on_hover_text("Privacy Shield")
                            .clicked()
                        {
                            app.show_privacy = !app.show_privacy;
                        }
                        if ui
                            .add_sized([30.0, 30.0], egui::Button::new("+").frame(false))
                            .on_hover_text("New tab")
                            .clicked()
                        {
                            app.new_tab();
                        }
                    });
                    if let Some(target) = navigation {
                        app.navigate_active(target, true);
                    }
                });
        });
}

fn render_page(
    app: &mut VeilApp,
    ctx: &egui::Context,
    ui: &mut egui::Ui,
    tab_index: usize,
    split: bool,
) {
    if tab_index >= app.tabs.len() {
        return;
    }
    if app.tabs[tab_index].page.url == HOME {
        app.render_home(ui, tab_index, split);
        return;
    }

    let page = app.tabs[tab_index].page.clone();
    let base_url = Url::parse(&page.url).ok();
    let privacy = base_url
        .as_ref()
        .map(|url| app.profiles.for_url(url))
        .unwrap_or_default();
    let mut navigation: Option<PendingNavigation> = None;

    if split {
        let active = tab_index == app.active_tab;
        egui::Frame::default()
            .fill(if active { ACTIVE } else { SURFACE })
            .corner_radius(10)
            .inner_margin(egui::Margin::symmetric(8, 4))
            .show(ui, |ui| {
                let title = crate::truncate_title(&page.title, 34);
                if ui
                    .add(
                        egui::Button::new(if active {
                            format!("● {title}")
                        } else {
                            title
                        })
                        .frame(false),
                    )
                    .on_hover_text("Make this the active pane")
                    .clicked()
                {
                    app.activate_tab(tab_index);
                }
            });
    }

    ScrollArea::vertical()
        .id_salt(("veil_page_v2", app.tabs[tab_index].id))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(if split { 8.0 } else { 10.0 });
            let viewport_width = ui.available_width().max(320.0);
            ui.horizontal(|ui| {
                ui.add_space(if split { 8.0 } else { 12.0 });
                ui.vertical(|ui| {
                    ui.set_max_width(
                        (viewport_width - if split { 16.0 } else { 24.0 }).max(280.0),
                    );
                    for block in &page.blocks {
                        render_visual_block(
                            app,
                            ctx,
                            ui,
                            tab_index,
                            block,
                            base_url.as_ref(),
                            privacy,
                            &mut navigation,
                        );
                    }
                    ui.add_space(72.0);
                });
                ui.add_space(if split { 8.0 } else { 12.0 });
            });
        });

    if let Some(target) = navigation {
        app.navigate_tab_with_method(tab_index, target.url, true, target.method);
    }
}

fn render_visual_block(
    app: &mut VeilApp,
    ctx: &egui::Context,
    ui: &mut egui::Ui,
    tab_index: usize,
    block: &RenderBlock,
    base_url: Option<&Url>,
    privacy: SitePrivacy,
    navigation: &mut Option<PendingNavigation>,
) {
    match block {
        RenderBlock::Heading { runs, style, .. } | RenderBlock::Paragraph { runs, style } => {
            visual_box(ui, style, |ui| {
                crate::render_runs(
                    ui,
                    runs,
                    base_url,
                    navigation,
                    style.text_align,
                    &app.web_font_registry,
                );
            });
        }
        RenderBlock::Container { children, style } => {
            visual_box(ui, style, |ui| {
                let mut ordered: Vec<&RenderBlock> = children.iter().collect();
                ordered.sort_by_key(|child| crate::block_order(child));
                match style.layout {
                    LayoutMode::Block | LayoutMode::FlexColumn => {
                        for (index, child) in ordered.iter().enumerate() {
                            render_visual_block(
                                app,
                                ctx,
                                ui,
                                tab_index,
                                child,
                                base_url,
                                privacy,
                                navigation,
                            );
                            if index + 1 < ordered.len() && style.gap > 0.0 {
                                ui.add_space(style.gap);
                            }
                        }
                    }
                    LayoutMode::FlexRow => {
                        let mut paint = |ui: &mut egui::Ui| {
                            for (index, child) in ordered.iter().enumerate() {
                                render_visual_block(
                                    app,
                                    ctx,
                                    ui,
                                    tab_index,
                                    child,
                                    base_url,
                                    privacy,
                                    navigation,
                                );
                                if index + 1 < ordered.len() && style.gap > 0.0 {
                                    ui.add_space(style.gap);
                                }
                            }
                        };
                        if style.flex_wrap {
                            ui.horizontal_wrapped(|ui| paint(ui));
                        } else if style.justify_content == JustifyContent::Center {
                            ui.horizontal_centered(|ui| paint(ui));
                        } else {
                            ui.horizontal(|ui| paint(ui));
                        }
                    }
                    LayoutMode::Grid => {
                        let cap = ((ui.available_width() / 180.0).floor() as usize).max(1);
                        let columns = style.grid_columns.max(1).min(cap);
                        egui::Grid::new(("veil_visual_grid", children.as_ptr() as usize))
                            .num_columns(columns)
                            .spacing([style.gap.max(8.0), style.gap.max(8.0)])
                            .show(ui, |ui| {
                                for (index, child) in children.iter().enumerate() {
                                    render_visual_block(
                                        app,
                                        ctx,
                                        ui,
                                        tab_index,
                                        child,
                                        base_url,
                                        privacy,
                                        navigation,
                                    );
                                    if (index + 1) % columns == 0 {
                                        ui.end_row();
                                    }
                                }
                            });
                    }
                }
            });
        }
        RenderBlock::Image {
            src,
            alt,
            width,
            height,
            style,
        } => {
            visual_box(ui, style, |ui| {
                app.render_image(ctx, ui, base_url, src, alt, *width, *height, privacy);
            });
        }
        RenderBlock::Rule { style } => {
            visual_box(ui, style, |ui| {
                ui.separator();
            });
        }
        RenderBlock::Code { text, style } => {
            visual_box(ui, style, |ui| {
                egui::Frame::default()
                    .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 8))
                    .corner_radius(8)
                    .inner_margin(8)
                    .show(ui, |ui| {
                        ui.label(RichText::new(text).monospace().size(style.font_size));
                    });
            });
        }
        _ => {
            app.render_block(
                ctx,
                ui,
                tab_index,
                block,
                base_url,
                privacy,
                navigation,
            );
        }
    }
}

fn visual_box(
    ui: &mut egui::Ui,
    style: &ComputedStyle,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    ui.add_space(style.margin.top.max(0.0));
    let avail = ui.available_width().max(1.0);
    let side_margin = style.margin.left.max(0.0) + style.margin.right.max(0.0);
    let mut width = style.width.unwrap_or((avail - side_margin).max(1.0));
    if let Some(min) = style.min_width {
        width = width.max(min);
    }
    if let Some(max) = style.max_width {
        width = width.min(max);
    }
    width = width.min((avail - side_margin).max(1.0)).max(1.0);

    ui.horizontal(|ui| {
        ui.add_space(style.margin.left.max(0.0));
        let mut frame = egui::Frame::default()
            .corner_radius(style.border_radius.clamp(0.0, 32.0) as u8);
        if let Some(background) = style.background {
            frame = frame.fill(crate::to_color32(background));
        }
        if style.border_width > 0.0 {
            frame = frame.stroke(egui::Stroke::new(
                style.border_width,
                Color32::from_rgba_unmultiplied(255, 255, 255, 46),
            ));
        }
        frame.show(ui, |ui| {
            ui.set_min_width(width);
            ui.set_max_width(width);
            if let Some(min_height) = style.min_height {
                ui.set_min_height(min_height.max(1.0));
            }
            if let Some(height) = style.height {
                ui.set_min_height(height.max(1.0));
            }
            if let Some(max_height) = style.max_height {
                ui.set_max_height(max_height.max(1.0));
            }
            ui.add_space(style.padding.top.max(0.0));
            ui.horizontal(|ui| {
                ui.add_space(style.padding.left.max(0.0));
                ui.vertical(|ui| {
                    let inner = (width
                        - style.padding.left.max(0.0)
                        - style.padding.right.max(0.0))
                    .max(1.0);
                    ui.set_max_width(inner);
                    add_contents(ui);
                });
                ui.add_space(style.padding.right.max(0.0));
            });
            ui.add_space(style.padding.bottom.max(0.0));
        });
        ui.add_space(style.margin.right.max(0.0));
    });
    ui.add_space(style.margin.bottom.max(0.0));
}

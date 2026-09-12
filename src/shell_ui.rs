use std::time::Duration;

use eframe::egui::{self, Color32, RichText, ScrollArea, Sense};

use crate::{VeilApp, COLLAPSED_DOCK_WIDTH, EXPANDED_DOCK_WIDTH};

const SHELL_BG: Color32 = Color32::from_rgb(12, 12, 16);
const PANEL_BG: Color32 = Color32::from_rgba_unmultiplied(22, 22, 28, 246);
const PAGE_BG: Color32 = Color32::from_rgb(18, 18, 22);
const HOVER_BG: Color32 = Color32::from_rgba_unmultiplied(255, 255, 255, 14);
const ACTIVE_BG: Color32 = Color32::from_rgba_unmultiplied(255, 255, 255, 22);
const ACCENT_BG: Color32 = Color32::from_rgba_unmultiplied(132, 102, 222, 34);
const ACCENT: Color32 = Color32::from_rgb(151, 123, 232);
const BORDER: Color32 = Color32::from_rgba_unmultiplied(255, 255, 255, 24);

fn sidebar_progress(app: &VeilApp, ctx: &egui::Context) -> f32 {
    ctx.animate_bool_with_time(
        egui::Id::new("veil_shell_sidebar_animation"),
        app.sidebar_expanded(ctx),
        0.18,
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
    let top_inset = 8.0;

    egui::CentralPanel::default()
        .frame(egui::Frame::default().fill(SHELL_BG))
        .show(ctx, |ui| {
            ui.add_space(top_inset);
            ui.horizontal(|ui| {
                ui.add_space(sidebar + 12.0);
                let width = (ui.available_width() - 8.0).max(320.0);
                let height = (ui.available_height() - 8.0).max(320.0);

                egui::Frame::default()
                    .fill(PAGE_BG)
                    .stroke(egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 16)))
                    .corner_radius(14)
                    .inner_margin(0)
                    .show(ui, |ui| {
                        ui.set_min_size(egui::vec2(width, height));
                        let primary_index = app.active_tab;
                        if let Some(split_index) = app.split_tab_index() {
                            ui.columns(2, |columns| {
                                app.render_page_for_tab(ctx, &mut columns[0], primary_index, true);
                                app.render_page_for_tab(ctx, &mut columns[1], split_index, true);
                            });
                        } else {
                            app.render_page_for_tab(ctx, ui, primary_index, false);
                        }
                    });
            });
        });
}

pub(crate) fn render_sidebar(app: &mut VeilApp, ctx: &egui::Context) {
    let t = sidebar_progress(app, ctx);
    let expanded = t > 0.55;
    let width = COLLAPSED_DOCK_WIDTH + (EXPANDED_DOCK_WIDTH - COLLAPSED_DOCK_WIDTH) * t;
    let height = (ctx.screen_rect().height() - 16.0).max(320.0);

    egui::Area::new(egui::Id::new("veil_shell_sidebar"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(8.0, 8.0))
        .show(ctx, |ui| {
            egui::Frame::default()
                .fill(PANEL_BG)
                .stroke(egui::Stroke::new(1.0, BORDER))
                .corner_radius(14)
                .inner_margin(6)
                .show(ui, |ui| {
                    ui.set_min_size(egui::vec2(width, height));
                    ui.set_max_width(width);

                    ui.horizontal(|ui| {
                        let logo_size = 28.0;
                        if let Some(logo_id) = app.logo.as_ref().map(|logo| logo.id()) {
                            if ui
                                .add(egui::Image::new((logo_id, egui::vec2(logo_size, logo_size))).sense(Sense::click()))
                                .on_hover_text("Veil")
                                .clicked()
                            {
                                let active = app.active_tab;
                                app.navigate_tab(active, crate::HOME.into(), true);
                            }
                        } else if ui.add_sized([logo_size, logo_size], egui::Button::new("V").frame(false)).clicked() {
                            let active = app.active_tab;
                            app.navigate_tab(active, crate::HOME.into(), true);
                        }

                        if expanded {
                            ui.add_space(3.0);
                            ui.label(RichText::new("Veil").strong().size(14.0));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let pin = if app.sidebar_pinned { "◆" } else { "◇" };
                                if ui.add_sized([28.0, 28.0], egui::Button::new(pin).frame(false))
                                    .on_hover_text("Keep sidebar open")
                                    .clicked()
                                {
                                    app.sidebar_pinned = !app.sidebar_pinned;
                                }
                            });
                        }
                    });

                    ui.add_space(8.0);

                    let mut requested_space = None;
                    ui.horizontal(|ui| {
                        if expanded {
                            ui.add_space(2.0);
                        }
                        for index in 0..3 {
                            let active = app.active_space == index;
                            let frame = egui::Frame::default()
                                .fill(if active { ACCENT_BG } else { Color32::TRANSPARENT })
                                .corner_radius(9)
                                .inner_margin(1);
                            frame.show(ui, |ui| {
                                let response = ui
                                    .add_sized(
                                        [30.0, 28.0],
                                        egui::Button::new(
                                            RichText::new(workspace_glyph(index))
                                                .color(if active { ACCENT } else { Color32::from_gray(165) }),
                                        )
                                        .frame(false),
                                    )
                                    .on_hover_text(workspace_name(index));
                                if response.clicked() {
                                    requested_space = Some(index);
                                }
                            });
                        }
                    });
                    if let Some(space) = requested_space {
                        app.switch_space(space);
                    }

                    if expanded {
                        ui.add_space(6.0);
                        egui::Frame::default()
                            .fill(Color32::TRANSPARENT)
                            .corner_radius(10)
                            .inner_margin(egui::Margin::symmetric(7, 4))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(workspace_glyph(app.active_space))
                                            .color(ACCENT)
                                            .size(13.0),
                                    );
                                    ui.label(RichText::new(workspace_name(app.active_space)).strong().size(13.0));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.add_sized([24.0, 24.0], egui::Button::new("+").frame(false))
                                            .on_hover_text("New tab")
                                            .clicked()
                                        {
                                            app.new_tab();
                                        }
                                    });
                                });
                            });
                    }

                    ui.add_space(4.0);
                    let available_for_tabs = (height - if expanded { 220.0 } else { 170.0 }).max(150.0);
                    let mut choose_tab = None;
                    let mut close_tab = None;

                    ScrollArea::vertical()
                        .id_salt("veil_shell_tabs")
                        .max_height(available_for_tabs)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for index in 0..app.tabs.len() {
                                if app.tabs[index].space != app.active_space {
                                    continue;
                                }
                                let title = crate::truncate_title(&app.tabs[index].page.title, 28);
                                let loading = app.tabs[index].loading;
                                let active = index == app.active_tab;

                                let row_fill = if active { ACTIVE_BG } else { Color32::TRANSPARENT };
                                let row_stroke = if active {
                                    egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 18))
                                } else {
                                    egui::Stroke::NONE
                                };

                                egui::Frame::default()
                                    .fill(row_fill)
                                    .stroke(row_stroke)
                                    .corner_radius(12)
                                    .inner_margin(egui::Margin::symmetric(4, 2))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            if active {
                                                let (rect, _) = ui.allocate_exact_size(egui::vec2(2.0, 22.0), Sense::hover());
                                                ui.painter().rect_filled(rect, 2.0, ACCENT);
                                            } else if expanded {
                                                ui.add_space(2.0);
                                            }

                                            let response = app.render_tab_icon(ctx, ui, index);
                                            if response.clicked() {
                                                choose_tab = Some(index);
                                            }

                                            if expanded {
                                                let label = if loading { format!("◌  {title}") } else { title };
                                                let response = ui.add_sized(
                                                    [(width - 98.0).max(100.0), 32.0],
                                                    egui::Button::new(
                                                        RichText::new(label)
                                                            .size(13.0)
                                                            .color(if active { Color32::WHITE } else { Color32::from_gray(205) }),
                                                    )
                                                    .frame(false),
                                                );
                                                if response.clicked() {
                                                    choose_tab = Some(index);
                                                }
                                                if ui
                                                    .add_sized([24.0, 24.0], egui::Button::new("×").frame(false))
                                                    .on_hover_text("Close tab")
                                                    .clicked()
                                                {
                                                    close_tab = Some(index);
                                                }
                                            }
                                        });
                                    });
                                ui.add_space(2.0);
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
                            ui.label(
                                RichText::new(&app.tabs[app.active_tab].status)
                                    .small()
                                    .color(Color32::from_gray(125)),
                            );
                            ui.add_space(4.0);
                        }

                        ui.horizontal(|ui| {
                            let new_tab = ui.add_sized([34.0, 32.0], egui::Button::new("+").frame(false));
                            if new_tab.on_hover_text("New tab · Ctrl+T").clicked() {
                                app.new_tab();
                            }
                            let split = ui.add_sized([34.0, 32.0], egui::Button::new("◫").frame(false));
                            if split.on_hover_text("Split view · Ctrl+Shift+S").clicked() {
                                app.toggle_split();
                            }
                            let privacy = ui.add_sized([34.0, 32.0], egui::Button::new("◈").frame(false));
                            if privacy.on_hover_text("Privacy Shield").clicked() {
                                app.show_privacy = !app.show_privacy;
                            }
                            if expanded {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(RichText::new("Private").small().color(Color32::from_gray(130)));
                                });
                            }
                        });
                    });
                });
        });
}

pub(crate) fn render_address_pill(app: &mut VeilApp, ctx: &egui::Context) {
    let screen = ctx.screen_rect();
    let sidebar = sidebar_width(app, ctx);
    let address_id = egui::Id::new("veil_address_bar");
    let focused = ctx.memory(|memory| memory.has_focus(address_id));
    let focus_t = ctx.animate_bool_with_time(egui::Id::new("veil_shell_urlbar_animation"), focused, 0.18);

    let compact_width = ((screen.width() - sidebar - 160.0) * 0.52).clamp(420.0, 680.0);
    let expanded_width = (screen.width() - sidebar - 96.0).clamp(620.0, 980.0);
    let width = compact_width + (expanded_width - compact_width) * focus_t;
    let content_left = sidebar + 20.0;
    let center_x = content_left + (screen.width() - content_left) * 0.5;
    let x = center_x - width * 0.5;
    let y = 14.0 + 4.0 * focus_t;

    egui::Area::new(egui::Id::new("veil_shell_urlbar"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(x.max(content_left + 8.0), y))
        .show(ctx, |ui| {
            egui::Frame::default()
                .fill(Color32::from_rgba_unmultiplied(23, 23, 30, 248))
                .stroke(egui::Stroke::new(
                    1.0,
                    if focused {
                        Color32::from_rgba_unmultiplied(190, 175, 240, 64)
                    } else {
                        BORDER
                    },
                ))
                .corner_radius(12)
                .inner_margin(4)
                .show(ui, |ui| {
                    ui.set_min_width(width);
                    let mut navigation: Option<String> = None;
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
                            if ui.add_sized([28.0, 28.0], egui::Button::new("×").frame(false))
                                .on_hover_text("Stop loading")
                                .clicked()
                            {
                                app.stop_loading();
                            }
                        } else if ui.add_sized([28.0, 28.0], egui::Button::new("↻").frame(false))
                            .on_hover_text("Reload · Ctrl+R")
                            .clicked()
                        {
                            app.reload();
                        }

                        ui.label(
                            RichText::new(crate::security_icon(&app.tabs[app.active_tab].address))
                                .color(Color32::from_gray(170)),
                        );

                        let text_width = (ui.available_width() - 58.0).max(180.0);
                        let response = {
                            let address = &mut app.tabs[app.active_tab].address;
                            ui.add_sized(
                                [text_width, 36.0 + 4.0 * focus_t],
                                egui::TextEdit::singleline(address)
                                    .id(address_id)
                                    .hint_text("Search or enter address")
                                    .font(egui::TextStyle::Body)
                                    .frame(false),
                            )
                        };
                        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            navigation = Some(app.tabs[app.active_tab].address.clone());
                        }

                        let shield = if privacy.shields { "◈" } else { "◇" };
                        if ui.add_sized([30.0, 30.0], egui::Button::new(shield).frame(false))
                            .on_hover_text("Privacy Shield")
                            .clicked()
                        {
                            app.show_privacy = !app.show_privacy;
                        }
                    });

                    if focused {
                        ui.label(
                            RichText::new("Search the web or enter a site address")
                                .small()
                                .color(Color32::from_gray(120)),
                        );
                    }

                    if let Some(target) = navigation {
                        app.navigate_active(target, true);
                    }
                });
        });

    if focused || app.tabs.iter().any(|tab| tab.loading) {
        ctx.request_repaint_after(Duration::from_millis(16));
    }
}

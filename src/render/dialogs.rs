// 模态对话框集合：导出 / 缩放 / 清空确认。
// 纯视图：只读 Model，直接修改 UiState 瞬态字段（开合、页签、防抖标记），
// 用户意图一律以 Action 返回，由 app 层统一分发。

use super::toolbar::fmt_bytes;
use crate::model::{Action, Edit, GifQuality, Model, ResizeFilter};
use crate::render::i18n::{tr, tra};
use crate::render::{theme, UiState};
use eframe::egui::{self, Context};
use std::time::{Duration, Instant};

/// 估算防抖窗口：参数停止变化 ≥300ms 才发起新估算（共识 Q8-c）
const ESTIMATE_DEBOUNCE: Duration = Duration::from_millis(300);

pub fn render_dialogs(ctx: &Context, model: &Model, ui_state: &mut UiState) -> Vec<Action> {
    let mut actions = Vec::new();
    if ui_state.export_open {
        actions.extend(export_dialog(ctx, model, ui_state));
    }
    if ui_state.resize_open {
        actions.extend(resize_dialog(ctx, ui_state));
    }
    if ui_state.confirm_clear {
        actions.extend(confirm_clear_dialog(ctx, ui_state));
    }
    actions
}

// ─── 导出对话框 ─────────────────────────────────────────────────

fn export_dialog(ctx: &Context, model: &Model, ui_state: &mut UiState) -> Vec<Action> {
    let mut actions = Vec::new();

    // Window::open 借用独立局部变量，避免与闭包内的 ui_state 借用冲突
    let mut open = ui_state.export_open;
    let mut closed_by_button = false;

    egui::Window::new(tr("export.title"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            // 页签：GIF / WebP动画 / PNG序列
            ui.horizontal(|ui| {
                ui.selectable_value(&mut ui_state.export_tab, 0, tr("export.tab_gif"));
                ui.selectable_value(&mut ui_state.export_tab, 1, tr("export.tab_webp"));
                ui.selectable_value(&mut ui_state.export_tab, 2, tr("export.tab_png"));
            });
            ui.separator();

            // 各页签参数
            match ui_state.export_tab {
                0 => {
                    ui.radio_value(
                        &mut ui_state.gif_quality,
                        GifQuality::High,
                        tr("export.gif_quality_high"),
                    );
                    ui.radio_value(
                        &mut ui_state.gif_quality,
                        GifQuality::Balanced,
                        tr("export.gif_quality_balanced"),
                    );
                    ui.radio_value(
                        &mut ui_state.gif_quality,
                        GifQuality::Fast,
                        tr("export.gif_quality_fast"),
                    );
                }
                1 => {
                    ui.horizontal(|ui| {
                        ui.label(tr("export.webp_quality"));
                        // 无损模式下质量滑条无意义，置灰
                        ui.add_enabled(
                            !ui_state.webp_lossless,
                            egui::Slider::new(&mut ui_state.webp_quality, 0.0..=100.0),
                        );
                    });
                    ui.checkbox(&mut ui_state.webp_lossless, tr("export.webp_lossless"));
                }
                _ => {
                    ui.label(tr("export.png_hint"));
                }
            }
            ui.separator();

            // ── 体积估算：真防抖（参数静默 ≥300ms 才发送）+ 单飞重估 ──
            // 审查修复：原先以「上次发送时刻」为基准 → 拖动滑条期间每
            // 300ms 泛滥触发试编码线程；现以「最近变更时刻」为基准。
            let current = ui_state.current_export_format();
            let differs_from_sent = ui_state
                .estimate_sent
                .as_ref()
                .map(|(f, _)| *f != current)
                .unwrap_or(true);
            if differs_from_sent {
                if ui_state.estimate_changed.is_none() {
                    ui_state.estimate_changed = Some(Instant::now());
                }
            } else {
                ui_state.estimate_changed = None;
            }
            // 帧/文档变化后估算被模型作废（result=None）：参数未变也要重估
            let needs_refresh =
                !differs_from_sent && model.estimate.result.is_none() && !model.estimate.in_flight;
            let debounce_due = match ui_state.estimate_changed {
                None => true, // 无未决变更（首次打开或 needs_refresh）可立即
                Some(changed) => changed.elapsed() >= ESTIMATE_DEBOUNCE,
            };
            if (differs_from_sent || needs_refresh) && debounce_due && !model.estimate.in_flight {
                ui_state.estimate_gen += 1;
                actions.push(Action::EstimateRequested {
                    generation: ui_state.estimate_gen,
                    format: current.clone(),
                });
                ui_state.estimate_sent = Some((current.clone(), Instant::now()));
                ui_state.estimate_changed = None;
            } else if differs_from_sent && !debounce_due {
                // 防抖等待期：静默计时到点后无输入也要醒来发送
                if let Some(changed) = ui_state.estimate_changed {
                    let remaining = ESTIMATE_DEBOUNCE
                        .saturating_sub(changed.elapsed())
                        .max(Duration::from_millis(1));
                    ctx.request_repaint_after(remaining);
                }
            }

            // 估算结果行（先跑防抖再取显示，保证本帧状态一致）
            let params_changed = ui_state
                .estimate_sent
                .as_ref()
                .is_some_and(|(f, _)| *f != current);
            let line = if params_changed {
                tr("export.estimate_modified")
            } else {
                match &model.estimate.result {
                    None => tr("export.estimate_pending"),
                    Some(Ok(bytes)) => {
                        tra("export.estimate_size", &[("size", fmt_bytes(*bytes))])
                    }
                    Some(Err(_)) => tr("export.estimate_failed"),
                }
            };
            ui.label(line);

            // ── 按钮 ──
            ui.separator();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::accent_button(ui, tr("export.start")).clicked() {
                    actions.push(Action::StartExport(ui_state.current_export_format()));
                    closed_by_button = true;
                }
                if ui.button(tr("export.cancel")).clicked() {
                    closed_by_button = true;
                }
            });
        });

    // 标题栏 X（open=false）或按钮（closed_by_button）任一关闭即同步回 UiState
    ui_state.export_open = open && !closed_by_button;

    // 关窗（按钮或标题栏 X）后清掉防抖标记：下次打开立即重新估算
    if !ui_state.export_open {
        ui_state.estimate_sent = None;
    }

    actions
}

// ─── 缩放对话框 ─────────────────────────────────────────────────

fn resize_dialog(ctx: &Context, ui_state: &mut UiState) -> Vec<Action> {
    let mut actions = Vec::new();

    // Window::open 借用独立局部变量，避免与闭包内的 ui_state 借用冲突
    let mut open = ui_state.resize_open;
    let mut closed_by_button = false;

    egui::Window::new(tr("resize.title"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            // 锁比例基准：编辑发生前的当前宽高。对话框打开时已被
            // open_resize 初始化为画布尺寸，锁定编辑下比例保持不变。
            let old_w = ui_state.resize_w;
            let old_h = ui_state.resize_h;

            ui.horizontal(|ui| {
                ui.label(tr("resize.width"));
                let w_resp = ui.add(egui::DragValue::new(&mut ui_state.resize_w).range(1..=10000));
                ui.label(tr("resize.height"));
                let h_resp = ui.add(egui::DragValue::new(&mut ui_state.resize_h).range(1..=10000));

                if ui_state.resize_lock_aspect && w_resp.changed() && old_w > 0 {
                    let new_h =
                        (ui_state.resize_w as f64 * old_h as f64 / old_w as f64).round() as u32;
                    ui_state.resize_h = new_h.clamp(1, 10000);
                } else if ui_state.resize_lock_aspect && h_resp.changed() && old_h > 0 {
                    let new_w =
                        (ui_state.resize_h as f64 * old_w as f64 / old_h as f64).round() as u32;
                    ui_state.resize_w = new_w.clamp(1, 10000);
                }
            });

            ui.horizontal(|ui| {
                ui.label(tr("resize.filter"));
                egui::ComboBox::from_id_source("resize_filter")
                    .selected_text(filter_label(ui_state.resize_filter))
                    .show_ui(ui, |ui| {
                        for f in [
                            ResizeFilter::CatmullRom,
                            ResizeFilter::Nearest,
                            ResizeFilter::Triangle,
                            ResizeFilter::Lanczos3,
                        ] {
                            ui.selectable_value(&mut ui_state.resize_filter, f, filter_label(f));
                        }
                    });
            });
            ui.checkbox(&mut ui_state.resize_lock_aspect, tr("resize.lock_aspect"));

            // ── 按钮 ──
            ui.separator();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::accent_button(ui, tr("resize.apply")).clicked() {
                    actions.push(Action::Edit(Edit::Resize {
                        width: ui_state.resize_w,
                        height: ui_state.resize_h,
                        filter: ui_state.resize_filter,
                    }));
                    closed_by_button = true;
                }
                if ui.button(tr("resize.cancel")).clicked() {
                    closed_by_button = true;
                }
            });
        });

    ui_state.resize_open = open && !closed_by_button;

    actions
}

/// 滤镜人话名
fn filter_label(f: ResizeFilter) -> String {
    match f {
        ResizeFilter::CatmullRom => tr("resize.filter_catmull"),
        ResizeFilter::Nearest => tr("resize.filter_nearest"),
        ResizeFilter::Triangle => tr("resize.filter_triangle"),
        ResizeFilter::Lanczos3 => tr("resize.filter_lanczos"),
    }
}

// ─── 清空/放弃编辑确认 ──────────────────────────────────────────

fn confirm_clear_dialog(ctx: &Context, ui_state: &mut UiState) -> Vec<Action> {
    let mut actions = Vec::new();

    // Window::open 借用独立局部变量，避免与闭包内的 ui_state 借用冲突
    let mut open = ui_state.confirm_clear;
    let mut closed_by_button = false;
    let has_pending_open = ui_state.pending_open.is_some();

    egui::Window::new(tr("confirm.title"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            // 脏文档触发：打开新文件 → 提示将丢弃编辑；清空 → 原文案
            let body_key = if has_pending_open {
                "confirm.body_open"
            } else {
                "confirm.body"
            };
            ui.label(tr(body_key));
            ui.separator();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::accent_button(ui, tr("confirm.clear")).clicked() {
                    // 有暂存路径则继续打开新文件，否则执行清空
                    if let Some(path) = ui_state.pending_open.take() {
                        actions.push(Action::FilePicked(Some(path)));
                    } else {
                        actions.push(Action::ClearConfirmed);
                    }
                    closed_by_button = true;
                }
                if ui.button(tr("confirm.cancel")).clicked() {
                    ui_state.pending_open = None;
                    closed_by_button = true;
                }
            });
        });

    ui_state.confirm_clear = open && !closed_by_button;

    actions
}

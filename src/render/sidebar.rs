//! 右侧属性栏：文件信息 / 所选帧 / 内存 三组折叠面板。
//!
//! 纯视图：只读 Model，可变交互一律产出 Action；
//! 时长输入框、应用到全部等瞬态存 egui memory（不进 UiState/Model，
//! 关闭文档或切换选中集时按签名重置）。

use super::i18n::{tr, tra};
use super::toolbar::fmt_bytes;
use crate::model::{Action, Edit, FrameId, Model, MEMORY_BUDGET_BYTES};
use crate::render::{theme, TextureCache, UiState};
use eframe::egui::{self, Context, Id, Ui};
use std::path::PathBuf;

// egui memory 键（跨帧瞬态）
const KEY_DUR_SIG: &str = "sidebar.duration_sel_sig";
const KEY_DURATION_MS: &str = "sidebar.duration_ms";
const KEY_APPLY_ALL: &str = "sidebar.apply_all";
/// 混合时长时输入框的初始值（规格指定 100ms）
const DEFAULT_DURATION_MS: u32 = 100;

pub fn render_sidebar(
    ui: &mut Ui,
    _ctx: &Context,
    model: &Model,
    textures: &mut TextureCache,
    _ui_state: &mut UiState,
) -> Vec<Action> {
    let Some(p) = model.project.as_ref() else {
        return Vec::new();
    };
    let mut actions = Vec::new();

    render_file_info(ui, p, &mut actions);
    if !p.selection.is_empty() {
        render_selection(ui, p, &mut actions);
    }
    render_memory(ui, p, textures);

    actions
}

// ── 文件信息 ────────────────────────────────────────────────────

fn render_file_info(ui: &mut Ui, p: &crate::model::Project, actions: &mut Vec<Action>) {
    egui::CollapsingHeader::new(theme::heading(tr("sidebar.file_info")))
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new("sidebar.file_info_grid")
                .num_columns(2)
                .spacing([10.0, 8.0])
                .show(ui, |ui| {
                    // 文件名（source 尾段），长名换行避免撑破侧栏
                    let name = p
                        .source
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "—".into());
                    ui.weak(tr("sidebar.file_name"));
                    ui.add(egui::Label::new(name).wrap());
                    ui.end_row();

                    // 文件大小：加载时读一次缓存于 Project（审查修复：
                    // 原先每帧 fs::metadata，网络路径会阻塞渲染线程）
                    ui.weak(tr("sidebar.file_size"));
                    let size = if p.file_size > 0 {
                        fmt_bytes(p.file_size)
                    } else {
                        "—".into()
                    };
                    ui.label(size);
                    ui.end_row();

                    ui.weak(tr("sidebar.canvas"));
                    ui.label(format!("{} × {}", p.canvas.0, p.canvas.1));
                    ui.end_row();

                    ui.weak(tr("sidebar.frame_count"));
                    ui.label(p.frame_count().to_string());
                    ui.end_row();

                    // 循环：勾选 = loop_count == 0（无限）；取消勾选由模型固定为 1 次
                    ui.weak(tr("sidebar.loop_mode"));
                    ui.horizontal(|ui| {
                        let mut infinite = p.loop_count == 0;
                        let resp = ui.checkbox(&mut infinite, tr("sidebar.loop_infinite"));
                        if resp.changed() {
                            actions.push(Action::SetLoopInfinite(infinite));
                        }
                        if !infinite {
                            ui.weak(format!("×{}", p.loop_count));
                        }
                    });
                    ui.end_row();
                });
        });
}

// ── 所选帧 ──────────────────────────────────────────────────────

fn render_selection(ui: &mut Ui, p: &crate::model::Project, actions: &mut Vec<Action>) {
    egui::CollapsingHeader::new(theme::heading(tr("sidebar.selection")))
        .default_open(true)
        .show(ui, |ui| {
            // 选中帧时长采样（selection ⊆ order ⊆ store，采样必非空）
            let durations: Vec<u32> = p
                .selection
                .iter()
                .filter_map(|id| p.store.get(*id))
                .map(|f| f.duration_ms)
                .collect();
            let mixed = durations.len() > 1 && durations.iter().any(|&d| d != durations[0]);

            // 选中集或文档变更时重置输入框：混合 → 100，一致 → 该公共时长
            let sig = (p.source.clone(), p.selection.clone());
            let selection_changed = ui.memory_mut(|mem| {
                let prev = mem
                    .data
                    .get_temp::<(PathBuf, Vec<FrameId>)>(Id::new(KEY_DUR_SIG));
                let changed = prev.as_ref() != Some(&sig);
                if changed {
                    mem.data.insert_temp(Id::new(KEY_DUR_SIG), sig);
                }
                changed
            });
            if selection_changed {
                let init = if mixed {
                    DEFAULT_DURATION_MS
                } else {
                    durations.first().copied().unwrap_or(DEFAULT_DURATION_MS)
                };
                ui.memory_mut(|mem| mem.data.insert_temp(Id::new(KEY_DURATION_MS), init));
            }

            // 读取当前输入值（跨帧驻留 egui memory，DragValue 拖动即时写回）
            let mut ms = ui.memory_mut(|mem| {
                *mem.data
                    .get_temp_mut_or_insert_with(Id::new(KEY_DURATION_MS), || DEFAULT_DURATION_MS)
            });

            ui.label(tra(
                "sidebar.selected_count",
                &[("n", p.selection.len().to_string())],
            ));

            // 时长编辑行：DragValue 10–60000ms；时长不一致时灰色提示「多种」
            ui.horizontal(|ui| {
                ui.weak(tr("sidebar.duration"));
                let resp = ui.add(egui::DragValue::new(&mut ms).range(10..=60000).speed(10.0));
                if resp.changed() {
                    ui.memory_mut(|mem| mem.data.insert_temp(Id::new(KEY_DURATION_MS), ms));
                }
                if mixed {
                    ui.weak(tr("sidebar.duration_mixed"));
                }
            });

            // 应用行：勾选「应用到所有帧」时目标 = 全部 order，否则 = 选中集
            ui.horizontal(|ui| {
                let mut apply_all = ui.memory_mut(|mem| {
                    *mem.data
                        .get_temp_mut_or_insert_with(Id::new(KEY_APPLY_ALL), || false)
                });
                ui.checkbox(&mut apply_all, tr("sidebar.apply_all"));
                ui.memory_mut(|mem| mem.data.insert_temp(Id::new(KEY_APPLY_ALL), apply_all));

                if ui.button(tr("sidebar.apply")).clicked() {
                    let ids = if apply_all {
                        p.order.clone()
                    } else {
                        p.selection_in_order()
                    };
                    actions.push(Action::Edit(Edit::SetDuration { ids, ms }));
                }
            });
        });
}

// ── 内存 ────────────────────────────────────────────────────────

fn render_memory(ui: &mut Ui, p: &crate::model::Project, textures: &TextureCache) {
    egui::CollapsingHeader::new(theme::heading(tr("sidebar.memory")))
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new("sidebar.memory_grid")
                .num_columns(2)
                .spacing([10.0, 8.0])
                .show(ui, |ui| {
                    ui.weak(tr("sidebar.mem_frames"));
                    ui.label(fmt_bytes(p.store.total_memory()));
                    ui.end_row();

                    ui.weak(tr("sidebar.mem_thumbs"));
                    ui.label(textures.thumb_count().to_string());
                    ui.end_row();

                    ui.weak(tr("sidebar.mem_budget"));
                    ui.label(fmt_bytes(MEMORY_BUDGET_BYTES));
                    ui.end_row();
                });
        });
}

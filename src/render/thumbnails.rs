//! 左侧帧列表：每帧一张卡片（拖拽手柄 | 缩略图 | 元信息）。
//!
//! 交互（共识 Q7）：
//! - 多选：单击单选 / Ctrl 切换 / Shift 范围（锚点语义见 model::apply）
//! - 右键菜单：镜像工具栏帧操作 + 单选时上移/下移一格
//! - 手柄拖拽重排：拖动中画插入指示线，松手提交 Edit::Reorder
//!
//! 性能红线：直接迭代 `order + store.get`，每帧渲染零集合构建、零像素拷贝
//! （缩略图纹理命中缓存路径）。

use crate::model::{Action, Edit, FrameId, Model};
use crate::render::i18n::{tr, tra};
use crate::render::{icons, theme, TextureCache, UiState};
use eframe::egui::{
    self, pos2, Align, Color32, Context, Id, Layout, Rect, Rounding, Sense, Stroke, Ui, Vec2,
};

/// 缩略图显示高度（px），宽度按比例
const THUMB_H: f32 = 96.0;
const CARD_PAD: f32 = 10.0;

/// 拖拽重排的跨帧瞬态：被拖帧 id（egui memory）
const DRAG_KEY: &str = "thumbs_drag_id";

pub fn render_thumbnails(
    ui: &mut Ui,
    ctx: &Context,
    model: &Model,
    textures: &mut TextureCache,
    ui_state: &mut UiState,
) -> Vec<Action> {
    let _ = ui_state; // 帧列表自身无 UI 瞬态（拖拽状态走 egui memory）
    let mut actions = Vec::new();

    let Some(project) = model.project.as_ref() else {
        render_empty(ui, "thumbs.empty");
        return actions;
    };
    if project.order.is_empty() {
        // 空文档：挂起的拖拽状态一并清除（审查修复：残留 DRAG_KEY 会在
        // 下个文档上凭空提交一次 Reorder）
        ctx.memory_mut(|m| m.data.remove::<FrameId>(Id::new(DRAG_KEY)));
        render_empty(ui, "thumbs.empty_doc");
        return actions;
    }

    // ── 面板头 ──
    ui.horizontal(|ui| {
        ui.label(theme::heading(tr("thumbs.title")));
        ui.label(
            egui::RichText::new(tra(
                "thumbs.count",
                &[("n", project.frame_count().to_string())],
            ))
            .small()
            .color(theme::TEXT_DIM),
        );
    });
    ui.separator();
    ui.add_space(4.0);

    let selection = project.selection_set();
    let playhead_id = project
        .order
        .get(project.view_index.min(project.order.len() - 1))
        .copied();
    let dragging: Option<FrameId> = ctx.memory(|m| m.data.get_temp(Id::new(DRAG_KEY)));

    // 本帧记录的卡片矩形（拖拽指示线/落点计算用）：(index, id, rect, thumb_rect)
    let mut card_rects: Vec<(usize, FrameId, Rect)> = Vec::new();

    for (idx, &id) in project.order.iter().enumerate() {
        let Some(frame) = project.store.get(id) else {
            continue;
        };
        let is_sel = selection.contains(&id);
        let is_playhead = playhead_id == Some(id);
        let is_dragging = dragging == Some(id);

        card_rects.push((
            idx,
            id,
            render_card(
                ui,
                ctx,
                model,
                textures,
                idx,
                id,
                &frame,
                is_sel,
                is_playhead,
                is_dragging,
                &mut actions,
            ),
        ));
    }

    // ── 拖拽重排落点处理 ──
    handle_drag_drop(ui, ctx, project, &card_rects, dragging, &mut actions);

    actions
}

/// 渲染单张卡片，返回卡片整体矩形
#[allow(clippy::too_many_arguments)]
fn render_card(
    ui: &mut Ui,
    ctx: &Context,
    model: &Model,
    textures: &mut TextureCache,
    idx: usize,
    id: FrameId,
    frame: &std::sync::Arc<crate::model::Frame>,
    is_sel: bool,
    is_playhead: bool,
    is_dragging: bool,
    actions: &mut Vec<Action>,
) -> Rect {
    let avail_w = ui.available_width();
    let aspect = frame.width as f32 / frame.height.max(1) as f32;
    let thumb_size = Vec2::new((THUMB_H * aspect).min(avail_w * 0.55), THUMB_H);
    let row_h = thumb_size.y + CARD_PAD * 2.0;

    // 卡片整体（ Sense::click：选择 + 右键菜单）
    let (card_rect, card_resp) = ui.allocate_exact_size(Vec2::new(avail_w, row_h), Sense::click());

    // 背景与边框（悬停浮出；选中靛蓝；拖拽整卡强化）
    let (bg, border, width) = if is_dragging {
        (
            Color32::from_rgba_premultiplied(110, 121, 239, 60),
            theme::ACCENT,
            2.5_f32,
        )
    } else if is_sel {
        (theme::SEL_BG, theme::ACCENT, 2.0_f32)
    } else if card_resp.hovered() {
        (theme::BG_HOVER, Color32::from_rgb(58, 58, 72), 1.0_f32)
    } else {
        (theme::BG_SURF, theme::BORDER, 1.0_f32)
    };
    ui.painter().rect_filled(card_rect, Rounding::same(10.0), bg);
    ui.painter()
        .rect_stroke(card_rect, Rounding::same(10.0), Stroke::new(width, border));

    // 内容行：手柄 | 缩略图 | 元信息
    let mut content = ui.child_ui(
        card_rect.shrink(CARD_PAD),
        Layout::left_to_right(Align::Center),
        None,
    );

    // 拖拽手柄（Sense::drag：不与卡片 click 冲突）；图标字形居中放入手柄区
    let (grip_rect, grip_resp) =
        content.allocate_exact_size(Vec2::new(18.0, row_h - CARD_PAD * 2.0), Sense::drag());
    content.put(
        grip_rect,
        egui::Label::new(
            egui::RichText::new(icons::glyph::DRAG_INDICATOR)
                .size(16.0)
                .family(icons::family())
                .color(theme::TEXT_FAINT),
        ),
    );
    if grip_resp.drag_started() {
        ctx.memory_mut(|m| m.data.insert_temp(Id::new(DRAG_KEY), id));
    }
    if grip_resp.dragged() {
        // 拖动中：整卡高亮（下一帧生效），并阻止误触点击
        ctx.request_repaint();
    }

    // 缩略图（小纹理缓存）
    let tex = textures.thumb(ctx, id, frame);
    content.image((tex, thumb_size));

    // 元信息列
    content.with_layout(Layout::top_down(Align::LEFT), |ui| {
        ui.horizontal(|ui| {
            if is_playhead {
                // 播放头标记：painter 画金色圆点（● 字形默认字体缺失）
                let (dot_rect, dot_resp) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                ui.painter().circle_filled(dot_rect.center(), 3.0, theme::GOLD);
                dot_resp.on_hover_text(tr("preview.playhead"));
            }
            ui.label(egui::RichText::new(format!("#{}", idx + 1)).strong().color(theme::TEXT));
        });
        ui.label(
            egui::RichText::new(format!("{}×{}", frame.width, frame.height))
                .small()
                .color(theme::TEXT_DIM),
        );
        ui.label(
            egui::RichText::new(format!("{} ms", frame.duration_ms))
                .small()
                .color(theme::TEXT_DIM),
        );
    });

    // ── 点击选择（含修饰键语义） ──
    if card_resp.clicked() {
        let (ctrl, shift) = ctx.input(|i| (i.modifiers.ctrl, i.modifiers.shift));
        actions.push(if ctrl {
            Action::ToggleSelect(id)
        } else if shift {
            Action::RangeSelect(id)
        } else {
            Action::SelectFrame(id)
        });
    }

    // ── 右键菜单：镜像工具栏 + 单选移动 ──
    let project = model.project.as_ref();
    card_resp.context_menu(|ui| {
        let Some(p) = project else { return };
        let sel_ids = p.selection_in_order();
        let has_sel = !sel_ids.is_empty();
        if ui
            .add_enabled(has_sel, egui::Button::new(tr("edit.delete")))
            .clicked()
        {
            actions.push(Action::Edit(Edit::DeleteFrames { ids: sel_ids }));
            ui.close_menu();
        }
        if ui
            .add_enabled(has_sel, egui::Button::new(tr("edit.reverse")))
            .clicked()
        {
            actions.push(Action::Edit(Edit::ReverseSelected));
            ui.close_menu();
        }
        ui.separator();
        for (label, edit) in [
            (tr("edit.rotl"), Edit::RotateLeft),
            (tr("edit.rotr"), Edit::RotateRight),
            (tr("edit.fliph"), Edit::FlipH),
            (tr("edit.flipv"), Edit::FlipV),
        ] {
            if ui
                .add_enabled(!p.order.is_empty(), egui::Button::new(label))
                .clicked()
            {
                actions.push(Action::Edit(edit));
                ui.close_menu();
            }
        }
        ui.separator();
        // 单选时上移/下移一格
        if p.selection.len() == 1 {
            let sel = p.selection[0];
            if let Some(pos) = p.order.iter().position(|x| *x == sel) {
                if pos > 0 && ui.button(tr("thumbs.move_up")).clicked() {
                    actions.push(Action::Edit(Edit::Reorder {
                        moved: vec![sel],
                        before: Some(p.order[pos - 1]),
                    }));
                    ui.close_menu();
                }
                // 下移 = 移到「再下一帧」之前；已在末尾相邻则移到队尾（before=None）
                let down_before: Option<Option<FrameId>> = if pos + 2 < p.order.len() {
                    Some(Some(p.order[pos + 2]))
                } else if pos + 1 < p.order.len() {
                    Some(None)
                } else {
                    None // 已是最后一帧，无法下移
                };
                if let Some(before) = down_before {
                    if ui.button(tr("thumbs.move_down")).clicked() {
                        actions.push(Action::Edit(Edit::Reorder {
                            moved: vec![sel],
                            before,
                        }));
                        ui.close_menu();
                    }
                }
            }
        }
    });

    card_rect
}

/// 拖拽指示线绘制 + 松手落点计算（缝隙下一帧 = before 目标）
fn handle_drag_drop(
    ui: &mut Ui,
    ctx: &Context,
    project: &crate::model::Project,
    card_rects: &[(usize, FrameId, Rect)],
    dragging: Option<FrameId>,
    actions: &mut Vec<Action>,
) {
    let Some(drag_id) = dragging else { return };
    // 拖拽的帧已不在序列中（文档切换/该帧被删）：丢弃陈旧拖拽状态，
    // 否则会在新文档上凭空提交一次 Reorder（审查修复）
    if !project.order.contains(&drag_id) {
        ctx.memory_mut(|m| m.data.remove::<FrameId>(Id::new(DRAG_KEY)));
        return;
    }
    let Some(pointer) = ctx.pointer_latest_pos() else {
        return;
    };

    // 指针是否仍按住：任意 pointer down 持续中视为拖动；松开即提交
    let button_down = ctx.input(|i| i.pointer.primary_down());
    if button_down {
        // 计算插入缝隙（在被拖帧之外 的卡片序列中）
        let others: Vec<&(usize, FrameId, Rect)> = card_rects
            .iter()
            .filter(|(_, id, _)| *id != drag_id)
            .collect();
        let gap = others
            .iter()
            .filter(|(_, _, r)| r.center().y < pointer.y)
            .count();
        // 画指示线
        let line_y = if gap < others.len() {
            others[gap].2.top()
        } else if let Some(last) = others.last() {
            last.2.bottom()
        } else {
            return;
        };
        let x0 = ui.min_rect().left();
        let x1 = ui.min_rect().right();
        ui.painter().line_segment(
            [pos2(x0 + 6.0, line_y), pos2(x1 - 6.0, line_y)],
            Stroke::new(2.5_f32, theme::ACCENT),
        );
        return;
    }

    // 松手 → 提交 Reorder
    ctx.memory_mut(|m| m.data.remove::<FrameId>(Id::new(DRAG_KEY)));
    let others: Vec<FrameId> = project
        .order
        .iter()
        .copied()
        .filter(|id| *id != drag_id)
        .collect();
    let gap = card_rects
        .iter()
        .filter(|(_, id, _)| *id != drag_id)
        .filter(|(_, _, r)| r.center().y < pointer.y)
        .count();
    let before = others.get(gap).copied();
    // 移动前后 id 相同（放回原位）则无操作
    if before != Some(drag_id) {
        let no_op = match before {
            Some(b) => {
                // 原位判断：drag_id 紧邻 b 之前且未跨过其他帧
                let pos = project.order.iter().position(|x| *x == drag_id);
                let bpos = project.order.iter().position(|x| *x == b);
                matches!((pos, bpos), (Some(p), Some(bp)) if bp == p + 1)
            }
            None => project.order.last() == Some(&drag_id),
        };
        if !no_op {
            actions.push(Action::Edit(Edit::Reorder {
                moved: vec![drag_id],
                before,
            }));
        }
    }
}

fn render_empty(ui: &mut Ui, key: &str) {
    ui.add_space(60.0);
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.label(egui::RichText::new(tr(key)).color(theme::TEXT_DIM));
    });
}

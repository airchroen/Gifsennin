//! 左侧帧列表：每帧一张卡片（拖拽手柄 | 缩略图 | 元信息）。
//!
//! 交互：
//! - 多选：单击单选 / Ctrl 切换 / Shift 范围（锚点语义见 model::apply）
//! - 右键菜单：镜像工具栏帧操作 + 单选时上移/下移一格
//! - 手柄拖拽重排：拖动中画插入指示线，松手提交 Edit::Reorder
//!
//! 性能约束：直接迭代 `order + store.get`，缩略图纹理走缓存路径，每帧不做像素拷贝。

use crate::model::{Action, Edit, Frame, FrameId, Model, Project};
use crate::render::i18n::{tr, tra};
use crate::render::{edit_label_key, icons, theme, TextureCache};
use eframe::egui::{
    self, pos2, Align, Context, Id, Layout, Rect, Rounding, Sense, Stroke, Ui, Vec2,
};
use std::sync::Arc;

/// 缩略图显示高度（px），宽度按比例
const THUMB_H: f32 = 96.0;
const CARD_PAD: f32 = 10.0;

/// 拖拽重排的跨帧瞬态：被拖帧 id（egui memory）
const DRAG_KEY: &str = "thumbs_drag_id";

/// 渲染一张卡片所需的输入
struct Card<'a> {
    /// 序列下标（1-based 帧号显示用）
    idx: usize,
    id: FrameId,
    frame: &'a Arc<Frame>,
    /// 选中 / 播放头 / 拖拽中：决定卡片配色与播放头标记
    selected: bool,
    playhead: bool,
    dragging: bool,
}

/// 本帧渲染过的卡片矩形（拖拽指示线 / 落点计算用）
struct CardRect {
    id: FrameId,
    rect: Rect,
}

pub fn render_thumbnails(
    ui: &mut Ui,
    ctx: &Context,
    model: &Model,
    textures: &mut TextureCache,
) -> Vec<Action> {
    let mut actions = Vec::new();

    let Some(project) = model.project.as_ref() else {
        render_empty(ui, "thumbs.empty");
        return actions;
    };
    if project.order.is_empty() {
        // 空文档：挂起的拖拽状态一并清除（残留会在下个文档上凭空提交一次 Reorder）
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

    let mut card_rects = Vec::with_capacity(project.order.len());
    for (idx, &id) in project.order.iter().enumerate() {
        let Some(frame) = project.store.get(id) else {
            continue;
        };
        let card = Card {
            idx,
            id,
            frame: &frame,
            selected: selection.contains(&id),
            playhead: playhead_id == Some(id),
            dragging: dragging == Some(id),
        };
        let rect = render_card(ui, ctx, project, textures, &card, &mut actions);
        card_rects.push(CardRect { id, rect });
    }

    // ── 拖拽重排落点处理 ──
    handle_drag_drop(ui, ctx, project, &card_rects, dragging, &mut actions);

    actions
}

/// 渲染单张卡片，返回卡片整体矩形
fn render_card(
    ui: &mut Ui,
    ctx: &Context,
    project: &Project,
    textures: &mut TextureCache,
    card: &Card<'_>,
    actions: &mut Vec<Action>,
) -> Rect {
    let (idx, id, frame) = (card.idx, card.id, card.frame);
    let avail_w = ui.available_width();
    let aspect = frame.width as f32 / frame.height.max(1) as f32;
    let thumb_size = Vec2::new((THUMB_H * aspect).min(avail_w * 0.55), THUMB_H);
    let row_h = thumb_size.y + CARD_PAD * 2.0;

    // 卡片整体（ Sense::click：选择 + 右键菜单）
    let (card_rect, card_resp) = ui.allocate_exact_size(Vec2::new(avail_w, row_h), Sense::click());

    // 背景与边框（悬停浮出；选中靛蓝；拖拽整卡强化）
    let (bg, border, width) = if card.dragging {
        (theme::ACCENT_DIM, theme::ACCENT, 2.5_f32)
    } else if card.selected {
        (theme::SEL_BG, theme::ACCENT, 2.0_f32)
    } else if card_resp.hovered() {
        (theme::BG_HOVER, theme::BORDER_HOVER, 1.0_f32)
    } else {
        (theme::BG_SURF, theme::BORDER, 1.0_f32)
    };
    ui.painter()
        .rect_filled(card_rect, Rounding::same(10.0), bg);
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

    let tex = textures.thumb(ctx, id, frame);
    content.image((tex, thumb_size));

    // 元信息列
    content.with_layout(Layout::top_down(Align::LEFT), |ui| {
        ui.horizontal(|ui| {
            if card.playhead {
                theme::playhead_dot(ui, &tr("preview.playhead"));
            }
            ui.label(
                egui::RichText::new(format!("#{}", idx + 1))
                    .strong()
                    .color(theme::TEXT),
            );
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
    card_resp.context_menu(|ui| {
        let sel_ids = project.selection_in_order();
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
        for edit in [
            Edit::RotateLeft,
            Edit::RotateRight,
            Edit::FlipH,
            Edit::FlipV,
        ] {
            if ui.button(tr(edit_label_key(&edit))).clicked() {
                actions.push(Action::Edit(edit));
                ui.close_menu();
            }
        }
        ui.separator();
        // 单选时上移/下移一格
        if project.selection.len() == 1 {
            let sel = project.selection[0];
            if let Some(pos) = project.index_of(sel) {
                if pos > 0 && ui.button(tr("thumbs.move_up")).clicked() {
                    actions.push(Action::Edit(Edit::Reorder {
                        moved: vec![sel],
                        before: Some(project.order[pos - 1]),
                    }));
                    ui.close_menu();
                }
                // 下移 = 移到「再下一帧」之前；已在末尾相邻则移到队尾（before=None）
                let down_before: Option<Option<FrameId>> = if pos + 2 < project.order.len() {
                    Some(Some(project.order[pos + 2]))
                } else if pos + 1 < project.order.len() {
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
    project: &Project,
    card_rects: &[CardRect],
    dragging: Option<FrameId>,
    actions: &mut Vec<Action>,
) {
    let Some(drag_id) = dragging else { return };
    // 拖拽的帧已不在序列中（文档切换/该帧被删）：丢弃陈旧拖拽状态，
    // 否则会在新文档上凭空提交一次 Reorder
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
        // 计算插入缝隙（在被拖帧之外的卡片序列中）
        let others: Vec<&CardRect> = card_rects.iter().filter(|c| c.id != drag_id).collect();
        let gap = others
            .iter()
            .filter(|c| c.rect.center().y < pointer.y)
            .count();
        // 画指示线
        let line_y = if gap < others.len() {
            others[gap].rect.top()
        } else if let Some(last) = others.last() {
            last.rect.bottom()
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
        .filter(|c| c.id != drag_id)
        .filter(|c| c.rect.center().y < pointer.y)
        .count();
    let before = others.get(gap).copied();
    // 放回原位则无操作：目标紧邻其后，或移到队尾而它本就是末帧
    let no_op = match before {
        Some(b) => {
            let pos = project.index_of(drag_id);
            let bpos = project.index_of(b);
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

fn render_empty(ui: &mut Ui, key: &str) {
    ui.add_space(60.0);
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.label(egui::RichText::new(tr(key)).color(theme::TEXT_DIM));
    });
}

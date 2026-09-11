//! 裁剪覆盖层：裁剪模式（`ui_state.crop.is_some()`）下的中央交互视图。
//!
//! 布局：标题行（模式提示 + 选区读数 + 取消/确认）→ 居中适配的播放头帧
//! 全分辨率纹理（暗遮罩 + 靛蓝选区框 + 四角手柄）→ 数值联动行。
//! 所有修改只作用于 `ui_state.crop` 草稿（立即模式）；确认才提交
//! `Action::Edit(Edit::Crop)`，取消直接清空草稿。

use crate::model::transform::clamp_crop;
use crate::model::{Action, Edit, Model};
use crate::render::i18n::{tr, tra};
use crate::render::{theme, CropDraft, TextureCache, UiState};
use eframe::egui::{
    pos2, vec2, Align, Color32, Context, CursorIcon, DragValue, Id, Layout, Pos2, Rect, Rounding,
    Sense, Stroke, Ui, Vec2,
};

// ─── 样式常量 ───────────────────────────────────────────────────

const SELECTION_STROKE: f32 = 1.5;
/// 选区外半透明暗遮罩
const MASK_COLOR: Color32 = Color32::from_black_alpha(128);
const HANDLE_RADIUS: f32 = 5.0;
/// 角柄命中区域略大于视觉圆点，便于抓取
const HANDLE_GRAB: f32 = 16.0;

// ─── 选区几何辅助 ───────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// 屏幕坐标 → 画布像素坐标（取整 + 夹取到画布内，最小 0）
fn screen_to_canvas(p: Pos2, image_rect: Rect, canvas: (u32, u32)) -> (u32, u32) {
    let scale_x = image_rect.width() / canvas.0 as f32;
    let scale_y = image_rect.height() / canvas.1 as f32;
    let x = ((p.x - image_rect.left()) / scale_x).round() as i64;
    let y = ((p.y - image_rect.top()) / scale_y).round() as i64;
    (
        x.clamp(0, canvas.0 as i64 - 1) as u32,
        y.clamp(0, canvas.1 as i64 - 1) as u32,
    )
}

/// 画布像素坐标 → 屏幕坐标（含缩放）
fn canvas_to_screen(x: u32, y: u32, image_rect: Rect, scale: f32) -> Pos2 {
    pos2(
        image_rect.left() + x as f32 * scale,
        image_rect.top() + y as f32 * scale,
    )
}

/// 由任意两个画布角点（含端点像素）构造合法草稿（clamp 后 w/h ≥ 1）
fn draft_from_corners(canvas: (u32, u32), a: (u32, u32), b: (u32, u32)) -> CropDraft {
    let x0 = a.0.min(b.0);
    let y0 = a.1.min(b.1);
    let x1 = a.0.max(b.0);
    let y1 = a.1.max(b.1);
    let (x, y, w, h) = clamp_crop(canvas, x0, y0, x1 - x0 + 1, y1 - y0 + 1);
    CropDraft { x, y, w, h }
}

/// 草稿四角（画布像素，含端点）
fn draft_corners(d: CropDraft) -> [(Corner, (u32, u32)); 4] {
    let right = d.x + d.w - 1;
    let bottom = d.y + d.h - 1;
    [
        (Corner::TopLeft, (d.x, d.y)),
        (Corner::TopRight, (right, d.y)),
        (Corner::BottomLeft, (d.x, bottom)),
        (Corner::BottomRight, (right, bottom)),
    ]
}

/// 角柄拖拽时的固定对角（画布像素，含端点）
fn opposite_corner(d: CropDraft, c: Corner) -> (u32, u32) {
    let right = d.x + d.w - 1;
    let bottom = d.y + d.h - 1;
    match c {
        Corner::TopLeft => (right, bottom),
        Corner::TopRight => (d.x, bottom),
        Corner::BottomLeft => (right, d.y),
        Corner::BottomRight => (d.x, d.y),
    }
}

// ─── 主入口 ─────────────────────────────────────────────────────

pub fn render_crop_overlay(
    ui: &mut Ui,
    ctx: &Context,
    model: &Model,
    textures: &mut TextureCache,
    ui_state: &mut UiState,
) -> Vec<Action> {
    let mut actions = Vec::new();

    // 守卫：无项目 / 空画布时自动退出裁剪模式
    let Some(project) = model.project.as_ref() else {
        ui_state.crop = None;
        return actions;
    };
    let canvas = project.canvas;
    if canvas.0 == 0 || canvas.1 == 0 {
        ui_state.crop = None;
        return actions;
    }
    // 裁剪模式未激活（调用方约定 Some 才进入），不做防御性创建
    let Some(draft) = ui_state.crop else {
        return actions;
    };
    let frame_idx = project
        .view_index
        .min(project.order.len().saturating_sub(1));
    let (Some(&frame_id), Some(frame)) =
        (project.order.get(frame_idx), project.frame_at(frame_idx))
    else {
        ui_state.crop = None;
        return actions;
    };

    // ── 标题行：模式提示 + 选区读数 + 取消/确认 ──
    ui.horizontal(|ui| {
        ui.label(tr("crop.hint"));
        ui.strong(tra(
            "crop.selection_size",
            &[("w", draft.w.to_string()), ("h", draft.h.to_string())],
        ));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if theme::accent_button(ui, tr("crop.confirm")).clicked() {
                actions.push(Action::Edit(Edit::Crop {
                    x: draft.x,
                    y: draft.y,
                    w: draft.w,
                    h: draft.h,
                }));
                ui_state.crop = None;
            }
            if ui.button(tr("crop.cancel")).clicked() {
                ui_state.crop = None;
            }
        });
    });
    if ui_state.crop.is_none() {
        return actions; // 本帧点了取消/确认，下帧交还常规预览
    }
    ui.separator();

    // ── 计算居中适配矩形（底部为数值行预留高度） ──
    let avail = ui.available_rect_before_wrap();
    let spacing = ui.spacing().item_spacing;
    let reserve = ui.spacing().interact_size.y.max(22.0) + spacing.y;
    let area = Rect::from_min_max(
        avail.min,
        pos2(avail.right(), avail.bottom() - reserve).max(avail.min + vec2(1.0, 1.0)), // 面板过矮时保证 ≥ 1px
    );
    let scale_x = area.width() / canvas.0 as f32;
    let scale_y = area.height() / canvas.1 as f32;
    let scale = scale_x.min(scale_y);
    let image_rect = Rect::from_center_size(
        area.center(),
        vec2(canvas.0 as f32 * scale, canvas.1 as f32 * scale),
    );

    // 画布→屏幕映射（选区像素 [x, x+w) 覆盖到屏幕 (x)*scale..(x+w)*scale）
    let sel_screen = |d: CropDraft| -> Rect {
        Rect::from_min_max(
            pos2(
                image_rect.left() + d.x as f32 * scale,
                image_rect.top() + d.y as f32 * scale,
            ),
            pos2(
                image_rect.left() + (d.x + d.w) as f32 * scale,
                image_rect.top() + (d.y + d.h) as f32 * scale,
            ),
        )
    };

    // ── 交互层：先注册（后注册者命中优先），随后统一绘制 ──

    // 整图拖拽框选：起点存 egui 瞬态记忆（跨帧），拖动中实时归一化为选区
    let area_resp = ui
        .allocate_rect(image_rect, Sense::drag())
        .on_hover_cursor(CursorIcon::Crosshair);

    let drag_start_id = Id::new("crop_overlay_drag_start");
    if area_resp.drag_started() {
        if let Some(p) = area_resp.interact_pointer_pos() {
            ctx.memory_mut(|m| m.data.insert_temp(drag_start_id, p));
        }
    }
    if area_resp.dragged() {
        if let (Some(start), Some(cur)) = (
            ctx.memory(|m| m.data.get_temp::<Pos2>(drag_start_id)),
            area_resp.interact_pointer_pos(),
        ) {
            let a = screen_to_canvas(start, image_rect, canvas);
            let b = screen_to_canvas(cur, image_rect, canvas);
            ui_state.crop = Some(draft_from_corners(canvas, a, b));
        }
    }

    // 四角手柄（在整图矩形之后分配 → 命中优先）：拖动对应角调整选区
    for (corner, (cx, cy)) in draft_corners(draft) {
        let center = canvas_to_screen(cx, cy, image_rect, scale);
        let grab = Rect::from_center_size(center, Vec2::splat(HANDLE_GRAB));
        let resp = ui
            .allocate_rect(grab, Sense::drag())
            .on_hover_cursor(match corner {
                Corner::TopLeft | Corner::BottomRight => CursorIcon::ResizeNwSe,
                Corner::TopRight | Corner::BottomLeft => CursorIcon::ResizeNeSw,
            });
        if resp.dragged() {
            if let Some(p) = resp.interact_pointer_pos() {
                let moved = screen_to_canvas(p, image_rect, canvas);
                let fixed = opposite_corner(draft, corner);
                ui_state.crop = Some(draft_from_corners(canvas, moved, fixed));
            }
        }
    }

    // ── 绘制：衬底 + 图 + 暗遮罩 + 选区框 + 角柄（读交互后的最新草稿） ──
    let painter = ui.painter();
    let Some(cur) = ui_state.crop else {
        return actions; // 交互不会清空草稿，防御性兜底
    };
    let sel_rect = sel_screen(cur);

    let tex = textures.full(ctx, frame_id, &frame);
    // 透明帧衬底（与中央画布同底色）
    painter.rect_filled(image_rect, Rounding::ZERO, theme::BG_BASE);
    // 播放头帧全分辨率纹理（UV 全图）
    painter.image(
        tex,
        image_rect,
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        Color32::WHITE,
    );

    // 选区外四块暗遮罩（上 / 下 / 左 / 右）
    painter.rect_filled(
        Rect::from_min_max(
            pos2(image_rect.left(), image_rect.top()),
            pos2(image_rect.right(), sel_rect.top()),
        ),
        Rounding::ZERO,
        MASK_COLOR,
    );
    painter.rect_filled(
        Rect::from_min_max(
            pos2(image_rect.left(), sel_rect.bottom()),
            pos2(image_rect.right(), image_rect.bottom()),
        ),
        Rounding::ZERO,
        MASK_COLOR,
    );
    painter.rect_filled(
        Rect::from_min_max(
            pos2(image_rect.left(), sel_rect.top()),
            pos2(sel_rect.left(), sel_rect.bottom()),
        ),
        Rounding::ZERO,
        MASK_COLOR,
    );
    painter.rect_filled(
        Rect::from_min_max(
            pos2(sel_rect.right(), sel_rect.top()),
            pos2(image_rect.right(), sel_rect.bottom()),
        ),
        Rounding::ZERO,
        MASK_COLOR,
    );

    // 选区靛蓝边框 + 四角手柄圆点
    painter.rect_stroke(
        sel_rect,
        Rounding::ZERO,
        Stroke::new(SELECTION_STROKE, theme::ACCENT),
    );
    for (_, (cx, cy)) in draft_corners(cur) {
        let center = canvas_to_screen(cx, cy, image_rect, scale);
        painter.circle_filled(center, HANDLE_RADIUS, theme::ACCENT);
        painter.circle_stroke(center, HANDLE_RADIUS, Stroke::new(1.0_f32, Color32::WHITE));
    }

    // ── 数值联动行：x/y/w/h 与草稿双向同步（改数值即改草稿，经 clamp） ──
    let mut numeric_changed = false;
    if let Some(d) = ui_state.crop.as_mut() {
        ui.horizontal(|ui| {
            ui.label(tr("crop.position"));
            let rx = ui.add(
                DragValue::new(&mut d.x)
                    .range(0u32..=canvas.0)
                    .speed(1.0)
                    .prefix(format!("{} ", tr("crop.x"))),
            );
            let ry = ui.add(
                DragValue::new(&mut d.y)
                    .range(0u32..=canvas.1)
                    .speed(1.0)
                    .prefix(format!("{} ", tr("crop.y"))),
            );
            ui.separator();
            ui.label(tr("crop.size"));
            let rw = ui.add(
                DragValue::new(&mut d.w)
                    .range(1u32..=canvas.0)
                    .speed(1.0)
                    .prefix(format!("{} ", tr("crop.w"))),
            );
            let rh = ui.add(
                DragValue::new(&mut d.h)
                    .range(1u32..=canvas.1)
                    .speed(1.0)
                    .prefix(format!("{} ", tr("crop.h"))),
            );
            numeric_changed = rx.changed() || ry.changed() || rw.changed() || rh.changed();
        });
    }
    if numeric_changed {
        if let Some(d) = ui_state.crop.as_mut() {
            let (x, y, w, h) = clamp_crop(canvas, d.x, d.y, d.w, d.h);
            *d = CropDraft { x, y, w, h };
        }
    }

    actions
}

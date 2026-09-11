//! 中央预览区：播放头帧全分辨率纹理（等比适配 + 透明棋盘格衬底）
//! + 播放控制条。裁剪模式激活时整体委托给 crop_overlay。

use crate::model::{Action, Model};
use crate::render::crop_overlay::render_crop_overlay;
use crate::render::i18n::{tr, tra};
use crate::render::{icons, theme, TextureCache, UiState};
use eframe::egui;
use eframe::egui::{pos2, vec2, Align, Color32, Context, Layout, Rect, Rounding, Sense, Ui};

/// 透明衬底棋盘格边长（px）——两档灰交替（比 BG_BASE 略亮的冷灰，衬托画面）
const CHECKER_CELL: f32 = 20.0;
const CHECKER_A: Color32 = Color32::from_rgb(28, 28, 35); // #1C1C23
const CHECKER_B: Color32 = Color32::from_rgb(20, 20, 26); // #14141A

pub fn render_preview(
    ui: &mut Ui,
    ctx: &Context,
    model: &Model,
    textures: &mut TextureCache,
    ui_state: &mut UiState,
) -> Vec<Action> {
    // 裁剪模式：整个面板交给覆盖层（拖选区 + 数值联动）
    if ui_state.crop.is_some() {
        return render_crop_overlay(ui, ctx, model, textures, ui_state);
    }

    let mut actions = Vec::new();

    let Some(project) = model.project.as_ref() else {
        render_empty(ui, "preview.empty_open", "preview.empty_open_hint", icons::glyph::MOVIE);
        return actions;
    };

    // ── 标题行：预览 | 播放头计数（右对齐） ──
    let frame_count = project.frame_count();
    ui.horizontal(|ui| {
        ui.label(theme::heading(tr("preview.title")));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                egui::RichText::new(tra(
                    "preview.counter",
                    &[
                        ("i", (project.view_index + 1).to_string()),
                        ("n", frame_count.to_string()),
                    ],
                ))
                .small()
                .color(theme::TEXT_DIM),
            )
            .on_hover_text(tr("preview.playhead"));
        });
    });
    ui.separator();

    if project.order.is_empty() {
        // 空文档：全部帧被删除（可撤销恢复）
        render_empty(ui, "preview.empty_doc", "preview.empty_doc_hint", icons::glyph::MOVIE);
        if project.history.can_undo() {
            ui.with_layout(Layout::top_down(Align::Center), |ui| {
                let tip = format!("{} (Ctrl+Z)", tr("toolbar.undo"));
                if theme::icon_button(ui, true, icons::glyph::UNDO, &tip).clicked() {
                    actions.push(Action::Undo);
                }
            });
        }
        return actions;
    }

    let view_index = project.view_index.min(frame_count - 1);
    let (Some(&frame_id), Some(frame)) =
        (project.order.get(view_index), project.frame_at(view_index))
    else {
        return actions;
    };

    // ── 图像区：等比适配居中（底部为控制条预留空间） ──
    let avail = ui.available_rect_before_wrap();
    let spacing = ui.spacing().item_spacing;
    let reserve = ui.spacing().interact_size.y.max(24.0) + spacing.y * 2.0;
    let area = Rect::from_min_max(
        avail.min,
        pos2(
            avail.right(),
            (avail.bottom() - reserve).max(avail.min.y + 1.0),
        ),
    );
    let canvas = (frame.width, frame.height);
    let scale = (area.width() / canvas.0 as f32).min(area.height() / canvas.1 as f32);
    let image_rect = Rect::from_center_size(
        area.center(),
        vec2(canvas.0 as f32 * scale, canvas.1 as f32 * scale),
    );
    // 占据布局空间（无交互——点击交给缩略图/播放控制）
    ui.allocate_rect(image_rect, Sense::hover());

    let painter = ui.painter();
    // 透明衬底棋盘格（便于观察 alpha 区域）
    let mut y = image_rect.top();
    let mut row = 0usize;
    while y < image_rect.bottom() {
        let h = CHECKER_CELL.min(image_rect.bottom() - y);
        let mut x = image_rect.left();
        let mut col = 0usize;
        while x < image_rect.right() {
            let w = CHECKER_CELL.min(image_rect.right() - x);
            let cell = Rect::from_min_max(pos2(x, y), pos2(x + w, y + h));
            painter.rect_filled(
                cell,
                Rounding::ZERO,
                if (row + col).is_multiple_of(2) {
                    CHECKER_A
                } else {
                    CHECKER_B
                },
            );
            x += w;
            col += 1;
        }
        y += h;
        row += 1;
    }
    // 全分辨率纹理（单槽缓存，换帧即换）
    let tex = textures.full(ctx, frame_id, &frame);
    painter.image(
        tex,
        image_rect,
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        Color32::WHITE,
    );

    // ── 播放控制条 ──
    // 布局修复：原 with_main_justify(true) 在水平布局里会把首个控件拉伸到
    // 全宽、其余控件挤出可视区（egui justify 语义是「占满主轴」，存量 bug）。
    // 居中方案：量出整行宽度后左侧垫等量空隙（ui.horizontal 会先占满宽，
    // top_down(Center) 包不住它，只能显式留白）。
    let playing = model.playback.playing && frame_count > 1;
    let duration = frame.duration_ms;
    let duration_text = tra("preview.duration", &[("ms", duration.to_string())]);
    ui.add_space(spacing.y);
    ui.horizontal(|ui| {
        // 行内容预估宽：3 枚 32px 图标钮 + 5 处 6px 间距 + 分隔线 1px + 时长文字 + 金点
        let font_id = egui::TextStyle::Small.resolve(ui.style());
        let label_w = ui
            .ctx()
            .fonts(|f| f.layout_delayed_color(duration_text.clone(), font_id, f32::INFINITY))
            .size()
            .x;
        let est = 3.0 * 32.0 + 5.0 * ui.spacing().item_spacing.x + 1.0 + label_w
            + if playing { 16.0 } else { 0.0 };
        let slack = ((ui.available_width() - est) / 2.0).max(0.0);
        ui.add_space(slack);

        let back_tip = format!("{} (←)", tr("preview.step_back"));
        if theme::icon_button(ui, true, icons::glyph::SKIP_PREVIOUS, &back_tip).clicked() {
            actions.push(Action::StepBack);
        }
        let play_tip = format!("{} (Space)", tr("preview.play"));
        let pause_tip = format!("{} (Space)", tr("preview.pause"));
        let can_play = frame_count > 1;
        // 播放（可按下）= 靛蓝主按钮；暂停（播放中）= 幽灵
        let play_clicked = if playing {
            theme::icon_button(ui, can_play, icons::glyph::PAUSE, &pause_tip).clicked()
        } else {
            theme::accent_icon_button(ui, can_play, icons::glyph::PLAY_ARROW, &play_tip).clicked()
        };
        if play_clicked {
            actions.push(Action::PlayPause);
        }
        let fwd_tip = format!("{} (→)", tr("preview.step_forward"));
        if theme::icon_button(ui, true, icons::glyph::SKIP_NEXT, &fwd_tip).clicked() {
            actions.push(Action::StepForward);
        }
        ui.separator();
        ui.label(
            egui::RichText::new(duration_text)
                .small()
                .color(theme::TEXT_DIM),
        );
        if playing {
            // 播放中指示：painter 画金色圆点（● 字形默认字体缺失，见缺字警告）
            let (dot_rect, dot_resp) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
            ui.painter().circle_filled(dot_rect.center(), 3.0, theme::GOLD);
            dot_resp.on_hover_text(tr("preview.playing"));
        }
    });

    // 播放头标记色带（顶部细线指示当前帧位置）
    if frame_count > 1 {
        let progress = view_index as f32 / frame_count as f32;
        let mark = Rect::from_min_max(
            pos2(area.left() + area.width() * progress, area.top() - 4.0),
            pos2(
                (area.left() + area.width() * progress + 3.0).min(area.right()),
                area.top() - 1.0,
            ),
        );
        ui.painter().rect_filled(mark, Rounding::ZERO, theme::GOLD);
    }

    actions
}

/// 空状态：大图标 + 标题 + 灰提示，垂直居中
fn render_empty(ui: &mut Ui, title_key: &str, hint_key: &str, icon: char) {
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.add_space(80.0);
        ui.label(icons::text_sized(icon, 44.0).color(theme::TEXT_FAINT));
        ui.add_space(12.0);
        ui.label(theme::heading(tr(title_key)));
        ui.add_space(4.0);
        ui.label(egui::RichText::new(tr(hint_key)).small().color(theme::TEXT_DIM));
    });
}

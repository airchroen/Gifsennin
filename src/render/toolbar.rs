//! 顶部工具栏：标题 | 文件操作 | 撤销/重做 | 帧操作 | 画布变换 | 更多菜单
//! （全选/语言）| 右侧状态区。
//!
//! 纯视图：按钮产出 Action 或设置 ui_state 局部意图（打开对话框）；
//! 状态区把 `StatusEvent` 翻译成 i18n 文案（含 Error 变体参数化展示）。
//! 图标化（Material Symbols）：文字进 tooltip；翻转对（水平/垂直）字体缺
//! 垂直字形，成对 painter 自绘保持视觉一致。

use crate::model::{Action, Edit, Model, StatusEvent};
use crate::render::i18n::{tr, tra};
use crate::render::{edit_label_key, icons, theme, UiState};
use eframe::egui::{self, Color32, Pos2, Response, Sense, Shape, Stroke, Ui, Vec2};

/// 工具栏分组竖分隔线：默认 separator 的 noninteractive 描边在面板底色上
/// 近乎不可见，这里用稍亮一档的定制线保证分组可读
fn vsep(ui: &mut Ui) {
    ui.add_space(2.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(1.0, 22.0), Sense::hover());
    let c = rect.center();
    ui.painter().line_segment(
        [Pos2::new(c.x, rect.top()), Pos2::new(c.x, rect.bottom())],
        Stroke::new(1.0_f32, Color32::from_rgb(62, 62, 76)), // #3E3E4C
    );
    ui.add_space(2.0);
}

pub fn render_toolbar(ui: &mut Ui, model: &Model, ui_state: &mut UiState) -> Vec<Action> {
    let mut actions = Vec::new();

    let project = model.project.as_ref();
    let has_frames = project.is_some_and(|p| !p.order.is_empty());
    let has_selection = project.is_some_and(|p| !p.selection.is_empty());
    let (can_undo, can_redo, undo_hint, redo_hint) = match project {
        Some(p) => (
            p.history.can_undo(),
            p.history.can_redo(),
            p.history.next_undo_edit().map(edit_label_key).map(tr),
            p.history.next_redo_edit().map(edit_label_key).map(tr),
        ),
        None => (false, false, None, None),
    };

    ui.horizontal(|ui| {
        // ── 标题 ──
        ui.label(icons::text_sized(icons::glyph::MOVIE, 20.0).color(theme::ACCENT));
        ui.label(
            egui::RichText::new(tr("common.app_name"))
                .size(15.0)
                .strong()
                .color(theme::TEXT),
        );
        vsep(ui);

        // ── 文件 ──
        // 忙时禁用打开：防止并发加载绕过单次内存预算（审查修复）
        if theme::icon_button(ui, !model.busy(), icons::glyph::FOLDER_OPEN, &tr("toolbar.open"))
            .clicked()
        {
            actions.push(Action::OpenFileDialog);
        }
        // 导出：只开关对话框（视图局部意图），确认后由对话框发 StartExport
        if theme::icon_button(ui, has_frames, icons::glyph::FILE_EXPORT, &tr("toolbar.export"))
            .clicked()
        {
            ui_state.export_open = true;
        }
        if theme::icon_button(ui, !model.busy(), icons::glyph::DELETE_SWEEP, &tr("toolbar.clear"))
            .clicked()
        {
            actions.push(Action::ClearRequested);
        }
        vsep(ui);

        // ── 撤销 / 重做（编辑器安全网，常驻） ──
        let undo_tip = shortcut_tip(&tr("toolbar.undo"), "Ctrl+Z", undo_hint.as_deref());
        if theme::icon_button(ui, can_undo, icons::glyph::UNDO, &undo_tip).clicked() {
            actions.push(Action::Undo);
        }
        let redo_tip = shortcut_tip(&tr("toolbar.redo"), "Ctrl+Shift+Z", redo_hint.as_deref());
        if theme::icon_button(ui, can_redo, icons::glyph::REDO, &redo_tip).clicked() {
            actions.push(Action::Redo);
        }
        vsep(ui);

        // ── 帧操作 ──
        if theme::icon_button(ui, has_selection, icons::glyph::DELETE, &tr("edit.delete"))
            .clicked()
        {
            let ids = project.map(|p| p.selection_in_order()).unwrap_or_default();
            actions.push(Action::Edit(Edit::DeleteFrames { ids }));
        }
        if theme::icon_button(ui, has_selection, icons::glyph::SWAP_VERT, &tr("edit.reverse"))
            .clicked()
        {
            actions.push(Action::Edit(Edit::ReverseSelected));
        }
        // 旋转/翻转作用于全部帧（共识 Q2 全局语义），只需项目有帧
        if theme::icon_button(ui, has_frames, icons::glyph::ROTATE_LEFT, &tr("edit.rotl"))
            .clicked()
        {
            actions.push(Action::Edit(Edit::RotateLeft));
        }
        if theme::icon_button(ui, has_frames, icons::glyph::ROTATE_RIGHT, &tr("edit.rotr"))
            .clicked()
        {
            actions.push(Action::Edit(Edit::RotateRight));
        }
        if flip_button(ui, has_frames, false, &tr("edit.fliph")).clicked() {
            actions.push(Action::Edit(Edit::FlipH));
        }
        if flip_button(ui, has_frames, true, &tr("edit.flipv")).clicked() {
            actions.push(Action::Edit(Edit::FlipV));
        }
        vsep(ui);

        // ── 画布变换入口（对话框意图为视图局部状态） ──
        let canvas = project.map(|p| p.canvas);
        if theme::icon_button(ui, has_frames, icons::glyph::CROP, &tr("toolbar.crop")).clicked() {
            if let Some(c) = canvas {
                ui_state.open_crop(c);
            }
        }
        if theme::icon_button(
            ui,
            has_frames,
            icons::glyph::ASPECT_RATIO,
            &tr("toolbar.resize"),
        )
        .clicked()
        {
            if let Some(c) = canvas {
                ui_state.open_resize(c);
            }
        }

        // ── 更多：全选 / 语言切换 ──
        egui::menu::menu_button(ui, icons::text(icons::glyph::MORE_VERT), |ui| {
            if ui
                .add_enabled(has_frames, egui::Button::new(tr("toolbar.select_all")))
                .clicked()
            {
                actions.push(Action::SelectAll);
                ui.close_menu();
            }
            ui.separator();
            ui.label(tr("toolbar.lang"));
            let cur = rust_i18n::locale().to_string();
            if ui.selectable_label(cur == "zh-CN", "中文").clicked() && cur != "zh-CN" {
                rust_i18n::set_locale("zh-CN");
            }
            if ui.selectable_label(cur == "en", "English").clicked() && cur != "en" {
                rust_i18n::set_locale("en");
            }
        });

        // ── 状态区（右对齐） ──
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (text, color) = status_text(model);
            ui.label(egui::RichText::new(text).size(13.0).color(color));
        });
    });

    actions
}

/// tooltip 文案：`撤销 (Ctrl+Z) · 裁剪`——操作名 + 快捷键 + 可选的下一编辑类型
fn shortcut_tip(label: &str, key: &str, edit_hint: Option<&str>) -> String {
    match edit_hint {
        Some(h) => format!("{label} ({key}) · {h}"),
        None => format!("{label} ({key})"),
    }
}

/// 手绘水平/垂直翻转图标按钮（Material Symbols 无垂直翻转字形；
/// 轴线 + 背离轴线的镜像三角，两枚成对，风格互相一致）
fn flip_button(ui: &mut Ui, enabled: bool, vertical: bool, tip: &str) -> Response {
    let resp = theme::blank_icon_button(ui, enabled, tip);
    let c = resp.rect.center();
    let col = if !enabled {
        theme::TEXT_FAINT
    } else if resp.hovered() {
        theme::TEXT
    } else {
        theme::TEXT_DIM
    };
    let painter = ui.painter_at(resp.rect);
    if vertical {
        // 水平轴线 + 上下三角
        painter.line_segment(
            [Pos2::new(c.x - 10.0, c.y), Pos2::new(c.x + 10.0, c.y)],
            Stroke::new(1.5_f32, col),
        );
        painter.add(Shape::convex_polygon(
            vec![
                Pos2::new(c.x - 5.0, c.y - 3.0),
                Pos2::new(c.x + 5.0, c.y - 3.0),
                Pos2::new(c.x, c.y - 10.0),
            ],
            col,
            Stroke::NONE,
        ));
        painter.add(Shape::convex_polygon(
            vec![
                Pos2::new(c.x - 5.0, c.y + 3.0),
                Pos2::new(c.x + 5.0, c.y + 3.0),
                Pos2::new(c.x, c.y + 10.0),
            ],
            col,
            Stroke::NONE,
        ));
    } else {
        // 垂直轴线 + 左右三角
        painter.line_segment(
            [Pos2::new(c.x, c.y - 10.0), Pos2::new(c.x, c.y + 10.0)],
            Stroke::new(1.5_f32, col),
        );
        painter.add(Shape::convex_polygon(
            vec![
                Pos2::new(c.x - 3.0, c.y - 5.0),
                Pos2::new(c.x - 3.0, c.y + 5.0),
                Pos2::new(c.x - 10.0, c.y),
            ],
            col,
            Stroke::NONE,
        ));
        painter.add(Shape::convex_polygon(
            vec![
                Pos2::new(c.x + 3.0, c.y - 5.0),
                Pos2::new(c.x + 3.0, c.y + 5.0),
                Pos2::new(c.x + 10.0, c.y),
            ],
            col,
            Stroke::NONE,
        ));
    }
    resp
}

/// StatusEvent → (文案, 颜色)。Error 红，Loading/Exporting 橙，其余灰。
fn status_text(model: &Model) -> (String, Color32) {
    match model.status.as_ref() {
        None | Some(StatusEvent::Ready) => (tr("status.ready"), theme::TEXT_DIM),
        Some(StatusEvent::Loading) => (tr("status.loading"), theme::WARN),
        Some(StatusEvent::Exporting) => (tr("status.exporting"), theme::WARN),
        Some(StatusEvent::DialogCancelled) => (tr("status.dialog_cancelled"), theme::TEXT_DIM),
        Some(StatusEvent::Cleared) => (tr("status.cleared"), theme::TEXT_DIM),
        Some(StatusEvent::Loaded {
            frames,
            file_bytes,
            mem_bytes,
        }) => (
            tra(
                "status.loaded",
                &[
                    ("frames", frames.to_string()),
                    ("file_bytes", fmt_bytes(*file_bytes)),
                    ("mem_bytes", fmt_bytes(*mem_bytes)),
                ],
            ),
            theme::TEXT_DIM,
        ),
        Some(StatusEvent::ExportDone { path, bytes, ms }) => (
            tra(
                "status.export_done",
                &[
                    ("path", path.display().to_string()),
                    ("bytes", fmt_bytes(*bytes)),
                    ("ms", ms.to_string()),
                ],
            ),
            theme::OK,
        ),
        Some(StatusEvent::Error(e)) => {
            let key = e.i18n_key();
            let text = match e {
                crate::errors::AppError::UnsupportedFormat(ext) => tra(key, &[("0", ext.clone())]),
                crate::errors::AppError::TooLarge { needed, budget } => {
                    tra(key, &[("0", fmt_bytes(*needed)), ("1", fmt_bytes(*budget))])
                }
                other => format!("{}: {other}", tr(key)),
            };
            (text, theme::ERR)
        }
    }
}

/// 字节数人性化（B/KB/MB/GB）。修复：原实现选对了单位却打印原始字节数，
/// 1.25 MB 显示成 1315873.0 MB（存量 bug，状态栏/侧栏由此统一用这一份）
pub(crate) fn fmt_bytes(b: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let b = b as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{b:.0} B")
    }
}

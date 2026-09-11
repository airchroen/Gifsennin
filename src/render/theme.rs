// 现代轻量风暗色主题（共识：Figma/Linear 参照，仅暗色）。
// 色板与间距常量集中于此；render 各处的散落色值一律引用这里，
// 将来扩展亮色主题只需在此并列一套色板 + apply_light。

use crate::render::icons;
use eframe::egui::{self, Color32, Context, Response, Stroke, Ui, Vec2};

// ── 色板 ────────────────────────────────────────────────────────
// 微冷灰三档底（带一丝蓝调，暗色下比纯灰更耐看）+ 靛蓝强调 + 金色播放头。

/// 最深：中央画布区（内容优先，深底衬托画面）
pub const BG_BASE: Color32 = Color32::from_rgb(16, 16, 20); // #101014
/// 工具栏 / 侧栏面板底
pub const BG_PANEL: Color32 = Color32::from_rgb(24, 24, 30); // #18181E
/// 浮层：卡片 / 常规按钮 / 对话框窗口
pub const BG_SURF: Color32 = Color32::from_rgb(32, 32, 40); // #202028
/// 悬停浮层
pub const BG_HOVER: Color32 = Color32::from_rgb(44, 44, 55); // #2C2C37
/// 分隔线 / 窗口描边
pub const BORDER: Color32 = Color32::from_rgb(46, 46, 58); // #2E2E3A

/// 靛蓝强调（选中 / 激活 / 主按钮）
pub const ACCENT: Color32 = Color32::from_rgb(110, 121, 239); // #6E79EF
/// 强调色 hover 亮一档
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(140, 150, 247); // #8C96F7
/// 播放头语义色（区别于选中靛蓝，现状语义保留）
pub const GOLD: Color32 = Color32::from_rgb(255, 200, 60);

pub const TEXT: Color32 = Color32::from_rgb(230, 231, 236);
pub const TEXT_DIM: Color32 = Color32::from_rgb(155, 157, 170);
/// 禁用态图标/文字
pub const TEXT_FAINT: Color32 = Color32::from_rgb(105, 107, 120);

/// 状态区语义色
pub const ERR: Color32 = Color32::from_rgb(255, 111, 111);
pub const WARN: Color32 = Color32::from_rgb(255, 170, 90);
pub const OK: Color32 = Color32::from_rgb(120, 220, 130);

/// 选中帧卡片底（靛蓝低透明度手绘用）
pub const SEL_BG: Color32 = Color32::from_rgba_premultiplied(110, 121, 239, 38);

/// 图标按钮统一尺寸（含内边距后的最小占位）
const ICON_BTN: Vec2 = Vec2::new(32.0, 30.0);

/// WidgetVisuals 简写构造（圆角统一 7px）
fn wv(bg: Color32, weak_bg: Color32, fg: Color32) -> egui::style::WidgetVisuals {
    egui::style::WidgetVisuals {
        bg_fill: bg,
        weak_bg_fill: weak_bg,
        bg_stroke: Stroke::NONE,
        fg_stroke: Stroke::new(1.0_f32, fg),
        rounding: egui::Rounding::same(7.0),
        expansion: 0.0,
    }
}

// ── 全局装配 ────────────────────────────────────────────────────

/// 应用主题（fonts::install 之后调用一次）
pub fn apply(ctx: &Context) {
    let mut style = egui::Style::default();
    let v = &mut style.visuals;

    // 密度「适中」档：控件间距 6×8，按钮内边距 7×4
    style.spacing.item_spacing = egui::vec2(6.0, 8.0);
    style.spacing.button_padding = egui::vec2(7.0, 4.0);

    v.panel_fill = BG_PANEL;
    v.window_fill = BG_SURF;
    v.extreme_bg_color = Color32::from_rgb(12, 12, 16); // 滑条槽/下拉底
    v.faint_bg_color = BG_HOVER;
    v.window_stroke = Stroke::new(1.0_f32, BORDER);
    v.window_rounding = egui::Rounding::same(12.0);
    v.menu_rounding = egui::Rounding::same(10.0);
    v.selection.bg_fill = Color32::from_rgba_premultiplied(110, 121, 239, 60);
    v.selection.stroke = Stroke::new(1.5_f32, ACCENT);
    v.hyperlink_color = ACCENT;

    // 非交互元素（分隔线走 noninteractive.bg_stroke）
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT_DIM);

    // 常规控件（对话框/菜单按钮）：浮层底色，hover 浮出，active 靛蓝
    // weak_bg_fill 供复选框未勾选态用（比 bg 亮一档，勾选前后有对比）
    v.widgets.inactive = wv(BG_SURF, BG_HOVER, TEXT);
    v.widgets.hovered = wv(BG_HOVER, BG_HOVER, TEXT);
    v.widgets.active = wv(ACCENT, ACCENT_SOFT, Color32::WHITE);
    v.widgets.open = wv(BG_HOVER, BG_HOVER, TEXT);

    ctx.set_style(style);
}

/// 顶部/侧边面板框（面板底色 + 发丝分隔边）
pub fn panel_frame() -> egui::Frame {
    egui::Frame {
        fill: BG_PANEL,
        stroke: Stroke::new(1.0_f32, BORDER),
        inner_margin: egui::Margin::same(8.0),
        ..Default::default()
    }
}

/// 中央画布框（最深底，无边框）
pub fn central_frame() -> egui::Frame {
    egui::Frame {
        fill: BG_BASE,
        ..Default::default()
    }
}

/// 面板小节标题（「预览」「帧列表」等）
pub fn heading(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).size(13.0).strong().color(TEXT)
}

// ── 按钮助手 ────────────────────────────────────────────────────

/// 在局部 Ui 上套用幽灵按钮样式（无底色、悬停浮出、按下靛蓝染底），
/// 用完恢复——不影响同面板里其他常规控件。
fn ghost_scope<R>(ui: &mut Ui, f: impl FnOnce(&mut Ui) -> R) -> R {
    let saved = ui.style().clone();
    {
        let v = ui.visuals_mut();
        v.widgets.inactive = wv(Color32::TRANSPARENT, BG_HOVER, TEXT_DIM);
        v.widgets.hovered = wv(BG_HOVER, BG_HOVER, TEXT);
        v.widgets.active = wv(
            Color32::from_rgba_premultiplied(110, 121, 239, 60),
            ACCENT_SOFT,
            Color32::WHITE,
        );
        v.widgets.open = wv(BG_HOVER, BG_HOVER, TEXT);
    }
    let r = f(ui);
    ui.set_style(saved);
    r
}

/// 幽灵图标按钮：无底色 + 悬停浮出，统一 32×30 占位；禁用时也显示 tooltip。
pub fn icon_button(ui: &mut Ui, enabled: bool, ch: char, tip: &str) -> Response {
    ghost_scope(ui, |ui| {
        let mut text = icons::text(ch);
        if !enabled {
            text = text.color(TEXT_FAINT); // 幽灵态禁用无底色对比，显式降档
        }
        ui.add_enabled(enabled, egui::Button::new(text).min_size(ICON_BTN))
            .on_hover_text(tip)
            .on_disabled_hover_text(tip)
    })
}

/// 强调图标按钮（播放等主操作）：靛蓝实底 + 白图标。
pub fn accent_icon_button(ui: &mut Ui, enabled: bool, ch: char, tip: &str) -> Response {
    let saved = ui.style().clone();
    {
        let v = ui.visuals_mut();
        v.widgets.inactive = wv(ACCENT, ACCENT_SOFT, Color32::WHITE);
        v.widgets.hovered = wv(ACCENT_SOFT, ACCENT_SOFT, Color32::WHITE);
        v.widgets.active = wv(ACCENT_SOFT, ACCENT_SOFT, Color32::WHITE);
        v.widgets.open = wv(ACCENT_SOFT, ACCENT_SOFT, Color32::WHITE);
    }
    let mut text = icons::text(ch);
    if !enabled {
        text = text.color(Color32::from_rgba_premultiplied(255, 255, 255, 140));
    }
    let resp = ui
        .add_enabled(enabled, egui::Button::new(text).min_size(ICON_BTN))
        .on_hover_text(tip)
        .on_disabled_hover_text(tip);
    ui.set_style(saved);
    resp
}

/// 与 icon_button 同尺寸的空白幽灵按钮（供 painter 叠绘自定义图标）。
/// 占位字符设为全透明：不依赖字体里的空格字形，也不响应态变色。
pub fn blank_icon_button(ui: &mut Ui, enabled: bool, tip: &str) -> Response {
    ghost_scope(ui, |ui| {
        let text = icons::text(icons::glyph::MORE_VERT).color(Color32::TRANSPARENT);
        ui.add_enabled(enabled, egui::Button::new(text).min_size(ICON_BTN))
            .on_hover_text(tip)
            .on_disabled_hover_text(tip)
    })
}

/// 主操作文字按钮（对话框「导出/应用/清空」）：靛蓝实底。
pub fn accent_button(ui: &mut Ui, label: impl Into<egui::WidgetText>) -> Response {
    let saved = ui.style().clone();
    {
        let v = ui.visuals_mut();
        v.widgets.inactive = wv(ACCENT, ACCENT_SOFT, Color32::WHITE);
        v.widgets.hovered = wv(ACCENT_SOFT, ACCENT_SOFT, Color32::WHITE);
        v.widgets.active = wv(ACCENT_SOFT, ACCENT_SOFT, Color32::WHITE);
        v.widgets.open = wv(ACCENT_SOFT, ACCENT_SOFT, Color32::WHITE);
    }
    let resp = ui.button(label);
    ui.set_style(saved);
    resp
}

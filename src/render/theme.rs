//! 现代轻量风暗色主题（参照 Figma/Linear，仅暗色）。
//! 色板与间距常量集中于此；render 各处的散落色值一律引用这里，
//! 将来扩展亮色主题只需在此并列一套色板 + apply_light。

use crate::render::icons;
use eframe::egui::{self, Color32, Context, Response, RichText, Sense, Stroke, Ui, Vec2};

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
/// 悬停态描边（卡片 / 控件 hover 时比 BORDER 亮一档）
pub const BORDER_HOVER: Color32 = Color32::from_rgb(58, 58, 72);
/// 工具栏分组竖分隔线
pub const VSEP: Color32 = Color32::from_rgb(62, 62, 76); // #3E3E4C

/// 靛蓝强调（选中 / 激活 / 主按钮）
pub const ACCENT: Color32 = Color32::from_rgb(110, 121, 239); // #6E79EF
/// 强调色 hover 亮一档
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(140, 150, 247); // #8C96F7
/// 强调色低透明度染底（幽灵按钮按下态 / 文本选区 / 拖拽中卡片）
pub const ACCENT_DIM: Color32 = Color32::from_rgba_premultiplied(110, 121, 239, 60);
/// 播放头语义色（与选中用的靛蓝区分）
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
    v.selection.bg_fill = ACCENT_DIM;
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

/// 交互控件四态样式：[未交互, 悬停, 按下, 打开]
type WidgetStyles = [egui::style::WidgetVisuals; 4];

/// 临时替换交互控件四态样式，执行 `f` 后恢复——不影响同面板里其他常规控件。
fn with_widget_styles<R>(ui: &mut Ui, styles: WidgetStyles, f: impl FnOnce(&mut Ui) -> R) -> R {
    let saved = ui.style().clone();
    let widgets = &mut ui.visuals_mut().widgets;
    [
        widgets.inactive,
        widgets.hovered,
        widgets.active,
        widgets.open,
    ] = styles;
    let r = f(ui);
    ui.set_style(saved);
    r
}

/// 幽灵按钮：无底色、悬停浮出、按下靛蓝染底
fn ghost_styles() -> WidgetStyles {
    [
        wv(Color32::TRANSPARENT, BG_HOVER, TEXT_DIM),
        wv(BG_HOVER, BG_HOVER, TEXT),
        wv(ACCENT_DIM, ACCENT_SOFT, Color32::WHITE),
        wv(BG_HOVER, BG_HOVER, TEXT),
    ]
}

/// 强调按钮：靛蓝实底 + 白色前景
fn accent_styles() -> WidgetStyles {
    [
        wv(ACCENT, ACCENT_SOFT, Color32::WHITE),
        wv(ACCENT_SOFT, ACCENT_SOFT, Color32::WHITE),
        wv(ACCENT_SOFT, ACCENT_SOFT, Color32::WHITE),
        wv(ACCENT_SOFT, ACCENT_SOFT, Color32::WHITE),
    ]
}

/// 图标按钮骨架：统一 32×30 占位，`styles` 只作用于本次调用；禁用时也显示 tooltip。
fn icon_button_styled(
    ui: &mut Ui,
    styles: WidgetStyles,
    enabled: bool,
    text: RichText,
    tip: &str,
) -> Response {
    with_widget_styles(ui, styles, |ui| {
        ui.add_enabled(enabled, egui::Button::new(text).min_size(ICON_BTN))
            .on_hover_text(tip)
            .on_disabled_hover_text(tip)
    })
}

/// 幽灵图标按钮：无底色 + 悬停浮出。
pub fn icon_button(ui: &mut Ui, enabled: bool, ch: char, tip: &str) -> Response {
    let mut text = icons::text(ch);
    if !enabled {
        text = text.color(TEXT_FAINT); // 幽灵态禁用无底色对比，显式降档
    }
    icon_button_styled(ui, ghost_styles(), enabled, text, tip)
}

/// 强调图标按钮：靛蓝实底 + 白图标。
pub fn accent_icon_button(ui: &mut Ui, enabled: bool, ch: char, tip: &str) -> Response {
    let mut text = icons::text(ch);
    if !enabled {
        text = text.color(Color32::from_rgba_premultiplied(255, 255, 255, 140));
    }
    icon_button_styled(ui, accent_styles(), enabled, text, tip)
}

/// 与 icon_button 同尺寸的空白幽灵按钮（供 painter 叠绘自定义图标）。
/// 占位字符设为全透明：不依赖字体里的空格字形，也不响应态变色。
pub fn blank_icon_button(ui: &mut Ui, enabled: bool, tip: &str) -> Response {
    let text = icons::text(icons::glyph::MORE_VERT).color(Color32::TRANSPARENT);
    icon_button_styled(ui, ghost_styles(), enabled, text, tip)
}

/// 主操作文字按钮（对话框「导出/应用/清空」）：靛蓝实底。
pub fn accent_button(ui: &mut Ui, label: impl Into<egui::WidgetText>) -> Response {
    with_widget_styles(ui, accent_styles(), |ui| ui.button(label))
}

/// 播放头金色圆点（● 字形默认字体缺失，改用 painter 绘制）
pub fn playhead_dot(ui: &mut Ui, tip: &str) {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 3.0, GOLD);
    resp.on_hover_text(tip);
}

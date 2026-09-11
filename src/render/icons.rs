// Material Symbols Rounded 图标字体（Apache 2.0，resources/fonts/ 下已
// 实例化为 FILL=0/wght=400 静态轮廓并子集化到所用图标，~5KB）。
// 独立 FontFamily 注册；码点常量按语义命名，渲染走 icons::text()。

use eframe::egui::{FontFamily, RichText};
use std::sync::Arc;

pub const FONT_BYTES: &[u8] =
    include_bytes!("../../resources/fonts/MaterialSymbolsRounded-subset.ttf");

/// 字体家族句柄（Arc 分配，全局一次）
static FAMILY: std::sync::LazyLock<FontFamily> =
    std::sync::LazyLock::new(|| FontFamily::Name(Arc::from("MaterialSymbols")));

/// 图标码点（与 resources/fonts 子集一一对应，改图标先补子集）
pub mod glyph {
    /// 打开文件
    pub const FOLDER_OPEN: char = '\u{e2c8}';
    /// 清空文档
    pub const DELETE_SWEEP: char = '\u{e16c}';
    /// 导出
    pub const FILE_EXPORT: char = '\u{f3b2}';
    pub const UNDO: char = '\u{e166}';
    pub const REDO: char = '\u{e15a}';
    /// 删除所选帧
    pub const DELETE: char = '\u{e92e}';
    /// 反转所选（帧序列上下交换）
    pub const SWAP_VERT: char = '\u{e8d5}';
    pub const ROTATE_LEFT: char = '\u{e419}';
    pub const ROTATE_RIGHT: char = '\u{e41a}';
    pub const CROP: char = '\u{e3be}';
    /// 缩放（宽高比框）
    pub const ASPECT_RATIO: char = '\u{e85b}';
    pub const MORE_VERT: char = '\u{e5d4}';
    pub const SELECT_ALL: char = '\u{e162}';
    pub const LANGUAGE: char = '\u{ea07}';
    pub const SKIP_PREVIOUS: char = '\u{e045}';
    pub const PLAY_ARROW: char = '\u{e037}';
    pub const PAUSE: char = '\u{e034}';
    pub const SKIP_NEXT: char = '\u{e044}';
    /// 应用标题 / 空状态
    pub const MOVIE: char = '\u{e404}';
    pub const ERROR: char = '\u{f8b6}';
    pub const CHECK_CIRCLE: char = '\u{f0be}';
    /// 帧卡片拖拽手柄
    pub const DRAG_INDICATOR: char = '\u{e945}';
}

pub fn family() -> FontFamily {
    FAMILY.clone()
}

/// 默认 20px 图标文本。不设颜色：让按钮按 hover/active 态取前景色。
pub fn text(ch: char) -> RichText {
    RichText::new(ch).size(20.0).family(family())
}

/// 指定字号的图标文本（空状态大图标等）
pub fn text_sized(ch: char, size: f32) -> RichText {
    RichText::new(ch).size(size).family(family())
}

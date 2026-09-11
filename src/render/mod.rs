// render 层（共识 Q5）：纯视图——读 Model，产出 Action。
// 不改模型状态；所有交互意图以 Action 返回给 app 层统一分发。

pub mod crop_overlay;
pub mod dialogs;
pub mod fonts;
pub mod i18n;
pub mod icons;
pub mod preview;
pub mod sidebar;
pub mod textures;
pub mod theme;
pub mod thumbnails;
pub mod toolbar;

pub use i18n::tr;
pub use textures::TextureCache;

use crate::model::{ExportFormat, GifQuality, ResizeFilter};
use std::time::Instant;

/// UI 侧瞬态状态（不进 Model：关闭即失，不参与 undo）。
/// 对话框开合、拖拽草稿、防抖时间戳都住在这里。
pub struct UiState {
    /// 清空确认弹窗
    pub confirm_clear: bool,
    /// 导出对话框
    pub export_open: bool,
    /// 导出对话框页签：0=GIF 1=WebP 2=PNG
    pub export_tab: usize,
    pub gif_quality: GifQuality,
    pub webp_quality: f32,
    pub webp_lossless: bool,
    /// 已发送估算的 (格式参数, 发送时刻)——参数变更后 300ms 防抖再发新估算
    pub estimate_sent: Option<(ExportFormat, Instant)>,
    /// 最近一次参数变更时刻（真防抖基准：静默 ≥300ms 才发送；
    /// 审查修复：原先以「上次发送」为基准，拖动滑条期间每 300ms 泛滥触发）
    pub estimate_changed: Option<Instant>,
    /// 估算代次计数器（与 Model.estimate.generation 对齐；跨文档单调不重置）
    pub estimate_gen: u64,
    /// 脏文档时打开新文件的暂存路径（确认框同意后继续加载）
    pub pending_open: Option<std::path::PathBuf>,
    /// 缩放对话框
    pub resize_open: bool,
    pub resize_w: u32,
    pub resize_h: u32,
    pub resize_lock_aspect: bool,
    pub resize_filter: ResizeFilter,
    /// 裁剪草稿（Some = 裁剪模式激活，preview 切换到 crop_overlay 渲染）
    pub crop: Option<CropDraft>,
}

/// 裁剪草稿：UI 拖拽/数值输入的临时矩形，确认才提交 Edit::Crop
#[derive(Debug, Clone, Copy)]
pub struct CropDraft {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl UiState {
    pub fn new() -> Self {
        Self {
            confirm_clear: false,
            export_open: false,
            export_tab: 0,
            gif_quality: GifQuality::Balanced,
            webp_quality: 80.0,
            webp_lossless: false,
            estimate_sent: None,
            estimate_changed: None,
            estimate_gen: 0,
            pending_open: None,
            resize_open: false,
            resize_w: 0,
            resize_h: 0,
            resize_lock_aspect: true,
            resize_filter: ResizeFilter::CatmullRom,
            crop: None,
        }
    }

    /// 打开缩放对话框时按画布初始化
    pub fn open_resize(&mut self, canvas: (u32, u32)) {
        self.resize_w = canvas.0;
        self.resize_h = canvas.1;
        self.resize_lock_aspect = true;
        self.resize_filter = ResizeFilter::CatmullRom;
        self.resize_open = true;
    }

    /// 打开裁剪模式：初始矩形 = 整个画布
    pub fn open_crop(&mut self, canvas: (u32, u32)) {
        self.crop = Some(CropDraft {
            x: 0,
            y: 0,
            w: canvas.0,
            h: canvas.1,
        });
    }

    /// 当前导出对话框所选格式的参数快照
    pub fn current_export_format(&self) -> ExportFormat {
        match self.export_tab {
            0 => ExportFormat::Gif {
                quality: self.gif_quality,
            },
            1 => ExportFormat::Webp {
                quality: self.webp_quality,
                lossless: self.webp_lossless,
            },
            _ => ExportFormat::PngZip,
        }
    }
}

impl Default for UiState {
    fn default() -> Self {
        Self::new()
    }
}

/// Edit 变体 → undo/redo/上下文菜单显示用的 i18n 标签键
pub fn edit_label_key(edit: &crate::model::Edit) -> &'static str {
    use crate::model::Edit;
    match edit {
        Edit::DeleteFrames { .. } => "edit.delete",
        Edit::ReverseSelected => "edit.reverse",
        Edit::Reorder { .. } => "edit.reorder",
        Edit::SetDuration { .. } => "edit.duration",
        Edit::SetLoop { .. } => "edit.loop",
        Edit::Crop { .. } => "edit.crop",
        Edit::Resize { .. } => "edit.resize",
        Edit::RotateLeft => "edit.rotl",
        Edit::RotateRight => "edit.rotr",
        Edit::FlipH => "edit.fliph",
        Edit::FlipV => "edit.flipv",
    }
}

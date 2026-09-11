use super::frame::FrameId;
use super::transform::ResizeFilter;
use crate::errors::AppError;
use std::path::PathBuf;
use std::sync::Arc;

use super::frame::Frame;

/// 可撤销的编辑命令（共识 Q3）。enum + match 穷尽分发——
/// 新增命令漏处理时编译器直接报错。
#[derive(Debug, Clone)]
pub enum Edit {
    /// 删除指定帧（像素数据留在仓库供 undo）
    DeleteFrames {
        ids: Vec<FrameId>,
    },
    /// 反转选中帧在序列中的相对顺序
    ReverseSelected,
    /// 把 `moved`（保持相对顺序）移动到 `before` 之前；None = 移到末尾
    Reorder {
        moved: Vec<FrameId>,
        before: Option<FrameId>,
    },
    /// 设置指定帧时长 ms
    SetDuration {
        ids: Vec<FrameId>,
        ms: u32,
    },
    /// 循环模式（true = 无限循环）。可撤销的文档属性
    /// （审查修复：原先在历史外直接改，会被无关撤销静默回滚）
    SetLoop {
        infinite: bool,
    },
    /// 全局裁剪（x/y/w/h 为画布坐标）
    Crop {
        x: u32,
        y: u32,
        w: u32,
        h: u32,
    },
    /// 全局缩放
    Resize {
        width: u32,
        height: u32,
        filter: ResizeFilter,
    },
    RotateLeft,
    RotateRight,
    FlipH,
    FlipV,
}

/// GIF 编码质量三档（共识 Q8）：内部映射 NeuQuant speed 参数
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GifQuality {
    High,
    Balanced,
    Fast,
}

/// 导出格式与参数（共识 Q8）
#[derive(Debug, Clone, PartialEq)]
pub enum ExportFormat {
    Gif { quality: GifQuality },
    Webp { quality: f32, lossless: bool },
    PngZip,
}

impl ExportFormat {
    /// 默认文件扩展名
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Gif { .. } => "gif",
            Self::Webp { .. } => "webp",
            Self::PngZip => "zip",
        }
    }
}

/// 导出结果
#[derive(Debug, Clone)]
pub struct ExportOutcome {
    pub path: PathBuf,
    pub bytes: u64,
    pub ms: u64,
}

/// 体积估算状态（共识 Q8-c：worker 试编码 + 防抖，过期代次结果丢弃）
#[derive(Debug, Default)]
pub struct EstimateState {
    /// 单调递增代次；EstimateReady 携带的代次落后于当前即丢弃。
    /// 代次跨文档保持单调（文档切换时保留计数，见 Model::apply）
    pub generation: u64,
    /// 是否有估算 worker 在途（单飞：在途时不再派发新估算，
    /// 结果落地后对话框按需重发——审查修复线程堆积）
    pub in_flight: bool,
    pub result: Option<Result<u64, AppError>>,
}

/// UI → 模型的唯一入口（共识 Q5 Action-MVU）。
/// 渲染函数只产出 Action，`Model::apply` 穷尽处理并返回需执行的副作用。
/// 不 derive Debug/Clone：负载含 Project（移动语义，跨线程单次投递）。
pub enum Action {
    // ── 文件 IO ─────────────────────────────────────────────
    OpenFileDialog,
    FilePicked(Option<PathBuf>),
    LoadFinished(Result<super::Project, AppError>),
    /// 工具栏「清空」；app 层在 dirty 时先弹确认框（见 app.rs dispatch）
    ClearRequested,
    ClearConfirmed,

    // ── 选择与播放 ──────────────────────────────────────────
    /// 单击：单选
    SelectFrame(FrameId),
    /// Ctrl+单击：切换选中
    ToggleSelect(FrameId),
    /// Shift+单击：从锚点到该帧的范围选择
    RangeSelect(FrameId),
    SelectAll,
    ClearSelection,
    PlayPause,
    StepForward,
    StepBack,

    // ── 编辑（经 history 记录） ─────────────────────────────
    Edit(Edit),
    Undo,
    Redo,

    // ── 导出 ────────────────────────────────────────────────
    /// 导出对话框「导出」按钮：暂存格式，等待另存为路径
    StartExport(ExportFormat),
    SavePathPicked(Option<PathBuf>),
    ExportFinished(Result<ExportOutcome, AppError>),
    EstimateRequested {
        generation: u64,
        format: ExportFormat,
    },
    EstimateReady {
        generation: u64,
        result: Result<u64, AppError>,
    },

    // ── 文档属性 ────────────────────────────────────────────
    /// true = 无限循环（loop_count = 0）
    SetLoopInfinite(bool),
}

/// `Model::apply` 返回的副作用，由 app 层执行（线程/系统对话框）。
/// 模型保持纯函数，可无头测试。
#[derive(Debug)]
pub enum Effect {
    SpawnOpenDialog,
    SpawnSaveDialog {
        default_name: String,
    },
    SpawnLoad(PathBuf),
    SpawnExport {
        frames: Vec<Arc<Frame>>,
        loop_count: u16,
        format: ExportFormat,
        path: PathBuf,
    },
    SpawnEstimate {
        generation: u64,
        frames: Vec<Arc<Frame>>,
        loop_count: u16,
        format: ExportFormat,
    },
}

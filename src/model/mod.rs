//! model 层：纯数据 + 纯函数，无 GUI 依赖，可无头测试。
//! 所有状态迁移必须经 `Model::apply(Action) -> Vec<Effect>` 单一入口。

mod actions;
mod frame;
mod frame_store;
mod history;
mod transform;

pub use actions::{Action, Edit, Effect, EstimateState, ExportFormat, ExportOutcome, GifQuality};
pub use frame::{Frame, FrameId};
pub use frame_store::FrameStore;
pub use history::{History, Snapshot, MAX_HISTORY};
pub use transform::{clamp_crop, ResizeFilter};

use crate::codec::DecodedAnimation;
use crate::errors::AppError;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

/// 内存预算护栏：帧 RGBA 数据总量超过此值即拒绝加载。
pub const MEMORY_BUDGET_BYTES: u64 = 1_610_612_736; // 1.5 GiB

/// 播放状态下限：0ms 帧按 16ms 播放（浏览器对 0 延迟 GIF 的常见处理）
pub const MIN_PLAYBACK_MS: u32 = 16;

/// 内存预算判定：`needed` 超预算即返回 [`AppError::TooLarge`]。
pub fn check_memory_budget(needed: u64) -> Result<(), AppError> {
    if needed > MEMORY_BUDGET_BYTES {
        Err(AppError::TooLarge {
            needed,
            budget: MEMORY_BUDGET_BYTES,
        })
    } else {
        Ok(())
    }
}

// ─── 播放 ───────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct Playback {
    pub playing: bool,
    /// 累积未消耗的时间（ms），由 tick 按当前帧时长逐帧扣除
    acc_ms: f64,
}

// ─── 状态栏事件（render 层翻译显示） ────────────────────────────

pub enum StatusEvent {
    Ready,
    Loading,
    Exporting,
    DialogCancelled,
    Cleared,
    Loaded {
        frames: usize,
        file_bytes: u64,
        mem_bytes: u64,
    },
    ExportDone {
        path: PathBuf,
        bytes: u64,
        ms: u64,
    },
    Error(AppError),
}

// ─── 项目（当前打开的文档） ─────────────────────────────────────

pub struct Project {
    pub source: PathBuf,
    /// 源文件大小；加载时读一次缓存（每帧 fs::metadata 在网络路径上会阻塞渲染线程）
    pub file_size: u64,
    pub canvas: (u32, u32),
    /// GIF 语义：0 = 无限循环
    pub loop_count: u16,
    /// 帧序列（身份列表）
    pub order: Vec<FrameId>,
    /// 选中集，按 id 升序去重
    pub selection: Vec<FrameId>,
    /// Shift 范围选择锚点
    pub anchor: Option<FrameId>,
    /// 播放头（order 下标）
    pub view_index: usize,
    pub store: FrameStore,
    pub history: History,
}

impl Project {
    /// 由解码结果构建项目。codec 层各解码器已保证不超预算，此处复核兜底。
    pub fn from_decoded(source: PathBuf, anim: DecodedAnimation) -> Result<Self, AppError> {
        if anim.frames.is_empty() {
            return Err(AppError::Decode("no frames decoded".into()));
        }
        let total: u64 = anim.frames.iter().map(|f| f.rgba.len() as u64).sum();
        check_memory_budget(total)?;
        let mut store = FrameStore::new();
        let mut order = Vec::with_capacity(anim.frames.len());
        for f in anim.frames {
            let frame = Frame::from_rgba(f.rgba, anim.width, anim.height, f.duration_ms)
                .ok_or_else(|| AppError::Decode("frame buffer size mismatch".into()))?;
            order.push(store.insert(frame));
        }
        let first = order.first().copied();
        let file_size = std::fs::metadata(&source).map_or(0, |m| m.len());
        Ok(Self {
            source,
            file_size,
            canvas: (anim.width, anim.height),
            loop_count: anim.loop_count,
            order,
            selection: first.iter().copied().collect(),
            anchor: first,
            view_index: 0,
            store,
            history: History::new(),
        })
    }

    pub fn frame_count(&self) -> usize {
        self.order.len()
    }

    pub fn frame_at(&self, index: usize) -> Option<Arc<Frame>> {
        self.order.get(index).and_then(|id| self.store.get(*id))
    }

    /// 播放头当前帧
    pub fn current_frame(&self) -> Option<Arc<Frame>> {
        self.frame_at(self.view_index.min(self.order.len().saturating_sub(1)))
    }

    /// 播放头当前帧的有效时长（ms）：过短时长按 MIN_PLAYBACK_MS 下限，无帧时取下限
    fn playback_duration_ms(&self) -> f64 {
        let fallback = f64::from(MIN_PLAYBACK_MS);
        self.current_frame()
            .map_or(fallback, |f| f64::from(f.duration_ms.max(MIN_PLAYBACK_MS)))
    }

    pub fn index_of(&self, id: FrameId) -> Option<usize> {
        self.order.iter().position(|&x| x == id)
    }

    /// 当前序列的全部帧（与 `order` 一一对应）
    fn frames(&self) -> Vec<Arc<Frame>> {
        self.order
            .iter()
            .filter_map(|id| self.store.get(*id))
            .collect()
    }

    /// 单选 `id` 并把播放头移过去
    fn select_single(&mut self, id: FrameId) {
        self.selection = vec![id];
        self.anchor = Some(id);
        self.view_index = self.index_of(id).unwrap_or(self.view_index);
    }

    pub fn selection_set(&self) -> HashSet<FrameId> {
        self.selection.iter().copied().collect()
    }

    /// 选中帧的顺序快照（按 order 中出现顺序）
    pub fn selection_in_order(&self) -> Vec<FrameId> {
        let sel = self.selection_set();
        self.order
            .iter()
            .copied()
            .filter(|id| sel.contains(id))
            .collect()
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            canvas: self.canvas,
            loop_count: self.loop_count,
            order: self.order.clone(),
            selection: self.selection.clone(),
            view_index: self.view_index,
        }
    }

    pub fn restore(&mut self, s: Snapshot) {
        self.canvas = s.canvas;
        self.loop_count = s.loop_count;
        self.order = s.order;
        self.selection = s.selection;
        self.view_index = s.view_index;
        self.clamp_indices();
    }

    /// 修复索引/选中集在编辑后的越界与失效
    pub fn clamp_indices(&mut self) {
        if self.order.is_empty() {
            self.view_index = 0;
            self.selection.clear();
            self.anchor = None;
            return;
        }
        self.view_index = self.view_index.min(self.order.len() - 1);
        let live: HashSet<FrameId> = self.order.iter().copied().collect();
        self.selection.retain(|id| live.contains(id));
        self.selection.sort_unstable();
        self.selection.dedup();
        self.anchor = self.anchor.filter(|a| live.contains(a));
    }

    /// 回收既不在当前序列、也不被任何历史快照引用的帧
    pub fn gc(&mut self) {
        let refs = self.history.referenced_ids(&self.order);
        self.store.retain(&refs);
    }

    /// old→new 局部映射迁移 selection/anchor（顺序按映射重建）
    fn remap_ids(&mut self, map: &HashMap<FrameId, FrameId>) {
        if map.is_empty() {
            return;
        }
        let map_id = |id: &FrameId| map.get(id).copied().unwrap_or(*id);
        self.order = self.order.iter().map(map_id).collect();
        self.selection = self.selection.iter().map(map_id).collect();
        self.selection.sort_unstable();
        self.selection.dedup();
        self.anchor = self.anchor.as_ref().map(map_id);
    }

    /// 对全部帧施加 `transform`，用结果（全新 id）替换当前序列，
    /// 并把 selection/anchor 迁移到新 id（画布变换路径）
    fn replace_all_frames(&mut self, transform: impl FnOnce(&[Arc<Frame>]) -> Vec<Frame>) {
        let frames = self.frames();
        let new_frames = transform(&frames);
        let old_order = std::mem::take(&mut self.order);
        let mut map = HashMap::with_capacity(old_order.len());
        let mut new_order = Vec::with_capacity(new_frames.len());
        for (old_id, f) in old_order.into_iter().zip(new_frames) {
            let nid = self.store.insert(f);
            map.insert(old_id, nid);
            new_order.push(nid);
        }
        self.order = new_order;
        self.remap_ids(&map);
    }
}

// ─── 模型 ───────────────────────────────────────────────────────

pub struct Model {
    pub project: Option<Project>,
    pub playback: Playback,
    pub status: Option<StatusEvent>,
    /// StartExport 后暂存的格式，等待另存为路径
    pub pending_export: Option<ExportFormat>,
    pub estimate: EstimateState,
}

impl Default for Model {
    fn default() -> Self {
        Self::new()
    }
}

impl Model {
    pub fn new() -> Self {
        Self {
            project: None,
            playback: Playback::default(),
            status: Some(StatusEvent::Ready),
            pending_export: None,
            estimate: EstimateState::default(),
        }
    }

    pub fn dirty(&self) -> bool {
        self.project.as_ref().is_some_and(|p| p.history.can_undo())
    }

    pub fn busy(&self) -> bool {
        matches!(
            self.status,
            Some(StatusEvent::Loading | StatusEvent::Exporting)
        )
    }

    /// 文档有 ≥2 帧才可播放
    pub fn can_playback(&self) -> bool {
        self.project.as_ref().is_some_and(|p| p.frame_count() > 1)
    }

    /// 正在播放：`playing` 置位且文档可播放
    pub fn is_playing(&self) -> bool {
        self.playback.playing && self.can_playback()
    }

    /// 唯一状态迁移入口。返回需 app 层执行的副作用。
    pub fn apply(&mut self, action: Action) -> Vec<Effect> {
        match action {
            // ── 文件 IO ──
            Action::OpenFileDialog => {
                if self.busy() {
                    return vec![];
                }
                vec![Effect::SpawnOpenDialog]
            }
            Action::FilePicked(None) => {
                self.status = Some(StatusEvent::DialogCancelled);
                vec![]
            }
            Action::FilePicked(Some(path)) => {
                if self.busy() {
                    return vec![];
                }
                self.reset_estimate_keep_generation();
                self.project = None;
                self.playback = Playback::default();
                self.status = Some(StatusEvent::Loading);
                vec![Effect::SpawnLoad(path)]
            }
            Action::LoadFinished(Ok(mut project)) => {
                let mem = project.store.total_memory();
                let frames = project.frame_count();
                let file_bytes = project.file_size;
                project.clamp_indices();
                self.reset_estimate_keep_generation();
                self.project = Some(project);
                self.playback = Playback {
                    playing: frames > 1, // 加载后自动播放
                    acc_ms: 0.0,
                };
                self.status = Some(StatusEvent::Loaded {
                    frames,
                    file_bytes,
                    mem_bytes: mem,
                });
                vec![]
            }
            Action::LoadFinished(Err(e)) => {
                self.project = None;
                self.status = Some(StatusEvent::Error(e));
                vec![]
            }
            Action::ClearRequested | Action::ClearConfirmed => {
                self.project = None;
                self.playback = Playback::default();
                self.pending_export = None;
                self.reset_estimate_keep_generation();
                self.status = Some(StatusEvent::Cleared);
                vec![]
            }

            // ── 选择与播放 ──
            Action::SelectFrame(id) => {
                if let Some(p) = self.project.as_mut().filter(|p| p.order.contains(&id)) {
                    p.select_single(id);
                }
                vec![]
            }
            Action::ToggleSelect(id) => {
                if let Some(p) = self.project.as_mut() {
                    if p.order.contains(&id) {
                        if let Some(pos) = p.selection.iter().position(|x| *x == id) {
                            p.selection.remove(pos);
                        } else {
                            p.selection.push(id);
                            p.selection.sort_unstable();
                        }
                        p.anchor = Some(id);
                    }
                }
                vec![]
            }
            Action::RangeSelect(id) => {
                if let Some(p) = self.project.as_mut() {
                    if !p.order.contains(&id) {
                        return vec![];
                    }
                    // 锚点到该帧的闭区间；无锚点（或锚点已失效）退化为单选
                    let anchor = p.anchor.filter(|a| p.order.contains(a));
                    let range = anchor.and_then(|a| p.index_of(a)).zip(p.index_of(id));
                    if let Some((a, b)) = range {
                        let (lo, hi) = (a.min(b), a.max(b));
                        p.selection = p.order[lo..=hi].to_vec();
                        p.selection.sort_unstable();
                    } else {
                        p.select_single(id);
                    }
                }
                vec![]
            }
            Action::SelectAll => {
                if let Some(p) = self.project.as_mut() {
                    p.selection = p.order.clone();
                }
                vec![]
            }
            Action::ClearSelection => {
                if let Some(p) = self.project.as_mut() {
                    p.selection.clear();
                }
                vec![]
            }
            Action::PlayPause => {
                if self.can_playback() {
                    self.playback.playing = !self.playback.playing;
                    if !self.playback.playing {
                        self.playback.acc_ms = 0.0;
                    }
                }
                vec![]
            }
            Action::StepForward => {
                self.step(1);
                vec![]
            }
            Action::StepBack => {
                self.step(-1);
                vec![]
            }

            // ── 导出 ──
            Action::StartExport(format) => {
                let Some(p) = self.project.as_ref() else {
                    return vec![];
                };
                if p.order.is_empty() {
                    self.status = Some(StatusEvent::Error(AppError::Encode(
                        "no frames to export".into(),
                    )));
                    return vec![];
                }
                let default_name = self.default_export_name(&format);
                self.pending_export = Some(format);
                vec![Effect::SpawnSaveDialog { default_name }]
            }
            Action::Undo => {
                if let Some(p) = self.project.as_mut() {
                    if let Some(entry) = p.history.pop_undo() {
                        let after = p.snapshot();
                        p.history.push_redo(entry.edit, after);
                        p.restore(entry.state);
                        p.gc();
                        self.estimate.result = None; // 帧内容变化 → 估算失效
                    }
                }
                self.stop_playback_if_short();
                vec![]
            }
            Action::Redo => {
                if let Some(p) = self.project.as_mut() {
                    if let Some(entry) = p.history.pop_redo() {
                        let before = p.snapshot();
                        p.history.push_undo_keep_redo(entry.edit, before);
                        p.restore(entry.state);
                        p.gc();
                        self.estimate.result = None;
                    }
                }
                self.stop_playback_if_short();
                vec![]
            }

            // ── 编辑 ──
            Action::Edit(edit) => {
                self.apply_edit(edit);
                self.estimate.result = None; // 帧内容变化 → 估算失效
                self.stop_playback_if_short();
                vec![]
            }
            Action::SavePathPicked(None) => {
                self.pending_export = None;
                self.status = Some(StatusEvent::DialogCancelled);
                vec![]
            }
            Action::SavePathPicked(Some(path)) => {
                let Some(format) = self.pending_export.take() else {
                    return vec![];
                };
                let Some(p) = self.project.as_ref() else {
                    return vec![];
                };
                if p.order.is_empty() {
                    self.status = Some(StatusEvent::Error(AppError::Encode(
                        "no frames to export".into(),
                    )));
                    return vec![];
                }
                self.status = Some(StatusEvent::Exporting);
                vec![Effect::SpawnExport {
                    frames: p.frames(),
                    loop_count: p.loop_count,
                    format,
                    path,
                }]
            }
            Action::ExportFinished(Ok(outcome)) => {
                self.status = Some(StatusEvent::ExportDone {
                    path: outcome.path,
                    bytes: outcome.bytes,
                    ms: outcome.ms,
                });
                vec![]
            }
            Action::ExportFinished(Err(e)) => {
                self.status = Some(StatusEvent::Error(e));
                vec![]
            }
            Action::EstimateRequested { generation, format } => {
                // 单飞：已有估算在途则跳过（对话框会在结果落地后按需重发），
                // 防止拖动滑条期间线程堆积
                if self.estimate.in_flight {
                    return vec![];
                }
                self.estimate.generation = generation;
                self.estimate.result = None;
                let Some(p) = self.project.as_ref() else {
                    return vec![];
                };
                if p.order.is_empty() {
                    return vec![];
                }
                self.estimate.in_flight = true;
                vec![Effect::SpawnEstimate {
                    generation,
                    frames: p.frames(),
                    loop_count: p.loop_count,
                    format,
                }]
            }
            Action::EstimateReady { generation, result } => {
                self.estimate.in_flight = false;
                if generation == self.estimate.generation {
                    self.estimate.result = Some(result);
                }
                vec![]
            }

            // ── 文档属性 ──
            Action::SetLoopInfinite(infinite) => {
                self.apply_edit(Edit::SetLoop { infinite });
                vec![]
            }
        }
    }

    /// 估算状态重置但保留代次计数（跨文档单调，防止陈旧 worker 的
    /// EstimateReady 撞上代次归零后的新估算）
    fn reset_estimate_keep_generation(&mut self) {
        let gen = self.estimate.generation;
        self.estimate = EstimateState::default();
        self.estimate.generation = gen;
    }

    /// 帧数不足以播放时停止播放（防止 remaining_ms 恒触发重绘循环）
    fn stop_playback_if_short(&mut self) {
        if !self.can_playback() {
            self.playback.playing = false;
        }
    }

    /// 播放推进（每帧调用，dt 秒）。窗口长时间无 repaint 时 dt 可能很大，
    /// 夹到 1s 防止疯狂追帧。
    pub fn tick_playback(&mut self, dt_secs: f64) {
        if !self.is_playing() {
            return;
        }
        let Some(p) = self.project.as_mut() else {
            return;
        };
        self.playback.acc_ms += dt_secs.min(1.0) * 1000.0;
        // 防御性上限：单次 tick 至多跨越 10_000 帧
        for _ in 0..10_000 {
            let cur_dur = p.playback_duration_ms();
            if self.playback.acc_ms < cur_dur {
                break;
            }
            self.playback.acc_ms -= cur_dur;
            p.view_index = (p.view_index + 1) % p.order.len();
        }
    }

    /// 当前帧剩余播放时间（app 层据此安排 repaint）。
    /// 未在播放（含单帧/空文档）返回 None（否则会恒触发 16ms 重绘）
    pub fn playback_remaining_ms(&self) -> Option<f64> {
        if !self.is_playing() {
            return None;
        }
        let cur_dur = self.project.as_ref()?.playback_duration_ms();
        Some((cur_dur - self.playback.acc_ms).max(1.0))
    }

    fn step(&mut self, dir: i32) {
        if let Some(p) = self.project.as_mut() {
            if p.order.is_empty() {
                return;
            }
            self.playback.playing = false;
            let len = p.order.len() as i32;
            let next = (p.view_index as i32 + dir).rem_euclid(len) as usize;
            p.view_index = next;
        }
    }

    fn default_export_name(&self, format: &ExportFormat) -> String {
        let stem = self
            .project
            .as_ref()
            .and_then(|p| {
                p.source
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "gifsennin".into());
        let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
        format!("{stem}_{ts}.{}", format.extension())
    }

    /// 按引用匹配 `&edit`：末尾 `push_undo(edit)` 需要 edit 完整。
    fn apply_edit(&mut self, edit: Edit) {
        let Some(p) = self.project.as_mut() else {
            return;
        };
        let before = p.snapshot();

        match &edit {
            Edit::DeleteFrames { ids } => {
                if ids.is_empty() {
                    return;
                }
                let dead: HashSet<FrameId> = ids.iter().copied().collect();
                p.order.retain(|id| !dead.contains(id));
                p.clamp_indices();
            }
            Edit::ReverseSelected => {
                let sel = p.selection_set();
                // 单帧反转是恒等置换：不入历史（否则产生"什么都不做"的
                // 撤销项并清空 redo）
                if sel.len() < 2 {
                    return;
                }
                // 收集选中位置，原位反转
                let positions: Vec<usize> = p
                    .order
                    .iter()
                    .enumerate()
                    .filter(|(_, id)| sel.contains(id))
                    .map(|(i, _)| i)
                    .collect();
                let ids: Vec<FrameId> = positions.iter().map(|&i| p.order[i]).collect();
                for (k, &pos) in positions.iter().enumerate() {
                    p.order[pos] = ids[ids.len() - 1 - k];
                }
            }
            Edit::Reorder {
                moved,
                before: target,
            } => {
                if moved.is_empty() {
                    return;
                }
                let mset: HashSet<FrameId> = moved.iter().copied().collect();
                let target: Option<FrameId> = *target;
                // 目标插入位置（基于原 order 计算，再扣除被移动元素的影响）
                let insert_at = target.and_then(|b| p.index_of(b)).unwrap_or(p.order.len());
                let before_moved = p
                    .order
                    .iter()
                    .take(insert_at)
                    .filter(|id| mset.contains(id))
                    .count();
                let insert_at = insert_at.saturating_sub(before_moved);
                let moved_seq: Vec<FrameId> = p
                    .order
                    .iter()
                    .copied()
                    .filter(|id| mset.contains(id))
                    .collect();
                p.order.retain(|id| !mset.contains(id));
                let insert_at = insert_at.min(p.order.len());
                p.order.splice(insert_at..insert_at, moved_seq);
            }
            Edit::SetDuration { ids, ms } => {
                if ids.is_empty() {
                    return;
                }
                let ms = *ms;
                let set: HashSet<FrameId> = ids.iter().copied().collect();
                let mut map = HashMap::new();
                let mut new_order = Vec::with_capacity(p.order.len());
                for id in p.order.iter().copied() {
                    if set.contains(&id) {
                        if let Some(f) = p.store.get(id) {
                            let nid = p.store.insert(f.with_duration(ms));
                            map.insert(id, nid);
                            new_order.push(nid);
                            continue;
                        }
                    }
                    new_order.push(id);
                }
                p.order = new_order;
                p.remap_ids(&map);
            }
            Edit::Crop { x, y, w, h } => {
                let (x, y, w, h) = transform::clamp_crop(p.canvas, *x, *y, *w, *h);
                // 全画布裁剪 = 恒等：不入历史
                if (x, y, w, h) == (0, 0, p.canvas.0, p.canvas.1) {
                    return;
                }
                p.canvas = (w, h);
                p.replace_all_frames(|frames| transform::crop_all(frames, x, y, w, h));
            }
            Edit::Resize {
                width,
                height,
                filter,
            } => {
                let (width, height, filter) = (*width, *height, *filter);
                if width == 0 || height == 0 {
                    return;
                }
                // 恒等缩放：不入历史
                if (width, height) == p.canvas {
                    return;
                }
                // 预算前置检查：对话框允许到 10000×10000，多帧大画布会在 rayon 里
                // 申请远超预算的内存
                let projected = p.order.len() as u64 * u64::from(width) * u64::from(height) * 4;
                if let Err(e) = check_memory_budget(projected) {
                    self.status = Some(StatusEvent::Error(e));
                    return;
                }
                p.canvas = (width, height);
                p.replace_all_frames(|frames| transform::resize_all(frames, width, height, filter));
            }
            Edit::SetLoop { infinite } => {
                p.loop_count = if *infinite { 0 } else { 1 };
            }
            Edit::RotateLeft | Edit::RotateRight => {
                let dir = match edit {
                    Edit::RotateLeft => transform::RotDir::Left,
                    Edit::RotateRight => transform::RotDir::Right,
                    _ => unreachable!("matched arm guarantees variant"),
                };
                p.canvas = (p.canvas.1, p.canvas.0);
                p.replace_all_frames(|frames| transform::rotate_all(frames, dir));
            }
            Edit::FlipH | Edit::FlipV => {
                let axis = match edit {
                    Edit::FlipH => transform::FlipAxis::Horizontal,
                    Edit::FlipV => transform::FlipAxis::Vertical,
                    _ => unreachable!("matched arm guarantees variant"),
                };
                p.replace_all_frames(|frames| transform::flip_all(frames, axis));
            }
        }

        p.history.push_undo(edit, before);
        p.clamp_indices();
        p.gc();
    }
}

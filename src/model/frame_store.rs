use super::frame::{Frame, FrameId};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// 帧身份全局单调计数器（审查修复：原为每仓库从 0 起算，跨文档 FrameId
/// 碰撞会让纹理缓存/拖拽状态命中前一文档的残留——身份必须进程内唯一）。
static NEXT_FRAME_ID: AtomicU64 = AtomicU64::new(0);

/// 帧仓库（共识 Q3）：只增不删的权威存储；「删除帧」只是从 `order` 移除，
/// 像素数据保留在仓库中供 undo 恢复。每次编辑后由 `gc()` 清理
/// 既不在当前序列、也不被任何历史快照引用的帧。
#[derive(Debug, Default)]
pub struct FrameStore {
    frames: HashMap<FrameId, Arc<Frame>>,
}

impl FrameStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, frame: Frame) -> FrameId {
        let id = FrameId(NEXT_FRAME_ID.fetch_add(1, Ordering::Relaxed));
        self.frames.insert(id, Arc::new(frame));
        id
    }

    pub fn get(&self, id: FrameId) -> Option<Arc<Frame>> {
        self.frames.get(&id).cloned()
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// 所有存活帧占用的字节数（sidebar 内存显示用）
    pub fn total_memory(&self) -> u64 {
        self.frames.values().map(|f| f.memory_size() as u64).sum()
    }

    /// 只保留 `keep` 中引用的帧（GC）
    pub fn retain(&mut self, keep: &HashSet<FrameId>) {
        self.frames.retain(|id, _| keep.contains(id));
    }
}

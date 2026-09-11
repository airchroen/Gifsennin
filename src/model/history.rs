use super::actions::Edit;
use super::frame::FrameId;
use std::collections::HashSet;

/// 文档结构快照（共识 Q3）。帧像素数据不在快照内——
/// 它们由 FrameStore 以 `Arc` 结构共享，快照只携带廉价的骨架数据
/// （300 帧 ≈ 2.4KB），因此快照式 undo 永远正确且近乎免费。
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub canvas: (u32, u32),
    /// GIF 语义：0 = 无限循环
    pub loop_count: u16,
    pub order: Vec<FrameId>,
    /// 按 id 升序（与插入序一致）的去重选中集
    pub selection: Vec<FrameId>,
    pub view_index: usize,
}

#[derive(Debug, Clone)]
pub struct HistoryEntry {
    /// 触发该次状态迁移的编辑（render 层据此显示撤销/重做标签）
    pub edit: Edit,
    pub state: Snapshot,
}

/// undo/redo 栈。约定：
/// - undo 栈中的 `state` = 编辑**前**的快照
/// - redo 栈中的 `state` = 编辑**后**的快照（undo 时压入）
pub struct History {
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
}

/// undo 历史上限（超出丢弃最旧条目）
pub const MAX_HISTORY: usize = 100;

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

impl History {
    pub fn new() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// 应用编辑前调用：压入编辑前快照，清空 redo（新编辑使重做分支失效）
    pub fn push_undo(&mut self, edit: Edit, before: Snapshot) {
        self.redo.clear();
        self.undo.push(HistoryEntry {
            edit,
            state: before,
        });
        if self.undo.len() > MAX_HISTORY {
            self.undo.remove(0);
        }
    }

    /// Redo 路径专用：压入撤销栈但**保留**剩余 redo 分支。
    /// （审查修复：Redo 复用 push_undo 会把 redo 清空，深度恒截断为 1。）
    pub fn push_undo_keep_redo(&mut self, edit: Edit, before: Snapshot) {
        self.undo.push(HistoryEntry {
            edit,
            state: before,
        });
        if self.undo.len() > MAX_HISTORY {
            self.undo.remove(0);
        }
    }

    pub fn pop_undo(&mut self) -> Option<HistoryEntry> {
        self.undo.pop()
    }

    pub fn push_redo(&mut self, edit: Edit, after: Snapshot) {
        self.redo.push(HistoryEntry { edit, state: after });
    }

    pub fn pop_redo(&mut self) -> Option<HistoryEntry> {
        self.redo.pop()
    }

    pub fn next_undo_edit(&self) -> Option<&Edit> {
        self.undo.last().map(|e| &e.edit)
    }

    pub fn next_redo_edit(&self) -> Option<&Edit> {
        self.redo.last().map(|e| &e.edit)
    }

    /// 当前序列 + 全部历史快照引用的帧 id 集合（GC 依据）
    pub fn referenced_ids<'a>(&'a self, current: &'a [FrameId], out: &'a mut HashSet<FrameId>) {
        out.extend(current.iter().copied());
        for entry in self.undo.iter().chain(self.redo.iter()) {
            out.extend(entry.state.order.iter().copied());
        }
    }
}

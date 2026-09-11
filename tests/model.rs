// model 层集成测试：Action 序列状态断言、undo 快照不变量、GC、播放。
// 共识 Q5：apply 是纯函数，全部可无头测试。

use gifsennin_rust::codec::{DecodedAnimation, DecodedFrame};
use gifsennin_rust::errors::AppError;
use gifsennin_rust::model::{Action, Edit, Model, Project, Snapshot, MEMORY_BUDGET_BYTES};
use std::path::PathBuf;

fn anim(n: usize, w: u32, h: u32, dur: u32) -> DecodedAnimation {
    DecodedAnimation {
        width: w,
        height: h,
        loop_count: 0,
        frames: (0..n)
            .map(|i| DecodedFrame {
                rgba: vec![i as u8; (w * h * 4) as usize],
                duration_ms: dur,
            })
            .collect(),
    }
}

/// n 帧已加载的模型（自动播放关掉，便于断言）
fn loaded(n: usize) -> Model {
    let mut m = Model::new();
    let project = Project::from_decoded(PathBuf::from("test.gif"), anim(n, 8, 8, 100)).unwrap();
    m.apply(Action::LoadFinished(Ok(project)));
    m.playback.playing = false;
    m
}

fn snap(m: &Model) -> Snapshot {
    m.project.as_ref().unwrap().snapshot()
}

fn order(m: &Model) -> Vec<gifsennin_rust::model::FrameId> {
    m.project.as_ref().unwrap().order.clone()
}

fn ids(m: &Model, indices: &[usize]) -> Vec<gifsennin_rust::model::FrameId> {
    let o = order(m);
    indices.iter().map(|&i| o[i]).collect()
}

// ── 加载 ──────────────────────────────────────────────────────

#[test]
fn load_initializes_selection_and_playhead() {
    let m = loaded(3);
    let p = m.project.as_ref().unwrap();
    assert_eq!(p.frame_count(), 3);
    assert_eq!(p.selection.len(), 1); // 首帧自动选中
    assert_eq!(p.view_index, 0);
    assert_eq!(p.canvas, (8, 8));
}

#[test]
fn budget_guard_rejects_oversized() {
    let result = Project::from_decoded(PathBuf::from("huge.gif"), anim(1, 30000, 30000, 100));
    // 30000×30000×4 ≈ 3.6GB > 1.5GB 预算
    match result {
        Err(AppError::TooLarge { needed, budget }) => {
            assert!(needed > budget);
            assert_eq!(budget, MEMORY_BUDGET_BYTES);
        }
        other => panic!("expected TooLarge, got {:?}", other.is_ok()),
    }
}

#[test]
fn empty_animation_rejected() {
    let result = Project::from_decoded(PathBuf::from("x.gif"), anim(0, 8, 8, 100));
    assert!(matches!(result, Err(AppError::Decode(_))));
}

// ── 选择语义（共识 Q7）────────────────────────────────────────

#[test]
fn select_toggle_range_semantics() {
    let mut m = loaded(5);
    // 单击第 2 帧
    m.apply(Action::SelectFrame(ids(&m, &[2])[0]));
    assert_eq!(m.project.as_ref().unwrap().selection, ids(&m, &[2]));
    // Ctrl+单击第 4 帧 → 加选
    m.apply(Action::ToggleSelect(ids(&m, &[4])[0]));
    assert_eq!(m.project.as_ref().unwrap().selection, ids(&m, &[2, 4]));
    // Shift+单击第 0 帧 → 锚点(4)到 0 的范围
    m.apply(Action::RangeSelect(ids(&m, &[0])[0]));
    assert_eq!(
        m.project.as_ref().unwrap().selection,
        ids(&m, &[0, 1, 2, 3, 4])
    );
    // Ctrl+单击第 2 帧 → 移除
    m.apply(Action::ToggleSelect(ids(&m, &[2])[0]));
    assert_eq!(
        m.project.as_ref().unwrap().selection,
        ids(&m, &[0, 1, 3, 4])
    );
    // Ctrl+A
    m.apply(Action::ClearSelection);
    m.apply(Action::SelectAll);
    assert_eq!(m.project.as_ref().unwrap().selection.len(), 5);
}

// ── undo 快照不变量（共识 Q3）─────────────────────────────────

/// 通用不变量：任意编辑 → undo 必须精确恢复该编辑发生前一瞬的完整快照
fn assert_undo_roundtrip(m: &mut Model, edit: Edit) {
    let before = snap(m);
    m.apply(Action::Edit(edit));
    assert!(
        m.project.as_ref().unwrap().history.can_undo(),
        "edit should be undoable"
    );
    m.apply(Action::Undo);
    assert_eq!(snap(m), before, "undo must restore the pre-edit snapshot");
}

#[test]
fn undo_restores_exact_snapshot_for_all_edit_kinds() {
    let mut m = loaded(4);

    // 删除
    let del_ids = ids(&m, &[1, 2]);
    assert_undo_roundtrip(&mut m, Edit::DeleteFrames { ids: del_ids });
    assert_eq!(m.project.as_ref().unwrap().frame_count(), 4);

    // 重排（第 3 帧移到最前）
    let moved = ids(&m, &[3]);
    let target = order(&m)[0];
    assert_undo_roundtrip(
        &mut m,
        Edit::Reorder {
            moved,
            before: Some(target),
        },
    );

    // 时长
    let dur_ids = ids(&m, &[0, 1]);
    assert_undo_roundtrip(
        &mut m,
        Edit::SetDuration {
            ids: dur_ids,
            ms: 250,
        },
    );
    // undo 恢复了旧 id，其帧对象时长原样
    assert_eq!(
        m.project.as_ref().unwrap().frame_at(0).unwrap().duration_ms,
        100
    );

    // 反转所选（先全选，反转全部等价于整序反转）
    m.apply(Action::SelectAll);
    assert_undo_roundtrip(&mut m, Edit::ReverseSelected);

    // 裁剪（画布变化也随快照恢复）
    assert_undo_roundtrip(
        &mut m,
        Edit::Crop {
            x: 1,
            y: 1,
            w: 4,
            h: 4,
        },
    );
    assert_eq!(m.project.as_ref().unwrap().canvas, (8, 8));

    // 旋转 / 翻转
    assert_undo_roundtrip(&mut m, Edit::RotateRight);
    assert_undo_roundtrip(&mut m, Edit::RotateLeft);
    assert_undo_roundtrip(&mut m, Edit::FlipH);
    assert_undo_roundtrip(&mut m, Edit::FlipV);
}

#[test]
fn redo_reapplies_and_new_edit_clears_redo() {
    let mut m = loaded(3);
    let victim = ids(&m, &[1]);
    m.apply(Action::Edit(Edit::DeleteFrames {
        ids: victim.clone(),
    }));
    m.apply(Action::Undo);
    assert!(m.project.as_ref().unwrap().history.can_redo());
    m.apply(Action::Redo);
    assert_eq!(m.project.as_ref().unwrap().frame_count(), 2);
    // undo 后做新编辑 → redo 分支失效
    m.apply(Action::Undo);
    m.apply(Action::Edit(Edit::SetDuration {
        ids: ids(&m, &[0]),
        ms: 50,
    }));
    assert!(!m.project.as_ref().unwrap().history.can_redo());
}

#[test]
fn undo_restores_selection_too() {
    let mut m = loaded(5);
    m.apply(Action::RangeSelect(ids(&m, &[3])[0])); // 0..3 选中
    m.apply(Action::Edit(Edit::DeleteFrames {
        ids: ids(&m, &[0, 1, 2, 3]),
    }));
    let p = m.project.as_ref().unwrap();
    assert_eq!(p.frame_count(), 1);
    assert!(p.selection.is_empty());
    m.apply(Action::Undo);
    let p = m.project.as_ref().unwrap();
    assert_eq!(p.frame_count(), 5);
    assert_eq!(p.selection.len(), 4); // 选择随快照恢复
}

// ── 重排语义 ──────────────────────────────────────────────────

#[test]
fn reorder_moves_before_target_preserving_rest() {
    let mut m = loaded(5); // [0 1 2 3 4]
    let moved = ids(&m, &[3]);
    let target = order(&m)[0]; // 移到最前
    m.apply(Action::Edit(Edit::Reorder {
        moved,
        before: Some(target),
    }));
    let o = order(&m);
    let names: Vec<u8> = o.iter().map(|id| frame_tag(&m, *id)).collect();
    assert_eq!(names, vec![3, 0, 1, 2, 4]);
}

#[test]
fn reorder_multi_moves_block_contiguously() {
    let mut m = loaded(6); // [0 1 2 3 4 5]
    let moved = ids(&m, &[1, 3]);
    let target = order(&m)[5]; // 移到 5 之前
    m.apply(Action::Edit(Edit::Reorder {
        moved,
        before: Some(target),
    }));
    let names: Vec<u8> = order(&m).iter().map(|id| frame_tag(&m, *id)).collect();
    assert_eq!(names, vec![0, 2, 4, 1, 3, 5]);
}

#[test]
fn reorder_to_end_with_none() {
    let mut m = loaded(4);
    let moved = ids(&m, &[0, 1]);
    m.apply(Action::Edit(Edit::Reorder {
        moved,
        before: None,
    }));
    let names: Vec<u8> = order(&m).iter().map(|id| frame_tag(&m, *id)).collect();
    assert_eq!(names, vec![2, 3, 0, 1]);
}

/// 帧标记：加载时用 i 填充 rgba[0]，用于识别帧身份
fn frame_tag(m: &Model, id: gifsennin_rust::model::FrameId) -> u8 {
    m.project
        .as_ref()
        .unwrap()
        .store
        .get(id)
        .map(|f| f.as_rgba()[0])
        .unwrap_or(255)
}

// ── 时长/画布变换的 id 重映射 ─────────────────────────────────

#[test]
fn set_duration_keeps_selection_and_order_positions() {
    let mut m = loaded(3);
    m.apply(Action::RangeSelect(ids(&m, &[2])[0])); // 全选
    let names_before: Vec<u8> = order(&m).iter().map(|id| frame_tag(&m, *id)).collect();
    m.apply(Action::Edit(Edit::SetDuration {
        ids: ids(&m, &[1]),
        ms: 500,
    }));
    let p = m.project.as_ref().unwrap();
    assert_eq!(p.frame_count(), 3);
    assert_eq!(p.selection.len(), 3); // 重映射后选择保持
    assert_eq!(p.frame_at(1).unwrap().duration_ms, 500);
    assert_eq!(p.frame_at(0).unwrap().duration_ms, 100);
    let names_after: Vec<u8> = order(&m).iter().map(|id| frame_tag(&m, *id)).collect();
    assert_eq!(names_before, names_after); // 顺序不变
}

#[test]
fn crop_remaps_selection_to_new_ids() {
    let mut m = loaded(3);
    m.apply(Action::SelectFrame(ids(&m, &[1])[0]));
    m.apply(Action::Edit(Edit::Crop {
        x: 0,
        y: 0,
        w: 4,
        h: 4,
    }));
    let p = m.project.as_ref().unwrap();
    assert_eq!(p.canvas, (4, 4));
    assert_eq!(p.frame_count(), 3);
    assert_eq!(p.selection.len(), 1); // 选中帧迁移到新 id
    assert_eq!(p.frame_at(1).unwrap().width, 4);
}

#[test]
fn reverse_selected_only_touches_selected_positions() {
    let mut m = loaded(5);
    m.apply(Action::SelectFrame(ids(&m, &[4])[0]));
    m.apply(Action::ToggleSelect(ids(&m, &[1])[0]));
    // 选中 {1,4}（id 升序存储）
    m.apply(Action::Edit(Edit::ReverseSelected));
    // 位置 1 与 4 上的帧互换，其余原位
    let names: Vec<u8> = order(&m).iter().map(|id| frame_tag(&m, *id)).collect();
    assert_eq!(names, vec![0, 4, 2, 3, 1]);
}

// ── GC（共识 Q3：帧仓库只增不删 + 引用回收）──────────────────

#[test]
fn gc_drops_unreferenced_after_history_trim() {
    let mut m = loaded(3); // ids: I0 I1 I2, store=3
    m.apply(Action::Edit(Edit::SetDuration {
        ids: ids(&m, &[0]),
        ms: 42,
    })); // N0 诞生, undo 引用 I0
    assert_eq!(m.project.as_ref().unwrap().store.len(), 4);
    m.apply(Action::Edit(Edit::SetDuration {
        ids: ids(&m, &[1]),
        ms: 43,
    })); // N1 诞生
    assert_eq!(m.project.as_ref().unwrap().store.len(), 5);
    // 全撤：order 回到 [I0,I1,I2]，但 redo 栈引用 N0、N1（redo 需要它们重建）→ 保留
    m.apply(Action::Undo);
    m.apply(Action::Undo);
    assert_eq!(m.project.as_ref().unwrap().store.len(), 5);
    assert_eq!(order(&m).len(), 3);
    // 新编辑：redo 分支被清空，N0/N1 不再被任何快照引用 → 回收；N2 诞生
    m.apply(Action::Edit(Edit::SetDuration {
        ids: ids(&m, &[2]),
        ms: 44,
    }));
    let p = m.project.as_ref().unwrap();
    assert_eq!(
        p.store.len(),
        4,
        "N0/N1 must be collected once redo is invalidated"
    );
    // 撤销 C：redo 引用 N2 → 仍保留
    m.apply(Action::Undo);
    let p = m.project.as_ref().unwrap();
    assert_eq!(p.store.len(), 4);
    assert_eq!(p.frame_count(), 3);
}

#[test]
fn deleting_then_undoing_keeps_frames_alive() {
    let mut m = loaded(3);
    m.apply(Action::Edit(Edit::DeleteFrames { ids: ids(&m, &[1]) }));
    // 仓库仍持有被删帧（undo 依赖），order 只有 2
    let p = m.project.as_ref().unwrap();
    assert_eq!(p.frame_count(), 2);
    assert_eq!(p.store.len(), 3);
    m.apply(Action::Undo);
    assert_eq!(m.project.as_ref().unwrap().frame_count(), 3);
}

// ── 播放（统一时基 ms）────────────────────────────────────────

#[test]
fn playback_advances_by_frame_durations() {
    let mut m = Model::new();
    let mut a = anim(3, 4, 4, 100);
    a.frames[1].duration_ms = 200;
    let project = Project::from_decoded(PathBuf::from("t.gif"), a).unwrap();
    m.apply(Action::LoadFinished(Ok(project)));
    assert!(m.playback.playing); // 加载自动播放

    m.tick_playback(0.05); // 50ms < 100ms → 不动
    assert_eq!(m.project.as_ref().unwrap().view_index, 0);
    m.tick_playback(0.06); // 累计 110ms ≥ 100 → 进入第 1 帧
    assert_eq!(m.project.as_ref().unwrap().view_index, 1);
    m.tick_playback(0.25); // 250 ≥ 200 → 第 2 帧
    assert_eq!(m.project.as_ref().unwrap().view_index, 2);
    m.tick_playback(0.11); // 110 ≥ 100 → 回绕第 0 帧
    assert_eq!(m.project.as_ref().unwrap().view_index, 0);
}

#[test]
fn step_pauses_and_wraps() {
    let mut m = loaded(3);
    m.playback.playing = true;
    m.apply(Action::StepForward);
    assert!(!m.playback.playing);
    assert_eq!(m.project.as_ref().unwrap().view_index, 1);
    m.apply(Action::StepBack);
    m.apply(Action::StepBack); // 回绕到末帧
    assert_eq!(m.project.as_ref().unwrap().view_index, 2);
}

#[test]
fn zero_duration_plays_at_16ms_floor() {
    let mut m = Model::new();
    let mut a = anim(2, 4, 4, 0);
    a.frames[0].duration_ms = 0;
    let project = Project::from_decoded(PathBuf::from("t.gif"), a).unwrap();
    m.apply(Action::LoadFinished(Ok(project)));
    m.tick_playback(0.017); // 17ms ≥ 16ms 下限 → 前进
    assert_eq!(m.project.as_ref().unwrap().view_index, 1);
}

// ── 空文档边界 ────────────────────────────────────────────────

#[test]
fn delete_all_frames_yields_empty_doc_and_undo_restores() {
    let mut m = loaded(3);
    m.apply(Action::SelectAll);
    m.apply(Action::Edit(Edit::DeleteFrames {
        ids: ids(&m, &[0, 1, 2]),
    }));
    let p = m.project.as_ref().unwrap();
    assert_eq!(p.frame_count(), 0);
    assert_eq!(p.view_index, 0);
    m.apply(Action::Undo);
    assert_eq!(m.project.as_ref().unwrap().frame_count(), 3);
}

#[test]
fn empty_selection_edits_are_noops_without_history() {
    let mut m = loaded(3);
    m.apply(Action::ClearSelection);
    m.apply(Action::Edit(Edit::DeleteFrames { ids: vec![] }));
    m.apply(Action::Edit(Edit::ReverseSelected)); // 空选择
    assert!(!m.project.as_ref().unwrap().history.can_undo());
    assert_eq!(m.project.as_ref().unwrap().frame_count(), 3);
}

// ── 审查修复回归测试 ────────────────────────────────────────────

#[test]
fn redo_restores_multi_level_history() {
    // 审查缺陷：Redo 复用 push_undo 清空了剩余 redo → 重做深度恒为 1
    let mut m = loaded(4); // [0 1 2 3]
    m.apply(Action::Edit(Edit::DeleteFrames { ids: ids(&m, &[3]) })); // 3 帧
    m.apply(Action::Edit(Edit::DeleteFrames { ids: ids(&m, &[2]) })); // 2 帧
    m.apply(Action::Undo); // 回 3 帧
    m.apply(Action::Undo); // 回 4 帧
    assert!(m.project.as_ref().unwrap().history.can_redo());
    m.apply(Action::Redo); // → 3 帧
    assert!(
        m.project.as_ref().unwrap().history.can_redo(),
        "一次 redo 后 redo 栈必须仍在（原 bug：被清空）"
    );
    m.apply(Action::Redo); // → 2 帧
    assert_eq!(m.project.as_ref().unwrap().frame_count(), 2);
}

#[test]
fn resize_budget_guard_blocks_oversize() {
    // 审查缺陷：Resize 无预算预检，可 3 击触发上百 GB 分配直接 abort
    let mut m = loaded(3); // 8×8×3 ≈ 768B
    m.playback.playing = false;
    m.apply(Action::Edit(Edit::Resize {
        width: 20000,
        height: 20000,
        filter: gifsennin_rust::model::ResizeFilter::Nearest,
    }));
    // 3 × 20000² × 4 ≈ 4.8GB > 预算 → 拒绝且不入历史
    let p = m.project.as_ref().unwrap();
    assert_eq!(p.canvas, (8, 8));
    assert!(!p.history.can_undo());
    assert!(matches!(
        m.status,
        Some(gifsennin_rust::model::StatusEvent::Error(
            gifsennin_rust::errors::AppError::TooLarge { .. }
        ))
    ));
    // 合法尺寸照常通过
    m.apply(Action::Edit(Edit::Resize {
        width: 16,
        height: 16,
        filter: gifsennin_rust::model::ResizeFilter::Nearest,
    }));
    assert_eq!(m.project.as_ref().unwrap().canvas, (16, 16));
}

#[test]
fn noop_edits_do_not_create_history() {
    // 审查缺陷：单帧反转/原尺寸缩放是恒等置换却入历史并清空 redo
    let mut m = loaded(4);
    m.apply(Action::SelectFrame(ids(&m, &[1])[0])); // 单选
    m.apply(Action::Edit(Edit::ReverseSelected)); // 恒等
    assert!(!m.project.as_ref().unwrap().history.can_undo());
    m.apply(Action::Edit(Edit::Resize {
        width: 8,
        height: 8, // 原尺寸
        filter: gifsennin_rust::model::ResizeFilter::Nearest,
    }));
    assert!(!m.project.as_ref().unwrap().history.can_undo());
    // 全画布裁剪同样恒等
    m.apply(Action::Edit(Edit::Crop {
        x: 0,
        y: 0,
        w: 8,
        h: 8,
    }));
    assert!(!m.project.as_ref().unwrap().history.can_undo());
}

#[test]
fn loop_toggle_is_undoable_and_marks_dirty() {
    // 审查缺陷：loop_count 在历史外直接改，被无关撤销静默回滚
    let mut m = loaded(3);
    m.apply(Action::SetLoopInfinite(true));
    assert_eq!(m.project.as_ref().unwrap().loop_count, 0);
    assert!(m.dirty(), "循环修改应使文档变脏");
    m.apply(Action::Undo);
    assert_eq!(m.project.as_ref().unwrap().loop_count, 0, "加载默认 0");
    m.apply(Action::Redo);
    // redo 后回到 SetLoop(true) 之后的状态 → 仍 0；再做一次反向验证
    m.apply(Action::SetLoopInfinite(false)); // loop = 1
    assert_eq!(m.project.as_ref().unwrap().loop_count, 1);
    m.apply(Action::Undo); // 回滚 SetLoopInfinite(false)
    assert_eq!(
        m.project.as_ref().unwrap().loop_count,
        0,
        "撤销必须恢复循环设置"
    );
}

#[test]
fn frame_ids_are_globally_unique_across_documents() {
    // 审查缺陷：FrameId 每仓库从 0 起算 → 跨文档纹理缓存/拖拽状态碰撞
    let a = Project::from_decoded(PathBuf::from("a.gif"), anim(3, 4, 4, 100)).unwrap();
    let b = Project::from_decoded(PathBuf::from("b.gif"), anim(3, 4, 4, 100)).unwrap();
    let set_a: std::collections::HashSet<_> = a.order.iter().copied().collect();
    let set_b: std::collections::HashSet<_> = b.order.iter().copied().collect();
    assert!(set_a.is_disjoint(&set_b), "不同文档的帧身份必须互不相同");
}

#[test]
fn estimate_single_flight_and_invalidation() {
    use gifsennin_rust::model::ExportFormat;
    let mut m = loaded(3);
    let fmt = ExportFormat::PngZip;
    m.apply(Action::EstimateRequested {
        generation: 1,
        format: fmt.clone(),
    });
    assert!(m.estimate.in_flight);
    // 在途时第二个请求被跳过（单飞）
    let effects = m.apply(Action::EstimateRequested {
        generation: 2,
        format: fmt.clone(),
    });
    assert!(effects.is_empty());
    // 结果落地
    m.apply(Action::EstimateReady {
        generation: 1,
        result: Ok(12345),
    });
    assert!(!m.estimate.in_flight);
    assert!(matches!(m.estimate.result, Some(Ok(12345))));
    // 帧编辑使估算失效
    m.apply(Action::Edit(Edit::DeleteFrames { ids: ids(&m, &[0]) }));
    assert!(m.estimate.result.is_none());
}

#[test]
fn playback_stops_and_no_repaint_loop_when_single_frame_left() {
    let mut m = loaded(2);
    m.playback.playing = true;
    m.apply(Action::Edit(Edit::DeleteFrames { ids: ids(&m, &[1]) }));
    assert!(!m.playback.playing, "帧数 < 2 应停播");
    assert!(m.playback_remaining_ms().is_none());
}

#[test]
fn open_rejected_while_loading() {
    let mut m = loaded(2);
    // 模拟加载中状态
    m.apply(Action::FilePicked(Some(PathBuf::from("x.gif"))));
    assert!(matches!(
        m.status,
        Some(gifsennin_rust::model::StatusEvent::Loading)
    ));
    let effects = m.apply(Action::OpenFileDialog);
    assert!(effects.is_empty(), "加载中不允许再开文件对话框");
    let effects = m.apply(Action::FilePicked(Some(PathBuf::from("y.gif"))));
    assert!(effects.is_empty(), "加载中不允许并发加载");
}

//! 静态图解码（PNG/JPG/JPEG/BMP → 单帧文档）。
//!
//! `image::open` 一次读入 → 统一转 RGBA8 → 包成单帧 `DecodedAnimation`。
//! 时长取占位值 100ms（UI 可后续编辑），loop_count = 0（无限）。

use super::{decode_err, DecodedAnimation, DecodedFrame};
use crate::errors::AppError;
use crate::model::{check_memory_budget, MEMORY_BUDGET_BYTES};
use std::path::Path;

/// 静态图单帧占位时长（ms）
const STATIC_FRAME_MS: u32 = 100;

/// 解码静态图为单帧文档。
pub fn load_static(path: &Path) -> Result<DecodedAnimation, AppError> {
    // 解码器默认 Limits（max_alloc 512MiB）与应用预算不一致：超 512MiB 的合法静态图
    // 会被误报为解码失败，显式对齐预算上限。
    let mut reader = image::ImageReader::open(path)
        .map_err(|e| decode_err(path, &e))?
        .with_guessed_format()
        .map_err(|e| decode_err(path, &e))?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MEMORY_BUDGET_BYTES);
    reader.limits(limits);
    let img = reader
        .decode()
        .map_err(|e| decode_err(path, &e))?
        .to_rgba8();
    let (width, height) = (img.width(), img.height());
    let rgba = img.into_raw();

    // 大图护栏：解码后 RGBA 超预算拒绝（静态图超 1.5GiB 罕见，防御性保留）
    check_memory_budget(rgba.len() as u64)?;

    Ok(DecodedAnimation {
        width,
        height,
        loop_count: 0, // 静态图无循环语义 → 无限
        frames: vec![DecodedFrame {
            rgba,
            duration_ms: STATIC_FRAME_MS,
        }],
    })
}

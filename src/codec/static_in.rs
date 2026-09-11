//! 静态图解码（PNG/JPG/JPEG/BMP → 单帧文档）。
//!
//! `image::open` 一次读入 → 统一转 RGBA8 → 包成单帧 `DecodedAnimation`。
//! 时长取占位值 100ms（UI 可后续编辑），loop_count = 0（无限）。

use super::{DecodedAnimation, DecodedFrame};
use crate::errors::AppError;
use crate::model::MEMORY_BUDGET_BYTES;
use std::path::Path;

/// 静态图单帧占位时长（ms）
const STATIC_FRAME_MS: u32 = 100;

/// 解码静态图为单帧文档。纯函数，无 GUI 依赖。
pub fn load_static(path: &Path) -> Result<DecodedAnimation, AppError> {
    let decode_err = |e: image::ImageError| AppError::Decode(format!("{}: {e}", path.display()));
    let io_err = |e: std::io::Error| AppError::Decode(format!("{}: {e}", path.display()));

    // 审查修复：image::open 走解码器默认 Limits（max_alloc 512MiB），与
    // 应用 1.5GiB 预算不一致——超 512MiB 的合法静态图会被错误归类为
    // 解码失败。此处显式对齐预算上限。
    let mut reader = image::ImageReader::open(path)
        .map_err(io_err)?
        .with_guessed_format()
        .map_err(io_err)?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MEMORY_BUDGET_BYTES);
    reader.limits(limits);
    let img = reader.decode().map_err(decode_err)?.to_rgba8();
    let (width, height) = (img.width(), img.height());
    let rgba = img.into_raw();

    // 大图护栏：解码后 RGBA 超预算拒绝（静态图超 1.5GiB 罕见，防御性保留）
    let needed = rgba.len() as u64;
    if needed > MEMORY_BUDGET_BYTES {
        return Err(AppError::TooLarge {
            needed,
            budget: MEMORY_BUDGET_BYTES,
        });
    }

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

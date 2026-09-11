//! PNG 单帧 / PNG 序列 zip 导出。
//!
//! ## 规格（实施时严格遵守）
//! - `encode_png(frame)`：`image::RgbaImage::from_raw(w, h, frame.as_rgba().to_vec())`
//!   → `image::DynamicImage::ImageRgba8(img).write_to(&mut Cursor, ImageFormat::Png)`
//!   → `Vec<u8>`。
//! - `encode_png_zip(frames)`：`zip` 2.x crate（已核实 registry 中 zip-2.4.2
//!   `src/write.rs`：`ZipWriter::new(W) -> ZipWriter<W>` 不可失败、
//!   `start_file(name, options) -> ZipResult<()>` 接收 `FileOptions<T>` 按值、
//!   `finish() -> ZipResult<W>`；`write::SimpleFileOptions = FileOptions<'static, ()>`）。
//!   - `ZipWriter::new(Cursor<Vec<u8>>)`，每帧
//!     `start_file(format!("frame_{:04}.png", i + 1), SimpleFileOptions::default()
//!     .compression_method(CompressionMethod::Deflated))` 后写入该帧 PNG 字节。
//!   - 返回 zip 的 `Vec<u8>`。
//! - 帧名从 1 开始（frame_0001.png …），与 UI 显示的帧号一致。
//! - 空帧序列 → `AppError::Encode("no frames")`。
use crate::errors::AppError;
use crate::model::frame::Frame;
use std::io::{Cursor, Write};
use std::sync::Arc;

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

/// 单帧编码为 PNG 字节。
pub fn encode_png(frame: &Frame) -> Result<Vec<u8>, AppError> {
    // Frame 不变量保证 from_raw 不会失败，但仍按可失败路径处理
    let img = image::RgbaImage::from_raw(frame.width, frame.height, frame.as_rgba().to_vec())
        .ok_or_else(|| AppError::Encode("png: frame buffer size mismatch".into()))?;
    let mut buf = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut buf, image::ImageFormat::Png)
        .map_err(|e| AppError::Encode(format!("png encode: {e}")))?;
    Ok(buf.into_inner())
}

/// 帧序列编码为 PNG zip 包（frame_0001.png 起，1-based 与 UI 帧号一致）。
pub fn encode_png_zip(frames: &[Arc<Frame>]) -> Result<Vec<u8>, AppError> {
    if frames.is_empty() {
        return Err(AppError::Encode("no frames".into()));
    }
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (i, frame) in frames.iter().enumerate() {
        let png = encode_png(frame)?;
        // FileOptions 按值传入 start_file，每次迭代重新构造（构造成本可忽略）
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file(format!("frame_{:04}.png", i + 1), options)
            .map_err(|e| AppError::Encode(format!("zip start_file: {e}")))?;
        zip.write_all(&png)
            .map_err(|e| AppError::Encode(format!("zip write: {e}")))?;
    }
    let cursor = zip
        .finish()
        .map_err(|e| AppError::Encode(format!("zip finish: {e}")))?;
    Ok(cursor.into_inner())
}

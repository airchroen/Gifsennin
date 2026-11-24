//! PNG 单帧 / PNG 序列 zip 导出。
//!
//! zip 内条目名从 1 开始（frame_0001.png …），与 UI 显示的帧号一致。
//!
//! 逐帧 PNG 压缩彼此独立且 CPU 密集，经 [`super::par_chunks`] 块内并行、
//! 按序写入 zip：条目名、顺序与内容都和逐帧串行压缩一致。
use super::par_chunks;
use crate::errors::AppError;
use crate::model::Frame;
use std::io::{Cursor, Write};
use std::sync::Arc;

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

/// 单帧编码为 PNG 字节。
pub fn encode_png(frame: &Frame) -> Result<Vec<u8>, AppError> {
    let img = frame.to_image();
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
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    par_chunks(
        frames,
        |frame| encode_png(frame),
        |start, pngs| {
            for (i, png) in pngs.iter().enumerate() {
                zip.start_file(format!("frame_{:04}.png", start + i + 1), options)
                    .map_err(|e| AppError::Encode(format!("zip start_file: {e}")))?;
                zip.write_all(png)
                    .map_err(|e| AppError::Encode(format!("zip write: {e}")))?;
            }
            Ok(())
        },
    )?;
    let cursor = zip
        .finish()
        .map_err(|e| AppError::Encode(format!("zip finish: {e}")))?;
    Ok(cursor.into_inner())
}

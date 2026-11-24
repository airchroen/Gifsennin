//! codec 层：编解码边界，纯函数。
//! 所有格式在入口处归一时间基为「每帧时长 ms」。

mod gif_in;
mod gif_out;
mod png_seq;
mod static_in;
mod webp_in;
mod webp_out;

use crate::errors::AppError;
use crate::model::{ExportFormat, Frame};
use rayon::prelude::*;
use std::fmt;
use std::path::Path;
use std::sync::Arc;

pub use gif_in::load_gif;
pub use gif_out::encode_gif;
pub use png_seq::{encode_png, encode_png_zip};
pub use static_in::load_static;
pub use webp_in::load_webp;
pub use webp_out::encode_webp;

/// 解码结果：帧已合成到画布尺寸（GIF 的 delta/disposal 在解码时处理完毕）
#[derive(Debug)]
pub struct DecodedAnimation {
    pub width: u32,
    pub height: u32,
    /// GIF 语义：0 = 无限循环；解析不到时默认 0
    pub loop_count: u16,
    pub frames: Vec<DecodedFrame>,
}

#[derive(Debug)]
pub struct DecodedFrame {
    /// 扁平 RGBA8，len == width * height * 4
    pub rgba: Vec<u8>,
    pub duration_ms: u32,
}

/// 按扩展名分派到具体解码器
pub fn load_any(path: &Path) -> Result<DecodedAnimation, AppError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "gif" => gif_in::load_gif(path),
        "webp" => webp_in::load_webp(path),
        "png" | "jpg" | "jpeg" | "bmp" => static_in::load_static(path),
        other => Err(AppError::UnsupportedFormat(other.to_string())),
    }
}

/// 按导出格式编码为字节；导出落盘与体积估算共用这一入口。
pub fn encode(
    frames: &[Arc<Frame>],
    loop_count: u16,
    format: &ExportFormat,
) -> Result<Vec<u8>, AppError> {
    match format {
        ExportFormat::Gif { quality } => gif_out::encode_gif(frames, loop_count, *quality),
        ExportFormat::Webp { quality, lossless } => {
            webp_out::encode_webp(frames, loop_count, *quality, *lossless)
        }
        ExportFormat::PngZip => png_seq::encode_png_zip(frames),
    }
}

/// 给解码错误补上路径上下文
pub(super) fn decode_err(path: &Path, e: impl fmt::Display) -> AppError {
    AppError::Decode(format!("{}: {e}", path.display()))
}

/// 帧级并行处理：帧间独立且 CPU 密集的编码步骤（GIF 量化 / PNG 压缩）交给 rayon。
/// 按线程数分块，块内并行、块间按序交给 `sink`（块起始下标，该块的编码结果），
/// 输出与逐帧串行处理一致；分块是为了限制中间缓冲的峰值内存。
pub(super) fn par_chunks<T, U, E>(
    items: &[T],
    encode: impl Fn(&T) -> Result<U, E> + Sync,
    mut sink: impl FnMut(usize, &[U]) -> Result<(), E>,
) -> Result<(), E>
where
    T: Sync,
    U: Send,
    E: Send,
{
    let chunk_len = rayon::current_num_threads().max(1);
    for (ci, chunk) in items.chunks(chunk_len).enumerate() {
        let encoded = chunk
            .par_iter()
            .map(&encode)
            .collect::<Result<Vec<_>, E>>()?;
        sink(ci * chunk_len, &encoded)?;
    }
    Ok(())
}

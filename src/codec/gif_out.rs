//! GIF 编码。
//!
//! gif crate 的几处行为：
//! - `Frame::from_rgba_speed` ⚠️ 会原地改写输入缓冲（NeuQuant 量化 + alpha 折叠），
//!   必须先克隆帧数据，绝不能碰仓库里的不可变帧数据。
//! - `set_repeat(Finite(0))` 会被 crate 跳过不写，语义为「只播一次」。
//! - `into_inner()` 显式写 trailer（不依赖 Drop）。
//!
//! 逐帧量化（NeuQuant）是 CPU 大头且帧间互不依赖：每帧的调色板与索引缓冲
//! 只取决于该帧自身像素，故经 [`super::par_chunks`] 块内并行、按序写出，
//! 输出字节与逐帧串行编码完全一致。
use super::par_chunks;
use crate::errors::AppError;
use crate::model::{Frame, GifQuality};
use std::sync::Arc;

// ─── 错误与单位换算 ─────────────────────────────────────────────

/// gif crate 的 EncodingError → AppError（枚举仅 Io / Format 两变体）。
fn map_enc_err(e: gif::EncodingError) -> AppError {
    match e {
        gif::EncodingError::Io(io) => AppError::Io(io),
        gif::EncodingError::Format(f) => AppError::Encode(f.to_string()),
    }
}

/// GIF 编码质量档位 → gif crate NeuQuant speed（1=最慢最好 … 30=最快最差）
fn neuquant_speed(quality: GifQuality) -> i32 {
    match quality {
        GifQuality::High => 3,
        GifQuality::Balanced => 10,
        GifQuality::Fast => 22,
    }
}

/// 帧时长 ms → GIF delay（百分之一秒）。round 后夹到 u16；0 保留 0。
fn delay_centis(duration_ms: u32) -> u16 {
    (f64::from(duration_ms) / 10.0)
        .round()
        .clamp(0.0, f64::from(u16::MAX)) as u16
}

/// 画布/帧尺寸换算：GIF 的尺寸字段是 u16，超出即无法编码
fn to_u16_dim(v: u32, what: &str) -> Result<u16, AppError> {
    u16::try_from(v).map_err(|_| AppError::Encode(format!("{what} {v} exceeds GIF u16 limit")))
}

// ─── 编码入口 ───────────────────────────────────────────────────

/// 单帧 → 带局部调色板的 GIF 帧
fn quantize(frame: &Frame, speed: i32) -> Result<gif::Frame<'static>, AppError> {
    // 用帧自身尺寸（与缓冲严格匹配，规避 from_rgba_speed 的长度断言；
    // 同尺寸是 model 层不变量，此处换算只为防御越界 panic）
    let fw = to_u16_dim(frame.width, "frame width")?;
    let fh = to_u16_dim(frame.height, "frame height")?;
    // ⚠️ from_rgba_speed 会原地改写输入缓冲，必须先克隆
    let mut rgba = frame.as_rgba().to_vec();
    let mut gframe = gif::Frame::from_rgba_speed(fw, fh, &mut rgba, speed);
    gframe.delay = delay_centis(frame.duration_ms);
    Ok(gframe)
}

/// 把帧序列编码为 GIF 字节。画布尺寸取首帧（全部帧同尺寸是 model 层不变量）。
pub fn encode_gif(
    frames: &[Arc<Frame>],
    loop_count: u16,
    quality: GifQuality,
) -> Result<Vec<u8>, AppError> {
    let first = frames
        .first()
        .ok_or_else(|| AppError::Encode("no frames".into()))?;

    let canvas_w = to_u16_dim(first.width, "canvas width")?;
    let canvas_h = to_u16_dim(first.height, "canvas height")?;

    let mut out = Vec::new();
    let mut encoder = gif::Encoder::new(&mut out, canvas_w, canvas_h, &[]).map_err(map_enc_err)?;

    // 循环语义（model 层）：0 = 无限循环
    encoder
        .set_repeat(match loop_count {
            0 => gif::Repeat::Infinite,
            n => gif::Repeat::Finite(n),
        })
        .map_err(map_enc_err)?;

    let speed = neuquant_speed(quality);
    par_chunks(
        frames,
        |frame| quantize(frame, speed),
        |_start, chunk| {
            for gframe in chunk {
                encoder.write_frame(gframe).map_err(map_enc_err)?;
            }
            Ok(())
        },
    )?;

    // 显式写 trailer 并归还输出缓冲（不依赖 Drop）
    encoder.into_inner().map_err(AppError::Io)?;
    Ok(out)
}

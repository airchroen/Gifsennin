//! GIF 编码。
//!
//! gif-0.13.3 API（已核实 registry 源码 `encoder.rs` / `common.rs` / `lib.rs`）：
//! - `Encoder::new(w: W, width: u16, height: u16, global_palette: &[u8])
//!   -> Result<Self, EncodingError>`；`from_rgba_speed` 每帧自带局部调色板，
//!   全局调色板传空切片即可。
//! - `set_repeat(gif::Repeat::Infinite | gif::Repeat::Finite(u16))` 写
//!   NETSCAPE2.0 循环扩展（Finite(0) 会被 crate 跳过不写，语义为「只播一次」）。
//! - `Frame::from_rgba_speed(width: u16, height: u16, pixels: &mut [u8],
//!   speed: i32) -> Frame`：⚠️ 原地改写输入缓冲（NeuQuant 量化 + alpha 折叠），
//!   必须先克隆帧数据，绝不能碰仓库里的不可变帧数据。
//! - `into_inner()`：显式写 trailer 并归还底层 writer（不依赖 Drop 行为）。
use super::gif_speed_for;
use crate::errors::AppError;
use crate::model::frame::Frame;
use crate::model::GifQuality;
use std::io::{self, Write};
use std::sync::Arc;

// ─── 字节计数包装器 ─────────────────────────────────────────────

/// 委托写入并统计实际送达底层 writer 的字节数（导出结果报告用）
struct CountingWriter<'a> {
    inner: &'a mut dyn Write,
    count: u64,
}

impl Write for CountingWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.count += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

// ─── 错误与单位换算 ─────────────────────────────────────────────

/// gif crate 的 EncodingError → AppError（枚举仅 Io / Format 两变体）。
fn map_enc_err(e: gif::EncodingError) -> AppError {
    match e {
        gif::EncodingError::Io(io) => AppError::Io(io),
        gif::EncodingError::Format(f) => AppError::Encode(f.to_string()),
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

/// 把帧序列编码为 GIF 写入 `out`，返回写入的字节数。
/// 画布尺寸取首帧（全部帧同尺寸是 model 层不变量）。
pub fn encode_gif(
    frames: &[Arc<Frame>],
    loop_count: u16,
    quality: GifQuality,
    out: &mut dyn Write,
) -> Result<u64, AppError> {
    let first = frames
        .first()
        .ok_or_else(|| AppError::Encode("no frames".into()))?;

    // GIF 逻辑屏尺寸是 u16 字段，超出即无法编码
    let canvas_w = to_u16_dim(first.width, "canvas width")?;
    let canvas_h = to_u16_dim(first.height, "canvas height")?;

    let mut writer = CountingWriter {
        inner: out,
        count: 0,
    };
    let mut encoder =
        gif::Encoder::new(&mut writer, canvas_w, canvas_h, &[]).map_err(map_enc_err)?;

    // 循环语义（model 层）：0 = 无限循环
    encoder
        .set_repeat(match loop_count {
            0 => gif::Repeat::Infinite,
            n => gif::Repeat::Finite(n),
        })
        .map_err(map_enc_err)?;

    let speed = gif_speed_for(quality);
    for frame in frames {
        // ⚠️ from_rgba_speed 会原地改写输入缓冲，必须先克隆
        let mut rgba = frame.as_rgba().to_vec();

        // 用帧自身尺寸（与缓冲严格匹配，规避 from_rgba_speed 的长度断言；
        // 同尺寸是 model 层不变量，此处换算只为防御越界 panic）
        let fw = to_u16_dim(frame.width, "frame width")?;
        let fh = to_u16_dim(frame.height, "frame height")?;

        let mut gframe = gif::Frame::from_rgba_speed(fw, fh, &mut rgba, speed);
        gframe.delay = delay_centis(frame.duration_ms);
        encoder.write_frame(&gframe).map_err(map_enc_err)?;
    }

    // 显式写 trailer 并归还计数 writer（不依赖 Drop）
    let writer = encoder.into_inner().map_err(AppError::Io)?;
    writer.inner.flush()?;
    Ok(writer.count)
}

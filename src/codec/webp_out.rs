//! 动画 WebP 编码（webp 0.3.0 AnimEncoder 路径）。
//!
//! 已核实的 crate API（webp-0.3.0 / libwebp-sys-0.9.6 源码）：
//! - `WebPConfig` 只有 `new() -> Result<Self, ()>` 与 `new_with_preset(preset, quality)`，
//!   没有 `new_with_quality`；quality/lossless 为公开字段（`f32` / `c_int`），直接赋值。
//! - `AnimEncoder::new(w, h, &config)`；`set_loop_count(i32)`——libwebp mux 语义
//!   与 GIF 一致：0 = 无限循环，>0 = 播放次数，u16 直接透传。
//! - `AnimFrame::from_rgba(&[u8], w, h, timestamp_ms) -> AnimFrame`（借用像素缓冲），
//!   因此先克隆 Arc 到本地 Vec 保证缓冲存活覆盖 add_frame/encode（零像素拷贝）。
//! - `try_encode() -> Result<WebPMemory, AnimEncodeError>`（`encode()` 失败会 panic，
//!   故用 try_encode 并映射 `AppError::Encode`）。
//! - `WebPMemory: Deref<Target = [u8]>` → `.to_vec()` 转自有数据返回。
//!
//! ## 已知 crate 局限（integration 阶段核实）
//! webp-0.3.0 的 `anim_encode` 以时间戳 0 收尾（`WebPAnimEncoderAdd(NULL, 0)`），
//! libwebp 对末帧时长取「前序帧时长均值」（anim_encode.c:1542-1546）——
//! 末帧时长无法经此 API 精确保留，解码侧会得到均值近似值。
use crate::errors::AppError;
use crate::model::frame::Frame;
use std::sync::Arc;
use webp::{AnimEncoder, AnimFrame, WebPConfig};

/// 逐帧 RGBA8 → 动画 WebP。画布尺寸取首帧，时间戳为前序帧时长累加（首帧 0）。
pub fn encode_webp(
    frames: &[Arc<Frame>],
    loop_count: u16,
    quality: f32,
    lossless: bool,
) -> Result<Vec<u8>, AppError> {
    if frames.is_empty() {
        return Err(AppError::Encode("no frames".into()));
    }
    let (canvas_w, canvas_h) = (frames[0].width, frames[0].height);

    // 帧必须与画布同尺寸：AnimEncoder 内部按画布尺寸原生读取缓冲，
    // 尺寸不符会造成越界读，这里显式拦截（model 层不变量之外的防御）。
    if frames
        .iter()
        .any(|f| f.width != canvas_w || f.height != canvas_h)
    {
        return Err(AppError::Encode("frame size mismatch with canvas".into()));
    }

    let mut config =
        WebPConfig::new().map_err(|_| AppError::Encode("WebPConfig init failed".into()))?;
    config.quality = quality.clamp(0.0, 100.0);
    config.lossless = i32::from(lossless);

    // 克隆 Arc（仅指针拷贝，不复制像素）到本地 Vec，
    // from_rgba 的借用生命周期由该 Vec 保证覆盖 encoder 的 add_frame/encode。
    let buffers: Vec<Arc<Vec<u8>>> = frames.iter().map(|f| f.arc_data()).collect();
    let slices: Vec<&[u8]> = buffers.iter().map(|b| b.as_slice()).collect();

    let mut encoder = AnimEncoder::new(canvas_w, canvas_h, &config);
    // 0 = 无限循环（libwebp 与 GIF 语义一致），>0 = 播放次数
    encoder.set_loop_count(i32::from(loop_count));

    let mut timestamp: i64 = 0;
    for (frame, slice) in frames.iter().zip(&slices) {
        // WebP 时间戳为 i32 ms，极端长动画累计溢出时夹到 i32::MAX
        let ts = timestamp.min(i32::MAX as i64) as i32;
        encoder.add_frame(AnimFrame::from_rgba(slice, canvas_w, canvas_h, ts));
        timestamp += i64::from(frame.duration_ms);
    }

    let memory = encoder
        .try_encode()
        .map_err(|e| AppError::Encode(format!("webp anim encode: {e:?}")))?;
    Ok(memory.to_vec())
}

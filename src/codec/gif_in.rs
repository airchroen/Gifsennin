//! GIF 解码。
//!
//! ## 实现要点（已核实 registry 源码）
//! - `image::codecs::gif::GifDecoder`（BufReader<File>，64KB 缓冲）的
//!   `into_frames()` 逐帧增量解码；image crate 内部已做画布合成
//!   （delta/disposal 处理完毕，`GifFrameIterator` 维护 non_disposed_frame），
//!   每帧尺寸恒等于画布。
//! - **逐帧累计内存**：每解码一帧把 `rgba.len()` 累加到 `cum`，
//!   `cum > MEMORY_BUDGET_BYTES` 立即返回 `AppError::TooLarge` 并丢弃已解码帧
//!   （共识 Q4 护栏：不许 `collect_frames()` 一把梭）。
//! - 时长：`frame.delay().numer_denom_ms()` 返回的 (num, den) 已是毫秒分数
//!   （image 内部 delay 单位 10ms → Ratio(delay*10, 1)），
//!   `duration_ms = (num / den).round()`。严禁再乘 1000；0 保留 0
//!   （播放层按 MIN_PLAYBACK_MS 下限处理）。
//! - loop_count：image crate 不暴露。gif-0.13 的 `Decoder` 在 `read_info`
//!   阶段即解析 NETSCAPE2.0 块并暴露 `repeat()`（reader/mod.rs `init`：
//!   HeaderEnd 前的 `Decoded::Repetitions` 事件写入 `self.repeat`），
//!   故对同一文件再开一个轻量 header 解码器读取。解析失败/不存在 → 0
//!   （无限循环，浏览器默认行为；`Repeat::default() == Finite(0)` 同样归 0）。
use super::{DecodedAnimation, DecodedFrame};
use crate::errors::AppError;
use crate::model::MEMORY_BUDGET_BYTES;
use image::AnimationDecoder;
use image::ImageDecoder;
use std::io::BufReader;
use std::path::Path;

pub fn load_gif(path: &Path) -> Result<DecodedAnimation, AppError> {
    // loop_count 与帧解码解耦：独立 header 解析失败只影响循环语义，不阻塞加载
    let loop_count = read_loop_count(path);

    let file = std::fs::File::open(path).map_err(|e| decode_err(path, &e))?;
    let buffered = BufReader::with_capacity(64 * 1024, file);
    let mut decoder =
        image::codecs::gif::GifDecoder::new(buffered).map_err(|e| decode_err(path, &e))?;
    let (width, height) = decoder.dimensions();

    // 审查修复：GifDecoder::new 默认 Limits::no_limits()，帧迭代器会在
    // 累计护栏生效前先分配整幅画布（40 字节声明 65535² 的恶意 GIF 即可
    // 触发 ~16GiB 分配直接 abort）。此处把分配上限交给 image crate，
    // 超限时迭代器返回 LimitError → 走 AppError::Decode 优雅失败。
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MEMORY_BUDGET_BYTES);
    decoder
        .set_limits(limits)
        .map_err(|e| decode_err(path, &e))?;

    let mut frames = Vec::new();
    let mut cum: u64 = 0;
    for frame in decoder.into_frames() {
        let frame = frame.map_err(|e| decode_err(path, &e))?;
        let (num, den) = frame.delay().numer_denom_ms();
        let duration_ms = if den == 0 {
            0
        } else {
            (f64::from(num) / f64::from(den)).round() as u32
        };
        let rgba = frame.into_buffer().into_raw();
        cum += rgba.len() as u64;
        if cum > MEMORY_BUDGET_BYTES {
            return Err(AppError::TooLarge {
                needed: cum,
                budget: MEMORY_BUDGET_BYTES,
            });
        }
        frames.push(DecodedFrame { rgba, duration_ms });
    }

    if frames.is_empty() {
        return Err(decode_err(path, "gif: no frames"));
    }

    Ok(DecodedAnimation {
        width,
        height,
        loop_count,
        frames,
    })
}

/// 给 Decode 错误补上路径上下文
fn decode_err(path: &Path, e: impl std::fmt::Display) -> AppError {
    AppError::Decode(format!("{}: {e}", path.display()))
}

/// NETSCAPE2.0 循环次数（GIF 语义：0 = 无限）。
/// gif crate 在 read_info 阶段完成解析；任何失败都兜底为 0。
fn read_loop_count(path: &Path) -> u16 {
    let Ok(file) = std::fs::File::open(path) else {
        return 0;
    };
    let reader = match gif::Decoder::new(BufReader::new(file)) {
        Ok(r) => r,
        Err(_) => return 0,
    };
    match reader.repeat() {
        gif::Repeat::Infinite => 0,
        // Finite(0) = 无 NETSCAPE 块（只播一次）→ 规格定为 0（无限兜底）；
        // Finite(n>0) = 追加播放次数，与编码侧 Repeat::Finite(n) 对称往返
        gif::Repeat::Finite(n) => n,
    }
}

//! 纹理缓存：
//! - 全分辨率纹理**只有当前预览帧一张**（单槽，换帧即换）
//! - 缩略图走 CPU 一次性降采样到 ≤128px 的小纹理，带真 LRU 驱逐
//! - key 一律用 FrameId——编辑产生新 id，旧纹理随 GC 自然失效，
//!   从根上消灭「改了帧但纹理还是旧的」这类 index-key 缓存 bug。

use crate::model::{Frame, FrameId};
use eframe::egui::{self, ColorImage, TextureId, TextureOptions};
use std::collections::HashMap;

const THUMB_MAX_EDGE: u32 = 128;
/// 缩略图纹理默认上限。128px 缩略图 ≈ 64KB/张，300 张 ≈ 20MB VRAM。
/// 加载文档时按帧数自适应上调（set_cap），避免 >400 帧的文档
/// 因容量不足在重绘间反复逐出/重建。
const THUMB_CACHE_DEFAULT_CAP: usize = 400;

/// 缓存中的一张缩略图：纹理句柄 + 最近一次访问序号（LRU 驱逐依据）
struct Thumb {
    handle: egui::TextureHandle,
    last_used: u64,
}

pub struct TextureCache {
    full: Option<(FrameId, egui::TextureHandle)>,
    thumbs: HashMap<FrameId, Thumb>,
    /// 访问序号发生器：每次 thumb() 自增，序号越大越新
    clock: u64,
    cap: usize,
}

impl Default for TextureCache {
    fn default() -> Self {
        Self::new()
    }
}

impl TextureCache {
    pub fn new() -> Self {
        Self {
            full: None,
            thumbs: HashMap::new(),
            clock: 0,
            cap: THUMB_CACHE_DEFAULT_CAP,
        }
    }

    /// 按当前文档帧数自适应容量（文档加载后调用）
    pub fn set_cap(&mut self, cap: usize) {
        self.cap = cap;
        self.evict();
    }

    /// 预览帧全分辨率纹理（单槽）
    pub fn full(&mut self, ctx: &egui::Context, id: FrameId, frame: &Frame) -> TextureId {
        let (_, handle) = self
            .full
            .get_or_insert_with(|| (id, Self::load_full(ctx, id, frame)));
        handle.id()
    }

    fn load_full(ctx: &egui::Context, id: FrameId, frame: &Frame) -> egui::TextureHandle {
        let image = ColorImage::from_rgba_unmultiplied(
            [frame.width as usize, frame.height as usize],
            frame.as_rgba(),
        );
        ctx.load_texture(format!("full_{id:?}"), image, TextureOptions::LINEAR)
    }

    /// 缩略图纹理（小尺寸，真 LRU）。首次创建时 CPU 降采样一次，之后命中缓存。
    pub fn thumb(&mut self, ctx: &egui::Context, id: FrameId, frame: &Frame) -> TextureId {
        self.clock += 1;
        if let Some(thumb) = self.thumbs.get_mut(&id) {
            thumb.last_used = self.clock;
            return thumb.handle.id();
        }
        let handle = Self::build_thumb(ctx, id, frame);
        let tex = handle.id();
        self.thumbs.insert(
            id,
            Thumb {
                handle,
                last_used: self.clock,
            },
        );
        self.evict();
        tex
    }

    fn build_thumb(ctx: &egui::Context, id: FrameId, frame: &Frame) -> egui::TextureHandle {
        let src = frame.to_image();
        let small = image::imageops::thumbnail(&src, THUMB_MAX_EDGE, THUMB_MAX_EDGE);
        let image = ColorImage::from_rgba_unmultiplied(
            [small.width() as usize, small.height() as usize],
            &small.into_raw(),
        );
        ctx.load_texture(format!("thumb_{id:?}"), image, TextureOptions::LINEAR)
    }

    /// LRU 驱逐：超容时移除最久未访问的缩略图
    fn evict(&mut self) {
        while self.thumbs.len() > self.cap {
            let Some((&oldest, _)) = self.thumbs.iter().min_by_key(|(_, t)| t.last_used) else {
                break;
            };
            self.thumbs.remove(&oldest);
        }
    }

    pub fn clear(&mut self) {
        self.full = None;
        self.thumbs.clear();
    }

    pub fn thumb_count(&self) -> usize {
        self.thumbs.len()
    }
}

//! 纹理缓存（共识 Q4 纹理分级）：
//! - 全分辨率纹理**只有当前预览帧一张**（单槽，换帧即换）
//! - 缩略图走 CPU 一次性降采样到 ≤128px 的小纹理，带真 LRU 驱逐
//! - key 一律用 FrameId——编辑产生新 id，旧纹理随 GC 自然失效，
//!   从根上消灭「改了帧但纹理还是旧的」这类 index-key 缓存 bug。

use crate::model::frame::{Frame, FrameId};
use eframe::egui::{self, ColorImage, TextureId, TextureOptions};
use std::collections::{HashMap, VecDeque};

const THUMB_MAX_EDGE: u32 = 128;
/// 缩略图纹理默认上限。128px 缩略图 ≈ 64KB/张，300 张 ≈ 20MB VRAM。
/// 加载文档时按帧数自适应上调（set_cap），避免 >400 帧的文档
/// 因容量不足在重绘间反复逐出/重建（审查修复 LRU 抖动）。
const THUMB_CACHE_DEFAULT_CAP: usize = 400;

pub struct TextureCache {
    full: Option<(FrameId, egui::TextureHandle)>,
    thumbs: HashMap<FrameId, egui::TextureHandle>,
    /// 访问顺序（LRU）：最近使用的在尾部
    thumb_order: VecDeque<FrameId>,
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
            thumb_order: VecDeque::new(),
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
        if !self.full.as_ref().is_some_and(|(i, _)| *i == id) {
            let image = ColorImage::from_rgba_unmultiplied(
                [frame.width as usize, frame.height as usize],
                frame.as_rgba(),
            );
            let handle = ctx.load_texture(format!("full_{id:?}"), image, TextureOptions::LINEAR);
            self.full = Some((id, handle));
        }
        self.full.as_ref().expect("just set").1.id()
    }

    /// 缩略图纹理（小尺寸，真 LRU）。首次创建时 CPU 降采样一次，之后命中缓存。
    pub fn thumb(&mut self, ctx: &egui::Context, id: FrameId, frame: &Frame) -> TextureId {
        if self.thumbs.contains_key(&id) {
            self.touch(id);
            return self.thumbs[&id].id();
        }
        let handle = Self::build_thumb(ctx, id, frame);
        self.thumbs.insert(id, handle);
        self.thumb_order.push_back(id);
        self.evict();
        self.thumbs[&id].id()
    }

    fn build_thumb(ctx: &egui::Context, id: FrameId, frame: &Frame) -> egui::TextureHandle {
        let src = image::RgbaImage::from_raw(
            frame.width,
            frame.height,
            frame.as_rgba().to_vec(), // 首次生成缩略图的一次性拷贝
        )
        .expect("Frame invariant: buffer matches dimensions");
        let small = image::imageops::thumbnail(&src, THUMB_MAX_EDGE, THUMB_MAX_EDGE);
        let image = ColorImage::from_rgba_unmultiplied(
            [small.width() as usize, small.height() as usize],
            &small.into_raw(),
        );
        ctx.load_texture(format!("thumb_{id:?}"), image, TextureOptions::LINEAR)
    }

    fn touch(&mut self, id: FrameId) {
        if let Some(pos) = self.thumb_order.iter().position(|x| *x == id) {
            self.thumb_order.remove(pos);
            self.thumb_order.push_back(id);
        }
    }

    fn evict(&mut self) {
        while self.thumbs.len() > self.cap {
            if let Some(oldest) = self.thumb_order.pop_front() {
                self.thumbs.remove(&oldest);
            } else {
                break;
            }
        }
    }

    pub fn clear(&mut self) {
        self.full = None;
        self.thumbs.clear();
        self.thumb_order.clear();
    }

    pub fn thumb_count(&self) -> usize {
        self.thumbs.len()
    }
}

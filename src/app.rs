// app.rs：eframe 装配层（共识 Q5 Action-MVU）。
// 拥有 Model + 纹理缓存 + UI 瞬态；每帧：tick 播放 → drain worker 通道 →
// 快捷键 → 渲染面板收集 Action → 统一 apply + 执行 Effect → 安排重绘。

use crate::errors::AppError;
use crate::model::{Action, Edit, Model};
use crate::render::{self, fonts, theme, TextureCache, UiState};
use crate::workers::{self, Workers};
use eframe::{egui, App};
use egui::Context;
use std::sync::mpsc::Receiver;
use std::time::Duration;

pub struct GifSenninApp {
    model: Model,
    workers: Workers,
    rx: Receiver<Action>,
    textures: TextureCache,
    ui: UiState,
    last_time: f64,
    /// 启动文件参数（`gifsennin <file>`）：首帧投递一次
    startup_file: Option<std::path::PathBuf>,
}

pub fn run() -> Result<(), AppError> {
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .format_timestamp_secs()
        .init();
    log::info!("=== GIFSennin v1 starting ===");

    rust_i18n::set_locale("zh-CN"); // 默认中文（共识 Q9）

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 900.0])
            .with_title("GIFSennin")
            .with_min_inner_size([1024.0, 700.0]),
        ..Default::default()
    };

    // `gifsennin <file>`：直接打开指定文件（不存在的路径走正常加载失败提示）
    let start_file = std::env::args().nth(1).map(std::path::PathBuf::from);

    eframe::run_native(
        "GIFSennin",
        options,
        Box::new(move |cc| Ok(Box::new(GifSenninApp::new(cc, start_file)))),
    )
    .map_err(|e| AppError::Other(format!("failed to run application: {e}")))?;

    log::info!("=== GIFSennin v1 exiting ===");
    Ok(())
}

impl GifSenninApp {
    pub fn new(cc: &eframe::CreationContext<'_>, startup_file: Option<std::path::PathBuf>) -> Self {
        fonts::install(&cc.egui_ctx);
        theme::apply(&cc.egui_ctx);
        let (workers, rx) = workers::channel();
        Self {
            model: Model::new(),
            workers,
            rx,
            textures: TextureCache::new(),
            ui: UiState::new(),
            last_time: 0.0,
            startup_file,
        }
    }

    /// 单一 Action 分发口：脏文档的破坏性操作先确认（清空/打开新文件），
    /// 文档切换时重置视图瞬态与纹理缓存；其余进模型；执行 Effect；触发重绘。
    fn dispatch(&mut self, action: Action, ctx: &Context) {
        let action = match action {
            Action::ClearRequested => {
                if self.model.dirty() {
                    self.ui.confirm_clear = true;
                    self.ui.pending_open = None;
                    return;
                }
                Action::ClearConfirmed
            }
            // 打开新文件同样丢弃未导出的编辑（审查修复：原先静默丢弃）
            Action::FilePicked(Some(path)) if self.model.dirty() => {
                self.ui.confirm_clear = true;
                self.ui.pending_open = Some(path);
                return;
            }
            a => a,
        };
        let doc_switch = matches!(
            action,
            Action::FilePicked(Some(_)) | Action::LoadFinished(_) | Action::ClearConfirmed
        );
        let effects = self.model.apply(action);
        for e in effects {
            self.workers.exec(e, &self.model);
        }
        if doc_switch {
            // 文档切换：释放旧纹理、调整缩略图容量、关闭悬挂的对话框草稿
            let frame_cap = self
                .model
                .project
                .as_ref()
                .map(|p| p.frame_count())
                .unwrap_or(0);
            self.textures.clear();
            self.textures.set_cap(400.max(frame_cap) + 64);
            self.ui.export_open = false;
            self.ui.estimate_sent = None;
            self.ui.estimate_changed = None;
            self.ui.crop = None;
            self.ui.resize_open = false;
            self.ui.pending_open = None;
        }
        ctx.request_repaint();
    }

    fn drain_workers(&mut self, ctx: &Context) {
        while let Ok(a) = self.rx.try_recv() {
            self.dispatch(a, ctx);
        }
    }

    /// 全局快捷键（共识 Q7/Q8）。文本输入聚焦时让路。
    fn shortcuts(&mut self, ctx: &Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        let selection: Vec<_> = self
            .model
            .project
            .as_ref()
            .map(|p| p.selection_in_order())
            .unwrap_or_default();
        let mut actions = Vec::new();
        ctx.input(|i| {
            let ctrl = i.modifiers.ctrl;
            if ctrl && !i.modifiers.shift && i.key_pressed(egui::Key::Z) {
                actions.push(Action::Undo);
            }
            if (ctrl && i.modifiers.shift && i.key_pressed(egui::Key::Z))
                || (ctrl && i.key_pressed(egui::Key::Y))
            {
                actions.push(Action::Redo);
            }
            if i.key_pressed(egui::Key::Delete) && !selection.is_empty() {
                actions.push(Action::Edit(Edit::DeleteFrames {
                    ids: selection.clone(),
                }));
            }
            if ctrl && i.key_pressed(egui::Key::A) {
                actions.push(Action::SelectAll);
            }
            if i.key_pressed(egui::Key::Space) {
                actions.push(Action::PlayPause);
            }
            // 步进（预览按钮 tooltip 承诺的 ←/→，审查修复补齐绑定）
            if i.key_pressed(egui::Key::ArrowLeft) {
                actions.push(Action::StepBack);
            }
            if i.key_pressed(egui::Key::ArrowRight) {
                actions.push(Action::StepForward);
            }
        });
        for a in actions {
            self.dispatch(a, ctx);
        }
    }
}

impl App for GifSenninApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // 0. 启动文件参数：首帧投递一次（新鲜文档不触发脏确认弹窗）
        if let Some(path) = self.startup_file.take() {
            self.dispatch(Action::FilePicked(Some(path)), ctx);
        }

        // 1. 时间推进与播放 tick
        let now = ctx.input(|i| i.time);
        let dt = if self.last_time == 0.0 {
            0.0
        } else {
            now - self.last_time
        };
        self.last_time = now;
        self.model.tick_playback(dt);

        // 2. worker 结果回流
        self.drain_workers(ctx);

        // 3. 快捷键
        self.shortcuts(ctx);

        // 4. 渲染面板（纯视图，收集 Action）
        //    借用拆分：model 只读给视图，textures/ui 可变。
        let mut actions = Vec::new();
        {
            let model = &self.model;
            let textures = &mut self.textures;
            let ui_state = &mut self.ui;

            egui::TopBottomPanel::top("toolbar")
                .frame(theme::panel_frame())
                .min_height(56.0)
                .show(ctx, |ui| {
                    actions.extend(render::toolbar::render_toolbar(ui, model, ui_state));
                });

            egui::SidePanel::left("frames")
                .frame(theme::panel_frame())
                .resizable(false)
                .exact_width(300.0)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        actions.extend(render::thumbnails::render_thumbnails(
                            ui, ctx, model, textures, ui_state,
                        ));
                    });
                });

            egui::SidePanel::right("props")
                .frame(theme::panel_frame())
                .resizable(false)
                .exact_width(300.0)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        actions.extend(render::sidebar::render_sidebar(
                            ui, ctx, model, textures, ui_state,
                        ));
                    });
                });

            egui::CentralPanel::default()
                .frame(theme::central_frame())
                .show(ctx, |ui| {
                    actions.extend(render::preview::render_preview(
                        ui, ctx, model, textures, ui_state,
                    ));
                });

            // 模态对话框（导出/缩放/清空确认）
            actions.extend(render::dialogs::render_dialogs(ctx, model, ui_state));
        }

        // 5. 统一分发
        for a in actions {
            self.dispatch(a, ctx);
        }

        // 6. 重绘调度：播放中按当前帧剩余时长请求；后台任务挂起时持续轮询。
        if let Some(remaining_ms) = self.model.playback_remaining_ms() {
            ctx.request_repaint_after(Duration::from_millis(remaining_ms as u64 + 2));
        }
        if self.model.busy() {
            ctx.request_repaint();
        }
        // 估算在途也要轮询：mpsc 无唤醒，不请求重绘的话结果落地后
        // 界面会停在「估算中…」直到下一次输入（审查修复重绘饥饿）
        if self.model.estimate.in_flight {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }
}

//! GIFSennin v1 — 桌面帧级 GIF/动画 WebP 编辑器
//!
//! 架构（Action-MVU）：
//! - `model/`   纯数据 + 纯函数（无头可测）
//! - `codec/`   编解码边界（GIF/WebP/PNG，纯函数）
//! - `render/`  egui 视图（无业务逻辑，产出 Action）
//! - `workers/` 线程任务（加载/导出/估算）
//! - `app.rs`   eframe 装配：拥有 Model，分发 Action，执行 Effect
//!
//! lib + 薄 `main.rs` 拆分只为让集成测试（`tests/`）能无头驱动 model / codec，
//! 因此这两个模块连同 `errors`、i18n 便捷包装对外可见；`app`、`workers`
//! 是装配与线程细节，crate 内私有。

mod app;
pub mod codec;
pub mod errors;
pub mod model;
pub mod render;
mod workers;

// i18n 文案表必须在 crate 根注册（t! 宏依赖根级辅助函数）
rust_i18n::i18n!("locales");

pub use app::run;
pub use errors::AppError;

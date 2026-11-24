//! workers：线程任务执行器。std::thread + channel，
//! 量小不值得上异步运行时。所有 worker 完成后把 Action 发回主循环，
//! app 层 drain 并 apply；工作线程绝不触碰 GUI。

use crate::codec;
use crate::errors::AppError;
use crate::model::{Action, Effect, ExportOutcome, Project};
use std::fs;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Instant;

pub struct Workers {
    tx: Sender<Action>,
}

/// 创建 worker 中枢与主循环持有的接收端
pub fn channel() -> (Workers, mpsc::Receiver<Action>) {
    let (tx, rx) = mpsc::channel();
    (Workers { tx }, rx)
}

impl Workers {
    /// 执行模型产出的副作用
    pub fn exec(&self, effect: Effect) {
        match effect {
            Effect::SpawnOpenDialog => self.spawn(|| {
                let picked = rfd::FileDialog::new()
                    .add_filter(
                        "GIF/WebP/Images",
                        &["gif", "webp", "png", "jpg", "jpeg", "bmp"],
                    )
                    .set_title("GIFSennin")
                    .pick_file();
                Action::FilePicked(picked)
            }),
            Effect::SpawnSaveDialog { default_name } => self.spawn(move || {
                let picked = rfd::FileDialog::new()
                    .set_file_name(default_name)
                    .set_title("GIFSennin")
                    .save_file();
                Action::SavePathPicked(picked)
            }),
            Effect::SpawnLoad(path) => self.spawn(move || {
                let result = codec::load_any(&path)
                    .and_then(|anim| Project::from_decoded(path.clone(), anim));
                Action::LoadFinished(result)
            }),
            Effect::SpawnExport {
                frames,
                loop_count,
                format,
                path,
            } => self.spawn(move || {
                let start = Instant::now();
                // 先编码到内存再一次性落盘：编码失败不会留下半截文件
                let bytes = codec::encode(&frames, loop_count, &format).and_then(|data| {
                    fs::write(&path, &data).map_err(AppError::Io)?;
                    Ok(data.len() as u64)
                });
                let result = bytes.map(|bytes| ExportOutcome {
                    path,
                    bytes,
                    ms: start.elapsed().as_millis() as u64,
                });
                Action::ExportFinished(result)
            }),
            Effect::SpawnEstimate {
                generation,
                frames,
                loop_count,
                format,
            } => self.spawn(move || {
                // 体积估算与导出走同一条编码路径，只是不落盘
                let result =
                    codec::encode(&frames, loop_count, &format).map(|data| data.len() as u64);
                Action::EstimateReady { generation, result }
            }),
        }
    }

    /// 起线程执行 `work`，把产出的 Action 发回主循环（接收端已随 app 退出则丢弃）
    fn spawn(&self, work: impl FnOnce() -> Action + Send + 'static) {
        let tx = self.tx.clone();
        thread::spawn(move || {
            let _ = tx.send(work());
        });
    }
}

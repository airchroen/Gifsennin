// workers（共识 Q5）：线程任务执行器。std::thread + channel，
// 量小不值得上异步运行时。所有 worker 完成后把 Action 发回主循环，
// app 层 drain 并 apply；工作线程绝不触碰 GUI。

use crate::codec;
use crate::errors::AppError;
use crate::model::frame::Frame;
use crate::model::{Action, Effect, ExportFormat, ExportOutcome, Model, Project};
use std::fs;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
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
    /// 执行模型产出的副作用（app 层调用）
    pub fn exec(&self, effect: Effect, _model: &Model) {
        match effect {
            Effect::SpawnOpenDialog => {
                let tx = self.tx.clone();
                thread::spawn(move || {
                    let picked = rfd::FileDialog::new()
                        .add_filter(
                            "GIF/WebP/Images",
                            &["gif", "webp", "png", "jpg", "jpeg", "bmp"],
                        )
                        .set_title("GIFSennin")
                        .pick_file();
                    let _ = tx.send(Action::FilePicked(picked));
                });
            }
            Effect::SpawnSaveDialog { default_name } => {
                let tx = self.tx.clone();
                thread::spawn(move || {
                    let picked = rfd::FileDialog::new()
                        .set_file_name(default_name)
                        .set_title("GIFSennin")
                        .save_file();
                    let _ = tx.send(Action::SavePathPicked(picked));
                });
            }
            Effect::SpawnLoad(path) => {
                let tx = self.tx.clone();
                thread::spawn(move || {
                    let result = codec::load_any(&path)
                        .and_then(|anim| Project::from_decoded(path.clone(), anim));
                    let _ = tx.send(Action::LoadFinished(result));
                });
            }
            Effect::SpawnExport {
                frames,
                loop_count,
                format,
                path,
            } => {
                let tx = self.tx.clone();
                thread::spawn(move || {
                    let start = Instant::now();
                    let result = export_to_file(&frames, loop_count, &format, &path);
                    let ms = start.elapsed().as_millis() as u64;
                    let _ = tx.send(Action::ExportFinished(result.map(|bytes| ExportOutcome {
                        path,
                        bytes,
                        ms,
                    })));
                });
            }
            Effect::SpawnEstimate {
                generation,
                frames,
                loop_count,
                format,
            } => {
                let tx = self.tx.clone();
                thread::spawn(move || {
                    let result = encode_to_memory(&frames, loop_count, &format);
                    let _ = tx.send(Action::EstimateReady { generation, result });
                });
            }
        }
    }
}

fn export_to_file(
    frames: &[std::sync::Arc<Frame>],
    loop_count: u16,
    format: &ExportFormat,
    path: &PathBuf,
) -> Result<u64, AppError> {
    match format {
        ExportFormat::Gif { quality } => {
            let file = fs::File::create(path)?;
            let mut w = BufWriter::new(file);
            let n = codec::encode_gif(frames, loop_count, *quality, &mut w)?;
            w.flush()?;
            Ok(n)
        }
        ExportFormat::Webp { quality, lossless } => {
            let data = codec::encode_webp(frames, loop_count, *quality, *lossless)?;
            fs::write(path, &data)?;
            Ok(data.len() as u64)
        }
        ExportFormat::PngZip => {
            let data = codec::encode_png_zip(frames)?;
            fs::write(path, &data)?;
            Ok(data.len() as u64)
        }
    }
}

/// 体积估算（共识 Q8-c）：与导出同一路径编码到内存
fn encode_to_memory(
    frames: &[std::sync::Arc<Frame>],
    loop_count: u16,
    format: &ExportFormat,
) -> Result<u64, AppError> {
    match format {
        ExportFormat::Gif { quality } => {
            let mut buf = Vec::new();
            let n = codec::encode_gif(frames, loop_count, *quality, &mut buf)?;
            Ok(n)
        }
        ExportFormat::Webp { quality, lossless } => {
            let data = codec::encode_webp(frames, loop_count, *quality, *lossless)?;
            Ok(data.len() as u64)
        }
        ExportFormat::PngZip => {
            let data = codec::encode_png_zip(frames)?;
            Ok(data.len() as u64)
        }
    }
}

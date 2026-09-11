// release 构建隐藏控制台窗口（纯 GUI）；debug 保留 stdout 供 env_logger 输出
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> Result<(), gifsennin_rust::AppError> {
    gifsennin_rust::run()
}

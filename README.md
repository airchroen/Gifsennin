# GIFSennin

A desktop editor for GIF and animated WebP with frame-level editing, built with Rust and egui.

## Features

- Import GIF, animated WebP, or static images (PNG / JPEG / BMP)
- Frame operations: delete, reorder, reverse, per-frame duration
- Canvas operations: crop (interactive overlay), resize, rotate, flip
- Loop setting (play once or loop forever)
- Export: GIF (three quality presets), animated WebP (lossy or lossless), PNG sequence as zip
- Snapshot-based undo/redo
- File loading, encoding, and export size estimation run on worker threads; the UI stays responsive
- UI language: Chinese and English

## Architecture

Single `Model` as the source of truth, updated through `Action`s (Action-MVU):

| Directory     | Responsibility                                                                 |
| ------------- | ------------------------------------------------------------------------------ |
| `src/model`   | Pure data and pure functions: frames, history, transforms. No UI, no I/O       |
| `src/codec`   | Encode/decode boundary for GIF / WebP / PNG, pure functions                    |
| `src/render`  | egui views. No business logic; emit `Action`s                                  |
| `src/workers` | Load / export / estimate jobs on `std::thread` + channels                      |
| `src/app.rs`  | eframe assembly: owns the `Model`, dispatches `Action`s, executes `Effect`s    |

All editing logic lives in `model/` and is testable without a window.

## Build

```sh
cargo run --release
```

Requires a stable Rust toolchain and a C compiler (libwebp is built from source).

## Test

```sh
cargo test
```

## License

[MIT](LICENSE)

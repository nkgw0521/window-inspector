mod window_enum;
mod window_info;

use tauri::{AppHandle, Emitter};
use window_enum::{enumerate_windows, register_hooks, unregister_hooks};
use window_info::WindowInfo;

// ---------------------------------------------------------------------------
// Tauri Commands（フロントエンドから invoke で呼び出す）
// ---------------------------------------------------------------------------

/// 現在のウィンドウ一覧を取得する
/// フロントエンド: await invoke("get_windows")
#[tauri::command]
fn get_windows() -> Vec<WindowInfo> {
    enumerate_windows()
}

// ---------------------------------------------------------------------------
// Tauriイベント名定数
// ---------------------------------------------------------------------------

/// ウィンドウ変化をフロントエンドへ通知するイベント名
const EVENT_WINDOWS_CHANGED: &str = "windows-changed";

// ---------------------------------------------------------------------------
// アプリセットアップ
// ---------------------------------------------------------------------------

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let app_handle: AppHandle = app.handle().clone();

            // WinEventHookに渡すクロージャ
            // ウィンドウイベント発生時にフロントエンドへ通知する
            // EmitFnはSend + Syncが必要なため AppHandle をcloneして移動
            let emit_fn = Box::new(move || {
                // 更新されたウィンドウ一覧をそのままイベントに乗せる
                let windows = enumerate_windows();
                if let Err(e) = app_handle.emit(EVENT_WINDOWS_CHANGED, &windows) {
                    eprintln!("emit error: {e}");
                }
            });

            register_hooks(emit_fn);
            Ok(())
        })
        .on_window_event(|_window, event| {
            // アプリ終了時にフックを解除する
            if let tauri::WindowEvent::Destroyed = event {
                unregister_hooks();
            }
        })
        .invoke_handler(tauri::generate_handler![get_windows])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

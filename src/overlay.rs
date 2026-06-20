use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// ハイライト用オーバーレイウィンドウのラベル
/// 既存ウィンドウがあれば閉じてから作り直すため固定ラベルにしている
const OVERLAY_LABEL: &str = "highlight-overlay";

/// オーバーレイ用の最小限なHTML（透明背景＋色付き枠線のみ）
/// data: URLとして埋め込むため、外部ファイル不要
fn overlay_html(color: &str) -> String {
    format!(
        r#"<!DOCTYPE html><html><head><style>
        html,body {{
            margin:0; padding:0; background:transparent;
            width:100vw; height:100vh; overflow:hidden;
        }}
        .frame {{
            position:absolute; inset:0;
            border: 4px solid {color};
            box-sizing: border-box;
            border-radius: 4px;
            box-shadow: 0 0 16px {color};
        }}
        </style></head><body><div class="frame"></div></body></html>"#,
        color = color
    )
}

/// 指定した矩形（スクリーン座標）に半透明の枠線オーバーレイを一瞬表示する
///
/// # 引数
/// x, y, width, height: ハイライト対象ウィンドウのスクリーン座標と寸法
/// duration_ms: 表示してから自動的に閉じるまでのミリ秒
///
/// # 設計メモ
/// - WebviewWindowBuilder::build() はメインスレッドで同期的に呼ぶ必要がある
///   （別スレッドからの呼び出しはWindows上でデッドロックする）。
///   そのため Tauri コマンドのハンドラ（メインスレッド実行）から直接呼ぶ。
/// - クリックを透過させたいが、Tauri 2.0時点でクリックスルーは
///   set_ignore_cursor_events で別途設定する。
/// - 自動クローズは別スレッドのタイマーで window.close() を呼ぶ。
///   close() 自体はどのスレッドからでも呼び出し可能。
#[cfg(target_os = "windows")]
pub fn show_highlight(
    app: &AppHandle,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    duration_ms: u64,
) -> Result<(), String> {
    // 既存のオーバーレイが残っていれば先に閉じる（連打対策）
    if let Some(existing) = app.get_webview_window(OVERLAY_LABEL) {
        let _ = existing.close();
    }

    let html = overlay_html("#cba6f7"); // テーマカラー（紫）に合わせる
    let data_url = format!("data:text/html;charset=utf-8,{}", urlencode(&html));

    let window = WebviewWindowBuilder::new(app, OVERLAY_LABEL, WebviewUrl::External(
        data_url.parse().map_err(|e| format!("URL生成エラー: {e}"))?
    ))
        .title("highlight")
        .position(x as f64, y as f64)
        .inner_size(width.max(1) as f64, height.max(1) as f64)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .shadow(false)
        .skip_taskbar(true)
        .focused(false)
        .build()
        .map_err(|e| format!("オーバーレイウィンドウの作成に失敗: {e}"))?;

    // クリックを下のウィンドウへ透過させる（ハイライトが操作の邪魔をしないように）
    let _ = window.set_ignore_cursor_events(true);

    // 一定時間後に自動で閉じる
    let app_handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(duration_ms));
        if let Some(w) = app_handle.get_webview_window(OVERLAY_LABEL) {
            let _ = w.close();
        }
    });

    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn show_highlight(
    _app: &AppHandle,
    _x: i32,
    _y: i32,
    _width: i32,
    _height: i32,
    _duration_ms: u64,
) -> Result<(), String> {
    Err("この機能はWindows専用です".into())
}

/// data: URL用の最小限なパーセントエンコード
/// HTML中で問題になりやすい文字のみを対象にした簡易実装
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{:02X}", b));
            }
        }
    }
    out
}

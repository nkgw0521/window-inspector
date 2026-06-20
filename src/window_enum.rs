use crate::window_info::WindowInfo;

#[cfg(target_os = "windows")]
use windows::Win32::{
    Foundation::{CloseHandle, BOOL, HWND, LPARAM},
    System::{
        ProcessStatus::GetModuleBaseNameW,
        Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
    },
    UI::{
        Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK},
        HiDpi::GetDpiForWindow,
        WindowsAndMessaging::{
            EnumWindows, GetClassNameW, GetParent, GetWindowRect, GetWindowTextW,
            GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible, IsZoomed,
            SetForegroundWindow, ShowWindow,
            EVENT_OBJECT_CREATE, EVENT_OBJECT_DESTROY,
            EVENT_OBJECT_NAMECHANGE,
            EVENT_OBJECT_SHOW, EVENT_OBJECT_HIDE,
            SW_RESTORE, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
        },
    },
};

// ---------------------------------------------------------------------------
// ウィンドウ列挙
// ---------------------------------------------------------------------------

/// EnumWindowsのコールバックに渡すコンテキスト
#[cfg(target_os = "windows")]
struct EnumContext {
    windows: Vec<WindowInfo>,
}

/// EnumWindowsコールバック関数
/// # Safety
/// Win32 APIのコールバックのため unsafe が必要
#[cfg(target_os = "windows")]
unsafe extern "system" fn enum_windows_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let ctx = &mut *(lparam.0 as *mut EnumContext);

    // タイトル取得
    let mut title_buf = [0u16; 512];
    let title_len = GetWindowTextW(hwnd, &mut title_buf);
    let title = String::from_utf16_lossy(&title_buf[..title_len as usize]);

    // クラス名取得
    let mut class_buf = [0u16; 256];
    let class_len = GetClassNameW(hwnd, &mut class_buf);
    let class_name = String::from_utf16_lossy(&class_buf[..class_len as usize]);

    // PID取得
    let mut pid: u32 = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));

    // 親ウィンドウHWND取得（トップレベルなら通常0=NULL）
    // GetParentは子ウィンドウなら親を、WS_POPUPなら所有者ウィンドウを返す
    let parent_hwnd = GetParent(hwnd).unwrap_or_default().0 as usize;

    // プロセス名取得
    // PROCESS_QUERY_INFORMATION | PROCESS_VM_READ が必要
    // 権限不足のプロセス（System等）は空文字になる
    let process_name = {
        match OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid) {
            Ok(hprocess) => {
                let mut name_buf = [0u16; 260];
                let name_len = GetModuleBaseNameW(hprocess, None, &mut name_buf);
                let _ = CloseHandle(hprocess);
                String::from_utf16_lossy(&name_buf[..name_len as usize])
            }
            Err(_) => String::from("(access denied)"),
        }
    };

    // ウィンドウ座標・サイズ取得
    let (x, y, width, height) = {
        let mut rect = windows::Win32::Foundation::RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_ok() {
            (
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
            )
        } else {
            (0, 0, 0, 0)
        }
    };

    // 表示状態
    let is_visible = IsWindowVisible(hwnd).as_bool();
    let is_minimized = IsIconic(hwnd).as_bool();
    let is_maximized = IsZoomed(hwnd).as_bool();

    // DPI（Windows 10 1607以降で利用可能）
    let dpi = GetDpiForWindow(hwnd);

    // Zオーダーは列挙順 = 前面から順番
    let z_order = ctx.windows.len();

    ctx.windows.push(WindowInfo {
        hwnd: hwnd.0 as usize,
        title,
        class_name,
        pid,
        process_name,
        x,
        y,
        width,
        height,
        is_visible,
        is_minimized,
        is_maximized,
        parent_hwnd,
        z_order,
        dpi,
    });

    // TRUEを返すと列挙継続
    BOOL(1)
}

/// すべてのトップレベルウィンドウを列挙して返す
/// # Safety
/// EnumWindowsはスレッドセーフだが unsafe を使うWin32 APIを内部で呼ぶ
#[cfg(target_os = "windows")]
pub fn enumerate_windows() -> Vec<WindowInfo> {
    let mut ctx = EnumContext {
        windows: Vec::new(),
    };
    unsafe {
        let _ = EnumWindows(
            Some(enum_windows_callback),
            LPARAM(&mut ctx as *mut _ as isize),
        );
    }
    ctx.windows
}

#[cfg(not(target_os = "windows"))]
pub fn enumerate_windows() -> Vec<WindowInfo> {
    // Windows以外では空リストを返す（開発用スタブ）
    vec![]
}

// ---------------------------------------------------------------------------
// WinEventHook（イベント駆動）
// ---------------------------------------------------------------------------

/// フックハンドルを保持するグローバル（シングルトン前提）
/// HWINEVENTHOOK は内部が生ポインタ（*mut c_void）で Send/Sync を実装しないため、
/// usize にキャストして保持する。使用時は HWINEVENTHOOK(ptr as *mut _) に戻す。
#[cfg(target_os = "windows")]
static HOOK_HANDLES: std::sync::Mutex<Vec<usize>> = std::sync::Mutex::new(Vec::new());

/// Tauriのイベント送信用クロージャ型
pub type EmitFn = Box<dyn Fn() + Send + Sync + 'static>;

/// WinEventHookコールバック
/// ウィンドウの作成・削除・タイトル変更・表示/非表示を検知する
///
/// # 設計メモ
/// - WINEVENT_OUTOFCONTEXT: フック登録元スレッドに同期配信される。
///   ここで重い処理（ウィンドウ列挙やemit）を行うと、OS全体でイベントが
///   連続発生した際にコールバックが詰まりアプリ全体がハングする。
///   そのため、ここでは DIRTY フラグを立てるだけに留め、
///   実際の処理は別スレッドのデバウンスループに任せる。
/// - WINEVENT_SKIPOWNPROCESS: 自プロセスのイベントはスキップ（自身のウィンドウは
///   enumerate_windows()で取得済みのため。設計変更時はこのフラグを外す）
/// - idObject == OBJID_WINDOW(0) のみ対象にするとウィジェット等を除外できる
#[cfg(target_os = "windows")]
unsafe extern "system" fn win_event_callback(
    _hook: HWINEVENTHOOK,
    _event: u32,
    _hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _id_event_thread: u32,
    _dwms_event_time: u32,
) {
    // OBJID_WINDOW = 0: ウィンドウ自体のイベントのみ処理
    // それ以外（メニュー、スクロールバー等）は無視
    if _id_object != 0 {
        return;
    }

    // 重い処理はせず、フラグを立てるだけ（即リターン）
    DIRTY.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// EmitFnをグローバルに保持
#[cfg(target_os = "windows")]
static EMIT_FN: std::sync::Mutex<Option<EmitFn>> = std::sync::Mutex::new(None);

/// デバウンス用: イベントが発生したことだけを記録するフラグ
/// win_event_callback はここに true を立てるだけで重い処理はしない。
/// 別スレッドが一定間隔でこのフラグを見て、立っていたら enumerate_windows を実行する。
#[cfg(target_os = "windows")]
static DIRTY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// WinEventHookを登録する
/// 対象イベント:
///   EVENT_OBJECT_CREATE  - ウィンドウ作成
///   EVENT_OBJECT_DESTROY - ウィンドウ破棄
///   EVENT_OBJECT_SHOW    - ウィンドウ表示
///   EVENT_OBJECT_HIDE    - ウィンドウ非表示
///   EVENT_OBJECT_NAMECHANGE - タイトル変更
///
/// 注意: EVENT_OBJECT_LOCATIONCHANGE は意図的に外している。
/// ドラッグ/リサイズ中に大量発火し、コールバックが詰まって
/// アプリがハングする原因になるため。座標更新は手動更新ボタンで対応する。
#[cfg(target_os = "windows")]
pub fn register_hooks(emit_fn: EmitFn) {
    // EmitFnを保存
    if let Ok(mut guard) = EMIT_FN.lock() {
        *guard = Some(emit_fn);
    }

    let event_pairs = [
        (EVENT_OBJECT_CREATE, EVENT_OBJECT_DESTROY),
        (EVENT_OBJECT_SHOW, EVENT_OBJECT_HIDE),
        (EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_NAMECHANGE),
    ];

    let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;

    let mut handles = HOOK_HANDLES.lock().unwrap();
    for (ev_min, ev_max) in &event_pairs {
        unsafe {
            let hook = SetWinEventHook(
                *ev_min,
                *ev_max,
                None,           // hmod: DLLなし（OUTOFCONTEXT時はNone）
                Some(win_event_callback),
                0,              // idProcess: 0 = 全プロセス対象
                0,              // idThread:  0 = 全スレッド対象
                flags,
            );
            // ハンドルが取れた場合のみ保持（NULLは失敗）
            if !hook.0.is_null() {
                handles.push(hook.0 as usize);
            }
        }
    }
    drop(handles);

    // デバウンススレッド起動
    // 300ms間隔でDIRTYフラグを確認し、立っていればまとめて1回だけ通知する。
    // これにより大量のイベントが来てもenumerate_windows()は高々1回/300msしか走らない。
    std::thread::spawn(|| loop {
        std::thread::sleep(std::time::Duration::from_millis(300));
        if DIRTY.swap(false, std::sync::atomic::Ordering::SeqCst) {
            if let Ok(guard) = EMIT_FN.lock() {
                if let Some(f) = guard.as_ref() {
                    f();
                }
            }
        }
    });
}

/// フック解除（アプリ終了時に呼ぶ）
#[cfg(target_os = "windows")]
pub fn unregister_hooks() {
    let mut handles = HOOK_HANDLES.lock().unwrap();
    for raw in handles.drain(..) {
        unsafe {
            let hook = HWINEVENTHOOK(raw as *mut core::ffi::c_void);
            let _ = UnhookWinEvent(hook);
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn register_hooks(_emit_fn: EmitFn) {}

// ---------------------------------------------------------------------------
// ウィンドウ操作（フォーカス切替）
// ---------------------------------------------------------------------------

/// 指定したHWNDのウィンドウをフォアグラウンドにする
///
/// # 戻り値
/// - Ok(true):  フォアグラウンド化に成功
/// - Ok(false): ウィンドウは存在するが SetForegroundWindow が失敗
///              （Windowsのフォーカス窃取防止ポリシーによるもの。エラーではない）
/// - Err:       ウィンドウが既に存在しない場合
///
/// # 設計メモ
/// SetForegroundWindowは「呼び出し元が最後に入力イベントを受け取った」等の
/// 条件を満たさないと失敗する（仕様）。本アプリはユーザーのクリック操作を
/// 起点に呼び出すため通常は成功するが、保険として以下を行う:
///   1. 最小化されていれば ShowWindow(SW_RESTORE) で先に復元する
///   2. それでも失敗したら呼び出し元にfalseを返し、UI側で
///      「最小化解除のみ成功・前面化は失敗」等の表示を可能にする
#[cfg(target_os = "windows")]
pub fn focus_window(hwnd_value: usize) -> Result<bool, String> {
    let hwnd = HWND(hwnd_value as *mut core::ffi::c_void);

    unsafe {
        if !IsWindow(hwnd).as_bool() {
            return Err("ウィンドウが見つかりません（既に閉じられた可能性があります）".into());
        }

        // 最小化されている場合は先に復元する
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }

        let ok = SetForegroundWindow(hwnd).as_bool();
        Ok(ok)
    }
}

#[cfg(not(target_os = "windows"))]
pub fn focus_window(_hwnd_value: usize) -> Result<bool, String> {
    Err("この機能はWindows専用です".into())
}

#[cfg(not(target_os = "windows"))]
pub fn unregister_hooks() {}

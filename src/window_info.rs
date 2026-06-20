/// 1ウィンドウの情報を保持する構造体
/// SerializeはTauriのinvokeでJSON化するために必要
#[derive(Debug, Clone, serde::Serialize)]
pub struct WindowInfo {
    /// ウィンドウハンドル（usize: 32/64bit両対応）
    pub hwnd: usize,

    /// ウィンドウタイトル（GetWindowTextW）
    pub title: String,

    /// ウィンドウクラス名（GetClassNameW）
    pub class_name: String,

    /// プロセスID（GetWindowThreadProcessId）
    pub pid: u32,

    /// プロセス名（OpenProcess + GetModuleBaseNameW）
    pub process_name: String,

    /// スクリーン座標 左上X（GetWindowRect）
    pub x: i32,

    /// スクリーン座標 左上Y（GetWindowRect）
    pub y: i32,

    /// ウィンドウ幅（GetWindowRect から計算）
    pub width: i32,

    /// ウィンドウ高さ（GetWindowRect から計算）
    pub height: i32,

    /// 表示状態（IsWindowVisible）
    pub is_visible: bool,

    /// 最小化状態（IsIconic）
    pub is_minimized: bool,

    /// 最大化状態（IsZoomed）
    pub is_maximized: bool,

    /// Zオーダー（EnumWindows の列挙順: 0が最前面）
    pub z_order: usize,

    /// ウィンドウDPI（GetDpiForWindow）
    /// システムDPI 96 = 100% スケール
    pub dpi: u32,
}

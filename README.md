# Window Inspector

Windows上で動作する全ウィンドウの情報を一覧表示するデスクトップツールです。
Rust + Tauri 2 で実装しています。

## 主な機能

- **ウィンドウ一覧表示**: タイトル、HWND、プロセス名、PID、クラス名、座標、サイズ、DPI、状態（表示/最小化/最大化）を一覧表示
- **フラット / ツリー表示切替**: 親子関係（`GetParent`）に基づいたツリー表示に対応
- **イベント駆動の更新検知**: `SetWinEventHook` でウィンドウの作成・破棄・表示/非表示・タイトル変更を検知（既定はOff、トグルで有効化可能）
- **タイトルのクリップボードコピー**: 行をクリックでそのウィンドウのタイトルをコピー
- **ウィンドウの前面化**: 行を素早く2回クリックすると対象ウィンドウを前面化（`SetForegroundWindow`、Alt+Tab代替）
- **フィルタ**: タイトル / プロセス名 / クラス名で絞り込み
- **非表示ウィンドウの表示切替**: 既定はOff

## 動作環境

- Windows のみ対応（Win32 API に直接依存しているため）
- Rust（stable）
- Node.js は不要（フロントエンドはCDN経由のバニラHTML/JSのみ）

## セットアップ

```bash
# Tauri CLIのインストール（未導入の場合）
cargo install tauri-cli

# 開発モードで起動
cargo tauri dev

# リリースビルド
cargo tauri build
```

> **注意**: プロジェクトはネットワークドライブ（UNCパス）ではなく、ローカルディスク上に置くことを推奨します。Tauriのリソース埋め込み処理がネットワークパスで不安定になることがあります。

## プロジェクト構成

```
window-inspector/
├── Cargo.toml              # Rust依存関係（windows crate, tauri）
├── build.rs                 # tauri-build エントリポイント
├── tauri.conf.json          # Tauriアプリ設定
├── capabilities/
│   └── default.json         # mainウィンドウの権限定義（Tauri 2の必須ファイル）
├── icons/
│   ├── icon.ico              # Windowsリソース埋め込み用
│   └── icon.png
├── src/
│   ├── main.rs               # Tauriエントリポイント・コマンド定義
│   ├── window_info.rs        # WindowInfo構造体（フロントへ渡すデータ）
│   └── window_enum.rs        # Win32 API呼び出し（列挙・イベント検知・前面化）
└── ui/
    └── index.html            # フロントエンド（HTML/CSS/JS、ビルド不要）
```

## アーキテクチャ

```
┌──────────────────────────────┐
│ Frontend (ui/index.html)      │  HTML/CSS/JavaScript（バニラ、ビルド不要）
│  - テーブル描画・フィルタ・ソート │
│  - クリック判定（コピー/前面化） │
└──────────────┬────────────────┘
               │ invoke() / listen()
┌──────────────▼────────────────┐
│ Backend (Rust / src/)          │
│  - Win32 API 呼び出し           │
│  - windows crate (0.58)        │
└────────────────────────────────┘
```

### Tauriコマンド一覧

| コマンド | 説明 |
|---|---|
| `get_windows` | 現在のウィンドウ一覧を取得 |
| `focus_window(hwnd)` | 指定ウィンドウを前面化（`Result<bool, String>`） |

### イベント

| イベント名 | 説明 |
|---|---|
| `windows-changed` | ウィンドウの作成・破棄・表示/非表示・タイトル変更を検知した際に発火。最新のウィンドウ一覧をペイロードとして送る |

## 取得しているウィンドウ情報

| フィールド | 取得元Win32 API |
|---|---|
| `hwnd` | - |
| `title` | `GetWindowTextW` |
| `class_name` | `GetClassNameW` |
| `pid` / `process_name` | `GetWindowThreadProcessId` / `OpenProcess` + `GetModuleBaseNameW` |
| `x, y, width, height` | `GetWindowRect`（物理ピクセル） |
| `is_visible` / `is_minimized` / `is_maximized` | `IsWindowVisible` / `IsIconic` / `IsZoomed` |
| `parent_hwnd` | `GetParent` |
| `z_order` | `EnumWindows` の列挙順 |
| `dpi` | `GetDpiForWindow` |

## 設計上の注意点

### イベント駆動更新とデバウンス

`SetWinEventHook` は `WINEVENT_OUTOFCONTEXT` で登録しており、フック登録元スレッドにイベントが同期配信されます。OS全体でイベントが連続発生するとコールバックが詰まりアプリがハングするため、コールバック内では `DIRTY` フラグを立てるだけに留め、実際の `enumerate_windows()` 呼び出しは別スレッドが300ms間隔で確認するデバウンス方式にしています。

`EVENT_OBJECT_LOCATIONCHANGE`（ウィンドウの移動・リサイズ）は意図的に監視対象から外しています。最も発火頻度が高く、ハングの主因になりやすいためです。座標の更新は手動更新で対応してください。

### 前面化（フォーカス切替）の制約

`SetForegroundWindow` はWindowsのフォーカス窃取防止ポリシーにより、呼び出し元が直前にユーザー入力を受け取っていない場合などに失敗することがあります（仕様）。失敗時はエラーではなく `Ok(false)` を返し、フロント側でトースト通知します。

### クリック判定

ウィンドウ一覧は自動更新時に `<tr>` 要素ごと再構築されるため、ブラウザ標準の `dblclick` イベント（同一DOM要素への2回クリックが前提）は機能しません。そのため `hwnd` をキーにした自前の時間差判定（500ms、Windowsの既定ダブルクリック時間）でシングルクリック/ダブルクリックを判定しています。

### 自身のウィンドウ

自プロセスのウィンドウも一覧に含めています（`WINEVENT_SKIPOWNPROCESS` でイベント検知のみ除外、列挙自体は含む）。

## 既知の制限

- システムプロセス等、権限不足で `OpenProcess` に失敗する場合プロセス名は `(access denied)` と表示されます
- マルチモニタでDPIスケールが異なる環境では、`GetWindowRect` が返す座標・サイズは物理ピクセルです
- macOS / Linux では Win32 API 呼び出し部分はスタブとなり、ウィンドウ一覧は常に空になります

## ライセンス

社内利用ツールとして作成。

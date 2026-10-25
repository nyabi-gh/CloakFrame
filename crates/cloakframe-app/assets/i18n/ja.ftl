# Messages under "needs review" are machine drafts that a Japanese reviewer has not checked yet.
# The others are 1.12.1's translations of the same text.

app-name = CloakFrame

drop-hint = 画像、動画、またはフォルダーをここにドロップ
## needs review
drop-release = 離すと { $count } 件を追加
add-files = ファイルを追加…
add-folder = フォルダーを追加…
clear-list = クリア
scanning = 写真と動画を探しています…
found-summary = 写真 { $photos } 件、動画 { $videos } 件
skipped-type = { $extension } 形式のファイル { $count } 件は処理できません
skipped-no-extension = 拡張子のないファイル { $count } 件は処理できません
unreadable = { $path } を読み込めません

## 1.12.1
what = 隠す対象
faces = 顔
plates = ナンバープレート
## needs review
how = 隠し方
method-fill = 塗りつぶし
method-mosaic = モザイク
method-blur = ぼかし
strength = 強さ
models-need-one = 顔とナンバープレートのどちらか、または両方を選んでください。

output-folder = 保存先
## 1.12.1
choose-folder = 選択…
## needs review
no-output-folder = フォルダーが選ばれていません
## 1.12.1
start = 開始
## needs review
start-needs-files = 先に写真または動画を追加してください。
start-needs-output = 先に保存先を選んでください。

menu-app-about = CloakFrame について
menu-app-quit = CloakFrame を終了
menu-file = ファイル
menu-file-quit = 終了
menu-edit = 編集
## 1.12.1
menu-edit-undo = 元に戻す
menu-edit-redo = やり直す
## needs review
menu-edit-cut = カット
menu-edit-copy = コピー
menu-edit-paste = ペースト
menu-edit-select-all = すべてを選択
menu-view = 表示
menu-help = ヘルプ

## 1.12.1
theme = テーマ
theme-system = システム
theme-light = ライト
theme-dark = ダーク
language = 言語
language-system = システム

## needs review
about-title = CloakFrame について
about-version = バージョン { $version }
about-license = CloakFrame は Apache License 2.0 のフリーソフトウェアです。
about-third-party = サードパーティーの表示
about-models = 検出モデル
close = 閉じる

settings-unknown-keys = このバージョンが知らない設定はそのまま残しました: { $keys }
settings-unreadable = 設定ファイルを読み込めなかったため、そのまま残しました: { $error }
## needs review
settings-newer = 新しいバージョンの CloakFrame が書いた設定です(形式 { $version })。ここでの変更は保存されません。

## needs review
review-first = 保存する前に確認
running = 処理しています…
cancel = キャンセル
state-queued = 待機中
state-analyzed = 分析済み
state-needs-review = 確認が必要
state-rendering = 保存中
state-verified = 確認済み
state-published = 保存済み
state-failed = 失敗
state-cancelled = キャンセル済み
run-summary = 保存 { $saved } 件、確認待ち { $review } 件、失敗 { $failed } 件
run-new = 新しく始める
start-needs-runtime = アプリの隣に ONNX Runtime が見つかりません。
model-missing = 検出モデル { $name } はまだダウンロードされていません。
videos-need-ffmpeg = アプリの隣に FFmpeg が見つからないため、動画は除外しました。

## needs review
models-title = 検出モデル
models-open = 検出モデル…
models-folder = フォルダー: { $path }
model-name-yolo5face-n = YOLO5Face-n (顔、推奨)
model-name-yunet = YuNet (顔、高速)
model-name-plates = YOLOv9-t (ナンバープレート)
model-terms-yolo5face-n = YOLO5Face (GPL-3.0) から変換し、非商用研究向けデータセット WIDER FACE で学習したモデルです。非商用の目的でのみ使ってください。
model-terms-yunet = OpenCV Zoo から MIT ライセンスで提供されます。
model-terms-plates = open-image-models プロジェクトから MIT ライセンスで提供されます。元になった YOLOv9 の条件も適用されます。
model-size = { $size } MB
model-license = ライセンス: { $license }
model-present = ダウンロード済み
model-not-downloaded = 未ダウンロード
model-different = その場所のファイルはこのモデルではありません。ダウンロードすると置き換えます。
model-accept-terms = この条件に同意します
model-download = ダウンロード
model-download-failed = ダウンロードできませんでした: { $error }

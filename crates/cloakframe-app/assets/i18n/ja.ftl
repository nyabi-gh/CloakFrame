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
state-cancelling = キャンセル中
summary-saved = { $count } 件保存
summary-review = { $count } 件要確認
summary-failed = { $count } 件失敗
summary-cancelled = { $count } 件キャンセル
summary-none = 保存したファイルはありません
summary-with-details = { $counts } — { $details }
summary-missed-faces = 動画 { $videos } 件で顔を { $seconds } 秒間見失いました
summary-missed-plates = 動画 { $videos } 件でナンバープレートを { $seconds } 秒間見失いました
summary-capped = { $count } 件で検出数の上限に達しました
summary-weak = { $count } 件に不確かな検出があります
summary-failed-frames = 動画 { $count } 件に解析できなかったフレームがあります
list-separator = 、
problem-missed-face = フレーム { $first }–{ $last } で顔を見失いました ({ $seconds } 秒)
problem-missed-plate = フレーム { $first }–{ $last } でナンバープレートを見失いました ({ $seconds } 秒)
problem-capped = 検出数の上限に達しました: { $found } 件中 { $kept } 件を保持
problem-failed-frame = フレーム { $frame } を解析できませんでした
problem-weak = 不確かな検出 (スコア { $score })
progress-percent = { $percent }%
progress-left-estimating = 残り時間を計算中
progress-left-under-minute = 残り 1 分未満
progress-left-minutes = 残り約 { $minutes } 分
progress-left-hours = 残り約 { $hours } 時間 { $minutes } 分
run-session-kept = 確認する項目を保存しました。アプリを再起動しても一覧に残ります。
item-reveal = フォルダーで表示
item-open = 開く
item-reveal-source = 元のファイルを表示
shell-failed = { $path } を表示できません: { $error }
log-title = 詳細
log-copy = コピー
quit-title = 処理中です
quit-body = 終了すると処理をキャンセルします。保存したファイルはそのままで、確認待ちの項目は後で確認できるように残します。
quit-confirm = キャンセルして終了
quit-keep = 続ける
quit-waiting = 停止しています。処理が止まるとウィンドウが閉じます。
sessions-title = 確認待ちの作業
session-age = { $days ->
    [0] 今日
    [1] 昨日
   *[other] { $days } 日前
}
session-row = { $output } に保存する項目 { $items } 件
session-changed = 元のファイルが変わった項目 { $count } 件
session-unreadable = 読み込めません: { $error }
session-open = 開く
session-discard = 破棄…
session-back = 戻る
session-item-changed = 元のファイルが変わったか、見つかりません。もう一度実行してください
discard-title = この確認を破棄しますか？
discard-body = 確認のために残した検出結果を削除します。元のファイルと保存したファイルはそのままです。
discard-confirm = 破棄
discard-keep = 残す
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

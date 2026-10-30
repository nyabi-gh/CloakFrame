# Messages under "needs review" are machine drafts that a Japanese reviewer has not checked yet.
# The others are 1.12.1's translations of the same text.

app-name = CloakFrame

drop-hint = 画像、動画、またはフォルダーをここにドロップ
## needs review
drop-release = 離すと { $count } 件を追加
add-files = ファイルを追加…
add-folder = フォルダーを追加…
clear-list = すべて外す
scanning = 写真と動画を探しています…
found-summary = 写真 { $photos } 件、動画 { $videos } 件
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
shape = 形
shape-rectangle = 四角形
shape-ellipse = 楕円
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
start-scanning = 写真と動画を探し終わるまでお待ちください。
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
about-graphics = グラフィックス: { $graphics }
about-third-party = サードパーティーの表示
about-models = 検出モデル
## 1.12.1
close = 閉じる

## needs review
settings-unknown-keys = このバージョンが知らない設定はそのまま残しました: { $keys }
settings-unreadable = 設定ファイルを読み込めなかったため、そのまま残しました: { $error }
settings-newer = 新しいバージョンの CloakFrame が書いた設定です(形式 { $version })。ここでの変更は保存されません。

## 1.12.1
review-first = 保存前に確認
## needs review
## 1.12.1
cancel = キャンセル
## needs review
state-queued = 待機中
state-analyzing = 分析中
state-analyzed = 分析済み
state-needs-review = 確認が必要
## 1.12.1
state-rendering = 保存中
## needs review
state-verified = 確認済み
## 1.12.1
state-published = 保存済み
state-failed = 失敗
## needs review
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
masked-nothing = 隠したものなし
masked-faces = 顔 { $faces }
masked-plates = ナンバープレート { $plates }
masked-both = 顔 { $faces } · ナンバープレート { $plates }
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
    [0] 今日 { $time }
    [1] 昨日 { $time }
   *[other] { $days } 日前
}
session-row = { $output } に保存する項目 { $items } 件
session-changed = 元のファイルが変わった項目 { $count } 件
session-unreadable = 読み込めません: { $error }
session-open = 開く
session-discard = 破棄…
session-item-changed = 元のファイルが変わったか、見つかりません。もう一度実行してください
discard-title = この確認を破棄しますか？
discard-body = 確認のために残した検出結果を削除します。元のファイルと保存したファイルはそのままです。
discard-confirm = 破棄
discard-keep = 残す
start-needs-runtime = アプリの隣に ONNX Runtime が見つかりません。
model-missing = 検出モデル { $name } はまだダウンロードされていません。
videos-need-ffmpeg = アプリの隣に FFmpeg が見つからないため、動画は除外しました。

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
## 1.12.1
model-download = ダウンロード
## needs review
model-download-failed = ダウンロードできませんでした: { $error }
item-review = 確認
start-during-review = 先に確認画面を閉じてください。
review-in-use = 別の処理か確認がこの作業を開いています。
review-done = この確認の項目をすべて保存しました。
review-options-guessed = アプリで作った作業ではないため、現在の設定で保存します。別の設定で解析した項目は拒否されます。
review-badge-changed = 元のファイルが変更
review-badge-saving = 保存中…
review-badge-again = 再度の判断が必要
review-badge-problems = 問題 { $count } 件
review-badge-nothing = 検出なし
review-badge-waiting = 確認待ち
editor-canvas = 写真エディター
editor-previous = 前へ
editor-next = 次へ
editor-position = { $count } 件中 { $at } 件目
editor-failed = 保存できませんでした: { $error }
editor-detections = 検出 { $count } 件のうち { $masked } 件を隠す
editor-problems = 解決する問題
editor-no-problems = 問題は見つかりませんでした。
editor-problem-open = 未解決
editor-problem-covered = 隠して解決
editor-problem-dismissed = 隠すものなし
editor-cover = 隠す
editor-dismiss = 隠すものなし
editor-reopen = 元に戻す
## 1.12.1
editor-undo = 元に戻す
editor-redo = やり直す
## needs review
editor-shortcuts = ? ショートカット
editor-approve = 承認して保存
editor-approve-blocked = 先にすべての問題を解決するか、「その他」メニューから警告付きで保存してください。
editor-export-warnings = 警告付きで保存…
editor-more = その他
editor-export-plain = 匿名化せずに書き出す…
review-left = { $count ->
    [0] 残りの確認はありません
   *[other] 確認 残り { $count } 件
}
review-discard = この確認を破棄…
review-kept = 保存した項目
editor-fit = 全体表示
editor-solve-first = 問題を解決すると保存できます
editor-added-title = 追加した領域
editor-added-area = 領域 { $n }
editor-remove = 削除
editor-hint = クリックで隠す・戻す · ドラッグで領域を追加 · Space を押している間は隠した結果
editor-kind-face = 顔
editor-kind-plate = ナンバープレート
editor-kind-drawn = 手描き
editor-track-weak = 不確か
editor-video-total = / { $total } · フレーム { $frame }
editor-video-loading = 動画を開いています…
editor-video-unreadable = 動画を表示できません: { $error }
editor-video-canvas = 動画エディター
editor-video-decoding = フレームをデコードしています…
editor-video-frame-failed = フレームを表示できません: { $error }
editor-video-play = 再生
## 1.12.1
editor-video-stop = 停止
## needs review
editor-video-masked = 隠した結果で表示
editor-timeline = タイムライン
editor-tracks = トラック { $count } 件のうち { $masked } 件を隠す
editor-track-low = 信頼度が低い: 隠しますが、顔かナンバープレートか確認してください。
editor-drawn-start = ここから開始
editor-drawn-end = ここで終了
editor-drawn-remove-keyframe = このキーフレームを削除
editor-drawn-remove = トラックを削除
editor-cover-gap = トラックを描いて覆う
editor-loading = 写真を開いています…
editor-unreadable = 写真を表示できません: { $error }
editor-preview = 隠した結果のプレビュー
refused-settings = この項目は別の設定で解析しました。もう一度実行してください。
refused-stale = その間に別の場所でこの項目が判断されました。確認画面を開き直してください。
refused-name = 保存先に同じ名前のファイルがあります。使える名前: { $name }
refused-rename = { $name } として保存
refused-invalid-name = 出力名がファイル名として正しくありません。
refused-unresolved = この項目には未解決の問題が残っています。
refused-not-covered = 追加した領域で覆ったとした問題を、その領域が覆わなくなりました。
refused-internal = CloakFrame の不具合のため、判断が拒否されました: { $reason }
confirm-warnings-title = 未解決の問題があるまま書き出しますか？
confirm-warnings-body = 問題 { $count } 件が未解決のため、隠れていない顔やナンバープレートが残る可能性があります。この項目は正常完了として数えません。
confirm-warnings = 警告付きで書き出す
confirm-plain-title = 匿名化せずに書き出しますか？
confirm-plain-body = 何も隠さず、メタデータの規則だけを適用して保存し直します。この項目は正常完了として数えません。
confirm-plain = 匿名化せずに書き出す
## 1.12.1
confirm-cancel = キャンセル
## needs review
shortcuts-title = ショートカット
## 1.12.1
shortcuts-close = 閉じる
## needs review
keys-click = クリック
keys-drag = ドラッグ
keys-arrows = 矢印キー
keys-alt-arrows = Alt + 矢印キー
keys-space = スペース (押し続ける)
keys-m = M (押し続ける)
keys-alt-command-arrows = Alt + { $command } + 矢印キー
video-shortcut-click = トラックを隠す・隠さない、描いたトラックを選ぶ
video-shortcut-drag = このフレームから 1 秒間のトラックを描く、描いたボックスをドラッグしてこのフレームの位置を決める
video-shortcut-space = 再生・停止
video-shortcut-mask = このフレームを隠した結果で表示
video-shortcut-frames = 前・次のフレーム (Shift: 1 秒)
video-shortcut-ends = 最初・最後のフレーム
video-shortcut-gaps = 前・次のギャップ
video-shortcut-enter = 選んだトラックを隠す・隠さない
video-shortcut-alt-arrows = 選んだ描いたボックスをこのフレームで移動 (Shift: 10 ピクセル)
video-shortcut-resize = 選んだ描いたボックスの大きさをこのフレームで変える
video-shortcut-delete = 選んだ描いたトラックを削除
shortcut-click = 検出領域を隠す・隠さない、追加した領域を選択
shortcut-drag = 隠す領域を追加、追加した領域はドラッグで移動
shortcut-tab = 次・前のボックスを選択
shortcut-enter = 選択した検出領域を隠す・隠さない
shortcut-arrows = 選択した領域を移動 (Shift: 10 ピクセル)、選択がなければ表示を移動
shortcut-alt-arrows = 選択した領域の大きさを変更
shortcut-delete = 選択した領域を削除
shortcut-space = 隠した結果をプレビュー
## 1.12.1
shortcut-undo = 元に戻す
shortcut-redo = やり直す
## needs review
shortcut-zoom = 拡大、縮小、画面に合わせる、元の解像度
shortcut-pages = 前・次の項目
shortcut-help = この一覧を表示・非表示
shortcut-scroll = スクロールで表示を移動し、ピンチか Ctrl + スクロールで拡大します。

editor-cancelled = 保存できませんでした: キャンセルしました

run-failed = 実行を開始できませんでした: { $error }
run-failed-no-data-folder = このユーザーのアプリデータフォルダーがありません。

failure-unsupported = CloakFrame が処理しない形式または内容のファイルです。
failure-too-large = CloakFrame が一度に処理できる大きさを超えるファイルです。
failure-unreadable = ファイルを読み込めません。破損している可能性があります。
failure-source-changed = 処理中にファイルが変更されたか、なくなりました。もう一度実行してください。
failure-name-taken = 実行中に出力と同じ名前のファイルができたため、上書きしませんでした。
failure-output = 保存先に出力を書き込めません。
failure-encode = 出力をエンコードできませんでした。
failure-detection = このファイルで検出に失敗しました。
failure-tool = FFmpeg を起動できません。
failure-other = このファイルの処理中に問題が起きました。

note-color-converted = 色を sRGB に変換しました
note-color-profile-ignored = 使用できないカラープロファイルを破棄し、sRGB として読み込みました
note-nothing-detected = 何も検出されませんでした
note-tracked-nothing = 追跡したものはありません
note-tracked-faces = 顔 { $faces } 件を追跡しました
note-tracked-plates = ナンバープレート { $plates } 件を追跡しました
note-tracked-both = 顔 { $faces } 件とナンバープレート { $plates } 件を追跡しました
note-analyzed-again-on-cpu = GPU で失敗したため、この項目を CPU で分析し直しました。この実行の残りも CPU を使います
note-encoded-again-in-software = ハードウェアエンコーダー { $encoder } が失敗したため、この動画をソフトウェアでエンコードし直しました。この実行の残りもソフトウェアでエンコードします: { $reason }
note-padded = 奇数のサイズを { $width }×{ $height } に埋めました
note-folder-not-flushed = 保存しましたが、保存先フォルダーをディスクに書き込めませんでした
note-temp-file-left = 保存しましたが、隣に一時ファイルが残りました。次回の実行で削除します
note-records-left = 保存しましたが、動画の分析記録がアプリデータフォルダーに残りました。次回の実行で削除します

run-note-not-swept = { $path } を削除できません: { $error }
run-note-model-copied = CloakFrame 1.12.1 から { $name } をコピーしました（{ $from }）
run-note-legacy-coreml = CloakFrame 1.12.1 が CoreML 用にコンパイルしたモデルを削除できません: { $error }
run-note-unused-coreml = このバージョンが使わないコンパイル済み CoreML モデルを削除できません: { $error }
run-note-gpu-pack-unused = NVIDIA GPU パックを使いません: { $error }
run-note-no-estimate = 作業量を見積もれません: { $error }
run-note-session-not-saved = 確認セッションを保存できません: { $error }
run-note-records-left = { $path } の動画分析記録を削除できません: { $error }
run-note-left-out-not-analyzed = { $name } は確認待ちにできません。実行が分析内容の記録を残していません。
run-note-left-out-changed = { $name } は確認待ちにできません。分析後に変更されたか、なくなりました。

## 1.12.1
advanced = 詳細オプション
## needs review
face-model = 顔モデル
## 1.12.1
face-model-accurate = 高精度  ·  YOLO5Face-n
face-model-fast = 高速  ·  YuNet
## needs review
sensitivity = 感度
sensitivity-lowest = 最低
sensitivity-low = 低
sensitivity-default = 標準
sensitivity-high = 高
sensitivity-highest = 最高
sensitivity-hint = 高くすると顔やナンバープレートを多く見つけますが、それ以外も多く隠します。低くすると見逃しが増えます。スコア { $score } 以上を隠します。
tiles = 小さな顔をタイルで探す
tiles-hint = 大きな写真や動画の重なり合う部分でも検出し、全体では見つけにくい小さな顔を見つけます。遅くなります。
## 1.12.1
keep-metadata = 選択した EXIF メタデータを保持
## needs review
keep-metadata-hint = オフ（既定）：保存した写真にメタデータは残りません。オン：カメラ、レンズ、露出、撮影日時のみをコピーし、位置情報は併せて選択した場合のみ保持します。動画はメタデータを残しません。
## 1.12.1
keep-gps = 位置情報（GPS）も保持
remove-audio = 動画から音声を削除
## needs review
remove-audio-hint = オフ（既定）：動画の音声を残します。声や話された内容（名前、電話番号など）は匿名化されません。
## 1.12.1
video-quality = 動画の品質
video-quality-high = 高品質（オリジナル相当）
video-quality-balanced = バランス
video-quality-space-saver = ファイルサイズ優先
video-quality-hint = 再エンコードする動画の画質です。画質を上げるとファイルは大きくなります。
video-codec = 動画コーデック
video-codec-h264 = H.264（互換性優先）
video-codec-hevc = HEVC（小さなファイル）
video-codec-hint = 再エンコードする動画のコーデックです。HEVC はファイルが小さくなりますが、古い端末では再生できないことがあります。
gpu = GPU アクセラレーションを使用
gpu-hint = 利用できる場合は、検出モデルと動画のエンコードを GPU で実行します。次回の実行から有効になります。
## needs review
advanced-next-run = 次回の実行から有効になります。確認はその実行の設定で保存します。
## 1.12.1
sample-title = プレビュー
sample-alt = 匿名化スタイルのプレビュー

## needs review
found-list = 処理する写真と動画
found-photo = 写真 { $name }
found-video = 動画 { $name }
run-list = この実行の項目
review-queue = 確認する項目

## needs review
graphics-failed-title = CloakFrame を起動できません
graphics-failed =
    このコンピューターのグラフィックスでは CloakFrame のウィンドウを描画できませんでした。グラフィックスドライバーを更新すると解決する場合があります。

    { $errors }

## needs review
new-task = 新しい作業
add = 追加…
add-menu-files = ファイル…
add-menu-folder = フォルダー…
drop-or = または
choose-files = ファイルを選択…
found-counts = 写真 { $photos } · 動画 { $videos }
skipped-summary = 処理しないファイル { $count } 件 — { $parts }
skipped-part-type = { $extension } { $count } 件（対応していない形式）
skipped-part-no-extension = 拡張子のないファイル { $count } 件
skipped-part-unreadable = 読み込めないファイル { $count } 件
change-folder = 変更…
advanced-open = 詳細オプション…
advanced-detection = 検出
advanced-photos = 写真
advanced-videos = 動画
done = 完了
session-unreadable-short = 読み込めません
running = 処理中
result-title = 完了
cancel-all = すべてキャンセル
cancel-item = この項目をキャンセル
review-all = 確認する
run-ended = { $when } · { $folder }

menu-check-updates = 更新を確認…
## 1.12.1
menu-check-updates-on-start = 起動時に更新を確認
## needs review
update-checking = 更新を確認しています…
update-none = 最新のバージョン CloakFrame { $version } を使っています。
update-check-failed = 更新を確認できませんでした: { $error }
## 1.12.1
update-available = CloakFrame { $version } が利用できます。
update-install = 更新
update-later = 後で
update-downloading = CloakFrame { $version } をダウンロード中…
update-ready = CloakFrame { $version } をダウンロードしました。今すぐ再起動してインストールを完了しますか？
update-restart = 今すぐ再起動
## needs review
update-restart-after-run = 進行中の作業が終わると押せます。
## 1.12.1
update-failed = 更新をインストールできませんでした: { $error }
update-refused = アップデートを検証できなかったため拒否しました。何もインストールされていません。

## needs review
gpu-pack-title = NVIDIA GPU
gpu-pack-name = NVIDIA GPU パック (CUDA)
gpu-pack-size = ダウンロード { $download } MB、インストール後 { $installed } MB
gpu-pack-about = 検出を NVIDIA GPU で実行します。NVIDIA ドライバーが必要で、GitHub と PyPI からダウンロードします。
gpu-pack-terms = これらのファイルには NVIDIA の条件が適用されます:
gpu-pack-remove = 削除
gpu-pack-damaged = GPU パックが不完全なため使いません: { $reason }。もう一度ダウンロードすると修復します。
gpu-pack-restart = GPU パックを使うには CloakFrame を再起動してください。

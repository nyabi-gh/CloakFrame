<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE TS>
<TS version="2.1" language="ja_JP">
<context>
    <name>cloakframe::MainWindow</name>
    <message>
        <location filename="../src/ModelDownloader.cpp" line="32"/>
        <source>Downloading model…</source>
        <translation>モデルをダウンロード中…</translation>
    </message>
    <message>
        <location filename="../src/ModelDownloader.cpp" line="33"/>
        <source>Cancel</source>
        <translation>キャンセル</translation>
    </message>
    <message>
        <location filename="../src/ModelDownloader.cpp" line="74"/>
        <location filename="../src/ModelDownloader.cpp" line="85"/>
        <location filename="../src/ModelDownloader.cpp" line="99"/>
        <location filename="../src/ModelDownloader.cpp" line="111"/>
        <location filename="../src/ModelDownloader.cpp" line="123"/>
        <source>Download Failed</source>
        <translation>ダウンロード失敗</translation>
    </message>
    <message>
        <location filename="../src/ModelDownloader.cpp" line="86"/>
        <source>Could not download the model.

%1</source>
        <translation>モデルをダウンロードできませんでした。

%1</translation>
    </message>
    <message>
        <location filename="../src/ModelDownloader.cpp" line="100"/>
        <source>The downloaded model failed its integrity check and was discarded.</source>
        <translation>ダウンロードしたモデルは整合性チェックに失敗したため破棄されました。</translation>
    </message>
    <message>
        <location filename="../src/ModelDownloader.cpp" line="75"/>
        <source>The download was much larger than expected and was stopped.</source>
        <translation>ダウンロードサイズが予想を大幅に超えたため中止しました。</translation>
    </message>
    <message>
        <location filename="../src/ModelDownloader.cpp" line="112"/>
        <source>Another account can change the model folder, so the download was not saved.

%1</source>
        <translation>他のアカウントがモデルフォルダーを変更できるため、ダウンロードしたファイルを保存しませんでした。

%1</translation>
    </message>
    <message>
        <location filename="../src/ModelDownloader.cpp" line="124"/>
        <source>Could not save the model file.</source>
        <translation>モデルファイルを保存できませんでした。</translation>
    </message>
    <message>
        <location filename="../src/ModelDownloader.cpp" line="136"/>
        <source>The %1 model isn&apos;t on this computer yet.

CloakFrame can download it once (%2 MB) from the yolov5-face-onnx-inference project on GitHub. The model is based on the GPL-3.0-licensed YOLO5Face project and was trained on the WIDER FACE dataset, so treat it as non-commercial only. Your images are never uploaded.

Download now?</source>
        <translation>%1 モデルはまだこのコンピューターにありません。

CloakFrame は GitHub の yolov5-face-onnx-inference プロジェクトから一度だけ（%2 MB）ダウンロードできます。このモデルは GPL-3.0 ライセンスの YOLO5Face プロジェクトを基に WIDER FACE データセットで学習されているため、非商用利用に限定してください。画像がアップロードされることはありません。

今すぐダウンロードしますか？</translation>
    </message>
    <message>
        <location filename="../src/ModelDownloader.cpp" line="144"/>
        <source>The %1 model isn&apos;t on this computer yet.

CloakFrame can download it once (%2 MB) from the OpenCV Zoo project on GitHub (MIT-licensed). Your images are never uploaded.

Download now?</source>
        <translation>%1 モデルはまだこのコンピューターにありません。

CloakFrame は GitHub の OpenCV Zoo プロジェクトから一度だけ（%2 MB）ダウンロードできます（MIT ライセンス）。画像がアップロードされることはありません。

今すぐダウンロードしますか？</translation>
    </message>
    <message>
        <location filename="../src/ModelDownloader.cpp" line="152"/>
        <source>The %1 model isn&apos;t on this computer yet.

CloakFrame can download it once (%2 MB) from its source project. Your images are never uploaded.

Download now?</source>
        <translation>%1 モデルはまだこのコンピューターにありません。

CloakFrame は提供元プロジェクトから一度だけ（%2 MB）ダウンロードできます。画像がアップロードされることはありません。

今すぐダウンロードしますか？</translation>
    </message>
    <message>
        <location filename="../src/ModelDownloader.cpp" line="165"/>
        <location filename="../src/ModelDownloader.cpp" line="182"/>
        <source>Download Model</source>
        <translation>モデルをダウンロード</translation>
    </message>
    <message>
        <location filename="../src/ModelDownloader.cpp" line="183"/>
        <source>The license plate detection model isn&apos;t on this computer yet.

CloakFrame can download it once (%1 MB) from the open-image-models project (MIT-licensed). Your images are never uploaded.

Download now?</source>
        <translation>ナンバープレート検出モデルはまだこのコンピューターにありません。

CloakFrame は open-image-models プロジェクト（MIT ライセンス）から一度だけ（%1 MB）ダウンロードできます。画像がアップロードされることはありません。

今すぐダウンロードしますか？</translation>
    </message>
    <message>
        <location filename="../src/ModelDownloader.cpp" line="203"/>
        <location filename="../src/ModelDownloader.cpp" line="211"/>
        <source>Invalid Model</source>
        <translation>無効なモデル</translation>
    </message>
    <message>
        <location filename="../src/ModelDownloader.cpp" line="204"/>
        <source>Choose an existing ONNX model file.</source>
        <translation>存在する ONNX モデルファイルを選択してください。</translation>
    </message>
    <message>
        <location filename="../src/ModelDownloader.cpp" line="212"/>
        <source>The selected model must use the .onnx extension.</source>
        <translation>選択するモデルの拡張子は .onnx である必要があります。</translation>
    </message>
    <message>
        <location filename="../src/ModelDownloader.cpp" line="219"/>
        <source>Model Too Large</source>
        <translation>モデルが大きすぎます</translation>
    </message>
    <message>
        <location filename="../src/ModelDownloader.cpp" line="220"/>
        <source>The selected ONNX file is larger than 512 MB. Choose a smaller SCRFD model.</source>
        <translation>選択した ONNX ファイルは 512 MB を超えています。より小さい SCRFD モデルを選んでください。</translation>
    </message>
    <message>
        <location filename="../src/ModelDownloader.cpp" line="232"/>
        <source>Load Custom Model</source>
        <translation>カスタムモデルを読み込む</translation>
    </message>
    <message>
        <location filename="../src/ModelDownloader.cpp" line="233"/>
        <source>Only load ONNX models from sources you trust.

Model: %1
Size: %2 MB

Continue?</source>
        <translation>信頼できる提供元の ONNX モデルだけを読み込んでください。

モデル: %1
サイズ: %2 MB

続行しますか？</translation>
    </message>
    <message>
        <location filename="../src/ModelDownloader.cpp" line="247"/>
        <source>Model File Changed</source>
        <translation>モデルファイルが変わりました</translation>
    </message>
    <message>
        <location filename="../src/ModelDownloader.cpp" line="248"/>
        <source>This file is no longer the model you approved.

Model: %1
Size: %2 MB

Something replaced its contents since you chose it. Continue only if you replaced it yourself.

Load it anyway?</source>
        <translation>このファイルは承認したモデルではありません。

モデル: %1
サイズ: %2 MB

選択したあとに中身が別のものへ置き換わっています。自分で置き換えた場合にのみ続行してください。

それでも読み込みますか？</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="473"/>
        <source>Local, private redaction of faces and license plates in photos and videos</source>
        <translation>写真や動画の顔とナンバープレートをローカルで安全に匿名化</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="373"/>
        <source>Remove Selected</source>
        <translation>選択項目を削除</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="376"/>
        <source>Clear All</source>
        <translation>すべてクリア</translation>
    </message>
    <message numerus="yes">
        <location filename="../src/MainWindow.cpp" line="1304"/>
        <source>Ignored %n unsupported file(s).</source>
        <translation>
            <numerusform>未対応のファイル %n 件を無視しました。</numerusform>
        </translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1007"/>
        <source>Preview</source>
        <translation>プレビュー</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="999"/>
        <source>Anonymization style preview</source>
        <translation>匿名化スタイルのプレビュー</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1001"/>
        <source>Sample of the current anonymization style and block size.</source>
        <translation>現在の匿名化スタイルとブロックサイズのサンプルです。</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="606"/>
        <source>Input images and folders</source>
        <translation>入力画像とフォルダー</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="608"/>
        <source>Right-click for options · Delete removes selected items</source>
        <translation>右クリックでオプション · Delete で選択項目を削除</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1114"/>
        <source>Processing progress</source>
        <translation>処理の進捗</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1121"/>
        <source>Open Output Folder</source>
        <translation>出力フォルダーを開く</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="491"/>
        <source>Model</source>
        <translation>モデル</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="500"/>
        <source>Choose speed vs. accuracy, or load a custom SCRFD ONNX file.</source>
        <translation>速度と精度のどちらを優先するかを選ぶか、カスタム SCRFD ONNX ファイルを読み込みます。</translation>
    </message>
    <message>
        <source>Bundled SCRFD model path</source>
        <translation type="vanished">内蔵 SCRFD モデルのパス</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="554"/>
        <location filename="../src/MainWindow.cpp" line="843"/>
        <source>Browse…</source>
        <translation>参照…</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="546"/>
        <source>Download</source>
        <translation>ダウンロード</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="584"/>
        <source>Inputs</source>
        <translation>入力</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="593"/>
        <source>Drag images, videos, or folders here, or use the buttons below.</source>
        <translation>画像・動画・フォルダーをここにドラッグするか、下のボタンを使ってください。</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="605"/>
        <source>Drop images, videos, or folders here</source>
        <translation>画像、動画、またはフォルダーをここにドロップ</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="618"/>
        <source>Add Files</source>
        <translation>ファイルを追加</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="624"/>
        <source>Add Folder</source>
        <translation>フォルダーを追加</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="630"/>
        <source>Clear</source>
        <translation>クリア</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="639"/>
        <source>Include subfolders</source>
        <translation>サブフォルダーを含める</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="646"/>
        <source>Review before saving</source>
        <translation>保存前に確認</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="653"/>
        <source>Review detections before output:
  • Images: exclude boxes or add missed regions
  • Videos: scrub the timeline, exclude false tracks, or add missed tracks with keyframes</source>
        <translation>出力前に検出結果を確認します：
  • 画像：ボックスを除外するか、見逃した領域を追加
  • 動画：タイムラインを移動し、誤検出トラックを除外するか、キーフレームで見逃したトラックを追加</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="685"/>
        <source>Output</source>
        <translation>出力</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="694"/>
        <source>Anonymized copies are written here, preserving folder structure.</source>
        <translation>匿名化したコピーが、フォルダー構成を保ったままここに保存されます。</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="704"/>
        <source>Choose…</source>
        <translation>選択…</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="719"/>
        <source>Preserve selected EXIF metadata</source>
        <translation>選択した EXIF メタデータを保持</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="728"/>
        <source>Off (default): output carries no metadata — GPS, camera, and timestamps are removed.
On: copies selected EXIF fields such as camera, timestamps, and location. Embedded previews, IPTC, XMP, comments, and color profiles are removed. Format and bit depth are preserved at maximum quality.</source>
        <translation>オフ（既定）：出力にはメタデータが含まれず、GPS、カメラ、撮影日時は削除されます。
オン：カメラ、撮影日時、位置情報など、選択した EXIF フィールドのみをコピーします。埋め込みプレビュー、IPTC、XMP、コメント、カラープロファイルは削除し、形式とビット深度は最高品質で保持します。</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="739"/>
        <source>Metadata preservation is unavailable in this build. Output metadata will be removed.</source>
        <translation>このビルドではメタデータを保持できません。出力のメタデータは削除されます。</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="764"/>
        <source>Advanced Options</source>
        <translation>詳細オプション</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="776"/>
        <source>Reset to defaults</source>
        <translation>デフォルトに戻す</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="439"/>
        <location filename="../src/MainWindow.cpp" line="440"/>
        <source>Settings</source>
        <translation>設定</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="2181"/>
        <source>Update available: %1</source>
        <translation>更新があります: %1</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="2190"/>
        <source>Update Available</source>
        <translation>更新があります</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="2193"/>
        <source>CloakFrame %1 is available. What&apos;s new:</source>
        <translation>CloakFrame %1 が利用できます。主な変更点:</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="2196"/>
        <source>No release notes were provided for this update.</source>
        <translation>このアップデートにはリリースノートがありません。</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="2199"/>
        <source>Update</source>
        <translation>更新</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="2200"/>
        <location filename="../src/MainWindow.cpp" line="2272"/>
        <source>Later</source>
        <translation>後で</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="511"/>
        <source>Faces</source>
        <translation>顔</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="512"/>
        <source>License plates</source>
        <translation>ナンバープレート</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="513"/>
        <source>Faces + license plates</source>
        <translation>顔 + ナンバープレート</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="795"/>
        <source>Tweak detection and anonymization behavior. Defaults work for most photos.</source>
        <translation>検出と匿名化の動作を調整します。ほとんどの写真では既定値で十分です。</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="807"/>
        <source>Mosaic (pixelate)</source>
        <translation>モザイク（ピクセル化）</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="808"/>
        <source>Gaussian blur</source>
        <translation>ガウスぼかし</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="809"/>
        <source>Solid fill (blackout)</source>
        <translation>塗りつぶし（黒塗り）</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="810"/>
        <location filename="../src/MainWindow.cpp" line="948"/>
        <source>Custom image</source>
        <translation>カスタム画像</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="816"/>
        <source>How detected faces are obscured.
Mosaic = pixelation (block size below).
Gaussian blur = strong smoothing scaled to face size.
Solid fill = opaque black box, irreversible.
Custom image = place your selected image over every detected region. Default: Mosaic</source>
        <translation>検出された顔を隠す方法です。
モザイク = ピクセル化（下のブロックサイズを使用）。
ガウスぼかし = 顔のサイズに応じた強いぼかし。
単色塗りつぶし = 元に戻せない不透明な黒いボックス。
カスタム画像 = 選択した画像を検出されたすべての領域に配置。既定値: モザイク</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="886"/>
        <source>Minimum confidence to accept a face.
Higher = fewer false positives but may miss small or side-profile faces.
Lower = catches more faces but may blur non-face regions. Default: 0.50</source>
        <translation>顔として認める最小の信頼度です。
高いほど = 誤検出は減りますが、小さい顔や横顔を見逃すことがあります。
低いほど = より多くの顔を捉えますが、顔以外の部分をぼかすことがあります。既定値: 0.50</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="901"/>
        <source>Non-Maximum Suppression overlap threshold for duplicate boxes.
Lower = more aggressively removes overlapping detections.
Higher = allows more overlap. Default: 0.40</source>
        <translation>重複した枠に対する Non-Maximum Suppression の重なりしきい値です。
低いほど = 重なった検出をより積極的に取り除きます。
高いほど = 重なりを多く許容します。既定値: 0.40</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="913"/>
        <source>Mosaic block size in pixels.
Larger = coarser blocks, harder to un-blur.
Smaller = finer mosaic, higher recovery risk. Default: 14</source>
        <translation>モザイクのブロックサイズ（ピクセル）です。
大きいほど = ブロックが粗くなり、復元されにくくなります。
小さいほど = モザイクが細かくなり、復元される危険が高まります。既定値: 14</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="927"/>
        <source>Extra margin around each detected face, as a fraction of its size.
Covers ears, hairline, and chin that the detector may miss.
0.00 = exact box, 0.18 = ~18% larger. Default: 0.18</source>
        <translation>検出した顔の周囲に、その大きさに対する割合で加える余白です。
検出器が取りこぼしやすい耳・生え際・あごを覆います。
0.00 = 枠のまま、0.18 = 約 18% 拡大。既定値: 0.18</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="941"/>
        <source>Anonymization</source>
        <translation>匿名化</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="955"/>
        <source>Shape</source>
        <translation>形状</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="870"/>
        <location filename="../src/MainWindow.cpp" line="962"/>
        <source>Soft edges</source>
        <translation>ソフトエッジ</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="872"/>
        <source>Fades the edge of the obscured region into the photo instead of a hard cutoff.
The fade only extends outward, so the detected area stays fully covered. Default: off</source>
        <translation>隠した領域の縁を、はっきり切らずに写真になじませます。
ぼかしは外側にだけ広がるため、検出された領域は完全に覆われたままです。既定値: オフ</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="853"/>
        <source>Rectangle</source>
        <translation>長方形</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="539"/>
        <source>Face model path</source>
        <translation>顔モデルのパス</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="854"/>
        <source>Rounded (ellipse)</source>
        <translation>丸型（楕円）</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="859"/>
        <source>Shape of the obscured region.
Rectangle = full padded box.
Rounded = elliptical mask that follows the face and leaves corners untouched. Default: Rectangle</source>
        <translation>隠す領域の形です。
長方形 = 余白を含む枠全体。
角丸 = 顔に沿った楕円形のマスクで、四隅はそのまま残ります。既定値: 長方形</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="969"/>
        <source>Score threshold</source>
        <translation>スコアしきい値</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="976"/>
        <source>NMS threshold</source>
        <translation>NMS しきい値</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="983"/>
        <source>Mosaic block size</source>
        <translation>モザイクのブロックサイズ</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="990"/>
        <source>Face padding</source>
        <translation>顔の余白</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1077"/>
        <source>Activity</source>
        <translation>アクティビティ</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1103"/>
        <source>Ready</source>
        <translation>準備完了</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1142"/>
        <source>Stop</source>
        <translation>停止</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1152"/>
        <source>Start</source>
        <translation>開始</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1174"/>
        <source>Ready. Drop images, videos, or folders to begin.</source>
        <translation>準備ができました。画像・動画・フォルダーをドロップして開始してください。</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1316"/>
        <source>Select SCRFD ONNX Model</source>
        <translation>SCRFD ONNX モデルを選択</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1316"/>
        <source>ONNX Models (*.onnx)</source>
        <translation>ONNX モデル (*.onnx)</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1326"/>
        <location filename="../src/MainWindow.cpp" line="2688"/>
        <location filename="../src/MainWindow.cpp" line="2707"/>
        <source>Could not read the custom model file.</source>
        <translation>カスタムモデルのファイルを読み取れませんでした。</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1331"/>
        <location filename="../src/MainWindow.cpp" line="1942"/>
        <source>Custom — %1</source>
        <translation>カスタム — %1</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1375"/>
        <source>Select Images or Videos</source>
        <translation>画像または動画を選択</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1377"/>
        <source>Images &amp; Videos (*.jpg *.jpeg *.png *.bmp *.tif *.tiff *.webp *.mp4 *.mov *.m4v *.webm)</source>
        <translation>画像・動画 (*.jpg *.jpeg *.png *.bmp *.tif *.tiff *.webp *.mp4 *.mov *.m4v *.webm)</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1392"/>
        <source>Select Folder</source>
        <translation>フォルダーを選択</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1407"/>
        <source>Select Output Folder</source>
        <translation>出力フォルダーを選択</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="2211"/>
        <source>Downloading CloakFrame %1…</source>
        <translation>CloakFrame %1 をダウンロード中…</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="2230"/>
        <source>Update Failed</source>
        <translation>更新失敗</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="2233"/>
        <source>The update could not be installed: %1</source>
        <translation>更新をインストールできませんでした: %1</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="2235"/>
        <source>Open Download Page</source>
        <translation>ダウンロードページを開く</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="2258"/>
        <location filename="../src/MainWindow.cpp" line="2265"/>
        <source>Update Ready</source>
        <translation>更新の準備完了</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="2259"/>
        <source>CloakFrame %1 will finish installing the next time the app starts.</source>
        <translation>次回起動時に CloakFrame %1 のインストールが完了します。</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="2269"/>
        <source>CloakFrame %1 has been downloaded. Restart now to finish installing?</source>
        <translation>CloakFrame %1 をダウンロードしました。今すぐ再起動してインストールを完了しますか？</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="2271"/>
        <source>Restart Now</source>
        <translation>今すぐ再起動</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="2596"/>
        <source>Accurate  ·  YOLO5Face-n</source>
        <translation>高精度  ·  YOLO5Face-n</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="2597"/>
        <source>Fast  ·  YuNet</source>
        <translation>高速  ·  YuNet</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="2701"/>
        <source>The custom model was not approved, so nothing was processed.</source>
        <translation>カスタムモデルが承認されなかったため、何も処理していません。</translation>
    </message>
    <message>
        <source>Choose a SCRFD ONNX model first.</source>
        <translation type="vanished">先に SCRFD ONNX モデルを選んでください。</translation>
    </message>
    <message>
        <source>Choose a valid SCRFD ONNX model first.</source>
        <translation type="vanished">先に有効な SCRFD ONNX モデルを選んでください。</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1513"/>
        <location filename="../src/MainWindow.cpp" line="1538"/>
        <location filename="../src/MainWindow.cpp" line="1632"/>
        <location filename="../src/MainWindow.cpp" line="2623"/>
        <source>Downloading %1…</source>
        <translation>%1 をダウンロード中…</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1516"/>
        <location filename="../src/MainWindow.cpp" line="1541"/>
        <location filename="../src/MainWindow.cpp" line="1637"/>
        <location filename="../src/MainWindow.cpp" line="2630"/>
        <source>Model download was cancelled or failed.</source>
        <translation>モデルのダウンロードが取り消されたか失敗しました。</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1520"/>
        <location filename="../src/MainWindow.cpp" line="1544"/>
        <location filename="../src/MainWindow.cpp" line="1650"/>
        <location filename="../src/MainWindow.cpp" line="2626"/>
        <source>Model ready: %1</source>
        <translation>モデルの準備ができました: %1</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1550"/>
        <source>Add at least one image or folder.</source>
        <translation>画像かフォルダーを 1 つ以上追加してください。</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1556"/>
        <source>Choose an output folder.</source>
        <translation>出力フォルダーを選んでください。</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1581"/>
        <source>Refusing to run: output folder is inside input &apos;%1&apos;. Pick a different output folder so originals aren&apos;t overwritten.</source>
        <translation>実行を中止します: 出力フォルダーが入力 &apos;%1&apos; の中にあります。元のファイルが上書きされないよう、別の出力フォルダーを選んでください。</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1710"/>
        <location filename="../src/MainWindow.cpp" line="1753"/>
        <source>Starting…</source>
        <translation>開始中…</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1761"/>
        <source>Stopping after the current processing step…</source>
        <translation>現在の処理段階が終わったら停止します…</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1762"/>
        <source>Stopping…</source>
        <translation>停止中…</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1850"/>
        <source>Cancelled.</source>
        <translation>キャンセルしました。</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1824"/>
        <source>Finished.</source>
        <translation>完了しました。</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1851"/>
        <source>Cancelled</source>
        <translation>キャンセル済み</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1825"/>
        <source>Done</source>
        <translation>完了</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1829"/>
        <source>Completed with warnings — review the results before sharing.</source>
        <translation>警告付きで完了しました — 共有する前に結果を確認してください。</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1834"/>
        <source>Review required</source>
        <translation>確認が必要です</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1837"/>
        <source>Review Required</source>
        <translation>確認が必要です</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1838"/>
        <source>Processing finished, but some results need attention.

Total: %1
Redacted: %2
Saved without redaction: %3
Copied: %4
Skipped: %5
Failed: %6

Check these results before sharing them.</source>
        <translation>処理は終わりましたが、確認が必要な結果があります。

合計: %1
匿名化: %2
匿名化せずに保存: %3
コピー: %4
スキップ: %5
失敗: %6

共有する前にこれらの結果を確認してください。</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1855"/>
        <source>Failed — check the log for details.</source>
        <translation>失敗しました — 詳細はログを確認してください。</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1859"/>
        <source>Failed — check the log</source>
        <translation>失敗しました — ログを確認してください</translation>
    </message>
    <message>
        <source>Fast  ·  SCRFD 2.5G</source>
        <translation type="vanished">高速  ·  SCRFD 2.5G</translation>
    </message>
    <message>
        <source>Accurate  ·  SCRFD 10G</source>
        <translation type="vanished">高精度  ·  SCRFD 10G</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="2643"/>
        <source>Not downloaded yet — click Download</source>
        <translation>未ダウンロード — 「ダウンロード」をクリック</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="104"/>
        <location filename="../src/MainWindow.cpp" line="112"/>
        <source>Choose an existing image file.</source>
        <translation>既存の画像ファイルを選択してください。</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="119"/>
        <source>The selected image must be no larger than 64 MB.</source>
        <translation>選択する画像は64 MB以下にしてください。</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="129"/>
        <source>The selected file is not a supported image.</source>
        <translation>選択したファイルはサポートされている画像ではありません。</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="138"/>
        <source>The selected image has invalid dimensions.</source>
        <translation>選択した画像のサイズ情報が無効です。</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="155"/>
        <source>The selected image could not be decoded: %1</source>
        <translation>選択した画像をデコードできませんでした: %1</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="838"/>
        <source>Choose an image to cover detected faces</source>
        <translation>検出された顔を覆う画像を選択</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="840"/>
        <source>The image keeps its aspect ratio and follows detected face tilt when available. Transparent pixels reveal a safety mosaic instead of the original image.</source>
        <translation>画像は縦横比を維持し、可能な場合は検出された顔の傾きに追従します。透明ピクセルには元の画像ではなく、安全用モザイクが表示されます。</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="844"/>
        <source>Choose custom image</source>
        <translation>カスタム画像を選択</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1349"/>
        <source>Select Custom Image</source>
        <translation>カスタム画像を選択</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1351"/>
        <source>Images (*.png *.jpg *.jpeg *.bmp *.tif *.tiff *.webp)</source>
        <translation>画像 (*.png *.jpg *.jpeg *.bmp *.tif *.tiff *.webp)</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1360"/>
        <source>Invalid Custom Image</source>
        <translation>無効なカスタム画像</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1501"/>
        <source>Choose a face ONNX model first.</source>
        <translation>先に顔検出用ONNXモデルを選択してください。</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1510"/>
        <location filename="../src/MainWindow.cpp" line="1667"/>
        <source>Choose a valid face ONNX model first.</source>
        <translation>先に有効な顔検出用ONNXモデルを選択してください。</translation>
    </message>
    <message>
        <location filename="../src/MainWindow.cpp" line="1630"/>
        <location filename="../src/MainWindow.cpp" line="1647"/>
        <source>Built-in model integrity check failed: %1</source>
        <translation>内蔵モデルの整合性チェックに失敗しました: %1</translation>
    </message>
</context>
<context>
    <name>cloakframe::ProcessorWorker</name>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="207"/>
        <source>cannot inspect image dimensions</source>
        <translation>画像のサイズを確認できません</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="217"/>
        <source>image too large, %1 x %2</source>
        <translation>画像が大きすぎます（%1 x %2）</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="889"/>
        <source>needs about %1 MB of memory, over the %2 MB limit</source>
        <translation>約 %1 MB のメモリーが必要で、%2 MB の上限を超えます</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="549"/>
        <source>Output name collision: &apos;%1&apos; and &apos;%2&apos; would both write to &apos;%3&apos;</source>
        <translation>出力名の衝突: &apos;%1&apos; と &apos;%2&apos; がどちらも &apos;%3&apos; に書き込まれます</translation>
    </message>
    <message>
        <source>Additional output name collisions omitted.</source>
        <translation type="vanished">これ以降の出力名の衝突は省略しました。</translation>
    </message>
    <message>
        <source>Loading SCRFD model...</source>
        <translation type="vanished">SCRFD モデルを読み込み中...</translation>
    </message>
    <message>
        <source>Reusing loaded SCRFD model.</source>
        <translation type="vanished">読み込み済みの SCRFD モデルを再利用します。</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="441"/>
        <source>Loading face detection model...</source>
        <translation>顔検出モデルを読み込み中...</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="467"/>
        <source>Reusing loaded face detection model.</source>
        <translation>読み込み済みの顔検出モデルを再利用します。</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="469"/>
        <source>Face detection backend: %1</source>
        <translation>顔検出のバックエンド: %1</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="454"/>
        <source>GPU acceleration can&apos;t run the face model; using the CPU instead.</source>
        <translation>GPU アクセラレーションでは顔モデルを実行できないため、CPU を使用します。</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="501"/>
        <source>License plate detection backend: %1</source>
        <translation>ナンバープレート検出のバックエンド: %1</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="477"/>
        <source>Loading license plate detection model...</source>
        <translation>ナンバープレート検出モデルを読み込み中...</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="499"/>
        <source>Reusing loaded license plate detection model.</source>
        <translation>読み込み済みのナンバープレート検出モデルを再利用します。</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="489"/>
        <source>GPU acceleration can&apos;t run the license plate model; using the CPU instead.</source>
        <translation>GPU アクセラレーションではナンバープレートモデルを実行できないため、CPU を使用します。</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="506"/>
        <source>Scanning inputs...</source>
        <translation>入力をスキャン中...</translation>
    </message>
    <message numerus="yes">
        <location filename="../src/ProcessorWorker.cpp" line="509"/>
        <source>Preflight: found %n supported file(s).</source>
        <translation>
            <numerusform>事前確認：対応ファイルが %n 件見つかりました。</numerusform>
        </translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="520"/>
        <source>No supported files were found.</source>
        <translation>対応するファイルが見つかりませんでした。</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="530"/>
        <source>Cannot create output directory: %1</source>
        <translation>出力フォルダーを作成できません: %1</translation>
    </message>
    <message>
        <source>Refusing to run because multiple inputs would write to the same output path.</source>
        <translation type="vanished">複数の入力が同じ出力パスに書き込まれるため、実行を中止します。</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="543"/>
        <source>Refusing to run because an output path is already in use.</source>
        <translation>すでに使われている出力パスがあるため、実行を中止します。</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="556"/>
        <source>Existing output would be overwritten: &apos;%1&apos;</source>
        <translation>既存の出力が上書きされます: &apos;%1&apos;</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="561"/>
        <source>Additional output conflicts omitted.</source>
        <translation>これ以降の出力の衝突は省略しました。</translation>
    </message>
    <message>
        <source>Preflight: output paths are unique.</source>
        <translation type="vanished">事前チェック: 出力パスはすべて重複していません。</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="565"/>
        <source>Preflight: output paths are available.</source>
        <translation>事前チェック: 出力パスはすべて使用できます。</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="748"/>
        <source>Skipped unsafe output path for: %1</source>
        <translation>スキップ: unsafe output path for: %1</translation>
    </message>
    <message>
        <source>Skipped (cannot create parent dir): %1 — %2</source>
        <translation type="vanished">スキップ: (cannot create parent dir): %1 — %2</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="791"/>
        <source>Skipped (file too large, %1 MB): %2</source>
        <translation>スキップ: (file too large, %1 MB): %2</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="853"/>
        <source>Loading</source>
        <translation>読み込み中</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="873"/>
        <location filename="../src/ProcessorWorker.cpp" line="888"/>
        <source>Skipped (%1): %2</source>
        <translation>スキップ: (%1): %2</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="859"/>
        <source>Skipped (animated or multi-page images are not supported): %1</source>
        <translation>スキップ（アニメーション画像またはマルチページ画像はサポートされていません）: %1</translation>
    </message>
    <message numerus="yes">
        <location filename="../src/ProcessorWorker.cpp" line="695"/>
        <source>Warning: %n file(s) finished with regions the output does not cover. Review them before sharing.</source>
        <translation>
            <numerusform>警告: %n 件のファイルが、出力で覆えなかった領域を残したまま終わりました。共有する前に確認してください。</numerusform>
        </translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="772"/>
        <location filename="../src/ProcessorWorker.cpp" line="916"/>
        <source>Skipped unreadable image: %1</source>
        <translation>スキップ: unreadable image: %1</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="783"/>
        <source>Source file changed during processing: %1</source>
        <translation>処理中に元のファイルが変更されました：%1</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="803"/>
        <location filename="../src/ProcessorWorker.cpp" line="830"/>
        <location filename="../src/ProcessorWorker.cpp" line="840"/>
        <source>Failed to create a private source snapshot: %1</source>
        <translation>元ファイルの非公開スナップショットを作成できませんでした：%1</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="944"/>
        <source>Skipped (image too large, %1 × %2): %3</source>
        <translation>スキップ: (image too large, %1 × %2): %3</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="954"/>
        <source>Detecting</source>
        <translation>検出中</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="985"/>
        <source>Reviewing</source>
        <translation>確認中</translation>
    </message>
    <message>
        <source>Review bridge unavailable; saved without review.</source>
        <translation type="vanished">確認機能を利用できないため、確認せずに保存しました。</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="1040"/>
        <source>Skipped without saving: %1</source>
        <translation>スキップ: without saving: %1</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="1054"/>
        <location filename="../src/ProcessorWorker.cpp" line="1110"/>
        <source>Saving</source>
        <translation>保存中</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="1082"/>
        <source>Failed to copy: %1</source>
        <translation>失敗しました: copy: %1</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="1088"/>
        <source>Skipped (original copied): %1</source>
        <translation>スキップ: (original copied): %1</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="1094"/>
        <source>Applying anonymization</source>
        <translation>匿名化を適用中</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="1130"/>
        <source>Failed to save: %1</source>
        <translation>失敗しました: save: %1</translation>
    </message>
    <message numerus="yes">
        <location filename="../src/ProcessorWorker.cpp" line="1149"/>
        <location filename="../src/ProcessorWorker.cpp" line="1484"/>
        <source>Redacted %n region(s): %1</source>
        <translation>
            <numerusform>%n 箇所を匿名化しました：%1</numerusform>
        </translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="1139"/>
        <source>Saved, but could not copy metadata: %1</source>
        <translation>保存しましたが、メタデータをコピーできませんでした: %1</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="1144"/>
        <location filename="../src/ProcessorWorker.cpp" line="1478"/>
        <source>Saved with no regions redacted: %1</source>
        <translation>隠した領域がないまま保存しました: %1</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="757"/>
        <source>Skipped (source and destination are the same file): %1</source>
        <translation>スキップ: (source and destination are the same file): %1</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="1168"/>
        <source>Error processing %1: %2</source>
        <translation>%1 の処理中にエラー: %2</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="727"/>
        <source>Unexpected error — processing stopped.</source>
        <translation>予期しないエラーが発生したため、処理を中止しました。</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="663"/>
        <source>Summary: %1 redacted, %2 saved without redaction, %3 copied, %4 skipped, %5 failed (of %6).</source>
        <translation>概要: 匿名化 %1、匿名化せずに保存 %2、コピー %3、スキップ %4、失敗 %5（全 %6 件）。</translation>
    </message>
    <message numerus="yes">
        <location filename="../src/ProcessorWorker.cpp" line="684"/>
        <source>Warning: %n image(s) were saved with no regions redacted. Check them before sharing.</source>
        <translation>
            <numerusform>警告：%n 枚の画像が匿名化されずに保存されました。共有前に確認してください。</numerusform>
        </translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="715"/>
        <source>Done.</source>
        <translation>完了しました。</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="710"/>
        <source>Completed with warnings. Review the summary before sharing.</source>
        <translation>警告付きで完了しました。共有する前に概要を確認してください。</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="721"/>
        <source>Error: %1</source>
        <translation>エラー: %1</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="1176"/>
        <source>Error processing %1</source>
        <translation>%1 の処理中にエラー</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="1349"/>
        <source>Reviewing video tracks</source>
        <translation>動画トラックを確認中</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="1194"/>
        <source>Metadata preservation is not available for videos; metadata was removed: %1</source>
        <translation>動画ではメタデータの保持に対応していないため、メタデータを削除しました: %1</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="431"/>
        <source>Error: review is on but no review window is available. Nothing was processed.</source>
        <translation>エラー: 確認が有効ですが確認ウィンドウを利用できません。何も処理していません。</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="1006"/>
        <source>Failed (review could not be shown, nothing was saved): %1</source>
        <translation>失敗 (確認を表示できなかったため保存していません): %1</translation>
    </message>
    <message numerus="yes">
        <location filename="../src/ProcessorWorker.cpp" line="1157"/>
        <location filename="../src/ProcessorWorker.cpp" line="1507"/>
        <source>Warning: %n detected region(s) exceeded the safety limit and were left unredacted in %1. Review before sharing.</source>
        <translation>
            <numerusform>警告: 検出された領域 %n 件が安全上限を超えたため、%1 で隠されずに残りました。共有する前に確認してください。</numerusform>
        </translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="1203"/>
        <location filename="../src/ProcessorWorker.cpp" line="1213"/>
        <source>Failed (%1): %2</source>
        <translation>失敗 (%1): %2</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="1208"/>
        <source>Inspecting</source>
        <translation>検査中</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="1221"/>
        <source>Failed (unsupported video: %1): %2</source>
        <translation>失敗 (非対応の動画: %1): %2</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="1228"/>
        <source>Note: variable frame rate is converted to a constant frame rate: %1</source>
        <translation>注意: 可変フレームレートは固定フレームレートに変換されます: %1</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="1327"/>
        <source>Analyzing %1%</source>
        <translation>解析中 %1%</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="1327"/>
        <source>Encoding %1%</source>
        <translation>エンコード中 %1%</translation>
    </message>
    <message numerus="yes">
        <location filename="../src/ProcessorWorker.cpp" line="1516"/>
        <source>Warning: %n track(s) in %1 held no confident detection and were dropped. Review before sharing.</source>
        <translation>
            <numerusform>警告: %1 で確度の高い検出がひとつもないトラック %n 件を破棄しました。共有する前に確認してください。</numerusform>
        </translation>
    </message>
    <message numerus="yes">
        <location filename="../src/ProcessorWorker.cpp" line="1525"/>
        <source>Warning: %n frame(s) of %1 fall inside a tracked region that the output does not cover. Review before sharing.</source>
        <translation>
            <numerusform>警告: %1 の %n フレームが、追跡された領域の内側にありながら出力で覆われていません。共有する前に確認してください。</numerusform>
        </translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="1530"/>
        <source>Uncovered frame ranges in %1: %2</source>
        <translation>%1 で覆われていないフレーム範囲: %2</translation>
    </message>
    <message numerus="yes">
        <location filename="../src/ProcessorWorker.cpp" line="1534"/>
        <source>%n further uncovered range(s) are not listed.</source>
        <translation>
            <numerusform>覆われていない範囲 %n 件は一覧に含めていません。</numerusform>
        </translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="1547"/>
        <source>Failed to process video %1: %2</source>
        <translation>失敗しました: process video %1: %2</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="1249"/>
        <source>Loading face detection model for video...</source>
        <translation>動画用の顔検出モデルを読み込み中...</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="1264"/>
        <source>GPU acceleration can&apos;t run the video face model at %1 px; using the CPU instead.</source>
        <translation>GPU アクセラレーションでは動画の顔モデルを %1 px で実行できないため、CPU を使用します。</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="1275"/>
        <source>Video face detection: %1 px · %2</source>
        <translation>動画の顔検出: %1 px · %2</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="1334"/>
        <source>%1m %2s left</source>
        <translation>残り %1 分 %2 秒</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="1335"/>
        <source>%1s left</source>
        <translation>残り %1 秒</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="1491"/>
        <source>Processed %1 frames in %2s (%3× real time): %4</source>
        <translation>%1 フレームを %2 秒で処理しました（実時間の %3 倍）: %4</translation>
    </message>
    <message>
        <location filename="../src/ProcessorWorker.cpp" line="1499"/>
        <source>Video encoder: %1</source>
        <translation>動画エンコーダー: %1</translation>
    </message>
</context>
<context>
    <name>cloakframe::ReviewDialog</name>
    <message>
        <location filename="../src/ReviewDialog.cpp" line="652"/>
        <source>Review — %1</source>
        <translation>確認 — %1</translation>
    </message>
    <message>
        <location filename="../src/ReviewDialog.cpp" line="676"/>
        <source>Click or Return toggles a box · Drag an empty area to add · Arrow keys move the selection · Hold Space to preview the result · Scroll to zoom, right-drag to pan, 0 resets · %1 / %2 to undo/redo · Esc skips this image without saving</source>
        <translation>クリックまたは Return で枠を切り替え · 空いた場所をドラッグして追加 · 矢印キーで選択を移動 · Space を押している間は結果をプレビュー · スクロールで拡大縮小、右ドラッグで移動、0 でリセット · %1 / %2 で元に戻す・やり直し · Esc は保存せずにこの画像をスキップ</translation>
    </message>
    <message>
        <location filename="../src/ReviewDialog.cpp" line="671"/>
        <source>Review image</source>
        <translation>画像を確認</translation>
    </message>
    <message>
        <location filename="../src/ReviewDialog.cpp" line="690"/>
        <source>Cancel All</source>
        <translation>すべてキャンセル</translation>
    </message>
    <message>
        <location filename="../src/ReviewDialog.cpp" line="730"/>
        <source>Cancel All?</source>
        <translation>すべてキャンセルしますか？</translation>
    </message>
    <message numerus="yes">
        <location filename="../src/ReviewDialog.cpp" line="731"/>
        <source>Stop reviewing and cancel the remaining %n image(s)?

Images already saved are kept.</source>
        <translation>
            <numerusform>確認を中止して、残り %n 枚の画像をキャンセルしますか？

保存済みの画像は保持されます。</numerusform>
        </translation>
    </message>
    <message>
        <location filename="../src/ReviewDialog.cpp" line="693"/>
        <source>Undo</source>
        <translation>元に戻す</translation>
    </message>
    <message>
        <location filename="../src/ReviewDialog.cpp" line="697"/>
        <source>Redo</source>
        <translation>やり直す</translation>
    </message>
    <message>
        <location filename="../src/ReviewDialog.cpp" line="701"/>
        <source>Do Not Save</source>
        <translation>保存しない</translation>
    </message>
    <message>
        <location filename="../src/ReviewDialog.cpp" line="704"/>
        <source>Copy Original</source>
        <translation>オリジナルをコピー</translation>
    </message>
    <message>
        <location filename="../src/ReviewDialog.cpp" line="707"/>
        <source>Saves the image without anonymizing it.</source>
        <translation>画像を匿名化せずに保存します。</translation>
    </message>
    <message>
        <location filename="../src/ReviewDialog.cpp" line="778"/>
        <source>Copy Original?</source>
        <translation>オリジナルをコピーしますか？</translation>
    </message>
    <message>
        <location filename="../src/ReviewDialog.cpp" line="779"/>
        <source>This image will not be anonymized.

%1

Continue?</source>
        <translation>この画像は匿名化されません。

%1

続行しますか？</translation>
    </message>
    <message>
        <location filename="../src/ReviewDialog.cpp" line="773"/>
        <source>The unredacted original will be saved to the output folder, including its original metadata (EXIF, GPS, timestamps).</source>
        <translation>匿名化していない元の画像が、メタデータ（EXIF、GPS、撮影日時）を含んだまま出力フォルダーに保存されます。</translation>
    </message>
    <message>
        <location filename="../src/ReviewDialog.cpp" line="775"/>
        <source>The unredacted original will be saved to the output folder (re-encoded without metadata).</source>
        <translation>匿名化していない元の画像が出力フォルダーに保存されます（メタデータなしで再エンコードされます）。</translation>
    </message>
    <message>
        <location filename="../src/ReviewDialog.cpp" line="709"/>
        <source>Save &amp;&amp; Next</source>
        <translation>保存して次へ</translation>
    </message>
</context>
<context>
    <name>cloakframe::SettingsDialog</name>
    <message>
        <location filename="../src/SettingsDialog.cpp" line="148"/>
        <source>Settings</source>
        <translation>設定</translation>
    </message>
    <message>
        <location filename="../src/SettingsDialog.cpp" line="149"/>
        <source>Theme</source>
        <translation>テーマ</translation>
    </message>
    <message>
        <location filename="../src/SettingsDialog.cpp" line="150"/>
        <source>Language</source>
        <translation>言語</translation>
    </message>
    <message>
        <location filename="../src/SettingsDialog.cpp" line="151"/>
        <source>System</source>
        <translation>システム</translation>
    </message>
    <message>
        <location filename="../src/SettingsDialog.cpp" line="152"/>
        <source>Light</source>
        <translation>ライト</translation>
    </message>
    <message>
        <location filename="../src/SettingsDialog.cpp" line="153"/>
        <source>Dark</source>
        <translation>ダーク</translation>
    </message>
    <message>
        <location filename="../src/SettingsDialog.cpp" line="154"/>
        <source>Check for updates on startup</source>
        <translation>起動時に更新を確認</translation>
    </message>
    <message>
        <location filename="../src/SettingsDialog.cpp" line="155"/>
        <source>Write a local log file</source>
        <translation>ローカルログファイルを保存</translation>
    </message>
    <message>
        <location filename="../src/SettingsDialog.cpp" line="156"/>
        <source>The log may include the names of files you process. Stored on this device only. Takes effect on the next launch.</source>
        <translation>ログには処理したファイルの名前が含まれることがあります。この端末にのみ保存されます。次回の起動から有効になります。</translation>
    </message>
    <message>
        <location filename="../src/SettingsDialog.cpp" line="158"/>
        <source>Use GPU acceleration</source>
        <translation>GPU アクセラレーションを使用</translation>
    </message>
    <message>
        <location filename="../src/SettingsDialog.cpp" line="159"/>
        <source>Runs detection models and video encoding on the GPU when available. Applies from the next run.</source>
        <translation>利用できる場合は、検出モデルと動画のエンコードを GPU で実行します。次回の実行から有効になります。</translation>
    </message>
    <message>
        <location filename="../src/SettingsDialog.cpp" line="161"/>
        <source>Video quality</source>
        <translation>動画の品質</translation>
    </message>
    <message>
        <location filename="../src/SettingsDialog.cpp" line="162"/>
        <source>High (near-original)</source>
        <translation>高品質（オリジナル相当）</translation>
    </message>
    <message>
        <location filename="../src/SettingsDialog.cpp" line="163"/>
        <source>Balanced</source>
        <translation>バランス</translation>
    </message>
    <message>
        <location filename="../src/SettingsDialog.cpp" line="164"/>
        <source>Smaller files</source>
        <translation>ファイルサイズ優先</translation>
    </message>
    <message>
        <location filename="../src/SettingsDialog.cpp" line="165"/>
        <source>Quality of re-encoded videos. Higher quality produces larger files.</source>
        <translation>再エンコードする動画の画質です。画質を上げるとファイルは大きくなります。</translation>
    </message>
    <message>
        <location filename="../src/SettingsDialog.cpp" line="167"/>
        <source>Video codec</source>
        <translation>動画コーデック</translation>
    </message>
    <message>
        <location filename="../src/SettingsDialog.cpp" line="168"/>
        <source>H.264 (most compatible)</source>
        <translation>H.264（互換性優先）</translation>
    </message>
    <message>
        <location filename="../src/SettingsDialog.cpp" line="169"/>
        <source>HEVC (smaller files)</source>
        <translation>HEVC（小さなファイル）</translation>
    </message>
    <message>
        <location filename="../src/SettingsDialog.cpp" line="170"/>
        <source>Codec for re-encoded videos. HEVC produces smaller files but may not play on older devices.</source>
        <translation>再エンコードする動画のコーデックです。HEVC はファイルが小さくなりますが、古い端末では再生できないことがあります。</translation>
    </message>
    <message>
        <location filename="../src/SettingsDialog.cpp" line="174"/>
        <source>Close</source>
        <translation>閉じる</translation>
    </message>
</context>
<context>
    <name>cloakframe::VelopackWorker</name>
    <message>
        <source>Could not download the signature for %1. The update was not applied.</source>
        <translation type="vanished">%1 の署名をダウンロードできませんでした。更新は適用されませんでした。</translation>
    </message>
    <message>
        <source>The update %1 failed its signature check and was not applied (%2).</source>
        <translation type="vanished">更新 %1 は署名の検証に失敗したため適用されませんでした(%2)。</translation>
    </message>
    <message>
        <source>The update cache at %1 is owned or writable by another account on this computer, so an update taken from it cannot be trusted. Remove that directory and try again.</source>
        <translation type="vanished">%1 の更新キャッシュはこのコンピューターの別のアカウントが所有しているか書き込み可能なため、そこから取得した更新は信頼できません。そのディレクトリを削除してからやり直してください。</translation>
    </message>
    <message>
        <source>The cached update %1 is not the package this release describes and was not applied.</source>
        <translation type="vanished">キャッシュされた更新 %1 はこのリリースが示すパッケージと異なるため適用されませんでした。</translation>
    </message>
</context>
<context>
    <name>cloakframe::VideoIo</name>
    <message>
        <location filename="../src/VideoIo.cpp" line="406"/>
        <source>FFmpeg was not found. Video processing is unavailable.</source>
        <translation>FFmpeg が見つかりません。動画処理は利用できません。</translation>
    </message>
    <message>
        <location filename="../src/VideoIo.cpp" line="423"/>
        <source>FFmpeg was found but could not be executed.</source>
        <translation>FFmpeg は見つかりましたが、実行できませんでした。</translation>
    </message>
    <message>
        <location filename="../src/VideoIo.cpp" line="73"/>
        <source>Could not read the FFmpeg checksum manifest.</source>
        <translation>できませんでした: read the FFmpeg checksum manifest.</translation>
    </message>
    <message>
        <location filename="../src/VideoIo.cpp" line="86"/>
        <location filename="../src/VideoIo.cpp" line="96"/>
        <source>Could not read the bundled FFmpeg binary.</source>
        <translation>できませんでした: read the bundled FFmpeg binary.</translation>
    </message>
    <message>
        <location filename="../src/VideoIo.cpp" line="106"/>
        <source>The bundled FFmpeg binary failed its integrity check.</source>
        <translation>同梱の FFmpeg バイナリが整合性チェックに失敗しました。</translation>
    </message>
    <message>
        <location filename="../src/VideoIo.cpp" line="492"/>
        <source>Could not inspect the video (ffprobe did not respond).</source>
        <translation>できませんでした: inspect the video (ffprobe did not respond).</translation>
    </message>
    <message>
        <location filename="../src/VideoIo.cpp" line="502"/>
        <source>Could not inspect the video: %1</source>
        <translation>できませんでした: inspect the video: %1</translation>
    </message>
    <message>
        <location filename="../src/VideoIo.cpp" line="624"/>
        <source>The file contains no video stream.</source>
        <translation>このファイルには映像ストリームがありません。</translation>
    </message>
    <message>
        <location filename="../src/VideoIo.cpp" line="636"/>
        <source>the video stream could not be read</source>
        <translation>映像ストリームを読み取れませんでした</translation>
    </message>
    <message>
        <location filename="../src/VideoIo.cpp" line="665"/>
        <source>unsupported video codec &apos;%1&apos; (H.264, HEVC, VP8 and VP9 only)</source>
        <translation>非対応の映像コーデック &apos;%1&apos;（H.264、HEVC、VP8、VP9 のみ対応）</translation>
    </message>
    <message>
        <location filename="../src/VideoIo.cpp" line="672"/>
        <source>10-bit or higher bit depth is not supported yet</source>
        <translation>10 ビット以上のビット深度にはまだ対応していません</translation>
    </message>
    <message>
        <location filename="../src/VideoIo.cpp" line="678"/>
        <source>HDR video is not supported yet</source>
        <translation>HDR 動画にはまだ対応していません</translation>
    </message>
    <message>
        <location filename="../src/VideoIo.cpp" line="702"/>
        <location filename="../src/VideoIo.cpp" line="940"/>
        <source>Invalid video dimensions.</source>
        <translation>動画の解像度が正しくありません。</translation>
    </message>
    <message>
        <location filename="../src/VideoIo.cpp" line="730"/>
        <location filename="../src/VideoIo.cpp" line="761"/>
        <source>Could not start FFmpeg for decoding.</source>
        <translation>できませんでした: start FFmpeg for decoding.</translation>
    </message>
    <message>
        <location filename="../src/VideoIo.cpp" line="787"/>
        <location filename="../src/VideoIo.cpp" line="860"/>
        <source>Decoding failed: %1</source>
        <translation>デコードに失敗しました: %1</translation>
    </message>
    <message>
        <location filename="../src/VideoIo.cpp" line="853"/>
        <source>Decoding ended mid-frame: %1</source>
        <translation>デコードがフレームの途中で終了しました: %1</translation>
    </message>
    <message>
        <location filename="../src/VideoIo.cpp" line="879"/>
        <source>Decoding timed out.</source>
        <translation>デコードがタイムアウトしました。</translation>
    </message>
    <message>
        <location filename="../src/VideoIo.cpp" line="1065"/>
        <source>Could not start FFmpeg for encoding.</source>
        <translation>できませんでした: start FFmpeg for encoding.</translation>
    </message>
    <message>
        <location filename="../src/VideoIo.cpp" line="968"/>
        <source>Could not create a temporary directory for encoding.</source>
        <translation>エンコード用の一時ディレクトリを作成できませんでした。</translation>
    </message>
    <message>
        <location filename="../src/VideoIo.cpp" line="1083"/>
        <location filename="../src/VideoIo.cpp" line="1113"/>
        <location filename="../src/VideoIo.cpp" line="1132"/>
        <location filename="../src/VideoIo.cpp" line="1140"/>
        <location filename="../src/VideoIo.cpp" line="1185"/>
        <source>Encoding failed: %1</source>
        <translation>エンコードに失敗しました: %1</translation>
    </message>
    <message>
        <location filename="../src/VideoIo.cpp" line="1089"/>
        <source>Internal error: frame does not match the video format.</source>
        <translation>内部エラー: フレームが動画の形式と一致しません。</translation>
    </message>
    <message>
        <location filename="../src/VideoIo.cpp" line="1174"/>
        <source>Encoding timed out while finalizing.</source>
        <translation>最終処理中にエンコードがタイムアウトしました。</translation>
    </message>
    <message>
        <location filename="../src/VideoIo.cpp" line="1253"/>
        <source>Could not move the finished video into place.</source>
        <translation>できませんでした: move the finished video into place.</translation>
    </message>
    <message>
        <location filename="../src/VideoIo.cpp" line="1251"/>
        <source>The output file already exists.</source>
        <translation>出力ファイルはすでに存在します。</translation>
    </message>
    <message>
        <location filename="../src/VideoIo.cpp" line="642"/>
        <source>the video resolution exceeds the safety limit</source>
        <translation>動画の解像度が安全上限を超えています</translation>
    </message>
    <message>
        <location filename="../src/VideoIo.cpp" line="648"/>
        <source>the video frame rate exceeds the safety limit</source>
        <translation>動画のフレームレートが安全上限を超えています</translation>
    </message>
    <message>
        <location filename="../src/VideoIo.cpp" line="654"/>
        <source>the video duration exceeds the safety limit</source>
        <translation>動画の長さが安全上限を超えています</translation>
    </message>
    <message>
        <location filename="../src/VideoIo.cpp" line="659"/>
        <source>the video frame count exceeds the safety limit</source>
        <translation>動画のフレーム数が安全上限を超えています</translation>
    </message>
    <message>
        <location filename="../src/VideoIo.cpp" line="1166"/>
        <location filename="../src/VideoIo.cpp" line="1195"/>
        <location filename="../src/VideoIo.cpp" line="1245"/>
        <source>The source video changed during processing.</source>
        <translation>処理中に元の動画が変更されました。</translation>
    </message>
</context>
<context>
    <name>cloakframe::VideoProcessor</name>
    <message>
        <location filename="../src/VideoProcessor.cpp" line="606"/>
        <location filename="../src/VideoProcessor.cpp" line="927"/>
        <source>No frames could be decoded.</source>
        <translation>デコードできるフレームがありません。</translation>
    </message>
    <message>
        <location filename="../src/VideoProcessor.cpp" line="380"/>
        <source>Could not inspect the source video.</source>
        <translation>元の動画を検査できませんでした。</translation>
    </message>
    <message>
        <location filename="../src/VideoProcessor.cpp" line="402"/>
        <location filename="../src/VideoProcessor.cpp" line="429"/>
        <location filename="../src/VideoProcessor.cpp" line="439"/>
        <source>Could not create a private snapshot of the source video.</source>
        <translation>元の動画の非公開スナップショットを作成できませんでした。</translation>
    </message>
    <message>
        <location filename="../src/VideoProcessor.cpp" line="391"/>
        <source>The source video changed during processing. Start the operation again.</source>
        <translation>処理中に元の動画が変更されました。操作を最初からやり直してください。</translation>
    </message>
    <message>
        <location filename="../src/VideoProcessor.cpp" line="534"/>
        <source>The video frame count exceeds the safety limit.</source>
        <translation>動画のフレーム数が安全上限を超えています。</translation>
    </message>
    <message>
        <location filename="../src/VideoProcessor.cpp" line="546"/>
        <location filename="../src/VideoProcessor.cpp" line="555"/>
        <location filename="../src/VideoProcessor.cpp" line="562"/>
        <source>Video detection data exceeds the safety limit.</source>
        <translation>動画の検出データが安全上限を超えています。</translation>
    </message>
    <message>
        <location filename="../src/VideoProcessor.cpp" line="640"/>
        <location filename="../src/VideoProcessor.cpp" line="698"/>
        <source>Video tracking data exceeds the safety limit.</source>
        <translation>動画の追跡データが安全上限を超えています。</translation>
    </message>
    <message>
        <location filename="../src/VideoProcessor.cpp" line="808"/>
        <location filename="../src/VideoProcessor.cpp" line="933"/>
        <source>The source video changed during processing (frame count differs between passes).</source>
        <translation>処理中に元の動画が変更されました（処理パス間でフレーム数が異なります）。</translation>
    </message>
    <message>
        <location filename="../src/VideoProcessor.cpp" line="858"/>
        <source>Video redaction failed.</source>
        <translation>動画の匿名化に失敗しました。</translation>
    </message>
</context>
<context>
    <name>cloakframe::VideoReviewCanvas</name>
    <message>
        <location filename="../src/VideoReviewDialog.cpp" line="140"/>
        <source>Loading frame preview…</source>
        <translation>フレームプレビューを読み込み中…</translation>
    </message>
    <message>
        <location filename="../src/VideoReviewDialog.cpp" line="142"/>
        <source>Could not load this frame preview.</source>
        <translation>このフレームのプレビューを読み込めませんでした。</translation>
    </message>
    <message>
        <location filename="../src/VideoReviewDialog.cpp" line="178"/>
        <source>Track %1</source>
        <translation>トラック %1</translation>
    </message>
    <message>
        <location filename="../src/VideoReviewDialog.cpp" line="205"/>
        <source>Manual %1</source>
        <translation>手動 %1</translation>
    </message>
</context>
<context>
    <name>cloakframe::VideoReviewDialog</name>
    <message>
        <location filename="../src/VideoReviewDialog.cpp" line="445"/>
        <source>Review video tracks — %1</source>
        <translation>動画トラックを確認 — %1</translation>
    </message>
    <message>
        <location filename="../src/VideoReviewDialog.cpp" line="451"/>
        <source>Scrub the timeline and uncheck false detections. To cover a missed region, draw a manual track and add keyframes as it moves; the boxes between keyframes are interpolated before encoding.</source>
        <translation>タイムラインを移動して誤検出のチェックを外してください。見逃した領域を覆うには、手動トラックを描画し、動きに合わせてキーフレームを追加します。キーフレーム間のボックスはエンコード前に補間されます。</translation>
    </message>
    <message>
        <location filename="../src/VideoReviewDialog.cpp" line="463"/>
        <source>Tracks marked &quot;low confidence&quot; had too few confident detections and are excluded by default — check any that cover a real face or plate.</source>
        <translation>「低信頼度」と表示されたトラックは確実な検出が少なすぎるため、既定で除外されます。実際の顔やナンバープレートを覆うトラックにはチェックを入れてください。</translation>
    </message>
    <message>
        <location filename="../src/VideoReviewDialog.cpp" line="470"/>
        <source>The red marks on the timeline are stretches where a track lost its subject for too long to guess the path. Nothing is masked there, so draw a manual track over any that matter.</source>
        <translation>タイムラインの赤い印は、トラックが対象を長く見失い、経路を推定できなかった区間です。そこでは何も覆われないため、重要な箇所には手動トラックを描いてください。</translation>
    </message>
    <message>
        <location filename="../src/VideoReviewDialog.cpp" line="509"/>
        <source>Track %1  ·  %2–%3</source>
        <translation>トラック %1  ·  %2–%3</translation>
    </message>
    <message>
        <location filename="../src/VideoReviewDialog.cpp" line="514"/>
        <source>%1  ·  low confidence</source>
        <translation>%1  ·  低信頼度</translation>
    </message>
    <message>
        <source>Few confident detections — excluded by default. Check it to redact this track anyway.</source>
        <translation type="vanished">確実な検出が少ないため既定で除外されます。このトラックを隠すにはチェックを入れてください。</translation>
    </message>
    <message>
        <location filename="../src/VideoReviewDialog.cpp" line="520"/>
        <source>Few confident detections — included by default. Uncheck it to leave this track unredacted.</source>
        <translation>確度の高い検出が少ないトラックです — 既定で含まれます。隠さない場合はチェックを外してください。</translation>
    </message>
    <message>
        <location filename="../src/VideoReviewDialog.cpp" line="574"/>
        <source>Add missed track</source>
        <translation>見逃したトラックを追加</translation>
    </message>
    <message>
        <location filename="../src/VideoReviewDialog.cpp" line="575"/>
        <source>Add / update keyframe</source>
        <translation>キーフレームを追加 / 更新</translation>
    </message>
    <message>
        <location filename="../src/VideoReviewDialog.cpp" line="576"/>
        <source>Set start here</source>
        <translation>ここを開始位置に設定</translation>
    </message>
    <message>
        <location filename="../src/VideoReviewDialog.cpp" line="577"/>
        <source>Set end here</source>
        <translation>ここを終了位置に設定</translation>
    </message>
    <message>
        <location filename="../src/VideoReviewDialog.cpp" line="578"/>
        <source>Remove manual track</source>
        <translation>手動トラックを削除</translation>
    </message>
    <message>
        <location filename="../src/VideoReviewDialog.cpp" line="618"/>
        <source>Include all</source>
        <translation>すべて含める</translation>
    </message>
    <message>
        <location filename="../src/VideoReviewDialog.cpp" line="619"/>
        <source>Exclude all</source>
        <translation>すべて除外</translation>
    </message>
    <message>
        <location filename="../src/VideoReviewDialog.cpp" line="685"/>
        <source>Cancel all</source>
        <translation>すべてキャンセル</translation>
    </message>
    <message>
        <location filename="../src/VideoReviewDialog.cpp" line="686"/>
        <source>Encode video</source>
        <translation>動画をエンコード</translation>
    </message>
    <message>
        <location filename="../src/VideoReviewDialog.cpp" line="731"/>
        <source>%1 / %2</source>
        <translation>%1 / %2</translation>
    </message>
    <message>
        <location filename="../src/VideoReviewDialog.cpp" line="922"/>
        <source>Drag a box around the missed region on the current frame.</source>
        <translation>現在のフレームで、見逃した領域を囲むボックスをドラッグしてください。</translation>
    </message>
    <message>
        <location filename="../src/VideoReviewDialog.cpp" line="923"/>
        <source>Drag the new box for manual track %1 on this frame.</source>
        <translation>このフレームで手動トラック %1 の新しいボックスをドラッグしてください。</translation>
    </message>
    <message numerus="yes">
        <location filename="../src/VideoReviewDialog.cpp" line="1099"/>
        <source>Manual %1  ·  %2–%3  ·  %n keyframe(s)</source>
        <translation>
            <numerusform>手動 %1  ·  %2–%3  ·  キーフレーム %n 件</numerusform>
        </translation>
    </message>
    <message>
        <location filename="../src/VideoReviewDialog.cpp" line="1113"/>
        <source>%1 of %2 automatic tracks included · %3 manual</source>
        <translation>自動トラック %2 件中 %1 件を含む · 手動 %3 件</translation>
    </message>
    <message numerus="yes">
        <location filename="../src/VideoReviewDialog.cpp" line="1120"/>
        <source>%n uncovered range(s)</source>
        <translation>
            <numerusform>覆われていない範囲 %n 件</numerusform>
        </translation>
    </message>
</context>
</TS>

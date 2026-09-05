<p align="center">
  <img src="assets/cloakframe-512.png" width="128" alt="CloakFrame アプリアイコン">
</p>

# CloakFrame

[![Latest Release](https://img.shields.io/github/v/release/nyattic/CloakFrame?style=for-the-badge&logo=github&logoColor=white&labelColor=1e1b2e&color=6366f1)](https://github.com/nyattic/CloakFrame/releases/latest)
[![Downloads](https://img.shields.io/endpoint?url=https%3A%2F%2Fraw.githubusercontent.com%2Fnyattic%2FCloakFrame%2Fdownload-badge%2Fdownloads.json&style=for-the-badge&logo=github&logoColor=white&labelColor=1e1b2e)](https://github.com/nyattic/CloakFrame/releases)
[![License](https://img.shields.io/badge/license-GPL--3.0--or--later-6366f1?style=for-the-badge&logo=gnu&logoColor=white&labelColor=1e1b2e)](LICENSE)
![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux-6366f1?style=for-the-badge&logo=qt&logoColor=white&labelColor=1e1b2e)

<p align="center"><a href="README.md">KR</a> · <a href="README.en.md">EN</a> · <b>JP</b> · <a href="README.zh.md">ZH</a></p>

写真や動画に写った顔と車のナンバープレートを自動で隠すデスクトップ
アプリです。ファイルはすべて自分のパソコン内で処理され、サーバーへ
アップロードされません。

写真、動画、フォルダーをウィンドウへドロップし、検出する対象を選ぶだけで、
元ファイルを変更せず匿名化したコピーを作成します。モザイク、ぼかし、単色、
好きな画像で隠せます。

> [!IMPORTANT]
> 自動検出は完全ではありません。**保存前に確認**を有効にし、共有する前に
> 結果を確認してください。**確認が必要です**で終了した処理は完了扱いにしないでください。

## ダウンロードとインストール

| プラットフォーム | 対応環境 | ダウンロード |
| --- | --- | --- |
| Windows | Windows 10以降、64ビット | [Windows版をダウンロード](https://github.com/nyattic/CloakFrame/releases/latest) |
| macOS | macOS 15以降、Apple Silicon | [macOS版をダウンロード](https://github.com/nyattic/CloakFrame/releases/latest) |
| Linux | x86_64 | [Linux版をダウンロード](https://github.com/nyattic/CloakFrame/releases/latest) |

リンクを開き、**Assets**から各プラットフォーム用のファイルを選んでください。
v1.11.0以降は、Windowsでは`CloakFrame-Windows-x64-Setup.exe`、macOSでは
`CloakFrame-macOS-arm64.dmg`、Linuxでは
`CloakFrame-Linux-x86_64.AppImage`を使用します。v1.10.2以前のファイル名には
バージョンが含まれます。LinuxではAppImageに実行権限を付けてから起動します。

```bash
chmod +x CloakFrame-Linux-x86_64.AppImage
./CloakFrame-Linux-x86_64.AppImage
```

内蔵検出モデルを初めて使うときだけ、GitHubからモデルをダウンロードして
保存します（約0.23～11 MB）。その後の処理はオフラインで行えます。

> [!NOTE]
> 以前の名称はRedactlyでした。既存ユーザーの設定とダウンロード済みモデルは、
> 初回起動時にそのまま引き継がれます。

## 使い方

1. 写真、動画、またはフォルダーをウィンドウへドロップします。
2. **顔**、**ナンバープレート**、または両方を選びます。
3. 顔モデルは**高精度 · YOLO5Face-n**（推奨）または**高速 · YuNet**を選びます。
4. 隠し方と出力フォルダーを指定します。
5. **開始**を押します。

元ファイルは変更されません。出力先が重複する場合や同名の結果がすでにある
場合は処理を開始しないため、以前の結果を黙って上書きしません。

### 保存前に確認する

**保存前に確認**を有効にすると、次の操作ができます。

- 写真の誤検出を削除し、見逃した領域を追加
- 動画の顔・ナンバープレートのトラックをタイムラインで確認
- 誤検出した動画トラックを全区間から除外
- 見逃した対象を手動トラックとして追加し、動きに合わせてキーフレームを調整

**追跡の空白（確認前）** の項目を選ぶと、その区間の先頭フレームへ移動します。
**前の空白 / 次の空白** は現在位置の前後にある空白の開始点へ移動し、同じ開始点は
一度だけ訪問します。左右の矢印キーまたはタイムライン横のボタンで1フレームずつ
移動できます。時刻は動画開始からの経過時間で、表示フレーム番号は1から始まります。
区間へ移動するだけでは警告は解除されません。

すべての項目が正常に隠された場合だけ**完了**になります。失敗、スキップ、
検出領域なしでの保存が一つでもあると**確認が必要です**になり、概要が表示されます。
処理後、アクティビティの **ファイル別の結果…** から要確認・失敗の項目を
絞り込み、詳細や入力・出力フォルダーを確認できます。次の処理を開始すると
結果はリセットされます。処理を開始しなかったファイルは一覧に表示しません。

## 対応ファイルと処理方式

- 画像: `.jpg` `.jpeg` `.png` `.bmp` `.tif` `.tiff` `.webp`
- 動画: `.mp4` `.mov` `.m4v` `.webm`（H.264/HEVC/VP8/VP9、8ビットSDR）
- 出力動画: H.264（既定）またはHEVC MP4

動画は双方向の検出・追跡とエンコードの2段階で処理します。MP4と互換性のある
音声は維持し、必要な場合だけAACへ変換します。回転は画素へ反映し、コンテナの
メタデータは削除します。10ビット/HDR入力は品質を黙って落とさず拒否します。

> [!WARNING]
> 動画機能はベータ版です。共有前に結果を最後まで再生して確認してください。
> Linuxの動画処理には自動テストがありますが、手動検証はまだ限定的です。

対応する経路が使える場合はGPUを使い、失敗するとCPUへ自動で切り替えます。

| プラットフォーム | アクセラレーション |
| --- | --- |
| macOS | CoreML検出 · VideoToolboxエンコード |
| Windows | DirectML検出 · NVENC/Quick Syncエンコード |
| Linux | ソースビルド時のCUDA/MIGraphX検出 · NVENC/Quick Syncエンコード |

公式Linux AppImageの検出は現在CPUを使用します。高速なYuNetモデルもすべての
プラットフォームでCPUを使用します。

## プライバシーとネットワーク

写真や動画の内容が端末の外へ送信されることはありません。CloakFrameが行う
ネットワーク通信は次の3種類だけで、画像や個人データは含まれません。

- 内蔵モデルを初めて使うときのダウンロード
- 起動時の新しいバージョン確認
- ユーザーが更新を承認した後のリリースファイル取得

バージョン確認は**設定 → 起動時に更新を確認**で無効にできます。
承認する前に更新ファイルがダウンロードされることはありません。

## よくある質問

### 何も検出されません

別の顔モデルを試し、顔またはナンバープレートの選択が正しいか確認して
ください。見逃した領域はレビュー中に手動で追加できます。

### 動画処理が始まりません

`ffmpeg`と`ffprobe`が必要です。公式配布版には同梱されています。ソースから
ビルドした場合は両方を`PATH`に置いてください。HDR・10ビット入力は非対応です。

### 自分のモデルを使えますか？

**参照…**からSCRFD `.onnx`モデルを選べます。ONNXファイルはネイティブ
ランタイムが実行するモデル入力なので、信頼できる配布元のものだけを使って
ください。

## モデルとライセンス

CloakFrameのソースコードは**GNU GPL v3.0以降**で配布します。条件は
[LICENSE](LICENSE)をご覧ください。Copyright 2026 Nyabi.

内蔵モデルはアプリやリポジトリに同梱されません。推奨のYOLO5Face-nモデルは
WIDER FACEデータセットの条件により、**非商用の研究用途のみ**として扱う必要が
あります。高速なYuNetとナンバープレートモデルは、それぞれのMIT条件に従います。
カスタムモデルには提供元の条件が適用されます。

モデルの配布元、論文の引用、ランタイム依存関係の正確な条件は
[モデルとサードパーティー通知](docs/MODELS.md)および
[THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt)にまとめています。

## 開発への参加

ソースからのビルドは[BUILDING.md](BUILDING.md)、変更の提案は
[CONTRIBUTING.md](CONTRIBUTING.md)をご覧ください。CMakeプリセット、テスト、
`clang-format`、`clang-tidy`の使い方も説明しています。

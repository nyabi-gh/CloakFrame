<p align="center">
  <img src="assets/cloakframe-512.png" width="128" alt="CloakFrame 应用图标">
</p>

# CloakFrame

[![Latest Release](https://img.shields.io/github/v/release/nyattic/CloakFrame?style=for-the-badge&logo=github&logoColor=white&labelColor=1e1b2e&color=6366f1)](https://github.com/nyattic/CloakFrame/releases/latest)
[![Downloads](https://img.shields.io/endpoint?url=https%3A%2F%2Fraw.githubusercontent.com%2Fnyattic%2FCloakFrame%2Fdownload-badge%2Fdownloads.json&style=for-the-badge&logo=github&logoColor=white&labelColor=1e1b2e)](https://github.com/nyattic/CloakFrame/releases)
[![License](https://img.shields.io/badge/license-GPL--3.0--or--later-6366f1?style=for-the-badge&logo=gnu&logoColor=white&labelColor=1e1b2e)](LICENSE)
![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux-6366f1?style=for-the-badge&logo=qt&logoColor=white&labelColor=1e1b2e)

<p align="center"><a href="README.md">KR</a> · <a href="README.en.md">EN</a> · <a href="README.ja.md">JP</a> · <b>ZH</b></p>

自动遮盖照片和视频中人脸与车牌的桌面应用。文件完全在你自己的电脑上处理，绝不会
上传到服务器。

把照片、视频或文件夹拖入窗口，选择要检测的对象，即可在不改动原件的前提下得到一份
匿名化的副本。可以用马赛克、模糊、纯色或你选择的图片来遮盖。

> [!IMPORTANT]
> 自动检测并不完美。请开启保存前审阅，确认结果后再分享。如果应用以
> **需要检查** 结束，就不要把这次任务当成已完成。

## 下载与安装

| 平台 | 运行环境 | 下载 |
| --- | --- | --- |
| Windows | Windows 10 或更高版本，64 位 | [下载 Windows 版](https://github.com/nyattic/CloakFrame/releases/latest) |
| macOS | macOS 15 或更高版本，Apple Silicon | [下载 macOS 版](https://github.com/nyattic/CloakFrame/releases/latest) |
| Linux | x86_64 | [下载 Linux 版](https://github.com/nyattic/CloakFrame/releases/latest) |

打开链接后，在 **Assets** 中选择适合你平台的文件。从 v1.11.0 起，Windows 使用
`CloakFrame-Windows-x64-Setup.exe`，macOS 使用 `CloakFrame-macOS-arm64.dmg`，
Linux 使用 `CloakFrame-Linux-x86_64.AppImage`。v1.10.2 及更早版本的文件名中包含
版本号。在 Linux 上请先给 AppImage 添加可执行权限再运行。

```bash
chmod +x CloakFrame-Linux-x86_64.AppImage
./CloakFrame-Linux-x86_64.AppImage
```

首次使用某个内置检测模型时，CloakFrame 会从 GitHub 下载一次（约 0.23–11 MB）并
缓存下来。之后即可离线处理。

> [!NOTE]
> CloakFrame 以前叫 Redactly。老用户首次启动后会自动沿用原有设置和已下载的模型。

## 使用方法

1. 把照片、视频或文件夹拖入窗口。
2. 选择 **人脸**、**车牌** 或两者。
3. 人脸模型可选 **精确 · YOLO5Face-n**（推荐）或 **快速 · YuNet**。
4. 选择遮盖方式和输出文件夹。
5. 点击 **开始**。

原始文件不会被修改。如果多个输入会写入同一个输出路径，或者同名结果已经存在，
CloakFrame 会拒绝开始，因此绝不会悄悄覆盖已有结果。

### 保存前审阅

开启 **保存前审阅** 后可以：

- 在照片中删除误检区域，并手动补上漏检的区域。
- 在时间轴上查看视频的人脸与车牌轨迹。
- 让某条误检的视频轨迹在整段时长内都被排除。
- 添加手动轨迹，并随目标移动调整关键帧。

选择 **跟踪空白（检查前）** 列表中的项目可跳转至该区间的首帧。
**上一个空白 / 下一个空白** 会跳转至当前位置前后的空白起点，相同起点只访问一次。
使用左右方向键或时间轴旁的按钮可逐帧移动。时间从视频开始计算，显示的帧编号从1开始。
访问区间并不会清除其警告。

只有当每一项都成功遮盖时，任务才会以 **完成** 结束。只要有失败、跳过，或者保存了
没有任何检测区域的文件，结果就会变成 **需要检查** 并给出摘要。任务结束后，
可在活动区域点击 **文件结果…**，筛选需要关注或失败的项目，查看详情并打开
输入或输出文件夹。开始下一次任务时结果会重置；未开始处理的文件不会列出。

## 支持的文件与处理方式

- 图片：`.jpg` `.jpeg` `.png` `.bmp` `.tif` `.tiff` `.webp`
- 视频：`.mp4` `.mov` `.m4v` `.webm`（H.264/HEVC/VP8/VP9，8 位 SDR）
- 输出视频：H.264（默认）或 HEVC MP4

视频分两遍处理：先做双向检测与跟踪，然后编码。兼容的音频会原样保留，其余音频转换为
AAC。旋转信息会写入像素，容器元数据则会被清除。对于 10 位和 HDR 视频，CloakFrame
会直接拒绝，而不是悄悄降低画质。

> [!WARNING]
> 视频功能仍处于测试阶段。分享前请完整播放一遍确认。Linux 的视频路径有自动化测试
> 覆盖，但目前人工验证范围有限。

在可用时 CloakFrame 会使用 GPU，失败则自动回退到 CPU。

| 平台 | 加速方式 |
| --- | --- |
| macOS | CoreML 检测 · VideoToolbox 编码 |
| Windows | DirectML 检测 · NVENC/Quick Sync 编码 |
| Linux | 源码构建时使用 CUDA/MIGraphX 检测 · NVENC/Quick Sync 编码 |

官方 Linux AppImage 目前使用 CPU 检测。快速的 YuNet 模型在所有平台上也都使用 CPU。

## 隐私与网络访问

照片和视频的内容不会离开你的设备。CloakFrame 只会发起以下三种网络请求，其中都不
包含图像或个人数据：

- 首次使用内置模型时下载该模型。
- 启动时检查是否有新版本。
- 在你同意更新之后下载发布文件。

可以在 **设置 → 启动时检查更新** 中关闭版本检查。在你同意之前，不会下载任何更新
文件。

## 常见问题

### 什么都没检测到

先换用另一个人脸模型，并确认人脸／车牌选项选对了。漏检的区域可以在保存前审阅中
手动添加。

### 视频处理无法开始

视频处理需要 `ffmpeg` 和 `ffprobe`。官方发行版已经内置；源码构建则需要这两个程序
位于 `PATH` 中。HDR 与 10 位视频不受支持。

### 可以使用自定义模型吗？

可以在 **浏览…** 中选择 SCRFD 的 `.onnx` 模型。请只使用你信任来源的文件：ONNX 文件
是由本地运行时库执行的模型输入。

## 模型与许可

CloakFrame 的源代码以 **GNU GPL v3.0 或更高版本** 发布，详细条款见
[LICENSE](LICENSE)。2026 Nyabi.

内置模型既不包含在应用中，也不在仓库里，而是首次使用时下载。推荐的 YOLO5Face-n
模型受 WIDER FACE 数据集条款限制，应按 **仅限非商业研究用途** 对待。快速的 YuNet
模型和车牌模型各自遵循 MIT 条款。自定义模型的条款则取决于其提供方。

模型来源、论文引用以及运行时组件的准确条款，整理在
[模型与第三方声明](docs/MODELS.md) 和
[THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt) 中。

## 参与开发

想自行构建请看 [BUILDING.md](BUILDING.md)，想提交改动请看
[CONTRIBUTING.md](CONTRIBUTING.md)。CMake 预设、测试、`clang-format` 和
`clang-tidy` 的用法也都在这两份文档里。

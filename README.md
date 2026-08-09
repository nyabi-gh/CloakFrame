<p align="center">
  <img src="assets/cloakframe-512.png" width="128" alt="CloakFrame 앱 아이콘">
</p>

# CloakFrame

[![Latest Release](https://img.shields.io/github/v/release/nyattic/CloakFrame?style=for-the-badge&logo=github&logoColor=white&labelColor=1e1b2e&color=6366f1)](https://github.com/nyattic/CloakFrame/releases/latest)
[![Downloads](https://img.shields.io/endpoint?url=https%3A%2F%2Fraw.githubusercontent.com%2Fnyattic%2FCloakFrame%2Fdownload-badge%2Fdownloads.json&style=for-the-badge&logo=github&logoColor=white&labelColor=1e1b2e)](https://github.com/nyattic/CloakFrame/releases)
[![License](https://img.shields.io/badge/license-GPL--3.0--or--later-6366f1?style=for-the-badge&logo=gnu&logoColor=white&labelColor=1e1b2e)](LICENSE)
![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux-6366f1?style=for-the-badge&logo=qt&logoColor=white&labelColor=1e1b2e)

<p align="center"><b>KR</b> · <a href="README.en.md">EN</a> · <a href="README.ja.md">JP</a> · <a href="README.zh.md">ZH</a></p>

사진과 영상 속 얼굴·자동차 번호판을 자동으로 가려 주는 데스크톱 앱입니다.
파일은 내 컴퓨터 안에서만 처리되며 서버로 업로드되지 않습니다.

사진, 영상, 폴더를 창에 끌어다 놓고 가릴 대상을 고르면 원본을 건드리지 않고
익명화된 사본을 만듭니다. 모자이크, 흐림, 단색, 원하는 이미지로 가릴 수
있습니다.

> [!IMPORTANT]
> 자동 검출은 완벽하지 않습니다. 저장 전 검토를 켜고 결과를 확인한 뒤
> 공유해 주세요. 앱이 **검토 필요**로 끝났다면 완료된 작업으로 간주하지 마세요.

## 다운로드와 설치

| 플랫폼 | 지원 환경 | 다운로드 |
| --- | --- | --- |
| Windows | Windows 10 이상, 64비트 | [Windows 다운로드](https://github.com/nyattic/CloakFrame/releases/latest) |
| macOS | macOS 15 이상, Apple Silicon | [macOS 다운로드](https://github.com/nyattic/CloakFrame/releases/latest) |
| Linux | x86_64 | [Linux 다운로드](https://github.com/nyattic/CloakFrame/releases/latest) |

링크를 연 뒤 **Assets**에서 플랫폼에 맞는 파일을 선택하세요. v1.11.0부터
Windows는 `CloakFrame-Windows-x64-Setup.exe`, macOS는
`CloakFrame-macOS-arm64.dmg`, Linux는
`CloakFrame-Linux-x86_64.AppImage`입니다. v1.10.2 이하에서는 파일 이름에 버전이
포함됩니다. Linux에서는 AppImage에 실행 권한을 준 뒤 실행하세요.

```bash
chmod +x CloakFrame-Linux-x86_64.AppImage
./CloakFrame-Linux-x86_64.AppImage
```

처음 사용하는 내장 검출 모델은 GitHub에서 한 번만 다운로드하여 캐시합니다
(약 0.23~11 MB). 이후에는 오프라인으로 처리할 수 있습니다.

> [!NOTE]
> 이전 이름은 Redactly였습니다. 기존 사용자는 첫 실행 때 설정과 이미 받은
> 모델을 자동으로 이어서 사용합니다.

## 사용 방법

1. 사진, 영상 또는 폴더를 창에 끌어다 놓습니다.
2. **얼굴**, **번호판** 또는 둘 다 선택합니다.
3. 얼굴 모델은 **정확 · YOLO5Face-n**(권장) 또는 **빠름 · YuNet**을 고릅니다.
4. 마스킹 방식과 출력 폴더를 정합니다.
5. **시작**을 누릅니다.

원본 파일은 수정하지 않습니다. 같은 이름의 결과가 이미 있거나 여러 입력이
한 출력 경로를 함께 사용하게 되면 작업을 시작하지 않으므로, 기존 결과를
실수로 덮어쓰지 않습니다.

### 저장 전에 검토하기

**저장 전 검토**를 켜면 다음 작업을 할 수 있습니다.

- 사진에서 잘못 잡힌 영역을 지우고 놓친 영역을 직접 추가
- 영상의 얼굴·번호판 트랙을 타임라인에서 확인
- 잘못 잡힌 영상 트랙을 전체 구간에서 제외
- 놓친 대상을 수동 트랙으로 추가하고 이동에 맞춰 키프레임 조정

모든 항목이 문제없이 가려져야 **완료**로 끝납니다. 실패, 건너뜀, 검출 영역
없는 저장이 하나라도 있으면 **검토 필요**와 요약을 표시합니다. 이때는 활동
로그에서 해당 파일을 찾고 결과를 직접 확인해 주세요.

## 지원 파일과 처리 방식

- 이미지: `.jpg` `.jpeg` `.png` `.bmp` `.tif` `.tiff` `.webp`
- 영상: `.mp4` `.mov` `.m4v` `.webm` (H.264/HEVC/VP8/VP9, 8비트 SDR)
- 출력 영상: H.264(기본) 또는 HEVC MP4

영상은 양방향 추적으로 대상을 찾은 뒤 인코딩하는 두 단계로 처리합니다. 원본
오디오는 MP4와 호환되면 유지하고, 필요할 때만 AAC로 변환합니다. 회전 정보는
픽셀에 반영하고 컨테이너 메타데이터는 제거합니다. 10비트/HDR 영상은 품질을
조용히 떨어뜨리는 대신 거부합니다.

> [!WARNING]
> 영상 기능은 베타입니다. 공유 전 결과를 끝까지 재생해 확인해 주세요. Linux
> 영상 경로는 자동 테스트로 검증하지만 아직 수동 검증 범위가 제한적입니다.

가능한 경우 GPU를 사용하고 실패하면 CPU로 자동 전환합니다.

| 플랫폼 | 가속 방식 |
| --- | --- |
| macOS | CoreML 검출 · VideoToolbox 인코딩 |
| Windows | DirectML 검출 · NVENC/Quick Sync 인코딩 |
| Linux | 소스 빌드 시 CUDA/MIGraphX 검출 · NVENC/Quick Sync 인코딩 |

공식 Linux AppImage의 검출은 현재 CPU를 사용합니다. 빠른 YuNet 모델도 모든
플랫폼에서 CPU를 사용합니다.

## 개인정보와 네트워크

사진과 영상 내용은 기기 밖으로 전송되지 않습니다. CloakFrame이 만드는 네트워크
요청은 다음 세 가지뿐이며 이미지나 개인 데이터를 포함하지 않습니다.

- 처음 쓰는 내장 모델 다운로드
- 시작할 때 새 버전 확인
- 사용자가 업데이트를 승인한 뒤 릴리스 파일 다운로드

업데이트 확인은 **설정 → 시작 시 업데이트 확인**에서 끌 수 있습니다. 사용자
승인 전에는 업데이트 파일을 다운로드하지 않습니다.

## 자주 묻는 질문

### 아무것도 검출되지 않아요

먼저 얼굴 모델을 바꾸거나, 번호판/얼굴 선택이 맞는지 확인하세요. 검출되지 않은
영역은 저장 전 검토에서 직접 추가할 수 있습니다.

### 영상 처리가 시작되지 않아요

`ffmpeg`와 `ffprobe`가 필요합니다. 공식 배포본에는 함께 들어 있지만 소스 빌드는
두 프로그램이 `PATH`에 있어야 합니다. HDR·10비트 영상은 지원하지 않습니다.

### 커스텀 모델을 쓸 수 있나요?

**찾아보기…**에서 SCRFD `.onnx` 모델을 선택할 수 있습니다. 신뢰하는 출처의
파일만 사용하세요. ONNX 파일은 네이티브 런타임이 실행하는 모델 입력입니다.

## 모델과 라이선스

CloakFrame 소스 코드는 **GNU GPL v3.0 이상**으로 배포합니다. 자세한 조건은
[LICENSE](LICENSE)를 확인하세요. 2026 Nyabi.

내장 모델은 앱이나 저장소에 포함되지 않고 처음 사용할 때 다운로드합니다. 권장
YOLO5Face-n 모델은 WIDER FACE 데이터 조건 때문에 **비상업적 연구 용도로만**
취급해야 합니다. 빠른 YuNet 모델과 번호판 모델은 각각의 MIT 조건을 따릅니다.
커스텀 모델의 조건은 제공처에 따라 다릅니다.

모델 출처, 논문 인용, 런타임 구성요소의 정확한 조건은
[모델과 제3자 고지](docs/MODELS.md) 및
[THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt)에 정리되어 있습니다.

## 개발에 참여하기

직접 빌드하려면 [BUILDING.md](BUILDING.md), 변경을 제안하려면
[CONTRIBUTING.md](CONTRIBUTING.md)를 확인하세요. CMake 프리셋, 테스트,
`clang-format`, `clang-tidy` 사용법도 두 문서에 있습니다.

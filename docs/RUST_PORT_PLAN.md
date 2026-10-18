# CloakFrame Rust 재작성 계획

작성 2026-10-15, 전면 개정 2026-10-16(같은 날 Windows·macOS M0 결과와 시간 예산 반영). 근거: [코드베이스 분석 보고서](REWRITE_ASSESSMENT.ko.md). M0 기록: [ORT](M0_ORT_SPIKE.md), [EXIF writer](M0_EXIF_WRITER.md), [egui](M0_EGUI.md), [FFmpeg 파이프](M0_FFMPEG_PIPE.md), [Windows·Linux 실행 절차](M0_PLATFORM_RUNBOOK.md), [기준 fixture](../tests/fixtures/README.md). M2 기록: [코덱 spike](M2_ENCODE_SPIKE.md). 이전 판은 git 기록(`a42dec2`의 이 파일)에 있다.

## 0. 요약

- **레거시 정책:** 1.12.1이 C++ 구현의 마지막 릴리스다. 기존 코드는 수정·릴리스하지 않고, 새 구현이 대체할 동작의 근거로만 쓴다. 기존 코드 참조는 `v1.12.1` 태그 기준이다.
- **스택:** Rust stable, UI는 egui/eframe, 추론은 ort + ONNX Runtime, 이미지는 `image` + `moxcms` + 자체 EXIF writer, 영상은 번들 FFmpeg/ffprobe 외부 프로세스, 업데이트는 Velopack(Windows·Linux)과 Sparkle(macOS).
- **순서:** headless engine과 CLI를 먼저 완성하고(M1–M5), 새 UX의 desktop 앱을 붙이고(M6), 배포·이전(M7), 검증·전환(M8)을 거쳐 stable 직후 HEIC·HDR 입력(M9)을 추가한다.
- **라이선스:** Apache-2.0(2026-10-16 결정, 10.1).
- **원칙:** 제품 계약(원본 불변, 덮어쓰기 금지, 미해결 결과 비공개, 메타데이터 최소화)은 지킨다. 방어 장치는 위협 모델에 맞춰 줄이고, 1인 유지보수 규모를 넘는 장치는 측정된 필요가 생겼을 때 추가한다.

### 0.1 이전 판에서 바뀐 결정

| 항목 | 이전 판 | 이번 판 | 이유 |
| --- | --- | --- | --- |
| 추론 위치 | 별도 worker 프로세스 + IPC | engine 안의 전용 추론 스레드. `Detector` trait 경계만 둠 | 해시로 고정한 번들 모델만 실행하므로 격리 이득이 작다. 1.12.1에서 관측된 GPU 장애는 프로세스 crash가 아니라 ORT 예외였다. IPC·supervisor·공유 메모리 부담을 없앤다. driver crash가 측정되면 trait 뒤에서 프로세스로 옮긴다 |
| 원본 처리 | 비공개 snapshot 사본 | 사본 없음. 사진은 한 번 읽은 바이트를 끝까지 쓰고, 영상은 패스 경계와 게시 직전에 identity를 비교 | 같은 사용자 권한의 TOCTOU는 위협 모델 밖이다. 사본은 "디스크 여유 부족" 실패를 새로 만든다 |
| 출력 게시 | root handle 기준 traversal, 비공개 stage·ACL, lease·marker 기반 정리 | 출력 폴더 안 임시 파일 → fsync → no-replace rename. 권한은 umask와 폴더 상속을 따름 | 1.12.1의 0600·보호 DACL 결함(D4)을 막고 수천 줄을 줄인다 |
| 상한 | 모든 큐·저장소·IPC에 예산 | 입력 경계의 상한 + bounded queue | 내부 데이터마다 "safety limit"을 거는 패턴을 반복하지 않는다 |
| 번들 바이너리 해시 | 실행 시 최종 해시 확인 | 하지 않음. 실행 파일 경로를 앱 번들로 고정하고 무결성은 코드 서명이 담당 | 같은 폴더의 해시 manifest는 보안 효과가 없고, 1.11.0–1.12.0 macOS 영상 장애의 원인이었다 |
| FFmpeg 격리 | Job Object·rlimit, 이후 OS sandbox | 취소 시 프로세스 트리 종료만(정리 목적) | 보안 경계로 쓰지 않는다. protocol allowlist는 유지 |
| UI | 기존 사용자 흐름 재현 | 새 정보 구조(6장) | 보고서 9장의 UX 결함 |
| HEIC·HDR 입력 | 대체 범위 밖 | stable 직후 첫 기능(M9) | 아이폰 기본 촬영물을 처리하지 못하는 것이 가장 큰 제품 공백 |
| 원본 복사 | 원본 바이트 복사와 메타데이터 제거 복사를 별도 경로로 유지 | 메타데이터를 제거한 재인코딩 "익명화 없이 내보내기"만 제공 | 바이트 복사는 GPS·시리얼까지 남긴다(D9). 원본 파일 복사는 OS 파일 관리자로 충분 |
| 라이선스 | GPL-3.0-or-later | Apache-2.0 | Windows ML·DirectML 재배포 약관과 GPL의 충돌을 없앤다(10.1) |
| 시험 게이트 | Miri·sanitizer·fuzz·통계 신뢰구간이 필수 | 단위·property·골든 출력·소규모 탐지 벤치마크가 필수, fuzz는 parser 두 곳만 | 범위 팽창 방지 |

## 1. 범위

### 1.1 유지할 제품 계약

- 로컬 처리, 원본 불변, 기존 파일 덮어쓰기 금지
- **미해결 자동 감지 문제**(상한으로 잘린 후보, 끊긴 track, 확인되지 않은 gap)가 있는 결과는 정상 출력으로 내보내지 않는다. 1.12.1과 다른 의도적 변경이다.
- 메타데이터는 기본 제거하고, 선택 보존은 기존 allowlist 31개 키(태그 30개) + GPS 별도 동의
- 사진 입력: JPG/PNG/BMP/TIFF/WebP, U8/U16, alpha, EXIF 방향 1–8. 다중 페이지·애니메이션은 거부
- 영상 입력: MP4/MOV/M4V/WebM, H.264/HEVC/VP8/VP9 8비트 SDR. 출력: MP4 H.264/HEVC, CFR
- 얼굴(YOLO5Face-n, YuNet), 번호판(YOLOv9-t), 채우기·모자이크·블러·사용자 이미지, 사각형·타원, padding, soft edge
- 수동 검토: 영역 추가·제외, undo/redo, 키보드 조작. 영상은 track 제외, 수동 keyframe, gap 검토
- 오디오 기본 보존(음성은 가리지 않음을 표시), 전체 제거 선택
- Windows 10 1903(빌드 18362)+ x64, macOS 15+ arm64, Linux x86_64 glibc 2.39+. Windows 하한은 Windows ML 패키지가 정한다(3장).
- 영어·한국어·일본어, 시스템·밝음·어두움 테마

### 1.2 제거

- 사용자 ONNX(SCRFD)와 ONNX graph patcher, 내용 기반 모델 승인
- 원본 바이트 복사(D9), Redactly 호환, GitHub API 업데이트 확인기
- 원본 snapshot, 출력 루트 기억 파일, 모델 폴더 비공개 검사(D5), 번들 FFmpeg 체크섬 manifest

### 1.3 첫 stable 이후

- **M9(최우선):** HEIC/HEIF 사진 입력, 10비트·HDR 영상 입력(SDR 톤매핑 출력)
- 그다음 후보: 원본 VFR 보존, RAW, MKV/AVI, Linux GPU(CUDA), Windows ML EP 카탈로그, 사람·머리 보조 검출, CLI 사용자 기능(preset·스크립팅)

## 2. 합격 기준: 1.12.1 결함

아래 항목은 모두 M0에서 **1.12.1의 실패를 재현하는 fixture와 올바른 기대 결과**를 먼저 만들고, 표의 단계에서 통과해야 한다. 1.12.1 출력은 결함이 없는 영역에서만 기대 결과로 쓴다. 근거와 재현 방법은 [분석 보고서](REWRITE_ASSESSMENT.ko.md) 3장에 있다.

| # | 1.12.1 결함 | 새 구현의 요구 | 단계 |
| --- | --- | --- | --- |
| D1 | 영상 출력이 BT.601로 변환되고 색 태그 없음 | 원본의 matrix·range·primaries·transfer를 `TimelineSpec`에 담아 디코드·인코드 양쪽에 같은 값을 쓰고 출력에 태그 | M4 |
| D2 | 사진 ICC 소실 | 알려진 색 공간(sRGB·Display P3·Adobe RGB)은 pixel을 유지하고 번들한 표준 profile을 붙임. 그 외 profile만 sRGB로 변환하고 결과에 표시 | M2 |
| D3 | 모자이크 셀 12개 상한 때문에 픽셀 단위 "블록 크기" 설정이 큰 얼굴에서 무효 | 설정을 픽셀 대신 "강도"(영역 짧은 변에 들어가는 셀 수, 4–16, 기본 8)로 바꿔 범위 안의 모든 값이 결과를 바꾸게 함. 16이 최소 강도 역할. 축소는 영역 평균 | M2 |
| D4 | 출력 0600/0700, Windows 보호 DACL | 출력 파일·폴더가 umask와 폴더 ACL 상속을 따름 | M1 |
| D5 | umask 002에서 모델 저장 거부 | 모델 폴더 권한 검사 없음. 로드할 때 해시로 검증 | M3 |
| D6 | JPEG q100 4:4:4, WebP 무손실 | JPEG q92·원본 크로마 서브샘플링 유지, 손실 WebP 원본은 손실 q90, PNG/TIFF는 무손실 | M2 |
| D7 | 같은 모델 중복 로드, 스캔 전 로드 | (모델, 입력 크기, EP)마다 세션 하나. 입력 스캔 후 필요한 모델만 로드 | M3 |
| D8 | 탐지기 포인터 데이터 경쟁 | 세션 소유자를 추론 스레드 하나로 둠. Rust 타입으로 보장 | M3 |
| D9 | 원본 복사 시 GPS·시리얼 보존 | 원본 바이트 복사 제거 | M2 |
| D10 | 대소문자 비교가 파일시스템과 무관 | 대소문자 동일시 여부를 파일시스템 조사로 한 곳에서 결정 | M1 |
| D11 | NMS 전 후보 상한 초과분을 누락 영역으로 셈 | NMS 후 상한 초과만 `CoverageEvidence`로 기록. NMS 전 후보는 상한을 넉넉히 두고 초과 시 타일로 재탐지 | M3 |
| D12 | 사진 사본을 시스템 임시 폴더에 둠 | 사본 자체를 만들지 않음 | M1 |
| P-1 | 경고가 있는 결과도 출력 폴더에 게시 | 미해결 자동 감지 문제가 있으면 `PublishPermit`을 발급하지 않음 | M1, M2, M5 |
| P-2 | 얼굴·번호판 결과에 종류 없음, 임계값 공유 | `DetectionKind` 보존, 종류별 임계값·NMS·추적 연관 | M2, M5 |
| P-3 | 분석과 미리보기가 다른 필터 경로로 프레임을 고름 | 단일 `TimelineSpec`, 미리보기도 같은 PTS 정규화 규칙 | M4 |
| P-4 | 출력 영상을 게시 전에 검사하지 않음 | mux 후 stream·duration·frame 수·metadata 확인과 전체 decode | M4 |
| P-5 | 홀수 크기 crop, 오디오 trim 근사 | 홀수 크기는 pad, 오디오는 영상 구간에 맞춰 정확히 trim | M4 |
| P-6 | 평활화가 보간 box를 줄일 수 있음, 연속 장면 전환 누락 | 관측·보간 box 모두 보호 영역을 줄이지 않음, 확인 대기 중 새 spike 평가 | M5 |
| P-7 | 공개 키 없는 빌드가 업데이트를 무검증 수락 | 키 없는 빌드는 업데이트를 거부하고 공식 빌드는 키 없이 성공하지 않음. Sparkle 서명 feed 필수 | M7 |

## 3. 기술 스택

| 분야 | 선정 | 메모 |
| --- | --- | --- |
| 언어 | Rust stable, `rust-toolchain.toml`로 정확한 버전 고정 | 최소 버전은 eframe MSRV(0.36.x 기준 1.95)가 결정 |
| UI | egui/eframe 0.36.x(wgpu 기본, glow 대체 경로), `rfd`(파일 대화상자), AccessKit(eframe 내장) | 네이티브 메뉴는 `muda`로 macOS·Windows만(Windows는 winit 메시지 훅에서 `TranslateAcceleratorW`). muda는 GTK 창에만 붙어 Linux는 egui 메뉴 막대. 한글·일본어는 Noto Sans KR/JP 부분 집합을 번들(OFL, Regular 2개 4.1 MB, 이름 이체자 처리는 M6). UI 시험은 `egui_kittest`([M0 기록](M0_EGUI.md)) |
| 추론 | `ort` =2.0.0-rc.14(`load-dynamic`, `api-27`, `default-features = false`) + ONNX Runtime | 런타임은 Windows가 Windows ML 2.4.89 안의 ORT 1.27.1, macOS·Linux가 공식 ORT 1.30.x다. ORT C API는 하위 호환이므로 API 수준은 가장 낮은 Windows에 맞춰 세 OS 공통 `api-27`로 둔다. Windows ML이 새 ORT를 담으면(2.5.77-rc는 1.28.0) 함께 올린다. 독립 배포 `Microsoft.ML.OnnxRuntime.DirectML`은 1.24.4가 마지막이고 DirectML은 두 경로 모두 1.15.4이므로, Windows ML로 옮겨 얻는 것은 이어지는 ORT 갱신이다. `api-30`으로 빌드하면 Windows에서 `BadVersion`으로 로드가 멈춘다(M0 확인). M0에서 `api-27`로 macOS 공식 1.30.0·Windows ML 1.27.1 모두 세 모델의 CPU 결과와 CoreML·DML 결과가 1.12.1과 같음을 확인. `ort`의 세션 빌더는 EP 자동 선택 정책 `MaxEfficiency`를 기본으로 걸어 ORT 1.31이 CPU 세션에도 CoreML을 붙이므로, 세션마다 `with_auto_device(PreferCPU)`로 덮고 EP를 명시한다. RC이므로 정확한 버전으로 고정 |
| 이미지 | `image` 0.25(PNG·TIFF·BMP·WebP, 필요 codec만), JPEG 디코드·인코드는 libjpeg-turbo(`turbojpeg`, 2026-10-18 결정: `image`의 zune-jpeg 0.5.15가 non-interleaved 4:2:0 JPEG를 조용히 잘못 디코드하고, libjpeg-turbo는 1.12.1의 OpenCV와 pixel까지 같음, [기록](M2_ENCODE_SPIKE.md)), 손실 WebP 인코드는 libwebp(`libwebp-sys`), `moxcms`(색), `kamadak-exif`(읽기) + 자체 allowlist EXIF writer | 모델 전처리 resize는 OpenCV `INTER_LINEAR`를 옮긴 자체 구현(M0에서 바이트 일치 확인). 정확히 2배 축소일 때 OpenCV가 쓰는 `INTER_AREA` 경로는 아직 옮기지 않았으므로 M2에서 이식한다. `fast_image_resize`는 화면 표시용에만 |
| 영상 | 번들 FFmpeg/ffprobe 외부 프로세스 | nonfree 없는 GPL/LGPL 빌드, 대응 소스 제공. 별도 프로세스라 앱의 Apache-2.0 라이선스에 영향이 없다. M0에서 파이프가 병목이 아님을 확인(4.4) |
| 동시성 | 작업 실행기는 std 스레드와 bounded 채널(항목 단위 순서 보존 병렬, 상태 이벤트는 유실 없는 bounded 채널, 진행률은 최신값 보드), Rayon pool(CPU 연산), 전용 추론 스레드(`Detector`를 단독 소유, D8). 작은 Tokio runtime은 HTTP(M3)와 자식 프로세스 감독(M4)이 들어올 때 추가 | CPU 연산을 async executor에서 돌리지 않는다. M1 실행기에는 async 작업이 없어 Tokio를 쓰지 않았다 |
| 설정·로그·오류 | serde + TOML, 앱 데이터 폴더는 자체 구현(`engine::app_dirs`. `directories`는 의존성 `option-ext`가 MPL-2.0이라 쓰지 않음, 2026-10-18), `tracing`, `thiserror` | 기본 로그에 경로·파일명을 넣지 않음 |
| 다국어 | `fluent-bundle` | 기존 `.ts` 번역을 스크립트로 `.ftl`로 이전 |
| 네트워크·무결성 | `reqwest` + rustls, `sha2`, `ed25519-dalek` | 모델·업데이트 URL allowlist |
| 파일시스템 | `rustix`(Unix), `windows` crate | no-replace rename과 identity 조회에만 사용 |
| 업데이트·패키징 | Velopack Rust SDK(Windows·Linux), Sparkle(macOS, `objc2` 어댑터) | 기존 feed와 package ID를 이어 써야 1.12.1 사용자가 자동 업데이트로 넘어온다 |

**추론 EP 정책**

| 플랫폼 | 가속 → 폴백 | 조건 |
| --- | --- | --- |
| Windows x64 | Windows ML self-contained 런타임의 DirectML → CPU | ORT를 따로 번들하지 않고 `Microsoft.Windows.AI.MachineLearning` 2.4.89의 `onnxruntime.dll`(ORT 1.27.1)·`DirectML.dll`을 넣는다. 이 패키지의 최소 OS가 Windows 10 1903(18362)이다(1809는 `Microsoft.WindowsAppSDK.ML`이 필요하므로 지원하지 않음). M0에서 `ort::init_from`으로 이 DLL을 `api-27`로 불러 DML EP가 붙는 것과, 네트워크 없는 Windows 11 Sandbox에서 실행되는 것을 확인했다([기록](M0_ORT_SPIKE.md#windows-ml--directml)). 세션 순차 실행, memory pattern 비활성화. 재배포 약관은 10.1 |
| macOS arm64 | CoreML(`ModelFormat=MLProgram`, `MLComputeUnits=ALL`) → CPU | 기본 옵션은 결과가 달라진다(M0). compiled cache는 앱 캐시 폴더 아래로 명시. 번호판 end2end 모델은 감지 0건일 때 CoreML이 빈 동적 텐서를 거부하므로 CPU로 돌린다(M0, ORT 1.30·1.31) |
| Linux x86_64 | CPU | CUDA는 1.3의 후보 |

Windows ML의 **EP 카탈로그**(NVIDIA TensorRT-RTX, Intel OpenVINO, AMD MIGraphX·VitisAI, Qualcomm QNN)는 첫 stable에서 쓰지 않는다. Windows 11 24H2 이상이 필요하고, 첫 실행에 네트워크로 받고 Windows Update로 자동 갱신되어 오프라인 실행·고정 manifest 원칙과 맞지 않는다. 이 앱의 모델은 작아 DML로 충분하다. 필요가 측정되면 사용자가 켜는 opt-in 기능으로 따로 설계한다.

GPU 실패(ORT 오류, GPU 메모리 부족) 시 현재 항목을 CPU 세션으로 한 번 다시 실행하고, 그 실행의 나머지는 CPU로 처리하며 이유를 표시한다. CPU 실패, shape 불일치, 모델 해시 오류는 폴백으로 숨기지 않는다.

**ORT 텔레메트리:** Microsoft가 배포하는 ORT 빌드는 텔레메트리가 기본으로 켜져 있다. Windows 구성 요소(Windows ML)는 OS 진단 데이터 경로(TraceLogging)로, macOS·Linux 공식 빌드는 1.29부터 들어 있는 1DS SDK로 앱 프로세스가 직접 `mobile.events.data.microsoft.com`에 HTTPS로 보내고, 사용자 Library(또는 홈) 아래 SQLite 저장소에 이벤트를 쌓는다. M0에서 확인한 결과 **`ORT_DISABLE_TELEMETRY=1`만이 1DS 전송을 막고, `DisableTelemetryEvents`(`with_telemetry(false)`)만으로는 전송 시도가 계속된다**([기록](M0_ORT_SPIKE.md#macos-api-27-재확인과-텔레메트리)). 그래서 `main` 첫 줄에서 다른 스레드가 생기기 전에 환경 변수를 설정하고, `with_telemetry(false)`도 함께 호출한다. 네트워크 감시(`spikes/ort/netwatch`)를 M8 릴리스 시험과 ORT 갱신마다 돌린다. 1.12.1 macOS 패키지와 Linux AppImage(둘 다 공식 1.29.0, 텔레메트리 호출 없음)는 전송을 시도하는 상태였다(Linux는 에뮬레이션 사전 실행).

## 4. 아키텍처

### 4.1 crate 구성

```text
crates/
  cloakframe-core      도메인 타입, 기하, 마스크 렌더러, 추적, 검토 규칙, 상태 전이
                       (std + serde만. 파일·네트워크·GUI·ORT 금지)
  cloakframe-engine    작업 실행, 스캔, codec·색·EXIF, 모델 관리와 추론(ORT), FFmpeg,
                       게시, 세션 저장, 설정
                       bin: cloakframe-cli  (M1부터, 검증 도구 겸 배치 실행)
  cloakframe-desktop   egui 앱 (M6)
```

- desktop은 pixel 처리와 FFmpeg 인자 생성을 하지 않는다. engine의 명령·이벤트·조회 API만 쓴다. CLI도 같은 공개 API만 쓴다.
- ORT는 `engine::detect` 안에서만 쓰고 `Detector` trait 뒤에 둔다. 나중에 프로세스로 분리해도 호출부가 바뀌지 않게 한다.
- trait는 교체·실패 주입이 필요한 경계(추론, 미디어 프로세스, 파일 게시, HTTP)에만 둔다. 기하·추적 내부는 구체 타입과 함수로 쓴다.
- `unsafe`는 기본 금지하고 OS·FFI 어댑터에서만 국소 허용한다.

### 4.2 상태와 게시 권한

정상 경로: `Queued → Analyzed → NeedsReview 또는 ReadyForRender → Rendering → Verified → Published`. 실패·취소는 어느 단계에서나 종료 상태로 간다.

| 값 | 내용 |
| --- | --- |
| `Detection` | 종류(Face/Plate), score, 원본 표시 좌표의 box·landmark, 모델 ID, frame |
| `CoverageEvidence` | 상한으로 잘린 후보, 실패 frame, 미해결 gap, 약한 후보, 사용자 제외·수정 기록 |
| `ReviewRevision` | 입력·모델·설정·타임라인 해시에 묶인 수정 번호. 오래된 응답은 적용하지 않음 |
| `TimelineSpec` | 유리수 FPS, 시작점, rotation, SAR, 색 태그, 분석 해상도 변환 |
| `PublishPermit` | engine 내부에서만 생성. 처리 성공, 미해결 문제 없음, 출력 검증, 취소 여부를 확인한 뒤 발급 |

- **처리 후 검토:** 분석은 검토보다 앞서 진행한다. 검토를 기다리는 결과는 앱 데이터 폴더의 세션(감지 결과와 편집 결정만, pixel 없음)에 두고, 승인되기 전에는 출력 폴더에 아무것도 쓰지 않는다.
- **자동 모드(검토 끔):** 미해결 문제가 없는 항목만 게시하고 "자동 처리·미검토"로 표시한다. 나머지는 "검토 필요" 목록에 남는다.
- **경고 포함 내보내기:** 미해결 문제를 알고도 내보내려면 검토 화면에서 항목별로 명시적으로 고른다. 정상 완료 수에 넣지 않는다.
- gap과 누락 후보는 수동 마스크로 덮거나 검토자가 오탐이라고 지정해야 해결된다. 해당 프레임을 열어 본 것만으로는 해결되지 않는다.

### 4.3 사진 파이프라인

1. **스캔:** 취소 가능, 파일 identity로 중복 제거, 출력 폴더와 숨김·시스템 파일 제외, 미지원 형식은 개수로 보고
2. **읽기:** 열고 identity 기록 → 바이트 전체 읽기(2 GB 상한) → 닫기. 이후 디코드, EXIF 방향, ICC, 메타데이터 allowlist는 모두 이 바이트만 쓴다. 이미지 하나를 여러 라이브러리가 다시 파싱하지 않는다.
3. **디코드:** 픽셀 상한(512 MP)과 메모리 permit, EXIF 방향을 pixel에 반영. 저장용 버퍼(U8/U16, alpha 유지)와 감지용 sRGB RGB8 버퍼를 따로 만든다.
4. **감지:** 전체 보기 한 번. 고품질 모드는 겹치는 타일 + 타일 간 NMS를 더한다.
5. **검토:** 원본 좌표의 편집 문서. 필요할 때만.
6. **렌더·인코딩:** core 렌더러(미리보기와 같은 함수)로 마스크 적용 → D6 기본값으로 인코딩 → D2 색 정책, 선택 시 allowlist EXIF를 새로 작성
7. **게시:** 원본 identity(크기, 수정 시각, 파일 ID)가 그대로인지 확인 → 4.5의 방식으로 게시. 원본이 바뀌었으면 그 항목은 실패

### 4.4 영상 파이프라인

- **조사:** ffprobe 결과로 `TimelineSpec`을 한 번 만들고 분석·미리보기·내보내기가 같은 값을 쓴다. 지원 범위를 벗어난 입력(10비트, HDR 등)은 여기서 이유와 함께 거부한다(M9 전까지).
- **1패스:** FFmpeg로 분석 해상도(긴 변 960 이하) 디코드 → 매 프레임 장면 전환 판정과 감지 → 감지 결과를 frame 색인이 있는 append-only 레코드 파일에 기록. 프레임을 건너뛰지 않는다.
- **추적:** 레코드 파일을 앞에서, 뒤에서 읽는 양방향 추적. 종류별 연관, high/low score 2단계, IoU·움직임 gating과 Hungarian 할당. active track만 메모리에 두므로 RAM이 영상 길이에 비례하지 않는다. 1.12.1의 보간·평활화·끝 연장·장면 전환 경계 처리를 이식하되 P-6을 고친다.
- **미리보기:** 가까운 keyframe에서 디코드하고 PTS를 같은 규칙으로 정규화해 목표 frame을 찾는다. 마스크는 core 렌더러로 그린다. 저해상도 연속 재생을 지원한다.
- **2패스:** 원본 해상도 디코드 → 마스크(트랙별로 고정한 모자이크 격자) → 인코딩(하드웨어 우선, 실패 시 소프트웨어로 2패스만 재시도). 오디오는 1.12.1 정책 유지, D1 색 태그, P-5 pad.
- **검증·게시:** P-4 검사 후 게시. 원본 identity는 1패스 전, 패스 사이, 게시 직전에 비교한다. 사본은 만들지 않는다.
- **처리량:** 원시 프레임을 파이프로 주고받으므로 4K30에서 방향당 약 750 MB/s가 오간다. M0 측정에서 파이프 상한은 약 9.5 GB/s였고 병목은 인코더였다(4K 2패스 VideoToolbox 58 fps, libx264 15 fps, 1.12.1 전체 44 fps와 같은 수준). 외부 프로세스 구조를 유지하고 하드웨어 인코더를 우선한다. yuv420p 파이프와 YUV 마스킹(4K 디코드 46% 빠름)은 M4의 선택적 최적화다([기록](M0_FFMPEG_PIPE.md)).

### 4.5 출력 게시

- 임시 파일은 대상 폴더 안에 `.cloakframe-tmp-<random>`으로 기본 권한(umask·ACL 상속)으로 만든다. 쓰기 → fsync → no-replace rename:
  - Linux: `renameat2(RENAME_NOREPLACE)`, 미지원 시 `link` + `unlink`
  - macOS: `renameatx_np(RENAME_EXCL)`, 미지원 시 `link` + `unlink`
  - Windows: `MoveFileExW`(`MOVEFILE_REPLACE_EXISTING` 없이)
  - 둘 다 안 되는 파일시스템(FAT·exFAT·일부 네트워크 볼륨): 대상 이름이 없음을 확인한 직후 일반 rename. 남는 경쟁 구간은 같은 사용자 범위라 수용한다.
- 대상 이름이 이미 있으면 사용자 파일로 보고 덮지 않는다. 검토 화면에서 새 이름(`photo (2).jpg`)을 제안한다.
- 정리: 실행이 쓰는 폴더에서 하루 넘은 `.cloakframe-tmp-*`만 지운다. 별도의 기억 파일이나 lock은 두지 않는다.
- 출력 하위 폴더는 입력 폴더 구조를 따르고 기본 권한으로 만든다.

### 4.6 모델

- manifest에 URL·SHA-256·크기·라이선스를 고정한다. 다운로드는 모델 폴더 안 임시 파일로 스트리밍하면서 해시를 계산하고, 일치하면 rename한다.
- 로드: 바이트를 읽고 해시 확인 → 그 바이트로 세션 생성(경로를 다시 열지 않음).
- 다운로드 전 모델별 이용 조건을 보여 준다(YOLO5Face-n은 비상업 전용). 원본 배포처가 개인 GitHub 릴리스이므로, 라이선스가 허락하면 자체 릴리스 자산으로 미러링한다.
- 기존 CloakFrame 모델 캐시는 해시를 확인해 새 캐시로 복사한다.

### 4.7 자원과 취소

- 상한은 입력 경계에만 둔다: 파일 크기, 픽셀 수, 영상 해상도·FPS·길이·프레임 수, 모델의 프레임당 후보 수. 모델 상한 초과는 실패가 아니라 `CoverageEvidence`다.
- 채널은 bounded. 진행률·미리보기 이벤트는 최신값으로 대체하고, 상태 전이와 사용자 편집은 유실 없는 경로로 보낸다.
- 취소 토큰은 항목·프레임·타일·다운로드 단위로 확인한다. 취소 시 자식 프로세스 트리를 종료한다.
- 진행률은 작업량 기준이다. 사진은 픽셀 수, 영상은 프레임 수 × 패스 수로 계산한다.

## 5. 보안·개인정보

**위협 모델:** 입력 미디어, ICC/EXIF, 업데이트 응답, 네트워크는 신뢰하지 않는다. 같은 사용자 권한의 다른 프로세스와 같은 기기의 다른 계정은 범위 밖이다(SECURITY.md와 같음).

| 유지 | 내용 |
| --- | --- |
| 메타데이터 | 기본 전부 제거. 선택 보존은 allowlist 31개 키(entry 128 B, EXIF 블록 65,527 B 제한), GPS는 별도 동의. orientation은 pixel 반영 후 1, ColorSpace·크기 등 구조 태그는 출력에서 계산 |
| 영상 부가 정보 | subtitle·data·attachment·chapter·위치·컨테이너 metadata 제거, 오디오 언어 코드만 재기록 |
| FFmpeg | shell 없이 인자 배열, `-protocol_whitelist file`과 `file:` 입력, 앱 번들의 실행 파일만 사용(릴리스 빌드는 PATH 탐색 안 함) |
| 모델 | 해시 고정, 해시한 바이트로 세션 생성, ONNX external data 거부 |
| 업데이트 | Velopack 업데이트는 기존 v2 Ed25519 payload(channel·version·filename·digest) 검증 유지, Sparkle은 EdDSA + `SURequireSignedFeed` + `SUVerifyUpdateBeforeExtraction`. 최고 설치 버전을 기록해 downgrade 차단 |
| 크래시·로그 | 릴리스 빌드 크래시 덤프 차단, 기본 로그는 단계·오류 코드·수치만. 상세 로그는 opt-in |
| 네트워크 | 모델 다운로드와 업데이트만. 앱 telemetry 없음, ORT telemetry 끔(3장). 업데이트 확인을 끄면 진행 중인 요청과 타이머에도 즉시 반영. Windows ML·DirectML 약관의 데이터 수집 조항(OS 진단 데이터 경로)은 정보 창과 개인정보 안내에 적는다 |

**익명화 기본값:** 단색 채우기를 기본값으로 한다. 모자이크는 셀 수 상한(D3)과 트랙별 고정 격자, 블러는 커널을 자르지 않고 최소 강도를 둔다. 사용자 이미지 밑에는 불투명 바탕을 깐다. soft edge는 보호 영역 바깥에만 적용한다. 이것들을 식별 불가의 증명으로 표현하지 않는다.

**버린 것(0.1 참고):** snapshot 사본, root handle traversal, stage lease·marker, 비공개 ACL, 내부 데이터 상한, 번들 바이너리 해시 확인, FFmpeg·추론 sandbox 계획.

## 6. UX 설계 (M6)

**메인 화면**은 위에서 아래로 세 단계다.
1. 큰 드롭 영역과 썸네일 그리드. 형식별 개수를 보여 주고, 미지원 파일은 목록 안에서 이유와 함께 표시한다.
2. *무엇을*(얼굴·번호판 토글)과 *어떻게*(채우기·모자이크·블러·이미지, 강도 슬라이더)를 한 줄에. 실제 샘플 사진에 바로 적용한 미리보기를 붙인다.
3. 저장 위치와 시작 버튼.

**고급 설정(접힘):** 정확/빠름 모델 토글, 민감도 슬라이더 하나(점수 임계값, NMS는 노출하지 않음), 타일 모드, 메타데이터 보존·GPS, 오디오 제거, 영상 품질·코덱, GPU 사용.

**처리 화면:** 파일별 상태 목록(대기·분석·검토 필요·저장·실패), 작업량 기준 진행률과 예상 시간. 로그는 상세 패널로 내린다.

**사진 검토**
- 분석이 앞서 나가고, 사용자는 "검토 필요" 큐를 처리한다. 그리드는 미해결 문제 → 감지 0건 → 저신뢰 순으로 정렬한다.
- 편집기: 원본 해상도까지 확대(필요한 타일만 디코드하고 타일 텍스처로 그림. wgpu 기본 텍스처 한도 8192), 클릭으로 토글, 드래그로 추가, 키보드 이동·크기 조절, Space로 마스킹 미리보기, undo/redo. 단축키 안내는 `?` 오버레이로.
- "익명화 없이 내보내기"는 보조 메뉴에 두고 확인을 받는다(메타데이터 제거 재인코딩).

**영상 검토:** track 레인과 gap 표시가 있는 타임라인, 저해상도 연속 재생 + 마스킹 미리보기, 프레임 단위 이동, keyframe 편집, gap마다 "덮음/오탐" 결정.

**결과:** "12개 저장, 2개 검토 필요 — 영상 1개에서 얼굴을 1.2초간 놓침" 같은 문장 요약과 항목별 바로가기. 숫자를 나열하는 모달은 쓰지 않는다. 재검토는 같은 세션에서 연다.

**플랫폼:** 시작 시 모달 금지(업데이트는 비모달 배너), 네이티브 메뉴, 정보 창(앱의 Apache-2.0 라이선스, 서드파티 고지와 Microsoft 약관, 모델 라이선스와 이용 조건), 표준 단축키, 시스템 테마, CJK 글꼴 번들, 접근성 이름과 포커스 순서, 창 위치 기억.

## 7. 저장소와 전환

```text
./
  Cargo.toml, Cargo.lock, rust-toolchain.toml
  crates/{cloakframe-core, cloakframe-engine, cloakframe-desktop}/
  assets/{icons, fonts, locales}/
  manifests/        모델·네이티브 런타임의 버전·URL·해시·라이선스
  tests/{fixtures, golden, integration}/
  packaging/{windows, macos, linux}/
  tools/            fixture 추출, 번역 이전, 패키지 검사
  spikes/           M0 spike(독립 Cargo 프로젝트, workspace 밖). M0 기록을 다시 돌리는 용도
```

- M1 첫 커밋에서 레거시 C++ 소스·빌드 파일·CI를 지운다(`v1.12.1` 태그로 보존). 아이콘 등 재사용 자산만 `assets/`로 옮긴다. THIRD_PARTY_NOTICES도 이때 새 의존성 기준으로 다시 쓴다.
- workspace의 모든 crate는 `license = "Apache-2.0"`이다. `cargo-deny`로 의존성 라이선스를 검사해 GPL·AGPL crate를 막는다. 네이티브 라이브러리는 manifest에 라이선스를 적고, LGPL은 동적 링크로만 쓴다.
- M0의 기준 측정과 fixture 추출은 1.12.1 릴리스 패키지(또는 태그 빌드)로 한다. `tools/fixture-extract/run.sh`는 `v1.12.1` 태그를 작업 폴더로 내보내 빌드하므로 레거시 소스를 지운 뒤에도 fixture를 다시 만들 수 있다.
- 1.12.1과 달라지는 동작은 `docs/CHANGES_FROM_1.12.md`에 한 줄씩 적는다(1.12.1 동작, 새 동작, 이유). 최종 CHANGELOG와 첫 실행 안내의 재료가 된다.

## 8. 로드맵

각 단계는 이전 단계의 완료 기준을 통과해야 시작한다.

**시간 예산(2026-10-16 결정):** 첫 stable(M8 완료)까지 전업 환산 **32주**, 투입은 전업(주 약 40시간)이라 달력 기간도 약 32주다. M0의 남은 실기기 확인이 끝난 주부터 센다(10월 19일에 시작하면 2027년 5월 말). M9는 이 예산 밖이다. 단계별 배분은 M0에서 확인한 작업량으로 추정한 값이다.

| 단계 | M1 | M2 | M3 | M4 | M5 | M6 | M7 | M8 | 합계 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 예산(주) | 3 | 4 | 3 | 4 | 5 | 7 | 4 | 2 | 32 |
| 실제 | 2일 | | | | | | | | |

**점검 규칙:** 단계가 끝날 때마다 실제 소요를 이 표 옆에 적는다. 어느 단계든 예산의 125%를 넘으면(예: 4주 예산이 5주를 넘음) 아래 범위 조정 순서에서 아직 미루지 않은 첫 항목을 첫 stable 뒤로 미룬다. M5 끝에는 누적 소요를 전체 예산과 비교해 한 번 더 점검하고, 남은 단계(M6–M8 13주)가 남은 예산에 들어가지 않으면 다음 항목도 미룬다.

### M0 — 위험 검증과 기준선 (2026-10-17 완료)

- **완료:**
  - Apple M4에서 `ort` rc.14 + ORT 1.30으로 세 모델의 CPU·CoreML MLProgram 결과가 1.12.1과 같음. YuNet 원시 출력 직접 해석. `INTER_LINEAR` 이식 resize 바이트 일치. ([기록](M0_ORT_SPIKE.md))
  - Windows ML 2.4.89(ORT 1.27.1)를 `ort::init_from`과 `api-27`로 불러 DML EP가 붙음. RTX 5080에서 세 모델의 CPU 결과가 1.12.1과 같고 DML 결과도 개수가 같음(IoU ≥ 0.999997). YOLO5Face-n·YuNet은 모든 노드가, 번호판은 NMS 하나만 빼고 DML에서 실행. 네트워크 없는 Windows 11 Sandbox에서 실행(정적 CRT 빌드 필요). x86에서 `INTER_LINEAR` 이식이 36개 크기 조합에서 바이트 일치. ([기록](M0_ORT_SPIKE.md#windows-ml--directml))
  - macOS egui spike: 한글·일본어 표시, IME 조합 이벤트 처리, AccessKit 트리, 파일 드롭 이벤트, muda 메뉴(macOS 실행, Windows 타입 검사), 4K 확대·이동과 1080p 업로드(vsync 60 fps 유지) 통과. egui·winit 공개 이슈 조사에서 한국어·일본어 IME와 AccessKit에 대안 재평가가 필요한 결함이 없어 **egui 유지 확정**(2026-10-17). 남은 이슈(macOS 입력 소스 전환 직후 첫 자모 분리, X11 Fcitx5 조합 표시 없음 등)와 사람 확인 절차는 M6 점검으로 옮김. ([기록](M0_EGUI.md#공개-이슈-조사-2026-10-17))
  - 시간 예산(전업 환산 32주)과 단계별 배분, 점검 규칙, 범위 조정 순서를 정함(이 장 처음)
  - macOS FFmpeg 파이프 처리량: 파이프 상한 약 9.5 GB/s, 4K 2패스는 인코더가 결정(VideoToolbox 58 fps). 1.12.1 기준선과 같은 수준. ([기록](M0_FFMPEG_PIPE.md))
  - 기준 fixture: 1.12.1 시험 166개의 실제 입력·출력·assert를 기록하고 상태(규범·의도·변경·제거)를 붙임. D1–D12·P-1–P-7 전부 올바른 기대 결과를 쓰고 13개는 1.12.1에서 실패를 재현(P-3은 재현 안 됨, 회귀 시험으로 둠). 감지 기준 3모델 × 6장 × 3임계값, allowlist EXIF 기준. ([fixture](../tests/fixtures/README.md), 생성: `tools/fixture-extract/run.sh`)
  - allowlist EXIF writer가 JPEG·PNG·WebP·TIFF 네 형식 모두에서 fixture 기대값과 같고, exiftool·Exiv2·Pillow·macOS ImageIO·libwebp·libtiff·ImageMagick이 모두 읽음. ([기록](M0_EXIF_WRITER.md))
  - macOS를 `api-27`로 다시 빌드해 공식 ORT 1.30.0에서 54건(세 모델 × 6장 × 3임계값)이 1.12.1 fixture와 같음. 텔레메트리는 `ORT_DISABLE_TELEMETRY=1`일 때만 전송 시도 0회. `ort` 기본 EP 정책과 CoreML 번호판 문제를 찾아 EP 정책에 반영. ([기록](M0_ORT_SPIKE.md#macos-api-27-재확인과-텔레메트리))
  - 버전·feature·네이티브 manifest: Rust 툴체인과 M0에서 정한 crate 버전·feature, ORT·Windows ML·FFmpeg·Sparkle·Velopack·글꼴·모델의 플랫폼별 버전·URL·SHA-256·라이선스·소스 위치. 모든 URL을 내려받아 해시 일치 확인. FFmpeg·Sparkle·Velopack은 1.12.1 고정값을 이어받아 M4·M7에서 다시 정함. ([manifests](../manifests/README.md))
  - Linux x86_64(GitHub Actions `ubuntu-24.04`, 실기기 대신): `api-27` CPU 54건과 폴백 CPU 재실행이 1.12.1 fixture와 같음, `INTER_LINEAR` 36쌍 바이트 일치(IPP 켬·끔), 텔레메트리는 `ORT_DISABLE_TELEMETRY=1`일 때만 시도 0회(libnetwatch·strace 모두), 1.12.1 AppImage는 전송 시도. FFmpeg 2패스는 파이프 없는 구성과 같은 처리량. 데스크톱 확인은 M6, 하드웨어 인코더는 M4로 옮김. ([ORT 기록](M0_ORT_SPIKE.md#linux-x86_64-실행-github-actions), [FFmpeg 기록](M0_FFMPEG_PIPE.md#linux-x86_64-github-actions-소프트웨어만))
- **뒤 단계로 옮긴 Windows 확인**(2026-10-17, Windows 10·Intel·AMD 장치를 지금 쓸 수 없음. 스크립트와 절차는 [실행 절차](M0_PLATFORM_RUNBOOK.md) 그대로 씀):
  - 작업 중 DML 실패(GPU 강제 리셋) 시 같은 입력의 CPU 재실행 → M3 완료 기준. 프로세스가 죽으면 0.1의 "driver crash → 프로세스 분리"를 그때 판단한다
  - 깨끗한 Windows 10 1903 VM에서 오프라인 실행 → M7 완료 기준(최소 지원 OS 설치·오프라인 실행). 실패하면 최소 OS 표기를 올린다. 오프라인 실행 자체는 Windows 11 Sandbox에서 확인함
  - Intel·AMD GPU의 DML 결과, 프로세스 소켓 감시 → M8(12장 실제 장치·네트워크 감시 항목)
- **완료 기준:** 버전·feature·네이티브 manifest, fixture 세트, 위 spike 결과와 결정 기록. 필수 항목(egui IME·접근성, DirectML)이 실패하면 대규모 구현 전에 대안을 재평가한다.

### M1 — headless 골격 (2026-10-18 완료)

- **작업:** workspace와 CI(fmt, clippy `-D warnings`, 3개 OS 테스트), core 타입과 상태 전이, engine 작업 실행기(명령·이벤트·취소), 스캐너, 게시 모듈, 세션 저장, CLI(`cloakframe-cli run`, 검토 결정 JSON 적용)
- **완료 기준:** 합성 입력의 정상·실패·취소 경로 동작, 원본과 기존 출력 불변, 임시 파일이 최종 이름으로 보이지 않음. D4·D10·D12·P-1(골격) 시험 통과
- **완료(2026-10-18):** 소요 2일(2026-10-17–18, 예산 3주), PR #18–#24. main `160f17b`에서 Linux·macOS·Windows CI가 위 기준의 시험을 모두 통과했다. 정상·실패·검토·이름 충돌·P-1 경로는 `cloakframe-cli`를 합성 processor(`synthetic` feature, 릴리스 빌드 제외)로 끝까지 돌리는 통합 시험(`crates/cloakframe-engine/tests/cli.rs`)으로, 취소는 실행기 시험으로, D4(Windows는 폴더 ACL 상속)·D12는 프로세스 전역 상태를 바꾸는 별도 시험 바이너리로 확인한다. 이월: CLI의 Ctrl+C 취소(신호 처리, M4의 자식 프로세스 감독과 함께), 1.12 설정 이전(M7)

### M2 — 사진 (CPU, YOLO5Face-n)

- **작업:** `INTER_LINEAR` 이식 규칙(M0 기록)을 시험으로 고정하고 2배 축소의 `INTER_AREA` 경로 추가, 지원 형식·bit depth·alpha·방향, 색 정책, EXIF writer, 마스크 4종·shape·padding·roll·soft edge, 인코딩 기본값, 게시 게이트, 검토 결정 적용, 익명화 없이 내보내기
- **완료 기준:** 사진 fixture 통과, D2·D3·D6·D9·P-2(타입) 시험 통과
- **진행(2026-10-18, 시작 같은 날):** 0–7단계로 나눠 단계마다 PR을 하나씩 올린다.
  - 완료: 0단계 코덱 spike(#25, JPEG는 libjpeg-turbo, 손실 WebP는 libwebp, 고정 ORT를 CI에서 3개 OS 비교), 1단계 읽기·디코드(#26, 형식 내용 판정·여러 프레임 거부·512 MP·메모리 예산·EXIF 방향·감지 사본)
  - PR 열림: 2단계 색 정책 D2(#27, colorimetry 판정, 번들 profile `assets/icc`, 그 밖은 sRGB 변환)
  - 남음: 3단계 감지(engine에 `ort`, `INTER_LINEAR` 고정·`INTER_AREA` 이식, YOLO5Face-n, NMS·IoU, 종류별 임계값, 감지 fixture 54건), 4단계 마스크 렌더러(core, D3), 5단계 인코딩·메타데이터(D6, EXIF writer, ICC 부착, D9), 6단계 PhotoProcessor·검토 결정의 수동 영역, 7단계 완료 확인
  - 결정: CMYK·YCCK JPEG는 거부(1.12.1과 다름, `CHANGES_FROM_1.12.md`)

### M3 — 모델·GPU·다운로드·타일

- **작업:** YuNet·번호판 어댑터, 3장 EP 정책과 CPU 폴백, 모델 다운로드·캐시, 타일 모드, 종류별 임계값
- **완료 기준:** 1.12.1 대비 허용오차 내 결과, 소규모 벤치마크 재현율, GPU 없을 때 CPU로 동작, Windows 실기기에서 GPU 강제 리셋 시 같은 입력을 CPU로 다시 실행(M0에서 옮김). D5·D7·D8·D11 시험 통과

### M4 — 영상 시간축·인코딩

- **작업:** `TimelineSpec`, 디코드·인코드 프로세스 관리, 색 태그, pad, 오디오 정책, 하드웨어→소프트웨어 재시도, 출력 검증, 고정 수동 마스크로 끝까지 내보내기
- **완료 기준:** 프레임 번호와 오디오 임펄스를 넣은 합성 영상에서 frame 일치·A/V 동기 확인. D1·P-3·P-4·P-5 시험 통과

### M5 — 영상 감지·추적·검토 결정

- **작업:** 매 프레임 감지, 레코드 파일, 종류별 양방향 추적, 장면 전환, `CoverageEvidence`·gap, 검토 결정(track 제외·keyframe·gap 해결) 적용, 미리보기 조회 API
- **완료 기준:** 추적 fixture 통과, 1시간 영상에서 RAM이 길이에 비례하지 않음, 미해결 문제가 게시를 막음. P-1(영상)·P-2(추적)·P-6 시험 통과. 범위 조정 점검 시점

### M6 — desktop 앱

- **작업:** 6장 UX 전체, 사진·영상 검토 화면, 설정, 3개 언어·테마, 정보 창
- **완료 기준:** 모든 흐름을 UI로 수행, `egui_kittest` 상호작용 시험, 실제 OS의 IME·파일 대화상자·스크린리더·드롭·메뉴 단축키·GPU 없는 환경·원격 데스크톱 점검([M0 egui 절차와 공개 이슈 목록](M0_EGUI.md), [Windows·Linux 절차](M0_PLATFORM_RUNBOOK.md)). preview 릴리스 가능

### M7 — 배포·업데이트·이전

- **작업:** 세 OS 패키지(Velopack 설치본·AppImage, DMG), 서명·공증, 기존 feed로 업데이트, 키 필수화와 서명 feed, 네이티브 manifest·SBOM·라이선스·대응 소스, CloakFrame 설정(QSettings 읽기 어댑터 → TOML)과 모델 캐시 이전
- **완료 기준:** 깨끗한 최소 지원 OS(Windows 10 1903 VM 포함, M0에서 옮김)에서 설치·오프라인 실행·업데이트·제거, 1.12.1 → Rust 업데이트 경로 확인, 이전 실패 시 기존 데이터 불변. P-7 통과

### M8 — 검증과 stable 전환

- **작업:** 1.12.1 대비 품질·성능 비교, 장시간 안정성, 번역·접근성 점검, `CHANGES_FROM_1.12.md` 정리, 릴리스 문서
- **완료 기준:** 12장 체크리스트 전부 충족. Windows는 NVIDIA·Intel·AMD 각각에서 DML 결과와 프로세스 소켓 감시를 포함(M0에서 옮김)

### M9 — HEIC·HDR 입력 (stable 직후)

- **작업:** HEIC/HEIF 디코드(플랫폼 디코더와 libheif 중 spike로 결정, 출력은 JPEG 또는 HEIC), 10비트·HDR 영상 디코드 → SDR 톤매핑 → 8비트 출력(변환 사실을 결과에 표시)
- **완료 기준:** 아이폰 기본 설정으로 촬영한 사진·영상 fixture 처리

**범위 조정 순서**(2026-10-16 확인, 예산을 넘으면 앞에서부터 첫 stable 뒤로 미룬다): ① 영상 마스킹 미리보기의 연속 재생(프레임 단위 미리보기는 유지) ② 앱 재시작을 넘는 검토 저장·재개 ③ 타일 모드(작은 얼굴 재현율에 영향이 있어 마지막). 2장의 결함 수정과 5장의 유지 항목은 조정 대상이 아니다.

## 9. 시험 전략

### 9.1 필수 시험

| 수준 | 범위 |
| --- | --- |
| core 단위·property | 좌표 변환·clip·rotation, 마스크가 보호 영역을 줄이지 않음, NMS·종류별 연관, 상태 전이, 레코드 파일 왕복 |
| 골든 출력 | 2장 D·P 항목 전부, 금지 메타데이터 0개, 색 태그·ICC, 출력 권한, 인코딩 파라미터 |
| 추론 일치 | 세 모델의 box·score·landmark를 M0 fixture와 허용오차로 비교 |
| 영상 | 합성 영상(프레임 번호·오디오 임펄스), 23.976/29.97 FPS, 시작 오프셋, B-frame, rotation·SAR, 홀수 크기, 여러 오디오 트랙, VFR |
| 실패 주입 | 단계별 취소, 디스크 가득 참, 권한 거부, 처리 중 원본 변경, FFmpeg 비정상 종료, GPU 오류 |
| UI | `egui_kittest` 상호작용 + 수동 OS 체크리스트(IME, 파일 대화상자, 스크린리더, HiDPI) |
| fuzz | EXIF 읽기 입력, ffprobe JSON 파서 두 곳만 |

CI에서 모델과 FFmpeg는 해시로 고정해 받고, `cargo-deny`로 의존성 라이선스와 보안 권고를 검사한다. 필수 시험에 필요한 자산이 없으면 건너뛰지 않고 실패한다.

### 9.2 탐지 품질

사용권을 확인한 소규모 라벨 세트(작은·큰 얼굴, 측면, 군중, 저조도, 번호판 크기·각도별)로 계층별 재현율·정밀도를 잰다. 같은 설정에서 1.12.1 대비 전체 재현율이 1%p 이상 떨어지면 안 된다. 타일 모드는 추가 비용 대비 작은 얼굴 재현율 개선을 따로 보고한다. 영상은 라벨이 있는 짧은 클립에서 노출 프레임 수를 잰다.

### 9.3 성능

고정 장치(Windows DML 장치, Apple silicon 16 GB, Linux x64 16 GB)에서 1.12.1 릴리스 패키지와 같은 입력·모델·설정으로 비교하고 p50/p95를 기록한다.

| 지표 | 목표 |
| --- | --- |
| 사진 처리량 | 같은 품질에서 1.12.1보다 10% 넘게 느려지지 않음 |
| 영상 처리량 | 1080p·4K 분석 fps와 최종 배속을 보고, 1.12.1보다 느려지지 않음 |
| 메모리 | 영상 길이 10분 → 1시간에서 RAM이 선형으로 늘지 않음 |
| UI | 반응 p95 50 ms 이하, 취소 접수 100 ms 이하, 협조적 중단 2초 이하 |
| A/V 동기 | 누적 drift 없음, 오차는 한 프레임 또는 20 ms 중 큰 값 이내 |

## 10. 배포

| 플랫폼 | 패키지 | 포함할 것 |
| --- | --- | --- |
| Windows | Velopack 설치본 | Windows ML 런타임 DLL, FFmpeg, 앱. 실행 파일은 `+crt-static`으로 빌드한다(VC++ 런타임 없는 PC에서 시작 실패를 막음, M0 확인). `System32`에 OS의 오래된 ORT·DirectML이 있으므로 `ort::init_from`에 앱 폴더의 절대 경로를 준다. DLL 검색 경로를 앱 폴더로 제한. Authenticode 확보 여부는 운영 결정(ROADMAP) |
| macOS | app bundle + DMG | Developer ID 서명과 공증, Sparkle 2.9.x, ORT dylib, FFmpeg |
| Linux | AppImage(Ubuntu 24.04에서 빌드) + Velopack | CPU ORT, FFmpeg |

- 네이티브 manifest 하나에 플랫폼별 버전·URL·아카이브 해시·라이선스·소스 위치를 적는다. 빌드가 임의 최신 ORT·FFmpeg를 내려받지 않게 한다.
- 릴리스 흐름: 빌드 → 패키지 실행 시험 → 보호된 release 환경에서 서명(서명 비밀은 environment secret으로) → 게시. 기존 `CloakFrame-updates` 저장소의 feed를 이어 쓴다.
- 1.11.3 이하처럼 옛 feed에 묶인 사용자는 수동 설치 안내를 유지한다(ROADMAP의 수용 위험).

### 10.1 라이선스: Apache-2.0 (2026-10-16 결정)

CloakFrame 소스는 Apache License 2.0으로 배포한다. 저장소의 `LICENSE`도 이날 바꿨다. 1.12.1까지의 패키지는 Exiv2(GPL-2.0-or-later)를 링크하므로 패키지 전체가 GPL-3.0-or-later로 배포됐고, 그 사실은 README와 THIRD_PARTY_NOTICES에 남긴다.

**바꾼 이유:** Windows ML 런타임(`Microsoft.Windows.AI.MachineLearning`)은 MIT인 ORT 소스와 달리 Microsoft Software License Terms로 배포된다. 재배포는 허용하지만(3a), 배포자와 최종 사용자가 Microsoft를 이 약관만큼 보호하는 조건에 동의하게 해야 하고(3b), 소스 공개를 요구하는 라이선스가 이 코드에 적용되도록 배포하면 안 된다(3c-ii). 이는 예외 없는 GPL 배포와 충돌한다. 1.12.1이 함께 배포한 `DirectML.dll`도 같은 종류의 약관이었으므로, Windows GPU 가속을 하는 한 피할 수 없었다. GPL에 7조 추가 허가를 붙이는 방법도 있었지만, 단순하게 Apache-2.0으로 충돌 자체를 없앴다.

**지켜야 할 것:**
- 링크하는 의존성은 Apache-2.0과 호환되어야 한다. GPL·AGPL crate는 쓰지 않고 `cargo-deny`로 막는다. LGPL 네이티브 라이브러리는 동적 링크로만 쓴다.
- FFmpeg는 별도 프로세스이므로 GPL 빌드를 번들해도 된다. nonfree 빌드는 금지하고, FFmpeg의 GPL 고지와 대응 소스 위치를 THIRD_PARTY_NOTICES에 둔다.
- Microsoft 약관(Windows ML 런타임) 전문을 THIRD_PARTY_NOTICES와 정보 창에 넣는다. 3b에 따라 설치 시 동의 화면까지 필요한지는 M7 전에 정한다.
- 레거시 코드를 Apache-2.0으로 바꾸는 것은 저작권자 전원의 결정이어야 한다. 커밋 작성자는 메인테이너의 계정들뿐이다.

이 절은 법률 자문이 아니다.

## 11. 위험

| 위험 | 확인 시점 | 대응 |
| --- | --- | --- |
| egui의 IME·접근성·그래픽 호환성 | M6 (M0 공개 이슈 조사로 유지 확정) | 필수 동작이 실패하면 iced·Slint를 같은 편집기 시나리오로 재평가. headless 우선이라 engine 작업은 버려지지 않음 |
| Windows ML이 ORT 버전을 정함(현재 1.27.1), DirectML은 유지보수 모드 | M0, M7 | Windows ML 패키지 버전을 manifest에 고정하고 세 OS의 `api-N`을 그 ORT에 맞춤. 맞는 짝이 없으면 그 버전에 머묾. DML 회귀는 실기기 게이트로 잡음 |
| `ort` RC | M0, M3 | 확인한 정확한 버전으로 고정, 추론 어댑터 뒤에 숨김 |
| 의존성 라이선스 호환 | 전 단계 | `cargo-deny`로 GPL·AGPL crate 차단, 네이티브는 manifest에 라이선스 기록(10.1) |
| Microsoft 약관 3b의 동의 요건 | M7 | 설치 화면에 약관 동의가 필요한지 M7 전에 정함 |
| ORT 텔레메트리 | M0, M8 | `ORT_DISABLE_TELEMETRY=1`(실제 차단) + `with_telemetry(false)` + 네트워크 감시. 새 ORT 버전마다 다시 확인 |
| 프로세스 내 추론의 GPU driver crash | M3, M8 | 측정되면 `Detector` trait 뒤에서 별도 프로세스로 분리 |
| FFmpeg 파이프 처리량 | M0(macOS 해소), M4 | 파이프 상한 9.5 GB/s, 병목은 인코더. Windows·Linux 하드웨어 인코더 경로는 같은 spike로 확인 |
| TIFF EXIF writer | M0(해소) | 네 형식 모두 성공([기록](M0_EXIF_WRITER.md)). M2에서 ICC 부착과 함께 다시 확인 |
| 모델 라이선스·배포처 | M3, M7 | 조건 표시, 허용되면 자체 미러, 상업용 대체 모델은 권리와 정확도 평가 후 |
| 범위 팽창 | 전 단계 | 8장의 시간 예산(32주)과 단계별 125% 점검, 조정 순서, 3개 crate와 headless 우선 유지 |

## 12. stable 전환 체크리스트

- [ ] 1.1의 모든 제품 계약에 구현과 시험 근거가 있고, 바뀐 동작은 `CHANGES_FROM_1.12.md`에 있다.
- [ ] 2장의 D1–D12, P-1–P-7 시험이 모두 통과한다.
- [ ] 사진·영상 전체 흐름이 UI와 CLI 양쪽에서 동작하고, 자동 미검토·검토 완료·경고 포함 내보내기·익명화 없이 내보내기가 구별된다.
- [ ] Windows DML·macOS CoreML 가속과 세 플랫폼 CPU 경로가 실제 장치에서 동작한다.
- [ ] 9.2 품질 기준과 9.3 성능 목표를 측정 원자료와 함께 충족한다.
- [ ] 한국어·영어·일본어, 키보드, 스크린리더, HiDPI, 파일 대화상자가 실제 OS에서 통과한다.
- [ ] 세 OS 패키지가 깨끗한 최소 지원 OS에서 설치·오프라인 실행·업데이트·제거를 통과하고, 1.12.1에서 업데이트로 넘어오는 경로와 설정·모델 이전이 확인됐다.
- [ ] 패키지와 정보 창에 Apache-2.0 LICENSE, 새 의존성 기준의 THIRD_PARTY_NOTICES, Microsoft 약관 전문이 들어 있고, `cargo-deny` 라이선스 검사가 통과한다.
- [ ] 세 OS에서 앱이 모델 다운로드와 업데이트 외에 네트워크 요청을 하지 않음을(ORT 텔레메트리 포함) 네트워크 감시로 확인했다.
- [ ] 필수 시험의 건너뜀, 미해결 핵심 결함, 미구현 핵심 기능을 예외로 남기지 않는다.

## 참고 자료

- egui/eframe: <https://github.com/emilk/egui>, <https://docs.rs/crate/eframe/0.36.2>, `egui_kittest`: <https://docs.rs/egui_kittest/latest/egui_kittest/>
- ort: <https://github.com/pykeio/ort>, rc.14: <https://github.com/pykeio/ort/releases/tag/v2.0.0-rc.14>
- ONNX Runtime EP: DirectML <https://onnxruntime.ai/docs/execution-providers/DirectML-ExecutionProvider.html>, CoreML <https://onnxruntime.ai/docs/execution-providers/CoreML-ExecutionProvider.html>
- Windows ML 배포: <https://learn.microsoft.com/en-us/windows/ai/new-windows-ml/distributing-your-app>, EP 카탈로그 비교: <https://learn.microsoft.com/en-us/windows/ai/new-windows-ml/windows-ml-eps-vs-bring-your-own>
- Windows ML 2.4.89 패키지(ORT 1.27.1, 최소 OS)·약관: <https://www.nuget.org/packages/Microsoft.Windows.AI.MachineLearning/2.4.89>, <https://www.nuget.org/packages/Microsoft.Windows.AI.MachineLearning/2.4.89/License>, 릴리스: <https://github.com/microsoft/WindowsML/releases/tag/v2.4.89>
- ort 기능 플래그(`api-17`–`api-31`): <https://docs.rs/crate/ort/2.0.0-rc.14/features>, `with_telemetry`: <https://docs.rs/ort/2.0.0-rc.14/ort/environment/struct.EnvironmentBuilder.html>
- ONNX Runtime 텔레메트리: <https://github.com/microsoft/onnxruntime/blob/main/docs/Privacy.md>
- Apache License 2.0: <https://www.apache.org/licenses/LICENSE-2.0>, cargo-deny: <https://github.com/EmbarkStudios/cargo-deny>
- image: <https://github.com/image-rs/image>, moxcms: <https://docs.rs/moxcms/latest/moxcms/>, kamadak-exif: <https://docs.rs/kamadak-exif/latest/exif/>, fast_image_resize: <https://docs.rs/fast_image_resize/latest/fast_image_resize/>
- FFmpeg: CLI <https://ffmpeg.org/ffmpeg.html>, 프로토콜 <https://ffmpeg.org/ffmpeg-protocols.html>, 라이선스 <https://ffmpeg.org/legal.html>, ffmpeg-next <https://github.com/zmwangx/rust-ffmpeg>
- Velopack Rust: <https://docs.velopack.io/getting-started/rust>, Sparkle: <https://sparkle-project.org/documentation/>
- Tokio·Rayon: <https://tokio.rs/tokio/topics/bridging>, <https://docs.rs/rayon/latest/rayon/>
- ByteTrack: <https://github.com/FoundationVision/ByteTrack>

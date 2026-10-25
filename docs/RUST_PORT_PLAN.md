# CloakFrame Rust 재작성 계획

작성 2026-10-15, 전면 개정 2026-10-16(같은 날 Windows·macOS M0 결과와 시간 예산 반영). 근거: [코드베이스 분석 보고서](REWRITE_ASSESSMENT.ko.md). M0 기록: [ORT](M0_ORT_SPIKE.md), [EXIF writer](M0_EXIF_WRITER.md), [egui](M0_EGUI.md), [FFmpeg 파이프](M0_FFMPEG_PIPE.md), [Windows·Linux 실행 절차](M0_PLATFORM_RUNBOOK.md), [기준 fixture](../tests/fixtures/README.md). M2 기록: [코덱 spike](M2_ENCODE_SPIKE.md), [M0–M2 재검토](M0_M2_REVIEW.md). M3 기록: [감지](M3_DETECTION.md), [EP·CPU 폴백](M3_EP.md), [모델 파일](M3_MODELS.md), [타일 모드](M3_TILES.md), [탐지 벤치마크](M3_BENCHMARK.md), [타일 오탐](M3_TILE_FALSE_DETECTIONS.md). M4 기록: [프로세스 감독](M4_PROCESS.md), [조사와 TimelineSpec](M4_PROBE.md), [디코드와 프레임 대응](M4_DECODE.md), [0–2단계 재검토](M4_REVIEW.md), [인코드](M4_ENCODE.md), [영상 처리기](M4_VIDEO.md), [완료 확인](M4_VERIFY.md). M5 기록: [단계 계획 실험](M5_PLAN.md), [core 추적](M5_TRACK.md), [분석 1패스](M5_ANALYSIS.md), [2패스와 게시 게이트](M5_RENDER.md), [검토 세션과 결정](M5_REVIEW.md), [영상 탐지 품질](M5_BENCHMARK.md), [완료 확인](M5_VERIFY.md), [M0–M5 재검토](M0_M5_REVIEW.md). M6 기록: [단계 계획](M6_PLAN.md). 이전 판은 git 기록(`a42dec2`의 이 파일)에 있다.

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
| D3 | 모자이크 셀 12개 상한 때문에 픽셀 단위 "블록 크기" 설정이 큰 얼굴에서 무효 | 설정을 픽셀 대신 "강도"(영역 짧은 변에 들어가는 셀 수, 4–12, 기본 8)로 바꿔 범위 안의 모든 값이 결과를 바꾸게 함. 12가 최소 강도 역할로, 1.12.1에서 가능했던 가장 약한 모자이크보다 약하게는 하지 않음(2026-10-19, 재검토 13번에서 16 → 12). 축소는 영역 평균 | M2 |
| D4 | 출력 0600/0700, Windows 보호 DACL | 출력 파일·폴더가 umask와 폴더 ACL 상속을 따름 | M1 |
| D5 | umask 002에서 모델 저장 거부 | 모델 폴더 권한 검사 없음. 로드할 때 해시로 검증 | M3 |
| D6 | JPEG q100 4:4:4, WebP 무손실 | JPEG q92·원본 크로마 서브샘플링 유지, 손실 WebP 원본은 손실 q90, PNG/TIFF는 무손실 | M2 |
| D7 | 같은 모델 중복 로드, 스캔 전 로드 | (모델, 입력 크기, EP)마다 세션 하나. 입력 스캔 후 필요한 모델만 로드 | M3 |
| D8 | 탐지기 포인터 데이터 경쟁 | 세션 소유자를 추론 스레드 하나로 둠. Rust 타입으로 보장 | M3 |
| D9 | 원본 복사 시 GPS·시리얼 보존 | 원본 바이트 복사 제거 | M2 |
| D10 | 대소문자 비교가 파일시스템과 무관 | 대소문자 동일시 여부를 파일시스템 조사로 한 곳에서 결정 | M1 |
| D11 | NMS 전 후보 상한 초과분을 누락 영역으로 셈 | NMS 후 상한 초과만 `CoverageEvidence`로 기록. NMS 전 후보는 자르지 않는다. 출력 크기가 고정된 YOLO5Face-n(25,200행)은 M2에서 NMS 전 상한(2026-10-18, crowd fixture 259 → 280개)과 NMS 뒤 상한 300(2026-10-19, 재검토 1번)을 없애 NMS를 통과한 감지를 모두 가리고, 그렇지 않은 모델은 상한을 넉넉히 두고 초과 시 타일로 재탐지. 고정한 세 모델은 모두 상한이 없어(M3 2단계) 재탐지는 그런 모델을 들일 때 만든다(2026-10-19 결정) | M2(YOLO5Face), M3 |
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
| UI | egui/eframe 0.37.x(M6 0단계에서 0.36.2에서 올림, wgpu 기본, glow 대체 경로), `rfd`(파일 대화상자), AccessKit(eframe 내장) | 네이티브 메뉴는 `muda`로 macOS·Windows만(Windows는 winit 메시지 훅에서 `TranslateAcceleratorW`). muda는 GTK 창에만 붙어 Linux는 egui 메뉴 막대. 한글·일본어는 Noto Sans KR/JP 부분 집합을 번들(OFL, Regular 2개 4.1 MB, 이름 이체자 처리는 M6). UI 시험은 `egui_kittest`([M0 기록](M0_EGUI.md)) |
| 추론 | `ort` =2.0.0-rc.15(`load-dynamic`, `api-27`, `default-features = false`) + ONNX Runtime | 런타임은 Windows가 Windows ML 2.4.89 안의 ORT 1.27.1, macOS·Linux가 공식 ORT 1.30.x다. ORT C API는 하위 호환이므로 API 수준은 가장 낮은 Windows에 맞춰 세 OS 공통 `api-27`로 둔다. Windows ML이 새 ORT를 담으면(2.5.77-rc는 1.28.0) 함께 올린다. 독립 배포 `Microsoft.ML.OnnxRuntime.DirectML`은 1.24.4가 마지막이고 DirectML은 두 경로 모두 1.15.4이므로, Windows ML로 옮겨 얻는 것은 이어지는 ORT 갱신이다. `api-30`으로 빌드하면 Windows에서 `BadVersion`으로 로드가 멈춘다(M0 확인). M0에서 `api-27`로 macOS 공식 1.30.0·Windows ML 1.27.1 모두 세 모델의 CPU 결과와 CoreML·DML 결과가 1.12.1과 같음을 확인. `ort`의 세션 빌더는 EP 자동 선택 정책 `MaxEfficiency`를 기본으로 걸어 ORT 1.31이 CPU 세션에도 CoreML을 붙이므로, 세션마다 `with_auto_device(PreferCPU)`로 덮고 EP를 명시한다. RC이므로 정확한 버전으로 고정. M5 5단계에서 rc.14의 세션 출력 누수(실행마다 272 B) 때문에 rc.15로 올림([기록](M5_VERIFY.md#찾은-것-ort-rc14의-출력-누수)) |
| 이미지 | `image` 0.25(PNG·TIFF·BMP·WebP, 필요 codec만), JPEG 디코드·인코드는 libjpeg-turbo(`turbojpeg`, 2026-10-18 결정: `image`의 zune-jpeg 0.5.15가 non-interleaved 4:2:0 JPEG를 조용히 잘못 디코드하고, libjpeg-turbo는 1.12.1의 OpenCV와 pixel까지 같음, [기록](M2_ENCODE_SPIKE.md)), 손실 WebP 인코드는 libwebp(`libwebp-sys`), TIFF 인코드(LZW)와 TIFF ICC 판독은 `tiff`(M2 5단계: `image`의 TIFF 인코더는 압축을 고를 수 없고, `image`는 작은 TIFF의 ICC를 limit 때문에 버림), BMP V5의 내장 ICC는 자체 판독(`image`가 읽지 않음, M2 7단계), `moxcms`(색), `kamadak-exif`(읽기) + 자체 allowlist EXIF writer(`engine::photo::metadata`) | 모델 전처리 resize는 OpenCV `INTER_LINEAR`를 옮긴 자체 구현(`engine::detect::resize`). 정확히 2배 축소일 때 OpenCV가 바꿔 쓰는 `INTER_AREA` 경로까지 옮겼고, x86 OpenCV 5.0.0 일반 경로와 92개 크기 조합에서 바이트 일치를 시험으로 고정했다(M2 3단계, `tests/fixtures/resize`). `fast_image_resize`는 화면 표시용에만 |
| 영상 | 번들 FFmpeg/ffprobe 외부 프로세스 | nonfree 없는 GPL/LGPL 빌드, 대응 소스 제공. 별도 프로세스라 앱의 Apache-2.0 라이선스에 영향이 없다. M0에서 파이프가 병목이 아님을 확인(4.4) |
| 동시성 | 작업 실행기는 std 스레드와 bounded 채널(항목 단위 순서 보존 병렬, 상태 이벤트는 유실 없는 bounded 채널, 진행률은 최신값 보드), Rayon pool(CPU 연산), 전용 추론 스레드(`Detector`를 단독 소유, D8). 작은 Tokio runtime은 자식 프로세스 감독(M4)에 필요할 때 추가 | CPU 연산을 async executor에서 돌리지 않는다. M1 실행기에는 async 작업이 없어 Tokio를 쓰지 않았고, M3 모델 다운로드도 동기 HTTP(ureq)라 쓰지 않는다(2026-10-19). M4 0단계의 자식 프로세스 감독도 도구마다 감시 스레드로 해 쓰지 않는다(2026-10-21) |
| 설정·로그·오류 | serde + TOML, 앱 데이터 폴더는 자체 구현(`engine::app_dirs`. `directories`는 의존성 `option-ext`가 MPL-2.0이라 쓰지 않음, 2026-10-18), `tracing`, `thiserror` | 기본 로그에 경로·파일명을 넣지 않음 |
| 다국어 | `fluent-bundle` | 기존 `.ts` 번역을 스크립트로 `.ftl`로 이전 |
| 네트워크·무결성 | `ureq`(동기) + rustls(ring) + `rustls-platform-verifier`(OS 인증서 저장소), `sha2`, `ed25519-dalek` | 모델·업데이트 URL allowlist. reqwest 대신 ureq(2026-10-19 결정: 의존성 114개 → 33개, Tokio 없음, [기록](M3_MODELS.md#http-crate)) |
| 파일시스템 | `rustix`(Unix), `windows` crate | no-replace rename과 identity 조회에만 사용 |
| 업데이트·패키징 | Velopack Rust SDK(Windows·Linux), Sparkle(macOS, `objc2` 어댑터) | 기존 feed와 package ID를 이어 써야 1.12.1 사용자가 자동 업데이트로 넘어온다 |

**추론 EP 정책**

| 플랫폼 | 가속 → 폴백 | 조건 |
| --- | --- | --- |
| Windows x64 | Windows ML self-contained 런타임의 DirectML → CPU | ORT를 따로 번들하지 않고 `Microsoft.Windows.AI.MachineLearning` 2.4.89의 `onnxruntime.dll`(ORT 1.27.1)·`DirectML.dll`을 넣는다. 이 패키지의 최소 OS가 Windows 10 1903(18362)이다(1809는 `Microsoft.WindowsAppSDK.ML`이 필요하므로 지원하지 않음). M0에서 `ort::init_from`으로 이 DLL을 `api-27`로 불러 DML EP가 붙는 것과, 네트워크 없는 Windows 11 Sandbox에서 실행되는 것을 확인했다([기록](M0_ORT_SPIKE.md#windows-ml--directml)). 세션 순차 실행, memory pattern 비활성화. 재배포 약관은 10.1 |
| macOS arm64 | CoreML(`ModelFormat=MLProgram`, `MLComputeUnits=ALL`) → CPU | 기본 옵션은 결과가 달라진다(M0). compiled cache는 앱 캐시 폴더 아래로 명시하고, 이 빌드가 쓰지 않는 ORT 빌드·모델의 폴더와 1.12.1의 cache는 지운다(M3 4단계). 번호판 end2end 모델은 감지 0건일 때 CoreML이 빈 동적 텐서를 거부하므로 CPU로 돌린다(M0, ORT 1.30·1.31) |
| Linux x86_64 | CPU | CUDA는 1.3의 후보 |

Windows ML의 **EP 카탈로그**(NVIDIA TensorRT-RTX, Intel OpenVINO, AMD MIGraphX·VitisAI, Qualcomm QNN)는 첫 stable에서 쓰지 않는다. Windows 11 24H2 이상이 필요하고, 첫 실행에 네트워크로 받고 Windows Update로 자동 갱신되어 오프라인 실행·고정 manifest 원칙과 맞지 않는다. 이 앱의 모델은 작아 DML로 충분하다. 필요가 측정되면 사용자가 켜는 opt-in 기능으로 따로 설계한다.

GPU 실패(ORT 오류, GPU 메모리 부족) 시 실패한 요청(타일 하나, 영상 프레임 하나, 조합 안에서는 실패한 모델만)을 CPU 세션으로 한 번 다시 실행하고, 그 실행의 나머지는 CPU로 처리하며 이유를 표시한다. 그 항목에서 앞서 GPU로 끝낸 타일은 GPU 결과를 쓴다. CPU 실패, shape 불일치, 모델 해시 오류는 폴백으로 숨기지 않는다.

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
| `ReviewRevision` | 입력·모델·설정·타임라인 해시에 묶인 수정 번호. 오래된 응답은 적용하지 않음. 모델·설정은 검토 결정을 적용할 때(영상은 영상 설정), 입력은 렌더할 때 다시 읽은 바이트의 해시로 비교한다(영상은 파일 전체를 2패스와 나란히 읽음). 타임라인은 영상의 `TimelineSpec`과 원본 프레임 목록의 해시로, 재개할 때 다시 조사해 비교한다(사진은 빈 값, M5 3단계). CLI 결정 파일은 항목·수정 번호만 들고 입력 키는 없어, 다른 입력의 세션에 쓴 결정 파일은 번호로만 막힌다([M0–M5 재검토](M0_M5_REVIEW.md) 9번) |
| `TimelineSpec` | 유리수 FPS, 시작점, 시간 기준, rotation, SAR, 색 태그. 원본 프레임 목록(`SourceFrames`)과 함께 출력 프레임 n이 보일 원본 프레임을 정함. 분석 해상도는 같은 표시 크기에서 `media::decode::frame_size`가 정함 |
| `PublishPermit` | core의 `PublishPermit::issue`만 만들 수 있고, 항목 상태(검증 완료)와 근거(미해결 문제 없음, 또는 명시적 내보내기)를 확인해 발급한다. 발급하는 곳은 engine 실행기 하나로, 처리 성공과 취소 여부를 확인한 뒤다. crate가 나뉘어 있어 "engine에서만"은 타입으로 막지 않는다 |

- **처리 후 검토:** 분석은 검토보다 앞서 진행한다. 검토를 기다리는 결과는 앱 데이터 폴더의 세션(감지 결과와 편집 결정만, pixel 없음)에 두고, 승인되기 전에는 출력 폴더에 아무것도 쓰지 않는다.
- **자동 모드(검토 끔):** 미해결 문제가 없는 항목만 게시하고 "자동 처리·미검토"로 표시한다. 나머지는 "검토 필요" 목록에 남는다.
- **경고 포함 내보내기:** 미해결 문제를 알고도 내보내려면 검토 화면에서 항목별로 명시적으로 고른다. 정상 완료 수에 넣지 않는다.
- **익명화 없이 내보내기:** 검토 화면에서 항목별로 명시적으로 고른다. 아무것도 가리지 않고 같은 메타데이터 규칙으로 다시 인코딩하며(D9), 미해결 문제가 있어도 게시하되 정상 완료 수에 넣지 않는다.
- gap과 누락 후보는 수동 마스크로 덮거나 검토자가 오탐이라고 지정해야 해결된다. 해당 프레임을 열어 본 것만으로는 해결되지 않는다.

### 4.3 사진 파이프라인

1. **스캔:** 취소 가능, 파일 identity로 중복 제거, 출력 폴더와 숨김·시스템 파일 제외, 미지원 형식은 개수로 보고
2. **읽기:** 열고 identity 기록 → 바이트 전체 읽기(2 GB 상한) → 닫기. 이후 디코드, EXIF 방향, ICC, 메타데이터 allowlist는 모두 이 바이트만 쓴다. 이미지 하나를 여러 라이브러리가 다시 파싱하지 않는다.
3. **디코드:** 픽셀 상한(512 MP)과 메모리 permit, EXIF 방향을 pixel에 반영. 저장용 버퍼(U8/U16, alpha 유지)와 감지용 sRGB RGB8 버퍼를 따로 만든다.
4. **감지:** 전체 보기 한 번. 타일 모드(기본 켬)는 원본 배율 1까지 층마다 겹치는 타일을 더하고 합친다([기록](M3_TILES.md)). 성긴 보기에서도 찾을 만한 크기인데 거기서 찾지 못한 저점수 타일 감지는 버린다([기록](M3_TILE_FALSE_DETECTIONS.md)).
5. **검토:** 원본 좌표의 편집 문서(가리지 않을 감지와 추가로 가릴 영역). 필요할 때만. 검토가 끝난 항목은 원본을 다시 읽고, 분석할 때와 identity가 같을 때만 렌더한다.
6. **렌더·인코딩:** core 렌더러(미리보기와 같은 함수)로 마스크 적용 → D6 기본값으로 인코딩 → D2 색 정책, 선택 시 allowlist EXIF를 새로 작성. 출력 형식은 출력 이름의 확장자를 따른다(내용과 이름이 다른 원본, 1.12.1과 같음). BMP 출력에는 profile을 붙이지 않으므로 Display P3·Adobe RGB는 sRGB로 변환하고 결과에 표시한다(BMP V5는 profile을 담을 수 있지만 macOS ImageIO가 그런 파일을 열지 못함, 2026-10-19)
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

- manifest에 URL·SHA-256·크기·라이선스를 고정한다(engine `model_files::MODEL_FILES`가 같은 값인지 시험). 다운로드는 HTTPS로 GitHub와 그 다운로드 호스트에만 가고(redirect마다 allowlist 검사), 모델 폴더 안 임시 파일로 스트리밍하면서 해시를 계산하고, 일치하면 rename한다.
- 모델 폴더는 앱 데이터 폴더의 `models`다(캐시 폴더가 아님). 폴더 권한은 보지 않는다(D5).
- 로드: 바이트를 읽고 해시 확인 → 그 바이트로 세션 생성(경로를 다시 열지 않음).
- 다운로드 전 모델별 이용 조건을 보여 준다(YOLO5Face-n은 비상업 전용, CLI는 `--accept-terms`가 있어야 받음). 원본 배포처가 개인 GitHub 릴리스이므로, 라이선스가 허락하면 자체 릴리스 자산으로 미러링한다(M7).
- 1.12.1 모델 폴더는 macOS·Windows에서 새 모델 폴더와 같은 경로라 그대로 쓴다. Linux와 옛 Redactly 폴더의 모델은 새 폴더에 없을 때 해시를 확인해 복사하고 원본은 둔다([기록](M3_MODELS.md)).

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
| 실제 | 2일 | 2일 | 2일 | 2일 | 4일 | | | | |

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

### M2 — 사진 (CPU, YOLO5Face-n) (2026-10-19 완료 확정)

- **작업:** `INTER_LINEAR` 이식 규칙(M0 기록)을 시험으로 고정하고 2배 축소의 `INTER_AREA` 경로 추가, 지원 형식·bit depth·alpha·방향, 색 정책, EXIF writer, 마스크 4종·shape·padding·roll·soft edge, 인코딩 기본값, 게시 게이트, 검토 결정 적용, 익명화 없이 내보내기
- **완료 기준:** 사진 fixture 통과, D2·D3·D6·D9·P-2(타입) 시험 통과
- **진행(2026-10-18, 시작 같은 날):** 0–7단계로 나눠 단계마다 PR을 하나씩 올린다.
  - 완료: 0단계 코덱 spike(#25, JPEG는 libjpeg-turbo, 손실 WebP는 libwebp, 고정 ORT를 CI에서 3개 OS 비교), 1단계 읽기·디코드(#26, 형식 내용 판정·여러 프레임 거부·512 MP·메모리 예산·EXIF 방향·감지 사본), 2단계 색 정책 D2(#27, colorimetry 판정, 번들 profile `assets/icc`, 그 밖은 sRGB 변환), 3단계 감지(#28, engine에 `ort`와 `OrtRuntime`(`ORT_DISABLE_TELEMETRY=1`이 아니면 로드 거부, `PreferCPU` 후 CPU EP 명시), `INTER_LINEAR` 고정·`INTER_AREA` 이식, YOLO5Face-n, core의 IoU·종류별 NMS·`Thresholds`(P-2), 결과 JSON의 `kind`. 엔진 디코드 경로로 YOLO5Face-n 감지 fixture 18건(6장 × 3임계값)을 3개 OS CI에서 비교. YuNet·번호판 36건은 M3 어댑터 전까지 `ort.yml` spike가 비교), 4단계 마스크 렌더러(#29, core `mask`: 채우기·모자이크·블러·사용자 이미지, 사각형·타원, padding, soft edge, 사용자 이미지 roll. 모자이크는 D3 강도(짧은 변 셀 수 4–16, 기본 8, 셀 경계 `floor(i·L/n)`, 긴 변 `round(n·long/short)`, 셀은 최소 2 px), 블러는 영역 밖 pixel까지 커널에 넣고 sigma 하한 4 px. 감지 box는 모든 조합에서 완전히 가림(property 시험). 사각형 채우기·soft edge·사용자 이미지는 1.12.1 fixture와 pixel이 같고, 타원은 1.12.1이 가린 pixel을 모두 가림), 5단계 인코딩·메타데이터(#30, `photo::encode`·`photo::metadata`: 원본 형식으로 저장, JPEG q92·원본 서브샘플링(libjpeg-turbo), 손실 WebP 원본은 libwebp q90, PNG·무손실 WebP·TIFF(LZW, 1.12.1과 같음)는 무손실(D6). 메타데이터 기본은 전부 제거, 보존 시 allowlist EXIF를 새로 쓰고 GPS는 별도 동의, ColorSpace는 출력에서 계산. D2 결과대로 번들 profile을 JPEG APP2·PNG iCCP·WebP ICCP·TIFF 34675에 붙임. 원본 바이트 복사 없음(D9). 네 컨테이너 출력이 exiftool `-validate`·exiv2·ImageMagick에서 읽힘. 작은 TIFF의 ICC를 `image`가 limit 때문에 버리던 2단계 판독 결함도 고침), 6단계 사진 처리기(#31, `photo::process::PhotoProcessor`: 읽기 → 디코드 → 색 정책 → sRGB 감지 사본으로 YOLO5Face-n → 마스크 → 인코딩 → 게시 직전 원본 identity 확인. 실행기가 분석에서 렌더로 넘기는 상태와 렌더 결과를 항목 안에서만 들고 있어, 검토로 가는 항목은 디코드한 pixel과 메모리 permit을 바로 놓음. `Found.over_limit`은 `CappedCandidates`로 기록해 P-1로 게시를 막고, NMS 전 후보 상한은 없앰(D11). 검토 결정의 제외·추가 영역(core `PhotoEdits`)을 마스크 영역으로 반영하고, "수동 마스크로 덮음" 해결은 추가 영역이 있을 때만 받음. 익명화 없이 내보내기(D9)는 별도 게시 근거로 정상 완료에 넣지 않음. 검토 결정은 분석 때와 같은 모델·설정의 처리기에만 적용. 출력 형식은 이름의 확장자(1.12.1 규범 fixture), 변환하거나 버린 색 profile은 결과 note로 표시(D2). 모델은 고정 SHA-256과 같은 바이트로만 세션을 만듦. CLI는 `--ort-library`·`--face-model`·`--keep-metadata`·`--keep-gps`로 실제 감지 경로를 돌리고, 스캔에서 처리할 파일이 나올 때만 모델을 불러옴. 시험: 가짜 감지기 파이프라인 11건(1.12.1 worker 시험 6개 포함), 실제 모델의 P-1(사진)·D11 2건, CLI 6건)
  - 완료: 7단계 완료 확인(m2-verify). 완료 기준과 P-1(사진)을 확인하는 시험은 아래 표. 이름으로 연결된 Rust 시험이 없던 1.12.1 M2 시험 5개(형식 내용 판정, 여러 프레임, EXIF 방향 두 개, 지원 확장자)는 fixture를 읽는 `decode_fixtures.rs`로, plan 항목이 없던 사진 시험 2개(메타데이터 판독 실패, 스레드 간 soft edge)와 D2 게시 파일 확인을 추가함. 확인 중 찾은 D2 누락: `image`가 BMP V5의 내장 profile을 읽지 않아 Display P3 BMP를 표시 없이 sRGB로 봤음 → 색 정책이 내장 profile(`MBED`)을 직접 읽고, 다른 파일을 가리키는 연결 profile(`LINK`)은 읽지 않고 버림 표시. BMP 출력은 profile 없이 sRGB로 변환(4.3). D2 기준 그림 `expected-{display-p3,adobe-rgb}-as-srgb.png`(Little CMS)를 `gen_inputs.py`로 추가
  - **완료 기준 확인:**

    | 항목 | 확인하는 시험(`crates/cloakframe-engine` 기준, core는 표시) |
    | --- | --- |
    | 사진 fixture: 감지·resize·디코드 | `detect_fixtures.rs`(YOLO5Face-n 18건), `detect::resize` 단위(`fixtures/resize`, x86은 `ort.yml`), `photo::decode` 단위(`fixtures/decode`), `decode_fixtures.rs` 5건 |
    | 사진 fixture: 1.12.1 마스크·메타데이터·worker | `mask_fixtures.rs` 9건, `encode_fixtures.rs`의 allowlist·썸네일·EXIF 판독 실패, `photo_pipeline.rs`의 worker 시험 6개, `cli_photo.rs` `a_rerun_keeps_the_earlier_output`, core `geometry`·`detection`의 IoU·NMS·잘못된 감지 |
    | D2 | `photo::color` 단위 11건(판정·번들 profile·유지·변환·회색 profile·Display P3와 Adobe RGB 감지 사본을 Little CMS와 비교·버린 profile·16 MiB 넘는 profile·작은 TIFF의 profile·BMP V5 profile·BMP 출력 변환), `photo::encode` `every_container_carries_the_allowlist_and_the_profile`, `photo_pipeline.rs` `published_photos_follow_the_d2_color_policy`(9건의 게시 파일 profile·pixel·note), `unusable_profiles_are_dropped_with_a_note`, `bmp_outputs_are_srgb_and_bmp_profiles_are_read`, `the_users_image_is_drawn_in_the_photos_color_space` |
    | D3 | `mask_fixtures.rs` `mosaic_strength_sets_the_cell_count`(강도 4–12)·`mosaic_touches_only_the_region`, core `mask` 단위(셀 평균, 작은·가장자리 영역, 모든 방식에서 감지 box를 가림) |
    | D6 | `encode_fixtures.rs` `d6_*` 2건(JPEG 양자화 표, 손실 WebP는 libwebp q90 출력과 같음), `photo::encode` `a_source_with_unusual_subsampling_gets_full_chroma` |
    | D9 | `encode_fixtures.rs` `d9_exports_carry_only_what_the_policy_allows`, `photo_pipeline.rs` `exporting_without_anonymization_*`, `cli_photo.rs` `exporting_without_anonymization_keeps_every_pixel` |
    | P-2(타입) | core `detection` 단위(종류별 NMS·임계값, 모델이 정하는 종류, 번호판 landmark 거부), core `serde_forms.rs`, `cli_photo.rs` 결과 JSON의 `kind` |
    | P-1(사진) | `photo_pipeline.rs` `capped_detections_hold_the_photo_until_it_is_exported_with_warnings`(가짜 감지기), `cli.rs`(M1 골격). 실제 모델은 NMS 뒤 상한이 없어 잘린 후보가 생기지 않음: `p1_photo.rs` 2건(crowd 280개, NMS 0.9의 601개를 모두 가림) |
  - 결정: CMYK·YCCK JPEG는 거부(1.12.1과 다름, `CHANGES_FROM_1.12.md`). YOLO5Face-n은 NMS 전 후보를 자르지 않음(2026-10-18, 상한 2,000개가 crowd fixture의 얼굴 21개를 버림, 최악의 NMS 약 0.3초). 6단계 판단 확정(2026-10-19): 감지 0건 사진은 미해결 문제가 아니라 정상 게시(1.12.1은 경고), 출력 형식은 이름의 확장자(1.12.1 규범 fixture), CLI 검토 결정은 run과 같은 모델·설정에만 적용. 넓은 색역 사진의 BMP 출력은 실패 대신 sRGB 변환·표시(위). 재검토 결정(2026-10-19): CMYK TIFF와 JPEG 압축 TIFF도 거부, 감지 0건은 결과에 표시
  - **완료(2026-10-19):** 소요 2일(2026-10-18–19, 예산 4주, 125% 점검 기준 5주 안), PR #25–#31과 7단계 `m2-verify`. 위 표의 시험이 macOS arm64(고정 ORT 1.30.0)에서 모두 통과, 3개 OS CI는 `m2-verify` PR에서 확인. 이월: YuNet·번호판 어댑터와 실제 모델의 종류별 임계값(M3)
  - **재검토(2026-10-19):** M0–M2 전체를 다시 봤다([기록](M0_M2_REVIEW.md)). 결함 10건, 위험 6건, 의심 5건과 문서 불일치를 찾았고, 그중 사실만 바로잡는 문서는 같은 날 고쳤다. 같은 날 1–21번을 메인테이너가 정한 순서로 항목마다 실패하는 시험을 먼저 넣고 고쳤다(`m2-verify`, 항목별 커밋, 처리 내용은 기록의 "처리" 절). 결정: 사진은 NMS 뒤 상한 없이 모두 가림(1번), CMYK TIFF도 거부(6번), 모자이크 강도 4–12(13번), 감지 0건은 표시하고 타일 모드는 범위 조정 목록에서 뺌(14번). macOS arm64에서 시험 216건 통과. PR #32(`m2-verify`)에서 Linux·macOS·Windows CI가 모두 통과해(Windows FAT32·긴 경로 시험 포함) 같은 날 완료를 확정하고 main을 `132e4bd`로 fast-forward했다. 남은 것: Windows debug 시험 빌드의 링커 경고 LNK4098은 `turbojpeg-sys`가 libjpeg-turbo의 C 런타임을 Rust와 맞추지 않아 생긴다. 배포 빌드(release, `+crt-static`)는 맞고 드러난 결함은 없다. upstream 수정을 올렸고([honzasp/rust-turbojpeg#44](https://github.com/honzasp/rust-turbojpeg/pull/44)), 그 판이 나오면 올린다([기록](M2_ENCODE_SPIKE.md#windows의-lnk4098-경고-2026-10-19))

### M3 — 모델·GPU·다운로드·타일 (2026-10-20 완료)

- **작업:** YuNet·번호판 어댑터, 3장 EP 정책과 CPU 폴백, 모델 다운로드·캐시, 타일 모드, 종류별 임계값
- **완료 기준:** 1.12.1 대비 허용오차 내 결과, 소규모 벤치마크 재현율, GPU 없을 때 CPU로 동작, Windows 실기기에서 GPU 강제 리셋 시 같은 입력을 CPU로 다시 실행(M0에서 옮김). D5·D7·D8·D11 시험 통과
- **단계(2026-10-19 계획):** 단계마다 브랜치·PR 하나.
  1. YuNet 어댑터(`m3-yunet`): engine `detect::yunet`, 감지 fixture 18건을 engine 시험으로 옮김(`ort.yml` spike는 고정 런타임 확인용으로 세 모델을 계속 비교). 이 단계 첫 커밋에 M2 완료 확정 문서. **완료(2026-10-19, PR #33, main `f7def9a`):** OpenCV `FaceDetectorYN` 후처리(정수 box NMS, top-k 5,000, 점수 엄격 비교, 얼굴 하나면 NMS 생략)를 옮겨 18건이 1.12.1과 같음(최소 IoU 0.99999, 점수 차 7e-7). 처리기·CLI의 모델 선택은 2단계 감지 구성에서. 3개 OS CI와 `ort.yml`의 고정 ORT 비교 통과
  2. 번호판 어댑터와 종류별 임계값(`m3-plate`): YOLOv9-t end2end, 얼굴·번호판 모델을 함께 돌리는 감지 구성, 실제 모델의 종류별 기본 임계값, fixture 18건. **완료(2026-10-19, PR #34, main `c4a4544`, [기록](M3_DETECTION.md)):** engine `detect::plate`가 1.12.1 `PlateDetector`의 전처리·해석을 옮겨 18건이 1.12.1과 같음(최소 IoU 0.9999966). 그래프 안 NMS의 상한 100개·IoU 0.45를 initializer 교체로 풀고 core NMS를 번호판 임계값으로 직접 돌림(원래 출력과 9장에서 같음, 100개 넘는 출력 시험). 그래서 고정 모델 중 NMS 뒤 출력이 제한된 모델은 없다. 모델별 기본 임계값은 core `ModelId::default_thresholds`(1.12.1이 실제로 쓴 값: 얼굴 score 0.5·NMS 0.4, 번호판 score 0.5·NMS 0.45). 감지 구성 `detect::models::DetectionModels`(얼굴 모델 하나 또는 없음, 번호판 켬·끔, 둘 다 없으면 거부)가 `PhotoSettings`에 들어가 review key의 `models`가 되고, 선택한 모델은 추론 스레드 하나에서 모델 사이 NMS 없이 합쳐짐(P-2). `Found.over_limit`은 모델별. CLI는 `--face-model` 대신 `--models DIR`·`--faces yolo5face-n|yunet|off`·`--plates on|off`, `review`도 같은 선택을 받음. 결정(2026-10-19): 기본 얼굴 모델 YOLO5Face-n, 번호판 기본 꺼짐(얼굴 없는 사진 179장 중 10장에 score 0.5 이상 번호판 오탐), 임계값은 1.12.1 값을 두고 6단계 벤치마크에서 다시 정함, 번호판 그래프 상한은 풂. 1단계 YuNet이 OpenCV처럼 NMS 전 후보를 5,000개로 잘라 약 440–770명 넘는 사진에서 얼굴을 버릴 수 있던 것을 찾아, 같은 날 결정으로 상한을 없앰(후보는 많아야 8,400개, 최악 NMS 60 ms). 3개 OS CI와 `ort.yml`의 고정 ORT 비교 통과(Windows는 CRLF checkout 때문에 manifest URL 줄 시험이 처음에 실패해 줄 끝에 기대지 않게 고침)
  3. EP 정책과 CPU 폴백(`m3-ep`): macOS CoreML(MLProgram, 번호판은 CPU), Windows DML, GPU 오류 시 그 항목부터 CPU 재실행과 표시(오류 주입 시험). Windows 실기기 GPU 리셋은 장치가 생기면 확인(M7·M8로 옮긴 확인과 같음). **완료(2026-10-19, PR #35, main `76a538f`, [기록](M3_EP.md)):** `detect::runtime::Backend`(CPU·CoreML·DirectML)와 1.12.1 세션 옵션(가속기는 `EXTENDED`·스레드 1, CoreML `MLProgram`·`ALL`, DML memory pattern 끔·순차·device 0). `detect::fallback`: macOS는 YOLO5Face-n·YuNet을 CoreML에, 번호판은 CPU에(후보 0개면 CoreML이 빈 텐서 거부, 그래프 NMS를 풀어도 같음), Windows는 세 모델을 DML에. 가속기에서 세션을 못 만들면 CPU로 시작하고, 사진이 가속기에서 실패하면 같은 사진을 CPU로 다시 분석한 뒤 그 실행의 나머지를 CPU로(모델 오류·CPU 실패는 다시 하지 않음). 그 항목에 note, CLI는 모델별 실행 위치와 폴백 이유를 stderr에. CoreML 컴파일 캐시는 앱 캐시 폴더 아래 ORT 빌드·모델별 폴더. 실행 위치는 review key에 넣지 않음(결과가 같음을 실행 위치마다 fixture로 확인). CLI `--gpu on|off`. 결정(2026-10-19): GPU 기본 켬, YuNet도 CoreML(macOS에서 18건 같고 2.5배 빠름), 표시는 항목 note와 CLI 줄. 실제 가속기 오류(번호판을 CoreML에 강제)로 폴백 시험. Windows runner에는 GPU가 없어 DML 결과와 번호판 0건 입력 확인은 장치가 생기면. 3개 OS CI 통과(Windows runner가 DML로 시작했는지는 시험이 출력을 잡아 로그에 남지 않음)
  4. 모델 manifest·다운로드·캐시(`m3-models`): URL allowlist, 모델 폴더 안 임시 파일로 받으며 해시 계산 후 rename, (모델, 입력 크기, EP)마다 세션 하나·스캔 뒤 필요한 모델만 로드(D5·D7), 이용 조건 표시 자료. 앞 단계에서 넘어온 것: CLI `--models`의 기본값을 앱 모델 폴더로, 1.12.1 모델 캐시를 해시 확인 후 새 캐시로 복사(4.6), CoreML 컴파일 캐시의 이전 ORT 빌드·모델 폴더 정리. **완료(2026-10-19, PR #36, main `832f829`, [기록](M3_MODELS.md)):** engine `model_files`: 고정 목록 `MODEL_FILES`(이름·URL·SHA-256·크기·이용 조건, manifest와 같은지 시험), `ModelStore`(모델 폴더 안 `.cloakframe-tmp-*`에 쓰며 해시, 크기·해시가 맞을 때만 rename, 권한 검사 없음), `download`(HTTP 경계 `Fetch`, `Https`는 ureq + rustls(ring) + OS 인증서 저장소, HTTPS·허용 호스트 4개만, redirect마다 검사). 모델 폴더는 `app_dirs::model_dir()`(데이터 폴더의 `models`)이고 macOS·Windows에서는 1.12.1 폴더와 같다. `legacy`: 1.12.1·Redactly 모델 폴더(없는 모델만 해시 확인 후 복사, 원본 둠), 1.12.1 CoreML 캐시 삭제. `OrtRuntime::remove_unused_coreml_caches`. `Backends::sessions_made`로 실행의 세션 수를 셈(D7). CLI: `--models` 생략 시 앱 폴더(1.12.1 모델 복사), `models`(상태·이용 조건), `models fetch yolo5face-n|yunet|plates`(YOLO5Face-n은 `--accept-terms` 필요), 없는 모델은 받을 명령을 안내. 결정(2026-10-19): 다운로드를 CLI 명령으로도 둠·비상업 모델은 동의 플래그, 모델 폴더는 데이터 폴더·1.12.1 모델은 복사(이동 아님), HTTP는 reqwest 대신 ureq(3장), CoreML 캐시는 쓰지 않는 폴더를 바로 지우고 1.12.1 캐시도 지움, 미러는 M7. rustls는 보안 권고 4건 때문에 공개 5일 된 0.23.46. `deny.toml`이 배포 대상 3개만 봄(wasm32 전용 CDLA 의존성 제외). 3개 OS CI 통과(Windows·Linux에서도 실제 GitHub 다운로드·D5·D7 시험). D7의 영상 경우는 M5
  5. 타일 모드(`m3-tiles`): 고해상도 사진의 작은 얼굴용 타일 감지와 겹침 병합, 출력 수가 제한된 모델의 상한 초과 시 타일 재감지(D11). **구현(2026-10-19, [기록](M3_TILES.md)):** engine `detect::tiles`: 전체 보기 배율에서 원본 배율 1까지 층 사이 4배 이하, 640 타일이 160 이상 겹침(원본 40 px 이상 물체는 어느 층에서 통째로 보임, 시험). 타일 입력은 `resize_region_linear`로 사본 없이 줄임. 병합은 타일 절반보다 큰 타일 box를 버리고(번호판 모델이 밋밋한 타일 전체를 번호판으로 봄), 타일 변에 잘린 box는 온전한 box가 80% 이상 덮으면 NMS 전에 버린 뒤 종류별 NMS(2026-10-20 성긴 보기 확인 추가, [기록](M3_TILE_FALSE_DETECTIONS.md)). 처리기는 타일을 하나씩 요청해 그 사이에 취소·진행률을 확인. `PhotoSettings::tiles`(review key), CLI `--tiles on|off`. 합성 12MP 사진에서 얼굴 96개 중 6–22개 → 60–66개, 얼굴 없는 36MP 13장에서 score 0.5 이상 오탐 0개(추상 그래픽 위주, 실제 사진은 6단계 벤치마크). 결정(2026-10-19): 피라미드 배치, 기본 켬, 번호판도 타일, D11 재감지는 상한 있는 모델을 들일 때. **완료(2026-10-20):** PR #37(`m3-tiles`), 3개 OS CI 통과 후 6단계와 함께 main `d4d8ed7` fast-forward(2026-10-21)
  6. 완료 확인(`m3-verify`): 소규모 벤치마크 재현율(9.2), D5·D7·D8·D11 시험 표, 문서. **완료(2026-10-20, [기록](M3_BENCHMARK.md)):** Open Images V7에서 고른 610장(Open Images의 S3 사본, 긴 변 1,024 px)과 그중 얼굴 사진 203장의 Flickr 원본으로 재현율·정밀도를 계층별로 잼(`tools/benchmark/`, 자료 `tests/fixtures/benchmark/`). 1.12.1 감지기를 v1.12.1 트리에서 빌드해 같은 사진으로 기록. 같은 설정에서 재현율 차이는 0–0.1%p(합격선 1%p). 남은 차이는 Apple silicon의 Homebrew OpenCV가 KleidiCV로 축소해 x86 OpenCV와 ±1 다르기 때문. 타일 모드는 원본에서 라벨 얼굴 재현율 82.3% → 87.1%, 라벨 밖 실제 얼굴 29 → 53개, 대신 확대한 질감의 오탐이 사진당 약 0.03 → 0.33개, 시간은 사진당 CPU +0.59초·CoreML +0.46초. 측면·각도·라벨 누락은 crop 시트로 손으로 태깅. CI `benchmark.yml`이 감지 관련 PR에서 1.12.1 기록 5가지 설정과 기본 설정의 재현율이 1%p 넘게 떨어지면 실패. 결정(2026-10-20): 라벨 세트는 Open Images V7, 이미지는 S3 사본 + 원본 보조 세트(Flickr가 원본 100–280개마다 429로 1시간 넘게 막음), 측면·각도는 손 태깅, 벤치마크는 도구 + 기록 + CI, YOLO5Face-n 임계값 0.5 유지(0.4·0.3은 재현율 +0.3–1.0%p에 오탐 2–3배), 타일 기본 켬 유지(타일 오탐 줄이기는 후속 과제). PR #38에서 3개 OS CI와 벤치마크 CI(Linux 재현율이 이 Mac의 기록과 같음)가 통과해 main `d4d8ed7` fast-forward(2026-10-21, #37과 함께)
- **완료 기준 확인(2026-10-20):**

    | 항목 | 확인하는 시험(`crates/cloakframe-engine` 기준, core는 표시) |
    | --- | --- |
    | 1.12.1 대비 허용오차 내 결과 | `detect_fixtures.rs` 세 모델 54건(CPU와 실행이 정하는 위치, 최소 IoU 0.99999), `the_plate_graph_limits_are_lifted`, CI `ort.yml`(고정 ORT를 3개 OS에서 비교) |
    | 소규모 벤치마크 재현율 | [벤치마크 기록](M3_BENCHMARK.md), CI `benchmark.yml`(`tools/benchmark/evaluate.py --gate`) |
    | GPU 없을 때 CPU로 동작 | `detect::fallback` 단위 4건, `detect_fixtures.rs` `an_image_that_fails_on_coreml_is_analyzed_again_on_the_cpu`, `photo_pipeline.rs` `a_photo_analyzed_again_on_the_cpu_says_so`, `cli_photo.rs` `the_models_run_where_the_gpu_setting_says`, 가속기 없는 Linux CI에서 실제 모델 시험 전부 |
    | Windows 실기기 GPU 강제 리셋 | 장치 없음. M7·M8 확인으로 옮김(M0에서 옮긴 확인과 같음) |
    | D5 | `d5_model_folder.rs` 2건(umask 002, 다른 계정이 쓸 수 있는 폴더), `model_files` 단위(해시 확인 뒤에만 이름 붙임, 바뀐 파일 교체), `cli_photo.rs` `only_the_pinned_models_run`(바뀐 모델은 로드에서 거부) |
    | D7 | `d7_sessions.rs` 2건(실행 전체에 모델·위치마다 세션 하나, 쓰는 모델만 읽음), `cli_models.rs` `nothing_is_loaded_before_the_scan_finds_media`·`the_plate_model_is_read_only_when_plates_are_on` |
    | D8 | 타입: `ort::Session::run`이 `&mut self`를 받고 `Detector::detect(&mut self)`도 같으며, 감지기는 `DetectorHandle::spawn`이 추론 스레드로 옮겨 그 스레드만 가짐. 시험: `detect` 단위 `serializes_calls_from_many_threads`(동시 호출이면 실패), `detector_errors_reach_the_caller_and_the_thread_keeps_serving`, `a_panicking_detector_makes_the_handle_unavailable` |
    | D11 | `detect_fixtures.rs` crowd 3건(280개 모두), `the_plate_graph_limits_are_lifted`, `p1_photo.rs` 2건, `yunet` 단위 `nms_considers_every_candidate`, `photo_pipeline.rs` `capped_detections_hold_the_photo_until_it_is_exported_with_warnings`, `detect::models` `combined_results_keep_every_kind_and_each_models_limit`, `detect::tiles` `dropped_counts_and_the_cpu_rerun_carry_over` |
- **완료(2026-10-20):** 소요 2일(2026-10-19–20, 예산 3주), PR #33–#37과 6단계 `m3-verify`. 위 표의 시험이 macOS arm64에서 통과하고 3개 OS CI는 각 단계 PR에서 확인. 남은 확인: Windows DML 결과·번호판 0건 입력·실기기 GPU 리셋(GPU 있는 Windows 장치가 생기면, M7·M8). 이월했던 타일 오탐 줄이기는 2026-10-20 완료([기록](M3_TILE_FALSE_DETECTIONS.md)): 성긴 보기에서 40 px 이상인데 거기서 찾지 못한 타일 감지는 임계값 + 0.2 이상일 때만 남김. 원본 세트 재현율 87.1% → 87.2%, 오탐 68 → 47개, S3 세트 88.0% → 87.9%, 46 → 33개

### M4 — 영상 시간축·인코딩 (2026-10-22 완료)

- **작업:** `TimelineSpec`, 디코드·인코드 프로세스 관리, 색 태그, pad, 오디오 정책, 하드웨어→소프트웨어 재시도, 출력 검증, 고정 수동 마스크로 끝까지 내보내기
- **완료 기준:** 프레임 번호와 오디오 임펄스를 넣은 합성 영상에서 frame 일치·A/V 동기 확인. D1·P-3·P-4·P-5 시험 통과
- **단계(2026-10-21 계획):** 단계마다 main에서 브랜치·PR 하나.
  0. 외부 프로세스 감독(`m4-process`): FFmpeg·ffprobe 판을 다시 고정하고 CI가 해시로 받음, 자식 프로세스 감독(취소하면 프로세스 트리 종료, stderr는 끝부분만 보관, 입력은 `file`·`pipe` 프로토콜만), M1에서 넘어온 CLI Ctrl+C 취소. **구현(2026-10-21, [기록](M4_PROCESS.md)):** 세 OS FFmpeg 9.0.2(Windows essentials), engine `media::process`(`ToolCommand`·`Supervised`: Unix 프로세스 그룹·Windows Job Object, 감시 스레드가 취소 토큰을 보고 트리를 죽임, 회수와 종료를 같은 잠금 아래에), `run::start_under`, CLI `ctrlc`(첫 신호 취소·두 번째 즉시 종료, 종료 코드 130). CI가 고정 FFmpeg를 받아 시험. **완료(2026-10-21, PR #41, main `dae40e5`):** 첫 CI에서 찾은 두 결함을 고침(회수 전에 끝난 도구에 닿은 kill을 취소로 보고하던 것, Windows 트리 시험의 손자가 같은 stdout에 먼저 쓰던 것). 3개 OS CI·ORT 비교·벤치마크 통과. 앞서 M3 후속 타일 오탐 줄이기를 PR #40으로 main `14f0013`에 넣음
  1. 조사(`m4-probe`): ffprobe 결과로 `TimelineSpec`(크기·회전·SAR·시간 기준·frame 수·색 matrix·range·primaries·transfer·오디오 stream). 10비트·HDR은 이유와 함께 거부(M9 전까지), 입력 상한(4.7). frame 색인과 PTS 정규화 규칙을 한 곳에 둠(P-3). **구현(2026-10-21, [기록](M4_PROBE.md)):** core `timeline::TimelineSpec`(정확한 유리수 시작·`frame_start(n)`, 회전·SAR, D1 `ColorSpec`), engine `media::probe`(스트림 선택·내용으로 컨테이너 판정·8비트 형식 목록·HDR 거부·상한). `fuzz/`의 두 대상(원시 보고, 뽑은 필드값)과 CI `fuzz.yml`. 구조화 fuzz가 회전값 overflow panic을 찾아 고침. **완료(2026-10-21, PR #42, main `c1b7772`):** 3개 OS CI와 fuzz workflow(Linux 원시 2,752만 번·구조화 160만 번) 통과
  2. 디코드(`m4-decode`): 원시 프레임 reader와 같은 규칙의 frame 조회(미리보기). P-3 fixture(CFR 오프셋·B-frame·23.976·VFR·회전·오디오 선행), 디코드 쪽 D1(matrix·range 명시). **완료(2026-10-21, PR #43, main `c70f2f6`, [기록](M4_DECODE.md)):** core `SourceFrames`(패킷 목록의 원본 pts, CFR 번호 n은 칸 가운데 시각에 화면에 있던 원본 프레임), engine `media::frames`(ffprobe 패킷 목록을 흘려 읽음, 버림 패킷 제외), `media::decode`(`FrameReader`: 필터 없이 원본 프레임을 rgb24로, 디코드 수가 목록과 다르면 실패; `source_frame`: 정확한 pts의 미리보기). P-3 다섯 fixture에서 순서·미리보기 바이트 일치, D1 디코드 색, CPU와 무관한 축소 해시(x86 Linux·Windows CI에서도 같음), 1080p 파이프 436 fps(macOS). CI가 3개 OS에서 디코드 파이프 처리량을 출력(Windows 125 fps·775 MB/s, Linux 170 fps, macOS runner 199 fps). 3개 OS CI·fuzz workflow 통과
  3. 인코드(`m4-encode`): writer, 하드웨어 인코더 우선·실패 시 소프트웨어로 2패스만 재시도, D1 태그와 같은 matrix, P-5 홀수 크기 pad와 오디오 정확한 trim(오디오 정책은 1.12.1 유지). 재검토에서 넘어온 것: 컨테이너 시작이 0이 아닌 파일의 오디오 `-ss`(FFmpeg가 컨테이너 시작을 더함, 1.12.1도 놓침, 시험 입력이 없음), 인코드 쪽 파이프 처리량 CI 측정(Windows), 회전한 프레임의 SAR 역수, `gbr` 원본의 출력 matrix, 읽지 않는 stdout 파이프로 `wait`가 멈추지 않게, 두 글자 언어 코드. **구현(2026-10-22, [기록](M4_ENCODE.md)):** engine `media::encode`(`FrameWriter`: rgb24를 stdin으로, stdout은 파이프 아님, 출력 파일은 `create_new`로 먼저 만듦; `Encoders`: 이 항목의 필터 그대로 3프레임 시험 인코드, 하드웨어 실패 뒤 실행 나머지는 소프트웨어), `media::language`(두 글자 → 세 글자), core `TimelineSpec::output_matrix`. 색 태그는 `setparams`로 프레임에 붙임(인코더 옵션만이면 primaries·transfer가 빠짐), 홀수 크기는 Rust가 가장자리 복제, 오디오는 `-copyts -itsoffset`으로 영상 시작을 0으로 옮기고 0.25초보다 앞선 패킷은 버림·끝은 `setts`(입력 `-ss`는 패킷 경계에서 AAC 앞 패킷을 버려 첫 프레임 소리를 잃음, 컨테이너 시작 문제도 함께 없어짐). FFmpeg 9가 이미 있는 출력에 종료 코드 0을 내는 것을 찾아 막음. CI가 인코드 파이프 처리량도 출력. **완료(2026-10-22, PR #45, main `2e35d1f`):** 3개 OS CI·fuzz workflow 통과(Opus WebM·aevalsrc 시험 입력도 Windows·Linux에서 처음 통과). 인코드 파이프 1080p(libx264 `medium`, 같은 프레임): Linux 99 fps·618 MB/s, macOS runner 113 fps·703 MB/s, Windows 84 fps·525 MB/s. NVENC·QSV는 장치가 없어 인자만 확인
  4. 영상 처리기(`m4-video`): 원본 identity 비교(1패스 전·패스 사이·게시 직전), P-4 출력 검사(stream·duration·frame 수·metadata·전체 decode), 고정 수동 마스크로 CLI 끝까지 내보내기. **구현(2026-10-22, [기록](M4_VIDEO.md)):** engine `video::process`(`VideoProcessor`: 분석은 identity → `source::open` → identity, 2패스는 원본 해상도 디코드 → 프레임마다 한 번 core 렌더러로 마스크 → `FrameWriter`가 출력 폴더 안 임시 파일에 → P-4 → identity, 게시 직전 identity), `media::verify`(ffprobe로 stream·크기·D1 태그·길이·태그 allowlist, `-xerror` 전체 decode의 프레임 수), `media_processor`(사진·영상을 한 실행에), `Note::EncodedAgainInSoftware`·`Note::Padded`, CLI `run --videos --ffmpeg --ffprobe --mask X,Y,W,H [--video-codec] [--video-quality] [--hardware-encode]`. 하드웨어 실패 뒤 소프트웨어 재시도는 함수로 시험. 로컬 전체 402개 통과(macOS). **완료(2026-10-22, PR #46, main `8c9aa21`):** 3개 OS CI·탐지 벤치마크 통과(Windows·Linux에서 영상 처리기·P-4·CLI 영상 시험의 첫 실행). 같은 실행의 1080p 파이프: 디코드 Linux 179 fps·macOS runner 236·Windows 101, 인코드(libx264 `medium`) Linux 164·macOS runner 151·Windows 82
  5. 완료 확인(`m4-verify`): 프레임 번호와 오디오 임펄스를 넣은 합성 영상으로 frame 일치·A/V 동기, D1·P-3·P-4·P-5 시험 표, 문서. **구현(2026-10-22, [기록](M4_VERIFY.md)):** engine 시험 `video_export.rs` 11건: 번호를 그린 프레임과 7프레임마다 클릭을 넣은 합성 입력 10개(CFR B-frame, 컨테이너 시작 1.25초, 23.976 20초·44.1 kHz, VFR 30→15·15→30·지터 ±5 ms, 오디오 0.4초 먼저·0.3초 늦게, 회전 90°, WebM VP9·Opus)를 runner로 끝까지 내보내 H.264(하드웨어 우선)·HEVC(libx265)로 확인하고, P-3 fixture 다섯 개의 `analysis_equals_output`. 모든 출력 프레임 n이 분석 pass의 `shown_at(n)`, 오디오는 영상 시작 기준 1 ms 안(측정 0–0.33 ms), 원본 프레임은 반 출력 프레임 안(측정 CFR 0, 지터 5 ms). 15→30 입력에서 출력 속도가 30 격자인데도 29.97로 골라지는 것(최대 10.9 ms 밀림)을 찾아 NTSC 짝 고르기를 고침(아래 결정). mutation 6개를 모두 잡음(오디오 10 ms, 반올림한 속도, 칸 시작 규칙, 컨테이너 시작 기준, 옛 짝 규칙, 프레임 순서를 바꾸는 필터: 이것은 P-4를 통과함). 로컬 전체 415개 통과(macOS). **완료(2026-10-22, PR #47, main `a6d1193`):** 첫 CI에서 fuzz가 1단계부터 있던 시작 시각 overflow를 찾아 영상 시작 2^31초 상한으로 고침([기록](M4_PROBE.md#fuzz)). 3개 OS CI·fuzz workflow 통과(원시 보고 4,757만 번·필드값 317만 번). 같은 실행의 1080p 파이프: 디코드 Linux 95.5 fps·macOS runner 186·Windows 101, 인코드(libx264 `medium`) Linux 77·macOS runner 107·Windows 82(4단계보다 Linux가 낮은데 이 PR은 파이프 경로를 바꾸지 않아 runner 편차로 봄)
- **재검토(2026-10-21, [기록](M4_REVIEW.md)):** 2단계 PR #43의 CI 뒤에 0–2단계와 문서·결정을 다시 봤다. 결함 6건: open GOP를 edit list 없이 자른 파일의 프레임 수(2단계), VFR 출력 속도로 쓴 `r_frame_rate`가 프레임을 버리거나 CFR을 부풀림(1단계 결정), 길이가 적히지 않은 WebM 거부, 색 태그 이름(D1), CLI가 무시된 SIGHUP을 다시 받음, 터미널이 닫히면 검토 세션을 잃음. 위험 6건과 문서 오류. 로컬 `tiles` 시험이 30분 걸린 것은 Mac이 덮개를 닫아 잠든 탓이었다. **수정(2026-10-21, `m4-review`, PR #44):** 결함 6건과 위험 7·8·9번을 항목마다 실패하는 시험을 먼저 넣고 고침: 출력 속도를 패킷 목록으로(`SourceFrames::constant_rate`, `media::source::open`), 시작이 깨끗하지 않은 GOP만 디코드해 목록 보정, 길이 없는 WebM·목록 기준 상한, ffprobe 색 이름·모르는 태그 거부·HD 크기 판정, FFmpeg의 표시 행렬 규칙, CLI가 무시된 신호를 유지·세션을 출력보다 먼저 저장·두 번째 Ctrl+C는 `_exit`, fuzz 불변식과 int 범위 유리수. 커밋마다 따로 fmt·clippy 두 조합과 그 영역 시험이 통과(스크래치 worktree). **완료(2026-10-21, PR #44, main `99a557c`):** 3개 OS CI 통과(Windows에서 `TerminateProcess` 코드의 첫 컴파일, gyan 빌드로 만든 open GOP·길이 없는 WebM·색 태그 시험 입력도 Windows에서 통과), fuzz workflow 통과(원시 2,540만 번·구조화 165만 번). Windows 디코드 파이프는 100 fps·625 MB/s(#43 때 125 fps·775 MB/s, 디코드 경로는 그대로라 runner 편차로 봄)
- **결정(2026-10-21, 재검토):** 수정은 #43을 main에 넣은 뒤 `m4-review`에서 항목마다 실패하는 시험을 먼저 넣고 고침(순서: VFR 속도, open GOP, 길이 없는 WebM과 상한, 색 태그, 회전, CLI 신호). VFR 출력 속도는 `r_frame_rate` 대신 패킷 목록으로 정함: 표준 속도(23.976·24·25·29.97·30·48·50·59.94·60·90·100·120·144·240)와 원본의 공칭·평균 속도 중 모든 원본 프레임이 적어도 한 칸에 나오는 가장 낮은 속도, 29.97과 30처럼 짝이 둘 다 되면 원본의 공칭 속도인 쪽(아니면 평균에 가까운 쪽; 5단계에서 원본 프레임 시각과 더 잘 맞는 쪽으로 바꿈), 240까지 그런 속도가 없으면 가장 많이 보이는 가장 낮은 속도("없으면 240"에서 구현 때 바꿈, [기록](M4_DECODE.md)). 색 태그는 ffprobe가 출력하는 이름으로 읽고 primaries·transfer는 그대로 넘기며, swscale이 디코드할 수 없는 matrix(ycgco·bt2020c 등)는 이유와 함께 거부. 태그 없는 영상의 HD 판정은 너비 1280 이상 또는 높이 576 초과(1단계의 높이 720 기준에서 바꿈, mpv와 같음)
- **완료 기준 확인(2026-10-22):**

    | 항목 | 확인하는 시험(`crates/cloakframe-engine` 기준, core는 표시) |
    | --- | --- |
    | 합성 영상의 frame 일치·A/V 동기 | `video_export.rs` 합성 입력 10건(CFR·VFR·시작 오프셋·회전·WebM, 두 인코더) |
    | D1 | `media_encode.rs` `the_output_keeps_the_sources_colour_description`(태그·중앙 RGB), `media_decode.rs` `frames_decode_with_the_specs_matrix_and_range`, 태그 없는 입력은 core `untagged_fields_follow_the_frame_size`·`media_probe.rs` 3건·`media::probe` 단위 2건, 내보낼 때마다 P-4가 태그를 값으로 비교 |
    | P-3 | `media_decode.rs` `the_preview_of_a_frame_is_the_passs_frame`(`analysis_equals_preview`)·`a_pass_gives_every_source_frame_once_in_order`(`cfr_no_loss`)·`the_variable_rate_output_shows_every_source_frame`(`vfr`), `video_export.rs` `the_output_shows_the_analysed_frame_in_every_frame`(`analysis_equals_output`)와 합성 입력 10건 |
    | P-4 | `media_verify.rs` 4건(정상, 한 프레임 모자람, 끝 4 KiB 자름, metadata), `media::verify` 단위 4건, `video::process` 단위(검사 실패는 항목 실패·하드웨어면 소프트웨어로 한 번 더) |
    | P-5 | `media_encode.rs` `an_odd_size_is_padded_and_the_longer_audio_is_cut`·`audio_that_starts_early_is_cut_at_the_videos_start`, `cli_video.rs`("padded" 표시), `video_export.rs` `sound_that_starts_before_the_video` |

    항목별 대응은 [기록](M4_VERIFY.md#시험-표)에 있다.
- **완료(2026-10-22):** 소요 2일(2026-10-21–22, 예산 4주), PR #41–#47. 위 표의 시험이 macOS arm64에서 통과하고 3개 OS CI는 각 단계 PR에서 확인. 넘어간 것: NVENC·QSV 실제 인코드와 Windows 4K 파이프 상한(M7·M8 실기기), 영상 검토 세션·분석 디코드·감지·ORT spin 측정(M5), Windows 콘솔 창과 Job 시점(M6 GUI 전), 1패스 중 원본 변경 시험과 중간 해상도 변경
- **결정(2026-10-22, 5단계):** 완료 확인은 engine runner(`VideoProcessor`)로 끝까지 내보내고(CLI 연결은 `cli_video.rs`), 입력은 시험이 고정 FFmpeg로 만듦(저장소에 넣지 않음). 입력마다 H.264 하드웨어 우선과 HEVC libx265로 두 번. 허용오차는 프레임 번호 정확히, 오디오 1 ms, 영상 반 출력 프레임. VFR의 NTSC 짝(29.97·30 등)이 둘 다 모든 프레임을 보이면 공칭 속도인 쪽, 아니면 원본 프레임 시작과 출력 칸 시작의 평균 차이가 작은 쪽(재검토 결정의 "평균 속도에 가까운 쪽"을 바꿈: VFR의 평균은 가장 빠른 구간보다 낮아 늘 낮은 쪽으로 기움). PR CI의 fuzz가 찾은 시작 시각 overflow(1단계부터)는 영상 시작 2^31초 상한과 첫 프레임부터 재는 목록 범위로 고침
- **결정(2026-10-22, 4단계):** 영상은 `--review off`일 때만(영상 검토 세션은 M5). 고정 수동 마스크는 `--mask X,Y,W,H`(표시 pixel, 여러 번, 모든 프레임), `--videos`에 필수이고 프레임과 겹치지 않으면 그 항목 실패. 하드웨어 인코드는 `--hardware-encode`로 감지 GPU와 따로 켜고, 하드웨어 항목이 인코드 실패나 P-4 실패로 끝나면 그 항목을 소프트웨어로 다시 하고 실행 나머지도 소프트웨어(디코드 실패·취소는 다시 하지 않음). P-4는 stream 구성·크기·D1 태그·길이(영상 ±1프레임, 오디오는 영상 + 1프레임 이하)·태그 allowlist·`-xerror` 전체 decode의 프레임 수. 영상 2패스는 한 번에 하나(실험: 둘을 함께 해도 2–8%만 빠름). FFmpeg·ffprobe 경로는 CLI 옵션(번들 위치는 M7)
- **결정(2026-10-22, 3단계 구현):** RGB로 부호화된 원본(`gbr`)의 출력 matrix는 태그 없는 원본과 같은 크기 규칙(primaries·transfer·range는 원본 값). 두 글자 ISO 639-1 언어 코드는 ISO 639-2/T 세 글자로 바꿈(MP4 muxer가 두 글자를 버림, 표에 없으면 버림). 오디오 시작은 입력 `-ss` 대신 `-copyts -itsoffset`과 0.25초 앞까지의 패킷(구현에서 바꿈, [기록](M4_ENCODE.md#자르기))
- **결정(2026-10-21, 3단계):** 하드웨어 인코더 후보는 1.12.1과 같음(Windows·Linux NVENC → QSV, macOS VideoToolbox), AMF·VAAPI는 확인할 장치가 생기면. 소프트웨어는 모든 OS libx264·libx265 `medium`(M0가 넘긴 Linux 프리셋 결정), 품질값·기본값(H.264, 고품질 crf 18, 인코더별 매핑)은 1.12.1과 같음. 하드웨어 인코더는 후보마다 3프레임 시험 인코드로 고르고, 실제 인코드가 실패하면 그 항목의 2패스를 소프트웨어로 다시 하고 그 실행의 나머지도 소프트웨어(M3 GPU 규칙과 같음), 결과에 표시. 홀수 크기는 오른쪽·아래 1 px를 가장자리 복제로 채워 짝수로(641×361 → 642×362, 4:2:0의 crop 단위가 2 px) 내보내고 결과에 표시. 오디오는 1.12.1 정책(MP4에 담을 수 있는 코덱은 복사, 그 밖은 AAC 192k) 그대로, 시작은 FFmpeg가 쓰는 edit list, 끝은 `setts`로 마지막 패킷 길이를 줄여 영상 구간에 정확히 맞춤(실험: 복사 + `-t`만이면 AAC 한 프레임까지 남음)
- **결정(2026-10-21, 2단계):** 프레임 대응은 FFmpeg `fps` 필터가 아니라 Rust 규칙: 패킷 목록(디코드 없음)으로 원본 프레임 시각을 한 번 얻고, 분석은 필터 없이 원본 프레임마다 한 번, CFR 번호 ↔ 원본 프레임은 `TimelineSpec` 쪽 규칙 하나, 미리보기는 그 원본 pts를 정확히 골라 꺼냄(실험에서 `fps` 필터 + 탐색은 시작 오프셋 파일과 VFR 반복 구간에서 틀림). 파이프는 rgb24(사진·감지기와 같은 채널 순서). 디코드는 `TimelineSpec`의 matrix·range를 명시해 full-range RGB로, 색역 변환 없음. 분석 해상도 축소는 `area`+`accurate_rnd`+`full_chroma_int`+`bitexact`(OS와 무관하게 같은 pixel). 모든 OS에서 stdout 파이프, Windows 처리량은 CI에서 잼
- **결정(2026-10-21, 1단계):** VFR 입력은 공칭 속도(`r_frame_rate`)의 CFR로 바꿔 원본 프레임을 버리지 않음(없거나 240 fps를 넘으면 평균 속도, 1.12.1은 평균 속도라 빠른 구간 프레임을 버림; 재검토에서 속도를 정하는 방법을 바꿈). 색 태그가 없는 필드는 높이 720 이상 bt709·미만 smpte170m, range tv(D1 fixture 제안; 재검토에서 HD 판정을 바꿈). 영상 스트림은 attached_pic이 아닌 첫 스트림, 더 있으면 버림을 표시. 입력 상한은 1.12.1 값(한 변 16384, 3,600만 pixel, 240 fps, 86,400초, 300만 프레임), 파일 크기 상한 없음(5단계에서 영상 시작 2^31초 상한을 더함: fuzz가 찾은 시각 계산 overflow, [기록](M4_PROBE.md#fuzz)). 컨테이너는 내용으로 판정: mov·mp4 계열, matroska·webm은 VP8·VP9일 때만. ffprobe JSON 파서는 cargo-fuzz(`fuzz/`, 고정 nightly, 별도 workflow로 파서 변경 PR에서 5분), EXIF 대상은 같은 틀에 나중에
- **결정(2026-10-21):** CLI Ctrl+C는 첫 번째에 실행을 취소하고(자식 프로세스 종료·임시 파일 정리, 종료 코드 130) 두 번째에 바로 끝냄. 파이프 형식은 bgr24로 시작하고(2단계에서 rgb24로 바꿈) yuv420p·YUV 마스킹은 5단계 측정에서 필요가 보일 때만. Windows FFmpeg는 gyan essentials(NVENC·QSV·AMF·libx264 포함 확인)

### M5 — 영상 감지·추적·검토 결정 (2026-10-25 완료)

- **작업:** 매 프레임 감지, 레코드 파일, 종류별 양방향 추적, 장면 전환, `CoverageEvidence`·gap, 검토 결정(track 제외·keyframe·gap 해결) 적용, 미리보기 조회 API
- **완료 기준:** 추적 fixture 통과, 1시간 영상에서 RAM이 길이에 비례하지 않음, 미해결 문제가 게시를 막음. P-1(영상)·P-2(추적)·P-6 시험 통과. 범위 조정 점검 시점
- **단계(2026-10-22 계획, [기록](M5_PLAN.md)):** 단계마다 main에서 브랜치·PR 하나.
  0. core 추적(`m5-track`): 장면 전환 판정(1.12.1 `SceneCutDetector`, 확인 대기 중 들어온 새 spike도 평가: P-6 b), 종류별 ByteTrack(high/low 2단계, IoU·크기 gating, 프레임 안 할당은 Hungarian을 core에 직접 구현), 양방향 병합, 후처리(보간·평활화·끝 연장·장면 전환 경계, 보간 한도는 다시 잇는 한도와 같은 1초), 미해결 gap의 `CoverageEvidence`, 추적 창을 유리수 FPS로. 파일·FFmpeg 없이 프레임을 차례로 넣고 끝난 track을 내보내는 API로 만들어 1단계가 레코드 파일을 앞·뒤로 읽으며 그대로 쓴다. 시험: 1.12.1 추적 시험 47개(`tests/fixtures/legacy/tracking_tests`, normative는 값까지·intent는 의도·changed는 P-6과 보간 한도), P-6 a·b, P-2(얼굴·번호판이 겹쳐도 track 둘), 평활화·보간이 box를 줄이지 않는 property. **구현(2026-10-22, [기록](M5_TRACK.md)):** core `tracking`: `scene`(48×27 썸네일, P-6 b), `tracker`(ByteTrack이 프레임마다 할당만 내보냄, track 번호 시작을 받아 종류별로 따로), `assign`(Hungarian), `merge`(뒤 track이 끝나는 프레임에 앞 track과의 평균 IoU로 합침, 1.12.1과 달리 순서에 기대지 않음), `post`(1.12.1 그대로의 전체 track 후처리, P-6 a), `stream`(같은 후처리를 프레임 순서로: 무작위 3,000가지에서 `post`와 같고 6만 프레임에서도 들고 있는 box 수가 2천 프레임과 같음). 기본 보간 창 1초. normative 26개는 기록된 호출을 값까지 다시 돌리고 intent 15개는 assert를 옮김(수동 track은 3단계, 취소는 1단계, 내부 상한은 dropped). mutation 13+9개 중 처음 놓친 3개(greedy 할당, 병합의 box 우선순위, P-6 a)는 시험을 더해 잡음. 로컬 전체 446개 통과(macOS). **완료(2026-10-22, PR #49, main `e9ba8a9`):** 3개 OS CI와 탐지 벤치마크 통과(core `detection.rs`·`geometry.rs`가 바뀌어 돎, 기본 설정 얼굴 재현율 87.9%로 기록과 같음). 같은 실행의 1080p 파이프: 디코드 Linux 107 fps·macOS runner 178·Windows 108, 인코드(libx264 `medium`) Linux 91.5·macOS runner 96·Windows 73(파이프 경로는 그대로라 runner 편차로 봄)
  1. 분석 1패스(`m5-analysis`): 분석 해상도(긴 변 960) 디코드와 감지를 디코드 스레드·bounded 채널로 겹침, 매 프레임 장면 전환 썸네일과 감지(score 하한 0.1, 종류별 임계값, 타일은 사진과 같은 설정), frame 색인이 있는 append-only 레코드 파일(버전 헤더, 앞·뒤로 읽기)을 앱 데이터 폴더의 세션 폴더에, 레코드에서 추적 → 프레임별 영역 파일·track 요약·gap evidence. ORT CPU 세션 spin 끔, CI가 3개 OS에서 감지 처리량(spin 켬·끔)을 출력. M4에서 넘어온 1패스 중 원본 변경 시험과 중간에 해상도가 바뀌는 스트림 조사. 10분·60분 합성 레코드로 추적·영역 읽기의 peak heap이 같음을 할당 시험으로. D7의 영상 경우(사진·영상이 같은 세션). **구현(2026-10-22, [기록](M5_ANALYSIS.md)):** engine `video::record`(머리·프레임 블록·꼬리의 자체 이진 형식, 블록 끝 길이로 뒤에서도 읽고 1,024프레임마다 색인, 감지 25 B·영역 30 B), `video::analysis`(디코드 스레드·4프레임 채널, 썸네일·`detect_views`, 감지를 표시 pixel로), `video::tracking`(뒤 방향 → scratch 레코드, 앞 방향 세 번으로 병합·요약·후처리, 종류별 추적기와 번호를 종류 순으로 이음), 처리기의 1패스 뒤 identity 비교와 track이 있는 영상의 렌더 거부, 레코드는 세션 저장소의 `<세션 id>/item-N`, ORT CPU spin 끔(`with_cpu_spinning`은 측정용). 결정(2026-10-22, 추천대로): gap 근거는 종류별로 합친 구간 + track 목록, 해상도가 바뀌는 스트림은 첫 크기로 늘림(1.12.1과 같음), 2단계 전에는 track이 있는 영상을 렌더하지 않음. 찾은 것: 기본 창(다시 잇기·보간·장면 전환 경계 모두 1초)에서는 미해결 gap이 생길 수 없다(무작위 20,000개·CC 영상 4개에서 0), 그래서 P-1(영상) fixture의 끊김은 실제 추적기로는 새 track이 되고 2단계가 P-1(영상) 확인 방법을 정해야 한다. 할당 시험: 10분 241 KB → 60분 363 KB(track 135 → 810개의 요약만큼). release 1080p YOLO5Face-n CPU 1패스 spin 끔 24.4 fps·켬 8.6 fps(이 Mac). 로컬 전체 462개 통과(macOS). **완료(2026-10-23, PR #50, main `209a261`):** 3개 OS CI와 탐지 벤치마크 통과. CI "Measure the first pass"(debug, 1080p YOLO5Face-n CPU)는 spin 끔·켬이 macOS runner 6.2·7.2, Linux 7.0·6.5, Windows 6.5·5.2 fps로 runner에서는 spin 이득이 보이지 않음(코어 3–4개, debug 전처리가 대부분). release 3개 OS 측정은 5단계(9.3)
  2. 2패스와 게시 게이트(`m5-render`): 영역 파일을 프레임 순서로 읽어 core 렌더러로 마스크(roll 반영, 트랙별 고정 모자이크 격자). P-1(영상): 미해결 gap이 있으면 게시하지 않고 `NeedsReview`, 경고 포함 내보내기·익명화 없이 내보내기. CLI `--videos`가 감지 모델로 동작하고 `--mask`는 선택 사항(모든 프레임의 고정 영역)으로 남김. 결과 JSON의 track·gap에 kind(P-2). `video_export.rs`·`cli_video.rs`는 고정 box를 내는 가짜 감지기 경로로 바꿔 프레임·소리 기준을 그대로 확인. **구현(2026-10-23, [기록](M5_RENDER.md)):** 2패스가 영역 레코드를 원본 프레임 순서로 읽어 그 프레임의 track 영역(roll 포함)과 고정 마스크를 그림(인코드 패스마다 다시 읽음, 프레임 번호가 다르면 손상). core `MosaicGrid`: track마다 칸 크기 = 가장 큰 padding 영역의 짧은 변 ÷ 강도(최소 2 px), 칸 경계는 영상 좌표, 칸 값은 칸 전체 평균. 추적 창 기본값을 2초로(다시 잇기·보간·장면 전환 경계). P-1(영상)은 보간만 1초로 줄인 창(`AnalysisSettings::windows`)으로 fixture 경우 30–64 → gap·`NeedsReview`·출력 없음을, 가짜 감지기의 상한 후보로도 확인. 경고 포함 내보내기는 track·고정 마스크를 그리고 gap은 그대로, 익명화 없이는 아무것도 그리지 않음(저장한 검토의 영상 재개는 3단계라 처리기 수준 시험). CLI `--mask` 선택 사항, 결과 줄에 "tracked N faces", 검토로 남는 영상은 세션 없이 "videos cannot be reviewed yet", 시험 빌드의 `--fixed-detection`. 결정(2026-10-23, 추천대로): 추적 창 2초(기본 1초 창에서는 fixture 경우가 새 track이 되어 29프레임이 경고 없이 드러남, CC 영상 4개에서 2초/1초는 gap 2·2·19·25곳, 2초/2초는 0곳), 영상 모자이크는 영상 좌표 격자(실제 track 흔들림을 옮긴 재구성 실험에서 정지 대상의 복원을 없앰, 움직이는 대상은 어느 격자로도 막지 못함), 결과 JSON의 track·gap kind는 3단계 세션 형식 2로. mutation 12개 모두 잡음, 로컬 전체 475개 통과(macOS). **완료(2026-10-24, PR #51, main `08778f0`):** 3개 OS CI와 탐지 벤치마크 통과(core `mask.rs`가 바뀌어 돎, 기본 설정 얼굴 재현율 87.9%로 기록과 같음). Windows·Linux에서는 이 PR이 2패스 마스크·`--fixed-detection`·실제 모델 영상 시험의 첫 실행이었다. 같은 실행의 1080p 파이프: 디코드 Linux 102 fps·macOS runner 160·Windows 101, 인코드(libx264 `medium`) Linux 93·macOS runner 111·Windows 83. 1패스(debug, spin 끔/켬/끔/타일): macOS runner 7.0/6.0/6.0/1.9, Linux 7.4/6.7/7.6/2.6, Windows 6.6/5.4/5.6/2.1 fps
  3. 검토 세션과 결정(`m5-review`): ReviewKey의 timeline(`TimelineSpec`과 원본 프레임 목록의 해시), 세션 형식 2(영상 항목은 레코드·영역 파일과 track 요약, gap evidence), 영상 편집(track 제외, 수동 track의 keyframe 보간: 1.12.1 `materializeManualVideoTrack`·`manualTrackRectAtFrame` 시험, gap의 "수동 마스크로 덮음"은 gap의 모든 프레임에 수동 track이 있을 때만), CLI `review`로 결정 적용과 세션에서 2패스 재개(원본 identity·timeline 비교), 미리보기 조회 API(프레임 k의 pixel, 그 프레임의 영역과 track, 마스크 미리보기), 끝난 세션의 레코드 정리. **구현(2026-10-24, [기록](M5_REVIEW.md)):** core `review`에 `ManualTrack`(1.12.1 규칙: keyframe 사이 선형 보간, 앞뒤는 끝 keyframe, 프레임 경계로 자르고 남는 것이 없는 프레임이 있으면 track 전체 거부)·`VideoEdits`(`excluded_tracks`·`added_tracks`, gap을 덮는지는 수동 track의 프레임 구간 합집합)·`Edits`. engine 세션 형식 2(`StoredMedia::Photo`·`Video`, 영상은 원본 프레임 수·표시 크기·종류가 있는 track 요약, 레코드는 저장소의 `<세션 id>/item-N`), `ReviewKey`의 timeline(`Source::timeline_digest`)과 영상 입력(파일 전체 SHA-256을 1패스·재개 2패스와 나란히), 처리기 fingerprint를 입력 종류별로, 저장한 분석에서 2패스 재개(stamp → 다시 조사 → timeline digest → 레코드, 제외 track은 그리지 않고 수동 track은 프레임마다 계산, 게시 전 digest 비교), 게시한 영상의 레코드 삭제, `VideoProcessor::preview`(프레임 k, 그 프레임의 영역·수동 box, 2패스와 같은 마스크, 작은 크기는 비례 축소), `SessionStore::lock`·`prune`·`sweep`(세션 잠금 파일, 비정상 종료로 남은 레코드 정리). CLI `run`이 검토로 남는 영상을 세션에 넣고(`LeftOut::Video`·`--review off` 제한 없앰), `review`가 영상 플래그를 받아 재개. 결정(2026-10-24, 추천대로): 영상 입력은 전체 SHA-256(SSD 2.5 GB/s, 1080p 1시간 2–4초), 남은 레코드는 세션 잠금 파일(OS가 kill 뒤에도 풂, macOS 확인)로 가림, 양쪽 track을 제외한 gap도 따로 해결, review는 run과 같은 영상 플래그. 1.12.1 수동 track fixture 호출 6개 재생, mutation 26개 모두 잡음, 로컬 전체 489개 통과(macOS). **완료(2026-10-24, PR #52, main `bd07db1`):** 3개 OS CI와 탐지 벤치마크 통과(기본 설정 얼굴 재현율 87.9%로 기록과 같음). Windows·Linux에서는 이 PR이 세션 잠금(`LockFileEx`·`flock`)과 kill 뒤 정리 시험의 첫 실행이었다. 같은 실행(Actions run 40075046983)의 1080p 파이프: 디코드 Linux 99.5 fps·macOS runner 196·Windows 102, 인코드(libx264 `medium`) Linux 94·macOS runner 150·Windows 82. 1패스(debug, spin 끔/켬/끔/타일): macOS runner 7.0/4.8/5.7/2.5, Linux 7.5/6.5/7.6/2.6, Windows 6.6/5.9/6.6/2.3 fps
  4. 영상 탐지 품질(`m5-benchmark`): CC 클립 몇 개(manifest·해시로 받고 저장소에 넣지 않음)에서 손으로 확인한 기준 track으로 노출 프레임 수(9.2)를 재고 1.12.1(v1.12.1 트리 빌드)과 비교. **구현(2026-10-24, [기록](M5_BENCHMARK.md)):** Commons 영상 세 구간(거리 19초·도로 20초·군중 12.5초, 원본 앞부분을 range로 받아 고정 FFmpeg로 자르고 해시 확인, `video_fetch.py`), 1.12.1은 v1.12.1 트리에 붙인 `legacy_video.cpp`가 `processVideo`의 검토 콜백에서 track을 기록, 엔진은 예제 `video_benchmark`(1패스·추적, 감지 기록을 값 그대로 쓰고 `--from-detections`로 추적만 다시). 기준은 세 실행의 track 상자를 표본 프레임에서 crop 시트로 태깅(얼굴 926·애매 683)하고 점검으로 놓친 얼굴 3개를 더함. 노출 = 기준 상자의 50% 미만이 padding 포함 마스크로 덮임. 얼굴 프레임 27,714 중 노출: 1.12.1 7,791(28.1%), 타일 끔 5,322(19.2%), 타일 켬(기본) 912(3.3%); 노출은 거의 모두 24 px 미만, 1.12.1의 gap 9곳이 새 엔진에선 0. CI `video-benchmark.yml`이 기록된 감지를 다시 추적해 기록 + 얼굴 프레임 1%와 1.12.1에 게이트. 결정(2026-10-24, 추천대로): 기존 세 구간, 표본 프레임 상자 태깅, 50% 미만 덮임·크기별, 추적만 다시 도는 CI 게이트. **완료(2026-10-24, PR #53, main `ed070a6`):** 3개 OS CI, 탐지 벤치마크(기본 설정 얼굴 재현율 87.9%로 기록과 같음), 첫 영상 추적 벤치마크 통과. x86 Linux에서 기록된 감지를 다시 추적한 노출이 이 Mac의 기록과 같았다(5,322·912·1.12.1 7,791). 같은 실행(Actions run 40089853036)의 1080p 파이프: 디코드 Linux 87 fps·macOS runner 219·Windows 102, 인코드 Linux 78·macOS runner 125·Windows 84. 1패스(debug, spin 끔/켬/끔/타일): macOS runner 6.5/6.9/5.7/2.0, Linux 6.8/6.4/6.9/2.4, Windows 6.6/5.8/6.6/2.1 fps
  5. 완료 확인(`m5-verify`): 추적 fixture 표, 1시간 영상의 RAM(실제 클립을 이어 붙인 1시간 영상으로 로컬 전체 실행, CI는 1단계 할당 시험), P-1(영상)·P-2·P-6 시험 표, 분석·2패스 처리량(9.3, 타일 끔으로 1.12.1과 비교하고 타일 켬은 따로), 범위 조정 점검(누적 소요와 예산), 문서. **구현(2026-10-25, [기록](M5_VERIFY.md)):** 1.12.1 추적 시험 47개의 대응 표(normative 28·intent 16·changed 2·dropped 1, index 메모 하나를 M5 결정대로 고침), P-1(영상)·P-2·P-6 시험 표. 처리량은 `tools/benchmark/video_speed.sh`(엔진 CLI와 v1.12.1 트리의 `processVideo` 하네스 `legacy_video_speed.cpp`, 벤치마크 세 클립과 4K 업스케일, GPU·CPU 각 3번)로 쟀다. 첫 측정에서 GPU 기본 설정의 엔진이 1.12.1보다 2–15% 느렸다. 원인은 프레임마다 Rust 전처리(2.5 ms)와 썸네일(0.7 ms)이 장치를 기다리는 CoreML 실행과 직렬인 것이어서, `Detector::preparer`·`detect_prepared`와 `DetectorHandle`의 전처리 스레드로 다음 이미지의 입력을 미리 만들고 영상 1패스가 다음 프레임을 먼저 넣게 고쳤다(감지 결과는 바이트까지 같음). 고친 뒤 1080p는 GPU 6–21%·CPU 19–36% 빠르고, 4K GPU는 P-4 전체 디코드(2.4–2.5초)만큼 6% 느리다(기록만 함). 1시간 영상(Commons 영상 넷을 1080p30으로 맞춰 이음)의 기본 설정 전체 실행에서 1패스 RSS가 길이를 따라 늘었고(60분 94 → 207 MiB), 원인은 `ort` rc.14가 세션 출력마다 `OrtTypeInfo`(272 B)를 놓지 않는 것이었다. rc.15(upstream 수정 `984a2ccd`)로 올린 뒤 CLI RSS 최대는 10분 161 MiB·60분 171 MiB이고 1패스 동안 평탄하다. 첫 60분 실행의 하드웨어 인코드 실패는 동시에 돈 영상 시험과 겹친 때만 났고, P-4와 소프트웨어 재시도가 손상 없는 출력을 냈다. 3개 OS release 1패스 처리량(spin 끔·켬)은 새 `video-throughput.yml`. 범위 조정 점검: 누적 9일, 남은 M6–M8 13주가 남은 예산에 들어가 미루는 항목 없음. **완료(2026-10-25, PR #54, main `1146100`):** CI·벤치마크·영상 벤치마크·ORT·영상 처리량 workflow 통과. 새 `video-throughput.yml`의 release 1패스(spin 끔/켬/끔/타일)는 macOS runner 12.1/12.6/9.7/3.6, Linux 17.8/17.3/17.9/6.3, Windows 14.4/15.9/15.7/5.5 fps로 runner에서는 release에서도 spin 차이가 보이지 않는다(코어 3–4개). 같은 실행의 ci.yml debug 1패스는 macOS 10.5/10.7/12.2/4.4, Linux 10.6/7.1/10.5/4.0, Windows 17.1/13.5/17.1/6.5 fps로 전처리 겹침 전(6.5–7.5)보다 높다. 1080p 파이프: 디코드 macOS runner 210 fps·Linux 99.7·Windows 155, 인코드 120·93·130
- **완료 기준 확인(2026-10-25):** 추적 fixture 47개, 1시간 영상의 RAM(CI 할당 시험과 실제 60분 전체 실행), 미해결 문제의 게시 차단과 P-1(영상)·P-2·P-6은 [기록](M5_VERIFY.md#m5-완료-기준)의 표에 있다. 처리량(9.3)은 1080p에서 1.12.1보다 빠르고 4K는 P-4 비용만큼 느리다
- **완료(2026-10-25):** 소요 4일(2026-10-22–25, 예산 5주), PR #48–#53과 5단계 PR. 넘어간 것: 9.3의 고정 장치(Windows DML·Linux x64) p50/p95와 DML의 rc.15 재확인(M8), 하드웨어 인코드 실패 이유의 표시(M6), 저조도·측면·번호판 영상 기준(M5_BENCHMARK 남은 것), 미리보기 연속 재생(M6, 범위 조정 순서 ①)
- **재검토(2026-10-25, [기록](M0_M5_REVIEW.md)):** M5 완료 뒤 M0–M5 전체를 다시 봤다. 재현한 결함 6건: 게시 실패 뒤 재시도 결정에 남는 "수동 마스크로 덮음"(P-1), 영상보다 먼저 시작해 다시 인코드하는 오디오의 끝 잘림(P-5, P-4 통과), zsh에서 터미널을 닫으면 SIGHUP 두 번으로 세션을 잃음, 점점 어두워지는 여러 프레임 섬광이 장면 전환 여러 개(P-6 b), 1 fps 미만 NTSC형 속도의 panic, 종료 코드 1이 3에 덮임. 그 밖에 위험 3건(영상 타일의 성긴 보기 확인, 결정 파일의 번호 결속, 움직이는 대상의 다시 잇기), 시험 공백 1건(`detect_prepared` 폴백), 드문 경우와 문서 오류. 결정(추천대로): 결함 1–6·시험 공백·문서를 `m0-m5-fixes` 한 브랜치에서 노출 위험 순으로 항목마다 실패하는 시험을 먼저 넣고 고침, 나머지 위험은 M6에서 다시 봄, 9.3에 4K의 P-4 비용 예외, turbojpeg은 upstream #43을 따름. **완료(2026-10-25, PR #55, main `a8b13ad`):** 커밋 13개(기록 2·항목 7·문서 1·기록 1·시험 1·디코드 1). 첫 CI의 macOS에서 `cancelling_stops_a_pass`가 kill 전에 끝까지 읽혀 실패해(감시 스레드 50 ms 폴링, 전부터 있던 경쟁) `FrameReader`가 프레임마다 취소를 보게 고친 뒤 CI 10개 통과
- **결정(2026-10-25, 5단계):** 1시간 영상은 Commons 영상 넷을 1080p30으로 맞춰 이어 붙여 기본 설정으로 로컬 전체 실행, 처리량은 벤치마크 세 클립과 4K 업스케일을 두 구현 모두 GPU·CPU로(엔진 타일 끔, 타일 켬은 따로), 1.12.1은 v1.12.1 트리 하네스, 3개 OS spin release 측정은 새 workflow. 측정 뒤: GPU 처리량은 이 단계에서 전처리를 세션 실행과 겹쳐 고침, 4K의 P-4 비용은 기록만, `ort`는 출력 누수 때문에 공개 2주 미만인 rc.15로 올림(2주 규칙의 예외, `api-27` 제약은 그대로)
- **결정(2026-10-22, [기록](M5_PLAN.md)):** 영상의 보간 한도는 추적기가 track을 다시 잇는 한도와 같은 1초(2단계에서 다시 잇기·보간·장면 전환 경계 모두 2초로 바꿈, 2026-10-23 결정; 1.12.1은 다시 잇기 1초·보간 2/3초라 그 사이 길이의 구멍이 모두 미해결 gap이 됨: CC 영상 4개의 gap 94곳이 모두 0.68–0.98초였고 1초로 맞추면 0곳. P-1 fixture의 영상 gap은 1초를 넘게 늘림). 영상 분석도 사진과 같은 타일 설정(기본 켬, 군중에서 강한 track 841 → 1,391개, 분석 약 3배 느림). ORT CPU 세션은 사진·영상 모두 spin을 끔(결과는 같고 이 계획 실험에서 영상 감지 1.6–2배·사진 실행 약 40% 빠름, 스레드 수는 그대로; 1단계 release 측정은 24.4 대 8.6 fps로 2.8배). 영상 탐지 품질 측정은 M5 4단계. 함께 정한 것: 감지 하한 0.1과 track 시작 임계값 `min(t, max(0.35, t − 0.1))`을 종류별로, "낮은 신뢰" track은 1.12.1처럼 가리고 표시만(게시를 막지 않음), 다른 track으로 다시 시작한 끊김은 1.12.1처럼 gap으로 보지 않음

### M6 — desktop 앱

- **작업:** 6장 UX 전체, 사진·영상 검토 화면, 설정, 3개 언어·테마, 정보 창
- **완료 기준:** 모든 흐름을 UI로 수행, `egui_kittest` 상호작용 시험, macOS 실제 OS의 IME·파일 대화상자·스크린리더·드롭·메뉴 단축키 점검([M0 egui 절차와 공개 이슈 목록](M0_EGUI.md)), Windows·Linux는 CI runner에서 앱 실행·`egui_kittest`·GPU 없는 환경의 렌더러 대체 경로·창 스크린샷. Windows Narrator·실제 IME·원격 데스크톱은 M7 Windows 10 VM과 M8 실기기로 옮김(2026-10-25, [Windows·Linux 절차](M0_PLATFORM_RUNBOOK.md)). macOS 확인 뒤 preview 릴리스 가능
- **단계(2026-10-25 계획, [기록](M6_PLAN.md)):** 단계마다 main에서 브랜치·PR 하나.
  0. 앱 골격·메인 화면(`m6-app`): 새 crate `cloakframe-app`, egui 0.37 재측정, 글꼴, fluent, 테마, `settings.toml`, 메뉴, 정보 창, 창 위치, 드롭·썸네일·무엇을/어떻게·저장 위치, engine 작업 스레드와 이벤트, `egui_kittest` 틀
  1. 처리·결과·세션(`m6-run`): 파일별 상태, 진행률·예상 시간, 취소, 로그 패널, 문장 요약, 앱 재시작 뒤 검토 세션 다시 열기(범위 조정 ②)
  2. 사진 검토(`m6-photo-review`): 검토 큐, 타일 텍스처 확대, 토글·추가·키보드·undo/redo, Space 미리보기, `?` 안내, 해결 표시와 두 내보내기, 결정을 `ReviewKey`까지 묶음([재검토](M0_M5_REVIEW.md) 9번)
  3. 영상 검토(`m6-video-review`): 타임라인·track 레인·gap, 프레임 이동·미리보기, 저해상도 연속 재생(범위 조정 ①), keyframe, gap 결정, 미리보기 전 `VideoEdits::check`(재검토 11번)
  4. 설정·미리보기·번역·접근성(`m6-settings`): 고급 설정, 샘플 미리보기, 하드웨어 인코드 실패 이유 표시(M5에서 넘어옴), `.ts` 이전 완료, 접근성 이름·역할·포커스
  5. 완료 확인(`m6-verify`): 흐름별 시험 표, macOS 실제 OS 점검, Windows·Linux CI 실행·스크린샷, 범위 조정 점검
- **결정(2026-10-25, [기록](M6_PLAN.md)):** 위 단계, 범위 조정 ①②는 미루지 않음, 재검토 7·10번은 M8 품질 비교 전 별도 브랜치. egui·egui_kittest·eframe은 0.37.x(0.37.0이 10월 29일에 공개 2주, 0단계는 그 뒤 시작하고 M0 자동 시험을 다시 돌림), muda·rfd도 그때 2주 지난 최신 판. 글꼴은 JIS X 0213 전체와 KS X 1001 한자를 번들, 메뉴는 muda "custom"(Linux는 egui 메뉴 막대), 번역은 영어 `.ftl` 기준으로 1.12.1 `.ts`에서 쓰는 문장만 옮김(새 일본어는 기계 초안 + 검토 필요 표시), 설정은 앱 데이터 폴더의 `settings.toml`, 렌더러는 wgpu 기본·실패 시 glow. **완료(2026-10-25, PR #56, main `87e0e22`):** CI 4개(형식·린트·라이선스, 3개 OS 시험) 통과

### M7 — 배포·업데이트·이전

- **작업:** 세 OS 패키지(Velopack 설치본·AppImage, DMG), 서명·공증, 기존 feed로 업데이트, 키 필수화와 서명 feed, 네이티브 manifest·SBOM·라이선스·대응 소스, CloakFrame 설정(QSettings 읽기 어댑터 → TOML) 이전(모델 폴더와 CoreML 캐시는 M3 4단계에서 넘어옴), 모델 미러(4.6)
- **완료 기준:** 깨끗한 최소 지원 OS(Windows 10 1903 VM 포함, M0에서 옮김)에서 설치·오프라인 실행·업데이트·제거, 1.12.1 → Rust 업데이트 경로 확인, 이전 실패 시 기존 데이터 불변. P-7 통과

### M8 — 검증과 stable 전환

- **작업:** 1.12.1 대비 품질·성능 비교, 장시간 안정성, 번역·접근성 점검, `CHANGES_FROM_1.12.md` 정리, 릴리스 문서
- **완료 기준:** 12장 체크리스트 전부 충족. Windows는 NVIDIA·Intel·AMD 각각에서 DML 결과와 프로세스 소켓 감시를 포함(M0에서 옮김)

### M9 — HEIC·HDR 입력 (stable 직후)

- **작업:** HEIC/HEIF 디코드(플랫폼 디코더와 libheif 중 spike로 결정, 출력은 JPEG 또는 HEIC), 10비트·HDR 영상 디코드 → SDR 톤매핑 → 8비트 출력(변환 사실을 결과에 표시)
- **완료 기준:** 아이폰 기본 설정으로 촬영한 사진·영상 fixture 처리

**범위 조정 순서**(2026-10-16 확인, 예산을 넘으면 앞에서부터 첫 stable 뒤로 미룬다): ① 영상 마스킹 미리보기의 연속 재생(프레임 단위 미리보기는 유지) ② 앱 재시작을 넘는 검토 저장·재개. 2장의 결함 수정과 5장의 유지 항목은 조정 대상이 아니다. 타일 모드는 2026-10-19에 목록에서 뺐다([재검토](M0_M2_REVIEW.md) 14번): 감지 0건 사진은 표시만 하고 게시되므로, 타일 모드 없이는 고해상도 사진의 작은 얼굴을 놓친 결과가 "자동 처리·미검토"로 나간다.

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

사용권을 확인한 소규모 라벨 세트(작은·큰 얼굴, 측면, 군중, 저조도, 번호판 크기·각도별)로 계층별 재현율·정밀도를 잰다. 같은 설정에서 1.12.1 대비 전체 재현율이 1%p 이상 떨어지면 안 된다. 타일 모드는 추가 비용 대비 작은 얼굴 재현율 개선을 따로 보고한다. 영상은 라벨이 있는 짧은 클립에서 노출 프레임 수를 잰다. 사진 세트는 M3 6단계에서 Open Images V7(CC BY 2.0 사진, CC BY 4.0 라벨)로 만들었고, CI `benchmark.yml`이 감지 관련 변경마다 이 기준을 확인한다([기록](M3_BENCHMARK.md)).

### 9.3 성능

고정 장치(Windows DML 장치, Apple silicon 16 GB, Linux x64 16 GB)에서 1.12.1 릴리스 패키지와 같은 입력·모델·설정으로 비교하고 p50/p95를 기록한다.

| 지표 | 목표 |
| --- | --- |
| 사진 처리량 | 같은 품질에서 1.12.1보다 10% 넘게 느려지지 않음 |
| 영상 처리량 | 1080p·4K 분석 fps와 최종 배속을 보고, 1.12.1보다 느려지지 않음. 예외: 4K는 P-4 전체 디코드만큼(M5에서 2.4–2.5초, 6%) 느릴 수 있고, M8에서 2패스와 출력 검사를 따로 잰다(2026-10-25, [M0–M5 재검토](M0_M5_REVIEW.md) 12번) |
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
| FFmpeg 파이프 처리량 | M0(macOS 해소), M4 | 파이프 상한 9.5 GB/s, 병목은 인코더. Windows·Linux 하드웨어 인코더 경로는 같은 spike로 확인. 디코드 파이프는 M4 2단계 CI가 3개 OS에서 잼(1080p rgb24, Windows 775 MB/s), 4K 내보내기에서 Windows 파이프 상한은 3단계 CI와 실기기로 확인 |
| TIFF EXIF writer | M0·M2(해소) | 네 형식 모두 성공([기록](M0_EXIF_WRITER.md)). M2 5단계에서 ICC 부착과 함께 exiftool `-validate`·exiv2·ImageMagick으로 다시 확인 |
| 모델 라이선스·배포처 | M3, M7 | 조건 표시(M3 4단계, CLI는 비상업 모델에 동의 플래그), 허용되면 자체 미러(M7), 상업용 대체 모델은 권리와 정확도 평가 후 |
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
- ort: <https://github.com/pykeio/ort>, rc.15: <https://github.com/pykeio/ort/releases/tag/v2.0.0-rc.15>
- ONNX Runtime EP: DirectML <https://onnxruntime.ai/docs/execution-providers/DirectML-ExecutionProvider.html>, CoreML <https://onnxruntime.ai/docs/execution-providers/CoreML-ExecutionProvider.html>
- Windows ML 배포: <https://learn.microsoft.com/en-us/windows/ai/new-windows-ml/distributing-your-app>, EP 카탈로그 비교: <https://learn.microsoft.com/en-us/windows/ai/new-windows-ml/windows-ml-eps-vs-bring-your-own>
- Windows ML 2.4.89 패키지(ORT 1.27.1, 최소 OS)·약관: <https://www.nuget.org/packages/Microsoft.Windows.AI.MachineLearning/2.4.89>, <https://www.nuget.org/packages/Microsoft.Windows.AI.MachineLearning/2.4.89/License>, 릴리스: <https://github.com/microsoft/WindowsML/releases/tag/v2.4.89>
- ort 기능 플래그(`api-17`–`api-31`): <https://docs.rs/crate/ort/2.0.0-rc.15/features>, `with_telemetry`: <https://docs.rs/ort/2.0.0-rc.15/ort/environment/struct.EnvironmentBuilder.html>
- ONNX Runtime 텔레메트리: <https://github.com/microsoft/onnxruntime/blob/main/docs/Privacy.md>
- Apache License 2.0: <https://www.apache.org/licenses/LICENSE-2.0>, cargo-deny: <https://github.com/EmbarkStudios/cargo-deny>
- image: <https://github.com/image-rs/image>, moxcms: <https://docs.rs/moxcms/latest/moxcms/>, kamadak-exif: <https://docs.rs/kamadak-exif/latest/exif/>, fast_image_resize: <https://docs.rs/fast_image_resize/latest/fast_image_resize/>
- FFmpeg: CLI <https://ffmpeg.org/ffmpeg.html>, 프로토콜 <https://ffmpeg.org/ffmpeg-protocols.html>, 라이선스 <https://ffmpeg.org/legal.html>, ffmpeg-next <https://github.com/zmwangx/rust-ffmpeg>
- Velopack Rust: <https://docs.velopack.io/getting-started/rust>, Sparkle: <https://sparkle-project.org/documentation/>
- Tokio·Rayon: <https://tokio.rs/tokio/topics/bridging>, <https://docs.rs/rayon/latest/rayon/>
- ureq: <https://docs.rs/ureq/3.4.2/ureq/>, rustls-platform-verifier: <https://github.com/rustls/rustls-platform-verifier>
- ByteTrack: <https://github.com/FoundationVision/ByteTrack>

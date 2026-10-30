# M7 spike: 가속기(Windows ML 카탈로그, Linux CUDA, 하드웨어 인코더)

작성 2026-10-30, 브랜치 `m7-accelerators`(main `4e387c2` 위). 메인테이너가 Linux CUDA를 정식판 전에 넣고 싶다고 했고, 계획의 오프라인·고정 원칙에 매이지 말고 TensorRT-RTX·OpenVINO까지 시험해 보라고 했다. 이 문서는 측정 기록이고, 무엇을 넣을지는 아래 "판단"을 두고 정한다.

## 장치와 방법

- 이 PC: Windows 11 Pro 26H2(26300), Intel Core Ultra 7 270K Plus(내장 Intel Graphics, NPU), NVIDIA GeForce RTX 5080(드라이버 617.42, compute 12.0).
- Linux: 같은 PC의 WSL2 Ubuntu 26.04, GPU는 WSL의 `libcuda.so`로 RTX 5080. 실제 Linux 설치가 아니다.
- 감지: engine의 `detect_fixtures` 시험에 넣은 `spike_accelerator`(무시되는 시험). 모델마다 실행 위치, 세션 생성 시간(처음·두 번째), astronaut 512×512와 crowd 640×640의 첫 실행과 40회 p50·p90(전처리 포함, release), 1.12.1 fixture 18건 비교(최소 IoU 0.9999, score 차 1e-5, roll 차 1e-4)를 낸다. 실행 위치는 이 브랜치에만 있는 `CLOAKFRAME_SPIKE_EP=<provider>[:<CPU|GPU|NPU>][:<vendor>]`로 바꾼다.
- Windows ML 카탈로그: Windows ML 2.4.89의 C API(`WinMLEpCatalog.h`)를 `engine/src/detect/catalog.rs`에서 부른다. `spikes/accelerators`는 카탈로그 목록·내려받기·ORT 장치 목록을 보는 도구다.
- 인코더: `media_encode`의 `hardware_off_picks_software_and_any_pick_encodes`와, `the_output_keeps_the_sources_colour_description`을 `CLOAKFRAME_SPIKE_ENCODER`로 하드웨어 인코더에 강제해 돌렸다.

## Windows ML 카탈로그

- 패키지 없는(unpackaged) Win32 앱에서 C API가 그대로 동작했다. 이 PC의 카탈로그는 `NvTensorRTRTXExecutionProvider`와 `OpenVINOExecutionProvider` 두 개를 내놓았다(둘 다 certified).
- 처음 `WinMLEpEnsureReady`가 Windows Update로 내려받았다: TensorRT-RTX 2.30.51.0은 63.7초·273 MB, OpenVINO 1.8.95.0은 10.8초·198 MB(`C:\Program Files\WindowsApps`의 MSIX 패키지).
- 설치된 뒤에도 프로세스마다 다시 `EnsureReady`를 불러야 한다(새 프로세스에서 "not ready"). 그 호출은 0.2–2.3초 걸렸다. 오프라인에서도 되는지는 이 원격 세션을 끊을 수 없어 확인하지 않았다.
- 등록 뒤 ORT 장치 목록: CPU, DirectML(Intel GPU, NVIDIA GPU), TensorRT-RTX(NVIDIA), OpenVINO(CPU, GPU, NPU).
- **부작용:** OpenVINO를 등록하면 엔진의 CPU 세션(`AutoDevicePolicy::PreferCPU`)이 ORT CPU 대신 OpenVINO의 CPU 장치를 골랐다(번호판 p50 25 ms → 8 ms). 카탈로그를 넣는다면 CPU 폴백은 ORT CPU EP를 명시해야 한다.

## 감지 결과

p50(ms)은 astronaut / crowd. 세션은 두 번째 생성 시간(첫 생성은 괄호).

| 위치 | YOLO5Face-n | YuNet | 번호판 | fixture | 세션(ms) |
| --- | --- | --- | --- | --- | --- |
| ORT CPU (Windows) | 23.9 / 24.1 | 9.3 / 9.2 | 24.8 / 27.0 | 모두 같음 | 53 / 7 / 51 |
| DirectML, RTX 5080 | 5.5 / 5.8 | 3.5 / 3.9 | 3.7 / 4.5 | 모두 같음 | 418 / 364 / 529 (504 / 367 / 511) |
| DirectML, Intel GPU | 23.6 / 22.3 | 8.5 / 8.8 | 11.1 / 11.7 | 모두 같음 | 299 / 174 / 398 (2413 / 1456 / 1527) |
| TensorRT-RTX | 4.3 / 4.7 | 5.2 / 5.6 | 시작 못 함 | 얼굴 다름 | 1874 / 1932 (2448 / 2135) |
| OpenVINO GPU (Intel) | 16.6 / 17.1 | 4.3 / 4.6 | 시작 못 함 | 얼굴 다름 | 335 / 161 (2715 / 976) |
| OpenVINO NPU | 프로세스가 segfault로 끝남 | | | | |
| OpenVINO CPU | 17.3 / 15.7 | 5.2 / 5.7 | 8.8 / 8.3 | 모두 같음 | 226 / 81 / 332 |
| ORT CPU (WSL, GPU 빌드) | 34.2 / 34.0 | 8.0 / 7.5 | 29.4 / 29.5 | 모두 같음 | 31 / 4 / 26 |
| CUDA 13, TF32 켬(기본) | 7.7 / 8.5 | 7.8 / 7.4 | 13.4 / 16.1 | 모두 다름 | 69 / 25 / 118 |
| CUDA 13, TF32 끔 | 9.1 / 7.3 | 6.9 / 7.4 | 12.8 / 12.6 | 모두 같음 | 92 / 30 / 118 (793 / 24 / 119) |

다른 결과의 크기(1.12.1 대비 최솟값·최댓값):

- TensorRT-RTX: YOLO5Face-n 최소 IoU 0.956, score 차 2.3e-3. YuNet 0.994, 2.3e-3. 개수는 같았다. 번호판은 "Failed to create serialized engine"으로 CPU로 갔다.
- OpenVINO GPU: YOLO5Face-n 0.927, 6.4e-3. YuNet 0.652, 4.5e-2. FP16 추론으로 보인다. `precision=FP32`와 `load_config`의 `INFERENCE_PRECISION_HINT=f32`를 줘도 결과가 바이트까지 같아, 이 경로(plugin EP + `with_devices` 옵션)로는 정밀도가 바뀌지 않았다. 번호판은 FP16 reshape 검사 오류로 CPU로 갔다.
- CUDA, TF32 켬: YOLO5Face-n 0.960, YuNet 0.656, 번호판 0.994. TF32를 끄면(`use_tf32=0`) 세 모델 모두 fixture 안이다.
- DirectML 번호판: 후보가 없는 사진(astronaut, crowd)에서도 오류 없이 0개를 냈다. M3에서 남긴 확인(CoreML이 거부한 빈 텐서)이 DirectML에서는 문제없다.

## Linux CUDA 배포 크기

- ORT 1.30.0의 Linux GPU 빌드는 CUDA 13판(`onnxruntime-linux-x64-gpu_cuda13-1.30.0.tgz`, 236 MB, SHA-256 `382d7913…`)과 CUDA 12판(439 MB)이 있다. CPU판은 11 MB다. CUDA 13은 Turing(compute 7.5) 미만 GPU를 지원하지 않는다고 NVIDIA가 밝혔다(이 PC에서는 확인할 수 없음).
- 풀었을 때: `libonnxruntime.so` 32 MB, `libonnxruntime_providers_cuda.so` 260 MB. GPU 빌드의 `libonnxruntime.so`는 CUDA 라이브러리 없이 CPU로 돈다(WSL의 CPU 기준 측정이 그 빌드다).
- CUDA provider가 링크하는 것: `libcudart.so.13`, `libcublas.so.13`(55 MB), `libcublasLt.so.13`(521 MB), `libcurand.so.10`(121 MB). 여기에 cuDNN 9(pip `nvidia-cudnn-cu13` 9.27, 884 MB)를 실행 중에 연다. NVIDIA의 pip wheel로 받은 크기다. cuDNN의 어느 부분만 필요한지는 확인하지 않았다. 합치면 풀었을 때 약 1.8 GB다.
- NVIDIA 라이브러리를 앱과 함께 재배포해도 되는지(CUDA·cuDNN 라이선스)는 확인하지 않았다.

## 하드웨어 인코더

- 엔진의 고르기(`Encoders::pick`)가 이 PC에서 `h264_nvenc`·`hevc_nvenc`를 골랐고 그 출력이 정상이었다.
- NVENC(H.264·HEVC)와 QSV(Intel Graphics, H.264·HEVC) 모두 `D1` 세 영상(bt709 tv·pc 1080p, bt601 tv 480p)에서 색 태그 네 개가 원본과 같고 중앙 화소가 3 이내였다. M4에서 장치가 없어 남긴 확인이다.
- AMF(AMD)와 VAAPI는 장치가 없어 시험하지 않았다.

## 판단

- **Windows 카탈로그(TensorRT-RTX, OpenVINO GPU·NPU)는 넣지 않는다.** 이 앱의 작은 모델에서 DirectML보다 빠르지 않거나(YuNet은 느림), 1.12.1 결과와 달라지고(정밀도를 고칠 방법을 찾지 못함), 번호판 모델을 돌리지 못하며, NPU는 앱을 죽인다. 세션 생성도 1–2초 늘어난다. 오프라인 원칙 때문이 아니라 측정 결과 때문이다. DirectML은 NVIDIA와 Intel GPU 모두에서 결과가 같고 빠르다.
- **OpenVINO CPU는 보류.** fixture가 같고 ORT CPU보다 1.4–3배 빠르지만, 쓸모 있는 GPU가 없는 Intel PC에서만 의미가 있고 Windows Update로 198 MB를 받아야 한다. GPU 없는 PC의 CPU 속도가 문제로 확인되면 다시 본다.
- **Linux CUDA는 넣을 만하다(TF32 끔).** 결과가 같고 YOLO5Face-n 3.7–4.6배, 번호판 2.3배 빠르다(YuNet은 같음). 문제는 약 1.8 GB의 NVIDIA 라이브러리다. 선택지:
  1. **켤 때 받는 GPU 묶음(추천).** AppImage·설치본은 지금처럼 작게 두고(ORT를 GPU 빌드의 `libonnxruntime.so`로 바꿀지만 정함), 사용자가 켜면 ORT CUDA provider와 NVIDIA 라이브러리를 모델처럼 고정 URL·SHA-256으로 받는다. NVIDIA 라이선스 확인이 먼저다.
  2. 시스템의 CUDA 13·cuDNN 9를 쓰고 ORT CUDA provider(260 MB)만 넣는다. 사용자가 CUDA를 직접 설치해야 한다.
  3. 넣지 않는다.
- **인코더는 지금대로.** NVENC·QSV를 실기기에서 확인했다.

## 결정과 구현: Linux GPU 묶음(2026-10-30)

메인테이너가 "켤 때 받는 GPU 묶음"을 골랐다.

- **받는 것**(`engine/src/gpu_pack.rs`, `manifests/native.toml`의 `gpu-pack-*`): ORT 1.30.0 CUDA 13 tgz(GitHub)와 NVIDIA가 PyPI에 올린 wheel 네 개(CUDA runtime 13.4.92, cuBLAS 13.8.2.12, cuRAND 10.4.4.72, cuDNN 9.27.0.42). 압축 파일마다 URL·크기·SHA-256을, 꺼내는 파일 18개마다 경로·크기를 고정했다. 받기 1,217 MB, 설치 1,870 MB. tgz는 `tar`+`flate2`, wheel은 이미 있던 `zip` 8.6으로 이름을 지정해 읽는다(`extract()`를 쓰지 않음, `deny.toml` 근거 갱신). 새 crate는 `tar`·`filetime` 둘이다.
- **설치:** 앱 데이터 폴더의 `gpu/.staging-<id>`에 풀고 모두 맞으면 `gpu/cuda13-ort1.30.0`으로 rename한다. 해시·크기·누락이 틀리거나 취소하면 아무것도 남기지 않는다. 상태는 파일 크기로 본다(Missing·Ready·Damaged).
- **불러오기**(`detect/cuda.rs`, `OrtRuntime::load_gpu_pack`, `startup::load_runtime`): `libcuda.so.1`(드라이버)을 먼저 열고, 묶음의 NVIDIA 라이브러리 14개를 전체 경로로 `dlopen(RTLD_GLOBAL)`한 뒤 묶음의 `libonnxruntime.so`로 ORT를 띄운다. CUDA provider에 RUNPATH가 없고 `LD_LIBRARY_PATH`는 시작 때만 읽히기 때문이다. 묶음이 있으면 GPU 설정과 관계없이 묶음의 ORT를 쓰고(CPU 세션 결과는 CPU 빌드와 같음), 드라이버가 없거나 묶음이 손상되면 "NVIDIA GPU 묶음을 쓰지 않습니다: 이유" 알림과 함께 앱이 담은 CPU 빌드를 쓴다.
- **CUDA 세션:** TF32 끔, CPU EP 폴백 끔(`session.disable_cpu_ep_fallback`). cuDNN 일부가 빠진 묶음으로 시험했을 때 폴백을 켜 두면 세션이 만들어진 채 합성곱만 CPU에서 돌아 CPU 속도가 됐고 아무 표시도 없었다. 끄면 첫 사진이 CUDA에서 실패하고 엔진의 기존 폴백이 CPU로 다시 분석하며 그 사실을 알린다.
- **종료 중 abort 수정:** CLI가 CUDA로 실행한 뒤 "CUDA failure 4: driver shutting down"(`cudaFreeHost`)으로 abort했다(exit 134). 추론 스레드가 detach돼 있어, 마지막 `DetectorHandle`이 사라진 뒤 그 스레드가 세션을 놓는 동안 프로세스가 끝나고 있었다. 이제 마지막 handle이 스레드를 join해 세션을 먼저 놓는다(`detect/mod.rs`). 드라이버를 먼저 여는 것만으로는 고쳐지지 않았다.
- **앱:** 고급 옵션 시트의 "NVIDIA GPU" 구역(Linux x86_64만): 크기, NVIDIA 조건 두 개의 링크와 동의 확인란, 받기·진행·취소, 지우기, 손상 표시, 이미 ORT를 띄운 뒤 설치하면 "다시 시작" 안내. 영어·한국어 문구, 일본어는 `## needs review`.
- **CLI:** `cloakframe-cli gpu-pack [fetch|remove] [--accept-terms] [--gpu-pack DIR]`, `run`은 `--gpu-pack DIR`(기본은 앱 데이터 폴더)의 묶음이 있으면 쓴다.

### 시험

- 단위: engine `gpu_pack` 4개(가짜 tgz·wheel로 설치·손상·재설치·제거, 해시 틀림·취소·파일 누락·크기 틀림에서 아무것도 남지 않음, 실제 목록이 manifest와 같음), `fallback`의 가속기별 위치, app `tests/gpu_pack.rs` 3개(조건 동의 전 받기 꺼짐, 실패 표시, 설치됨·지우기·다시 시작 안내, 손상 표시). Windows에서 워크스페이스 시험 584개 통과(FAT32 제외), Windows·WSL 모두 clippy 경고 없음.
- WSL2(RTX 5080)에서 CLI `gpu-pack fetch --accept-terms`로 실제로 받기: 32초, 1,217 MB, 설치 1.9 GB. 그 묶음으로 `detect_fixtures` 전부 통과(세 모델 모두 CUDA, 폴백 없음), CLI 사진 실행 세 번 모두 CUDA로 게시하고 exit 0. 드라이버를 숨긴 mount namespace에서는 알림 뒤 CPU로 게시하고 exit 0.

### WSL2에서 다시 해 보기

WSL의 Ubuntu는 Windows의 NVIDIA 드라이버를 `/usr/lib/wsl/lib/libcuda.so.1`로 본다. 빌드 도구(`build-essential cmake nasm pkg-config`)와 rustup이 있으면 저장소를 `/mnt/c/...`에서 그대로 빌드할 수 있다(`CARGO_TARGET_DIR`는 WSL 홈 아래로).

```bash
export ORT_DISABLE_TELEMETRY=1 CLOAKFRAME_TEST_ASSETS=/mnt/c/Users/<user>/.cache/cloakframe-test-assets
export CLOAKFRAME_ORT_LIBRARY=<CPU 빌드의 libonnxruntime.so.1.30.0>
cargo run --release -p cloakframe-engine --bin cloakframe-cli -- gpu-pack fetch --accept-terms --gpu-pack ~/gpu
CLOAKFRAME_GPU_PACK=~/gpu/cuda13-ort1.30.0 cargo test --release -p cloakframe-engine --test detect_fixtures -- --include-ignored --nocapture
```

드라이버가 없는 경우는 root로 `unshare -m`을 하고 빈 폴더를 `/usr/lib/wsl/lib`에 bind mount해 재현했다.

## 확인하지 못한 것

- 실제 Linux(WSL 아님)에서 CUDA와 앱 화면, 그리고 Turing 이전 GPU(CUDA 13 미지원)에서의 동작. 그런 GPU에서는 세션이 실패해 CPU로 갈 것으로 보지만 확인하지 않았다.
- NVIDIA 조건을 앱에서 보여 주고 받게 하는 방식이 NVIDIA 라이선스에 맞는지에 대한 법률 검토. 앱은 파일을 재배포하지 않고 사용자가 NVIDIA가 PyPI에 올린 파일을 직접 받는다.
- 카탈로그 provider의 오프라인 `EnsureReady`, Windows 10과 24H2 미만에서 카탈로그가 없을 때의 동작.
- AMD(MIGraphX·VitisAI, AMF), Qualcomm(QNN).
- TensorRT-RTX에 TF32 같은 정밀도 옵션이 있는지.

## spike 코드

측정에 쓴 `CLOAKFRAME_SPIKE_*` 환경 변수, Windows ML 카탈로그 등록(`detect/catalog.rs`), `Backend::Plugin`은 main에 넣지 않았다. 카탈로그 목록·내려받기 도구는 `spikes/accelerators`에 남겼다. 지연 측정은 `detect_fixtures`의 무시되는 시험 `accelerator_latency`가 런타임의 가속기로 한다.

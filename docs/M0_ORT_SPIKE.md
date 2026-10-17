# M0 spike: ort + ONNX Runtime 모델 3종

실행일: 2026-10-16. 장치: Apple M4, macOS 27.0.1. [Rust 포팅 계획](RUST_PORT_PLAN.md) M0의 "ort/ORT/EP 쌍으로 세 모델 로드·추론" 항목의 첫 결과다. Windows ML DirectML은 [아래 절](#windows-ml--directml), macOS `api-27` 재확인과 텔레메트리 감시는 [그다음 절](#macos-api-27-재확인과-텔레메트리), Linux는 [에뮬레이션 사전 실행](#linux-사전-실행-x86_64-에뮬레이션)과 [GitHub Actions x86_64 실행](#linux-x86_64-실행-github-actions)에 있다. spike 코드는 [`spikes/ort`](../spikes/ort)에 있다.

## 결론

- `ort` 2.0.0-rc.14로 YOLO5Face-n, YuNet, 번호판 YOLOv9-t를 모두 로드·추론했고, CPU에서 **1.12.1 감지기와 같은 결과**를 냈다. 감지 개수, box, score, roll이 출력 정밀도(소수 4–5자리) 안에서 같다.
- YuNet은 OpenCV `FaceDetectorYN` 없이 ORT 원시 출력(`cls/obj/bbox/kps` × stride 8·16·32)을 직접 해석해 같은 결과를 냈다. 계획의 "YuNet raw 출력 해석" 위험은 해소됐다.
- 모델 전처리의 resize는 OpenCV `INTER_LINEAR`를 그대로 옮겨야 한다. 옮긴 구현은 시험한 8개 크기 조합 전부에서 OpenCV 출력과 바이트 단위로 같았다. 계획 3장의 이미지 항목에 반영했다.
- CoreML은 1.12.1과 같은 옵션(`ModelFormat=MLProgram`, `MLComputeUnits=ALL`)을 줘야 한다. 이때 결과가 CPU와 같고, 프로파일상 YOLO5Face-n·번호판 그래프 대부분이 CoreML 파티션에서 실행된다. 기본값(NeuralNetwork 형식)에서는 결과가 조금 달라졌다(box IoU 최소 0.954, 12px 얼굴의 roll 차이 최대 0.22 rad).

## 조건

| 항목 | 값 |
| --- | --- |
| Rust 쪽 | `ort` =2.0.0-rc.14, features `std, load-dynamic, api-30, coreml`, `default-features = false` |
| ORT | Homebrew 1.30.0 dylib을 `ort::init_from`으로 로드(1.12.1은 1.29). 자동 다운로드 미사용 |
| 기준 출력 | 저장소의 1.12.1 감지기 소스(`Yolo5FaceDetector`, `YuNetFaceDetector`, `PlateDetector`, `OrtAcceleration`)를 Homebrew OpenCV 5.0.0(1.12.1 번들과 같은 버전)·ORT 1.30·Qt Core에 링크한 별도 harness |
| 모델 | CI fixture와 같은 URL·SHA-256 |
| 이미지 | scikit-image `astronaut.png`, OpenCV 4.10.0 `samples/data`의 `messi5.jpg`·`lena.jpg`, open-image-models `test/assets/car_image.webp`. JPEG 디코더 차이를 없애려고 모두 OpenCV로 PNG 변환 후 양쪽이 같은 PNG를 읽음 |
| 임계값 | score 0.25·0.5, NMS 0.4 |

## 결과

| 모델 | CPU(Rust vs 1.12.1) | CoreML MLProgram vs 1.12.1 CPU |
| --- | --- | --- |
| YOLO5Face-n | 4장 × 2 임계값 모두 개수 같음, IoU 1.0, score 차 0 | 같음 |
| YuNet(ORT CPU) | 같음(roll 차 ≤ 1e-5) | 해당 없음(1.12.1도 CPU 전용) |
| 번호판 | 같음(차 이미지에서 1건 검출) | 같음 |

지연시간(512×512 입력, 전처리 포함, 50회 p50)은 실행마다 크게 흔들렸다. 예를 들어 Rust CoreML YOLO5Face-n은 실행에 따라 12–23 ms, 번호판은 7.5–15.8 ms였다(MLProgram 기준). 아래는 한 번씩 잰 참고값이며, 성능 판정은 계획 9.3절대로 고정 장치에서 반복 측정해야 한다.

| 모델 | 1.12.1 CPU | Rust CPU | 1.12.1 CoreML | Rust CoreML |
| --- | --- | --- | --- | --- |
| YOLO5Face-n | 47.6 ms | 37.4 ms | 13.4 ms | 11.9–23.3 ms |
| YuNet | 6.8 ms(OpenCV) | 9.5 ms(ORT) | — | — |
| 번호판 | 40.2 ms | 25.5 ms | 7.2 ms | 7.5–15.8 ms |

Rust 전처리(512→640 resize + letterbox + blob)는 아직 SIMD가 없는 단순 루프라 p50 4.1 ms다. 최적화는 M2 범위다.

## 계획에 반영할 것

- 모델 전처리 resize는 core가 소유하는 OpenCV 호환 구현으로 한다(계획 3장 반영).
- CoreML 옵션은 `MLProgram` + `ALL`을 명시하고, compiled-model cache 위치는 앱 캐시 폴더 아래로 지정한다. 1.12.1 harness가 앱 이름 없이 실행되자 `~/Library/Caches/coreml-models`에 cache를 만들었다. Rust에서는 경로를 앱 cache 아래로 명시해야 한다.
- rc.14는 `Session::builder(&Environment)`로 Environment를 명시적으로 넘긴다. 계획의 "번들 절대 경로에서 ORT 로드, 전역 자동 설정에 맡기지 않음"과 맞는다.
- 남은 M0 추론 항목: Linux CPU, 작은 얼굴·번호판 양성 샘플을 늘린 비교, 깨끗한 Windows 10 설치. Windows ML 런타임 라이선스는 Apache-2.0 전환으로 정리됐다(계획 10.1).

## Windows ML · DirectML

실행일: 2026-10-16. 장치: Windows 11 Pro 26300, NVIDIA GeForce RTX 5080(드라이버 617.14). 이 실행에 쓴 Windows spike 코드는 저장소에 없다. 같은 세션 옵션의 DML 경로와 폴백 시험은 이후 [`spikes/ort`](../spikes/ort)에 다시 넣었고(`--dml`), Windows에서의 실행은 [실행 절차](M0_PLATFORM_RUNBOOK.md) 2장으로 확인한다.

### 결론

- **Windows ML NuGet의 ORT는 1.31이 아니라 1.27.1이다.** 최신 안정판 `Microsoft.Windows.AI.MachineLearning` 2.4.89(nupkg SHA-256 `5c68ecfb947223267abf159a023f5192ad42725e4e9cc995e7c1470dd54dff63`)의 `onnxruntime.dll`은 1.27.1, `DirectML.dll`은 1.15.4다. 따라서 Windows는 `ort` feature `api-27`이 상한이다. `api-30`으로 빌드하면 `ort::init_from`이 `BadVersion { version_str: "1.27.1" }`을 돌려주며 멈춘다(크래시 없음).
- `ort` 2.0.0-rc.14(`api-27`, `directml`, `load-dynamic`)가 그 DLL을 절대 경로로 불러 DML EP를 붙였고, 세 모델 모두 **CPU에서 기준과 같은 결과**, **DML에서 개수가 모두 같고 IoU ≥ 0.999997, score 차 ≤ 1.2e-6, roll 차 ≤ 4.2e-6 rad**였다.
- 프로파일상 YOLO5Face-n·YuNet은 모든 노드가 DML에서, 번호판은 end-to-end `NonMaxSuppression` 1개만 CPU에서 실행된다.
- 독립 배포 `Microsoft.ML.OnnxRuntime.DirectML`은 1.24.4(1.12.1이 쓰는 버전)가 마지막이다. DirectML은 두 경로 모두 1.15.4로 같으므로, Windows ML로 옮겨 얻는 것은 새 DML이 아니라 이어지는 ORT 갱신이다.
- 네트워크를 끈 Windows Sandbox(Windows 11 Enterprise 26100, VC++ 재배포 패키지 없음)에서 같은 결과로 CPU·DML 모두 실행됐다. 이 `onnxruntime.dll`은 UCRT(`api-ms-win-crt-*`)·`dxgi`·`d3d12`·`DirectML.dll`만 import하므로 VC++ 런타임이 필요 없다. 반면 기본 설정(`/MD`)의 Rust 실행 파일은 `VCRUNTIME140.dll`이 없어 `0xC0000135`로 시작하지 못했고, `+crt-static`으로 빌드하면 실행됐다. 배포 실행 파일은 정적 CRT로 빌드해야 한다.
- **라이선스가 1.12.1과 다르다.** 1.12.1의 ORT는 MIT이고 DirectML은 1(a)항의 단순 재배포 허용이다. Windows ML 런타임 약관(nupkg의 `license.txt`)은 `onnxruntime.dll`까지 포함한 재배포에 3(b)항 조건을 붙인다: 앱에 주요 기능을 더할 것, 배포자·최종 사용자가 이 약관만큼 Microsoft를 보호하는 조건에 동의하게 할 것, Microsoft를 면책할 것. 이 조건은 GPL 배포와 충돌하므로, CloakFrame은 같은 날 Apache-2.0으로 라이선스를 바꿨다(계획 10.1).

### 조건

| 항목 | 값 |
| --- | --- |
| Rust 쪽 | `ort` =2.0.0-rc.14, features `std, load-dynamic, api-27, directml`, `default-features = false`. 세션 옵션은 1.12.1 `OrtAcceleration`과 같게: CPU는 `All`·논리 코어 수 스레드, DML은 memory pattern 끔·순차 실행·`Level2`(EXTENDED)·스레드 1·device 0 |
| ORT | Windows ML 2.4.89의 `runtimes/win-x64/native` DLL 2개를 실행 파일 옆에 두고 `ort::init_from`으로 로드. 실행 중 모듈 목록에서 두 DLL 모두 그 경로에서 로드됐다. `System32`에는 OS의 ORT 1.17·DirectML 1.15.6이 있으므로 경로 고정이 필요하다 |
| 기준 출력 | Windows에는 1.12.1 빌드 의존성이 남아 있지 않아 C++ harness 대신, 1.12.1 감지기 소스를 줄 단위로 옮긴 Python 구현을 썼다. OpenCV는 1.12.1 번들과 같은 5.0.0(`opencv-python-headless` 5.0.0.93, IPP 사용), 추론은 `onnxruntime` 1.31.0 CPU. YuNet은 1.12.1처럼 OpenCV `FaceDetectorYN`으로 돌렸다 |
| 모델·이미지·임계값 | macOS spike와 같다. 이미지는 OpenCV로 PNG 변환 후 양쪽이 같은 PNG를 읽음 |

### 결과

| 모델 | Rust CPU vs 기준 | Rust DML vs 기준 |
| --- | --- | --- |
| YOLO5Face-n | 4장 × 2 임계값 모두 개수 같음, IoU 1.0, score 차 0 | 개수 같음, IoU ≥ 0.999998, score 차 ≤ 1.2e-6 |
| YuNet(ORT, OpenCV `FaceDetectorYN` 대비) | 개수 같음, IoU ≥ 0.999997, score 차 ≤ 4.5e-7 | 개수 같음, IoU ≥ 0.999997, score 차 ≤ 6e-7 |
| 번호판 | 같음(차 이미지에서 1건), score 차 0 | 같음, score 차 3e-7 |

모델 입력(letterbox canvas와 float blob)은 세 모델 모두 기준과 바이트 단위로 같았다.

지연시간(astronaut 512×512, 50회, 추론만, 전처리 제외)은 참고값이다.

| 모델 | Rust CPU p50 | Rust DML p50 | DML 세션 생성 | DML 첫 실행 |
| --- | --- | --- | --- | --- |
| YOLO5Face-n | 19.7 ms | 2.7 ms | 1.7 s | 63 ms |
| YuNet | 4.0 ms | 1.0 ms | 0.37 s | 7 ms |
| 번호판 | 20.2 ms | 2.4 ms | 0.66 s | 51 ms |

Sandbox의 가상 GPU에서는 YOLO5Face-n DML 세션 생성이 4.3 s, p50이 15.9 ms였다. 장치 성능 판단에는 쓰지 않는다.

### OpenCV `INTER_LINEAR` 이식 규칙

macOS에서 맞춘 구현을 다시 쓰면서 x86에서 처음엔 1 차이가 났다. OpenCV 5.0과 36개 크기 조합(1×1부터 1280×853, IPP 켜고 끈 두 경우)에서 바이트 단위로 같아진 규칙은 다음과 같다. M2의 core resize는 이 규칙을 시험으로 고정한다.

- 계수는 11-bit 고정소수점(`saturate_cast<short>`, 짝수 반올림)이다.
- 가로는 경계에서 `fx = 0`으로 고정하지만, **세로는 경계에서도 소수부 `fy`를 그대로 두고 행 번호만 clip한다.** 그래서 첫 행과 마지막 행은 같은 원본 행을 `(1 - fy, fy)`로 섞는다.
- 세로 합성은 128-bit SIMD 경로의 식 `(((S0 >> 4) * b0 >> 16) + ((S1 >> 4) * b1 >> 16) + 2) >> 2`를 쓴다. 마지막 벡터를 행 끝에 겹쳐 처리하므로 한 행이 8 요소 이상이면 끝부분까지 이 식이다. 8 요소 미만의 행만 스칼라 식 `(S0 * b0 + S1 * b1 + 2^21) >> 22`를 쓴다.
- 가로·세로 모두 정확히 2배 축소이면 OpenCV는 `INTER_AREA`로 바꾼다. 이 경로는 아직 옮기지 않았다.

### 계획에 반영할 것

- Windows의 `ort` API feature는 Windows ML 패키지의 ORT 버전으로 정해진다. 2.4.89 기준 `api-27`.
- Windows 배포 실행 파일은 `+crt-static`으로 빌드한다.
- Windows ML 런타임의 재배포 조건은 Apache-2.0 전환으로 해결했다(계획 10.1). Microsoft 약관 3(b)의 동의 요건은 M7 전에 정한다.
- 남은 Windows 항목: 깨끗한 Windows 10 설치, 작업 중 DML 실패 시 같은 입력을 CPU로 다시 실행하는 폴백, 다른 GPU 공급사(Intel·AMD) 실기기.

## macOS `api-27` 재확인과 텔레메트리

실행일: 2026-10-16(같은 날 오후). 장치: Apple M4, macOS 27.0.1. 코드: [`spikes/ort`](../spikes/ort)(감시 도구는 [`spikes/ort/netwatch`](../spikes/ort/netwatch)).

### 결론

- `ort` 2.0.0-rc.14를 **`api-27`**로 빌드해 공식 ORT 1.30.0(배포 대상)과 Homebrew 1.31.0을 `ort::init_from`으로 불렀다. 세 모델 × [감지 fixture](../tests/fixtures/detect) 6장 × score 0.1·0.25·0.5(54건) 모두 1.12.1과 개수가 같고 IoU ≥ 0.99999, score 차 ≤ 8e-7, roll 차 ≤ 5.1e-6 rad였다. YOLO5Face-n은 CoreML(MLProgram, ALL)에서도 같다.
- **공식 ORT macOS·Linux 빌드는 1.29.0부터 이미 1DS 텔레메트리 SDK를 담고 있다**(계획의 "1.31부터"보다 이르다). 1.29.0·1.30.0·1.31.0 macOS dylib에 `Microsoft::Applications::Events`, `NSURLSession`, collector `https://mobile.events.data.microsoft.com/OneCollector/1.0`, SQLite 이벤트 저장소가 있고 `CFNetwork`·`Network.framework`를 링크한다. Linux 1.29.0·1.30.0 `.so`도 같은 collector와 정적 libcurl을 담는다. Homebrew 빌드에는 없다.
- **끄는 방법은 `ORT_DISABLE_TELEMETRY=1`이다.** `DisableTelemetryEvents`(`with_telemetry(false)`)만 호출하면 전송 시도가 그대로 일어났다. 환경 변수만 설정한 실행, 둘 다 적용한 실행은 시도 0회였다.
- **1.12.1 macOS 패키지는 텔레메트리를 보내는 상태로 배포됐다.** 패키지의 `libonnxruntime.1.dylib`은 공식 1.29.0이고, 1.12.1 코드는 텔레메트리를 켜거나 끄는 호출을 하지 않는다. 그 방식대로 세션을 만들면 90초 동안 collector 이름 조회 5회와 연결 5회를 시도했다. 1.12.1 Linux AppImage의 ORT 1.29.0도 같았다([Linux 사전 실행](#linux-사전-실행-x86_64-에뮬레이션)).
- 텔레메트리가 켜진 실행은 `~/Library/Application Support/Microsoft/DeveloperTools/.onnxruntime/onnxruntime.db`(SQLite)와 `deviceid`에 이벤트를 쌓는다. 이 Mac에는 9월 26일에 만들어진 저장소가 있었고 ORT 1.30.0 이벤트 697건(9/28–10/2, 출처 프로그램은 확인하지 않음)이 남아 있었다. 이벤트에는 기기 모델, OS 버전, 세션 생성 정보(모델 그래프 이름, opset, EP와 할당 노드 수, 하드웨어 ID), 실행 횟수·시간, CPU 시간·메모리, 오류 코드가 들어 있다.
- **`ort`의 세션 빌더는 `api-22` 이상에서 EP 선택 정책 `MaxEfficiency`(NPU 우선)를 기본으로 건다.** Homebrew 1.31은 이 정책 때문에 CPU EP를 명시해도 CoreML 파티션을 만들었다(세션 생성 3.4 s, 아래 번호판 오류). 정책을 `PreferCPU`로 덮고 EP를 명시하면 CPU만 쓴다(56 ms). 공식 1.30.0은 자동 선택을 하지 않았지만 버전마다 달라지므로 엔진은 항상 정책을 덮고 EP를 명시한다.
- **번호판 end2end 모델은 CoreML에서 감지 0건일 때 실패한다.** NMS 뒤의 `Gather`가 CoreML 파티션에 들어가 빈 동적 텐서(`{0}`)를 받으면 `has a dynamic shape … zero elements. This is not supported by the CoreML EP`로 멈춘다(1.30.0·1.31.0, `RequireStaticInputShapes`로도 안 풀림). 번호판은 macOS에서도 CPU로 돌린다(p50 15 ms). 1.12.1에서는 이 오류가 "가속 실패 → CPU 폴백"으로 가려졌을 것으로 본다.
- **`INTER_LINEAR` 이식은 OpenCV 일반 경로와 바이트 일치하지만 KleidiCV HAL과는 ±1 다르다.** Homebrew OpenCV 5.0.0_11(KleidiCV 26.03)과 1.12.1 macOS 패키지의 OpenCV는 KleidiCV HAL을 포함하고, 축소(548×342→512×320, 640→512)에서 바이트의 21%·26%가 ±1 다르다. 확대·같은 크기는 일치한다. 감지 결과에는 영향이 없었다(위 54건). 같은 날 오전 Homebrew OpenCV가 다시 설치되기 전의 출력과는 바이트 일치했고, Windows의 36개 조합도 일반 경로다. 기준은 일반 경로로 두며, 1.12.1 자체가 macOS와 Windows·Linux에서 전처리가 ±1 다르다는 뜻이기도 하다. 이 판정을 위해 이식 코드에 Windows 절의 규칙(짝수 반올림 계수, 8 요소 이상 행 전체 SIMD 식)을 반영했다.

### 감시 방법

| 층 | 방법 | 검증 |
| --- | --- | --- |
| 정적 | dylib 문자열·링크 라이브러리 조사 | — |
| 시도 기록 | `DYLD_INSERT_LIBRARIES`로 `socket`·`connect`·`sendto`·`getaddrinfo`·`DNSServiceGetAddrInfo`·`DNSServiceQueryRecord`·`nw_endpoint_create_host/url`·`nw_connection_create`를 가로채 기록(`netwatch.c`) | URLSession 시험 프로그램의 조회·연결을 잡음 |
| 차단 | `sandbox-exec`로 outbound·bind·DNS-SD mach 서비스 거부. 텔레메트리가 켜진 실행은 이 안에서만 돌려 실제로는 아무것도 나가지 않게 함 | curl·URLSession이 이름 조회 단계에서 실패 |
| 소켓 | 실행 중 0.2초마다 `lsof -i` | — |
| 저장소 | 실행 후 `onnxruntime.db*` 변경 확인 | — |

각 실행은 추론 뒤 환경을 90초 살려 두고 관찰했다.

| ORT | 설정 | 이름 조회·연결 시도 | 비고 |
| --- | --- | --- | --- |
| 공식 1.30.0 | `ort` 기본(`EnableTelemetryEvents`) | 5·5 | sandbox 안 |
| 공식 1.30.0 | 호출 없음(1.12.1 방식) | 5·5 | sandbox 안 |
| 공식 1.30.0 | `DisableTelemetryEvents`만 | 5·5 | sandbox 안 |
| 공식 1.30.0 | `ORT_DISABLE_TELEMETRY=1`만 | 0 | sandbox 안 |
| 공식 1.30.0 | 둘 다 | 0 | sandbox 안·밖 모두, 소켓 0, 저장소 변경 없음 |
| 공식 1.31.0 | `ort` 기본 / 둘 다 | 5·5 / 0 | sandbox 안 |
| 공식 1.29.0(1.12.1 패키지) | 호출 없음 / 환경 변수만 | 5·5 / 0 | sandbox 안 |
| Homebrew 1.31.0 | `ort` 기본 | 0 | 1DS 없음 |

### 결과

| 경로 | 결과(54건, 1.12.1 fixture 대비) | p50(512×512, 전처리 포함) |
| --- | --- | --- |
| 공식 1.30.0 CPU | 모두 개수 같음, IoU ≥ 0.99999 | YOLO5Face 21.9 ms, YuNet 8.7 ms, 번호판 14.3 ms |
| 공식 1.30.0 CoreML(YOLO5Face), 번호판 CPU | 같음 | YOLO5Face 11.8 ms |
| Homebrew 1.31.0 CPU(정책 재설정 후) | 같음(번호판 score 차 0) | YOLO5Face 36.4 ms, 번호판 27.2 ms |
| Homebrew 1.31.0 CoreML(YOLO5Face), 번호판 CPU | 같음 | YOLO5Face 11.7 ms |

공식 1.30.0 macOS arm64 아카이브 SHA-256: `6ebb5062a934537c352937821f9fe9718e7de1a2db1122a93dd363ffd53a7012`(1.31.0: `1031b36dd3bdaa8d976a6130d9cf2a05e9be320b93ae804f8427b91dc3432afc`).

### 계획에 반영할 것

- ORT 텔레메트리: 공식 빌드는 1.29부터 1DS를 담는다. 끄는 수단은 `ORT_DISABLE_TELEMETRY=1`이고, `with_telemetry(false)`는 보조로만 둔다. 환경 변수는 `main` 첫 줄, 다른 스레드가 생기기 전에 설정한다. 릴리스 시험에 netwatch 감시를 넣고, 새 ORT 버전마다 다시 돌린다.
- EP: 세션마다 `with_auto_device(PreferCPU)`로 `ort`의 기본 정책을 덮은 뒤 EP를 명시한다. 번호판 모델은 모든 플랫폼에서 CPU로 시작하고, 가속 EP는 YOLO5Face에만 쓴다(Windows DML은 M0에서 번호판도 통과했으므로 M3에서 감지 0건 입력으로 다시 확인).
- 1.12.1 패키지의 텔레메트리는 레거시 정책상 코드로 고치지 않고, README·릴리스 노트로 따로 안내하지도 않는다(2026-10-17 메인테이너 결정).

## Linux 사전 실행 (x86_64 에뮬레이션)

실행일: 2026-10-16. Apple M4의 Docker Desktop에서 `ubuntu:24.04`(linux/amd64, glibc 2.39)를 에뮬레이션으로 돌려 [Linux 스크립트](../spikes/platform-runs/linux/run_m0_linux.sh)를 `--no-gui`로 실행했다. 실기기가 아니므로 **결과 일치는 근거로 쓰되 시간·처리량은 쓰지 않는다.** 실기기 확인은 [실행 절차](M0_PLATFORM_RUNBOOK.md) 1장이다.

- 공식 ORT 1.30.0 linux-x64 + `api-27` CPU: 54건 모두 1.12.1 fixture와 같음(개수 같음, IoU ≥ 0.99999, score 차 ≤ 4.5e-7).
- `INTER_LINEAR` 이식: `opencv-python-headless` 5.0.0.93(x86, KleidiCV 없음)과 36쌍 모두 바이트 일치, IPP 켬·끔 둘 다. macOS의 차이는 KleidiCV 때문이라는 판단과 맞는다.
- 텔레메트리(감시 라이브러리 `netwatch_linux.c` 기준, 90초):

| ORT | 설정 | collector 이름 조회 | 저장소 |
| --- | --- | --- | --- |
| 공식 1.30.0 | `ORT_DISABLE_TELEMETRY=1` + `with_telemetry(false)`, 네트워크 허용 | 0 | 변경 없음 |
| 공식 1.30.0 | `ort` 기본, 네트워크 namespace | 10(`mobile.events.data.microsoft.com:443`) | 기록함 |
| 1.12.1 AppImage의 1.29.0 | 호출 없음(1.12.1 방식), 네트워크 namespace | 10 | 기록함 |
| 1.12.1 AppImage의 1.29.0 | 환경 변수만, 네트워크 namespace | 0 | 변경 없음 |

  에뮬레이션에서는 `strace`가 시스템 호출을 해석하지 못해 모든 경우 0으로 나왔다. 그래서 스크립트는 `LD_PRELOAD` 감시를 주 근거로, `strace`를 보조로 쓴다.
- **1.12.1 Linux AppImage도 macOS 패키지처럼 텔레메트리 전송을 시도하는 상태였다.**
- FFmpeg 파이프 spike는 Linux에서 실행 경로가 동작함을 확인하고 4K 인코드 중에 멈췄다(에뮬레이션 수치는 의미 없음).

## Linux x86_64 실행 (GitHub Actions)

실행일: 2026-10-17. 실기기를 쓸 수 없어 GitHub Actions `ubuntu-24.04` 러너(AMD EPYC 7763 4 vCPU, 16 GB, Ubuntu 24.04.5, glibc 2.39, 커널 6.17 azure)에서 같은 스크립트를 `--no-gui`로 돌렸다(`.github/workflows/m0-linux.yml`, run 39015269891). 에뮬레이션이 아닌 x86_64라 `strace`도 동작한다.

- 공식 ORT 1.30.0 linux-x64 + `api-27` CPU: 54건 모두 1.12.1 fixture와 같음(개수 같음, IoU ≥ 0.999990, score 차 ≤ 4.5e-7, roll 차 ≤ 5.0e-6 rad). 시뮬레이션한 가속 실패 뒤 CPU 재실행 결과도 fixture와 같음. 지연 p50(512×512, 전처리 포함): YOLO5Face 58 ms, YuNet 19 ms, 번호판 39 ms.
- `INTER_LINEAR` 이식: `opencv-python-headless` 5.0.0.93과 36쌍 모두 바이트 일치(IPP 켬·끔).
- 텔레메트리(90초):

| ORT | 설정 | collector 이름 조회(libnetwatch) | 시스템 호출(strace) | 저장소 |
| --- | --- | --- | --- | --- |
| 공식 1.30.0 | `ORT_DISABLE_TELEMETRY=1` + `with_telemetry(false)`, 네트워크 허용 | 0 | 0 | 변경 없음 |
| 공식 1.30.0 | `ort` 기본, 네트워크 namespace | 10(`mobile.events.data.microsoft.com:443`) | 40 | `~/.cache/Microsoft/DeveloperTools/.onnxruntime/onnxruntime.db` |
| 1.12.1 AppImage의 1.29.0 | 호출 없음(1.12.1 방식), 네트워크 namespace | 8 | 32 | 같은 저장소 |
| 1.12.1 AppImage의 1.29.0 | 환경 변수만, 네트워크 namespace | 0 | 0 | 변경 없음 |

  strace 수는 이름 조회 한 번에 DNS 질의 패킷 여러 개가 함께 잡혀 libnetwatch보다 크다. 두 감시가 같은 경우에서만 0이 아니다. 에뮬레이션 사전 실행의 결론이 실제 x86_64에서 그대로 확인됐다.

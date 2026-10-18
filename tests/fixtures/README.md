# 기준 fixture (M0)

1.12.1(`v1.12.1` 태그) 동작에서 뽑은 언어 중립 시험 자료다. 입력은 파일과 JSON, 기대 결과는 JSON이다. Rust 구현의 시험은 이 폴더를 읽어 같은 판정을 한다. 근거는 [계획](../../docs/RUST_PORT_PLAN.md) 2장과 [분석 보고서](../../docs/REWRITE_ASSESSMENT.ko.md) 3장이다.

## 다시 만들기

```bash
tools/fixture-extract/run.sh <작업 폴더>
```

macOS에서 1.12.1 빌드 의존성(Homebrew Qt·OpenCV·ONNX Runtime·FFmpeg 등)과 `exiv2`, `exiftool`, `imagemagick`, Python `numpy`·`Pillow`가 필요하다. 스크립트는 태그를 작업 폴더로 내보내 그 사본의 시험 소스만 바꾼다. 저장소의 C++ 코드는 건드리지 않는다. 2026-10-16 생성 환경: macOS 27.0.1(Apple M4), OpenCV 5.0.0, ORT 1.31.0(Homebrew), FFmpeg 9.0.2, Exiv2 0.28.9.

생성된 영상은 로컬 FFmpeg·x264 빌드에 따라 바이트가 달라지므로 파일 자체를 커밋하고, 만든 명령은 [generated-media.json](generated-media.json)에 남긴다. 다시 만들면 영상 바이트가 바뀔 수 있으니, 바뀐 경우 관측값(`observed_1121.json`)도 함께 다시 커밋한다.

## 구성

| 경로 | 내용 |
| --- | --- |
| `legacy/<suite>/<test>.json` | 1.12.1 시험 하나. `asserts_cpp`는 그 시험의 assert 식과 통과 횟수, `calls`는 시험이 core API에 넣은 실제 입력과 출력이다. 이미지·파일 입출력은 같은 이름의 폴더에 PNG·원본 파일로 있다 |
| `legacy/index.json` | 시험마다 상태(`normative`·`intent`·`changed`·`dropped`), 관련 계획 단계·항목, 메모 |
| `detect/<model>/<image>@<score>.json` | 1.12.1 감지기 3종(CPU) 결과. NMS 0.4, score 0.1·0.25·0.5 |
| `detect/inputs/` | 생성한 감지 입력: 크기 5단계 astronaut(작은 얼굴), 같은 얼굴 280개(D11) |
| `defects/<D·P 항목>/` | `expected.json`(올바른 기대 결과, 손으로 작성), `observed_1121.json`(1.12.1 재현 결과, 생성), 입력 파일 |
| `metadata/` | allowlist EXIF 기준. 허용·금지·GPS 태그를 모두 넣은 JPEG와 1.12.1 `copyMetadata` 결과 |
| `assets.json` | 저장소에 넣지 않고 해시로 받는 외부 입력(모델, 표본 이미지). `tools/fetch_test_assets.py <폴더>`로 받는다 |
| `resize/` | OpenCV 5.0.0 `cv::resize(INTER_LINEAR)`의 x86 일반 경로 출력 SHA-256(2배 축소의 `INTER_AREA` 경로 포함). 입력은 파일에 적힌 식으로 만드는 잡음 패턴이라 저장하지 않는다. 생성: `tools/resize-reference/make.py`(macOS arm64 pip 판은 KleidiCV HAL 때문에 축소가 ±1 달라 쓰지 않음, CI `ort.yml`이 실제 x86에서 다시 만들어 비교) |
| `decode/` | 디코더 회귀 입력. `noninterleaved-420.jpg`는 성분마다 scan이 따로인 baseline 4:2:0 JPEG(`cjpeg -quality 92 -sample 2x2 -scans`, 0·1·2 성분 각각 한 scan), `interleaved-420.jpg`는 같은 그림의 일반 JPEG. `*.djpeg.png`는 libjpeg-turbo 3.2.0 `djpeg` 결과로, 기대 pixel이다. 그림은 96×64 수식 패턴([M2 기록](../../docs/M2_ENCODE_SPIKE.md)) |

`legacy`에 없는 1.12.1 시험 묶음: `custom_model_consent_tests`(사용자 ONNX 제거), `stage_cleanup_tests`(snapshot·stage 제거), `update_cache_tests`(Velopack 사용으로 캐시 권한 검사 제거). Windows 전용 `testScanDoesNotFollowJunctions`는 macOS에서 실행되지 않아 기록이 없다.

### 상태 값

- `normative`: assert 의도를 지키고 기록된 출력값과 같아야 한다(부동소수는 허용오차).
- `intent`: assert 의도만 규범이다. 기록값은 참고용(구현 방식이 의도적으로 다름. 예: Hungarian 할당, 모자이크 강도).
- `changed`: 계획이 동작을 바꿨다. 1.12.1 동작을 기대값으로 쓰지 않고 `note`와 해당 `defects/` 항목을 따른다.
- `dropped`: 없앤 기능이다.

결함 영역(`defects/`)에서는 1.12.1 출력을 기대값으로 쓰지 않는다. `expected.json`의 `proposed`에 적은 세부 규칙은 계획에 없는 것으로, 해당 단계에서 확정한다.

## 결함 재현 요약

| # | 1.12.1에서 재현 | 관측 |
| --- | --- | --- |
| D1 | 예 | 출력 색 태그 전부 `unknown`, BT.709 원본 중앙 (201,59,39)가 BT.709로 재생하면 (211,71,35). full range는 limited로 바뀜 |
| D2 | 예 | Display P3·Adobe RGB·wide gamut 입력 모두 출력 ICC 없음, pixel 그대로 |
| D3 | 예 | 600px 영역에서 블록 4–40px가 모두 12×12 셀 |
| D4 | 예 | umask 022·002·077 모두 파일 0600, 폴더 0700. 영상 출력도 0600 |
| D5 | 예 | umask 002에서 `folder_not_private` |
| D6 | 예 | JPEG는 q100 4:4:4(원본 대비 1.9–3.3배), 손실 WebP는 무손실(12.2배) |
| D7 | 예(스캔 전 로드) | media 없는 폴더 + 없는 모델 경로 → run 실패. 중복 로드는 코드 확인 |
| D8 | 코드 확인 | GPU 실패 시점에만 발생해 fixture는 실패 주입 시나리오로만 적음 |
| D9 | 코드 확인 | `legacy/tests/testWorkerReportsCopiedOriginalAsWarning` |
| D10 | 코드 확인 | `MainWindow`(core 밖)라 harness로 호출하지 않음 |
| D11 | 예 | NMS 후 259개(상한 300 미만)인데 omitted 4017(score 0.1) |
| D12 | 예 | 36 MB PNG 처리 중 `TMPDIR/.cloakframe-stage-*/source.png`에 전체 사본 |
| P-1 | 예 | 영상 gap 30–54가 보고됐지만 completed로 게시, 그 구간 얼굴 영역 비마스킹. 사진도 경고와 함께 게시 |
| P-2 | 코드 확인 | 번호판도 `FaceDetection`, 임계값 공유 |
| P-3 | **아니오** | CFR(오프셋·B-frame)·23.976·VFR·회전·오디오 선행 입력 모두 미리보기·분석·출력 프레임 일치. 두 필터 경로에 기대는 구조적 위험으로 보고 회귀 시험으로 둠 |
| P-4 | 코드 확인 | 게시 전 출력 검사 없음 |
| P-5 | 예 | 641×361 → 640×360 crop, 오디오 2.005 s(영상 2.0 s). 오디오 선행 입력의 A/V 동기는 맞음 |
| P-6 | 예 | 보간 box가 평활화로 줄어듦, 1프레임 장면 뒤 전환 누락 |
| P-7 | 코드 확인 | `UpdateTrust::Unpinned`를 수락 |

## 사용 조건

`defects/`, `detect/inputs/`, `metadata/`의 생성 파일은 수식이나 astronaut.png(NASA, public domain)에서 만든 것이고 ICC profile도 primaries에서 직접 계산한 것이라 제3자 권리가 없다. `assets.json`의 OpenCV·open-image-models 표본은 재배포하지 않고 해시로만 받는다.

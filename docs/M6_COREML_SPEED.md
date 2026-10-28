# 영상 분석 속도: CoreML 설정

실행일: 2026-10-28. 장치: Apple M4(10코어), macOS 27, release, 고정 ORT 1.30.0·FFmpeg 9.0.2. [M6 4단계 기록](M6_UX.md)의 "영상 분석 속도"(메인테이너의 1280×720 30 fps 29.5초 클립이 타일 켬 CLI 50.3초, GPU 76–84%, 추론 스레드가 CoreML 예측 안에서 78%)에서 이어진 조사다. 품질 기준은 사진 탐지 벤치마크의 S3 세트 610장([M3_BENCHMARK](M3_BENCHMARK.md))과 영상 벤치마크 세 클립([M5_BENCHMARK](M5_BENCHMARK.md))이다.

## 결론

- **Neural Engine으로 바꾸는 것은 답이 아니다.** 지금 세션은 fp32 MLProgram이라 `MLComputeUnits=CPUAndNeuralEngine`을 주면 CoreML이 CPU에서 돌린다. 메인테이너 클립 1패스가 58.8초로 CoreML CPU 전용(58.3초)과 같았고 CPU 시간이 84초로 늘었다. Neural Engine에서 실제로 도는 NeuralNetwork 형식(fp16)은 파이프라인에서 현행보다 빠르지 않다. 메인테이너 클립 중앙값은 35.3초 대 35.8초, 영상 벤치마크 1패스 합계는 195.1초 대 154.9초였다. 상자가 CPU와 달라져 영상 벤치마크 타일 끔 노출이 5,322 → 5,637로 게이트(허용 277)를 넘는다.
- **원인: 동적 batch 차원 때문에 그래프가 CoreML 조각 두 개로 나뉜다.** YOLO5Face-n 입력은 `[batch, 3, 640, 640]`이다. ORT CoreML EP가 입력 모양이 정해지지 않은 첫 `Reshape`(`/model.1/Reshape`)를 받지 않아 그 노드 하나만 CPU에 남는다. ORT는 세션을 만들 때마다 "number of partitions supported by CoreML: 2 … number of nodes supported by CoreML: 427"(428개 중) 경고를 냈다. 뷰마다 CoreML 예측 두 번과 CPU 왕복이 생긴다.
- **추천: YOLO5Face-n 세션에서 batch를 1로 고정한다**(ORT free dimension override `batch` = 1). 428개 노드가 모두 한 조각에 들어간다. 결과는 현행과 같다. 사진 610장(기본 설정, 타일 끔)과 영상 감지 기록 6개(세 클립 × 타일 끔·켬)가 바이트 단위로 같았다. 시간은 사진 벤치마크 33.3 → 25.9초, 영상 벤치마크 1패스 합계 154.9 → 115.2초(−26%), 메인테이너 클립 CLI 전체 실행 중앙값 57.9 → 41.7초(−28%), 1패스 40.7 → 25.0초(−39%)로 줄었다([표](#메인테이너-클립)).
- **함께 바꿀 것: CoreML 컴파일 캐시 키.** CoreML은 그래프 구조가 같으면 컴파일된 모델을 다시 쓴다([M3_EP](M3_EP.md)). 조각 나누기가 바뀌면 같은 모델 폴더에 남은 옛 조각의 컴파일 결과를 불러올 수 있다. 그래서 캐시 폴더 이름에 세션 설정을 넣거나 지금 폴더를 버려야 한다. 이번 조사는 설정마다 폴더를 나눠 피했다.
- **효과가 없던 것:** `AllowLowPrecisionAccumulationOnGPU`는 출력이 현행과 비트 단위로 같았다(fp16으로 누적했다면 점수 넷째 자리가 달라졌어야 함). 그래서 처음 보인 속도 차이는 흔들림이다. `SpecializationStrategy=FastPrediction`과 `CPUAndGPU`도 흔들림 안이었다. NeuralNetwork 형식을 GPU에 고정하면 2.7배 느려졌다.
- **안 되는 것: 프레임의 보기를 한 번에 묶기(batch > 1).** 입력 batch는 동적이지만 그래프 안의 reshape 상수가 batch 1로 박혀 있어, batch 3을 넣으면 첫 `Reshape`에서 실패한다. 묶으려면 모델 그래프를 고쳐야 한다.

## 방법

1. 작업 트리(main 1d3462c)를 스크래치에 복사하고, `OrtRuntime::session`의 CoreML 옵션(형식, 연산 장치, fp16 누적, FastPrediction, batch 고정)을 환경 변수로 바꾸게 했다. CoreML 캐시 폴더 이름에도 그 값을 넣었다. 계측한 복사본과 스크립트는 저장소에 넣지 않았다.
2. **파이프라인 시간:** 메인테이너 클립을 `video_benchmark` 예제(1패스, `detect_seconds`)와 CLI `run`(앱과 같은 설정: YOLO5Face-n, `--gpu on --tiles on --hardware-encode on --review off`)으로 쟀다. 같은 설정도 실행마다 ±10% 흔들려서 설정을 번갈아 여러 번 쟀다. GPU 사용률은 `ioreg`의 Device Utilization을 2초마다 읽은 평균이다.
3. **추론만:** 한 프로세스에서 설정마다 세션을 만들고, 같은 입력(결정적 의사 난수, [0, 1])으로 10번 데운 뒤 200바퀴를 번갈아 돌려 뷰 하나의 `run` 시간과 CPU EP 대비 출력 차이를 쟀다.
4. **품질:** 사진은 `benchmark` 예제를 기본 설정과 YOLO5Face-n 0.5 타일 끔으로 돌려 `evaluate.py`로 채점하고, CPU 결과와 사진마다 비교했다. 영상은 `video_benchmark`를 `--gpu on`으로 세 클립 × 타일 끔·켬 돌려 `video_evaluate.py --gate`로 채점했다. 이 Mac의 CPU 결과는 기록된 `engine/default.json`과 사진마다 같았다.
5. **조각 확인:** ORT 로그 수준을 Verbose로 올려 CoreML EP가 받지 않은 노드(`supported: [0]`)와 조각 수를 봤다. `ProfileComputePlan` 옵션은 이 ORT 빌드에서 아무것도 출력하지 않아 연산 장치 배치는 시간·CPU 시간·GPU 사용률로 가렸다.

## 측정

### 메인테이너 클립

1패스(`video_benchmark`), 타일 켬, 884프레임. 훑어보기 한 번과 번갈아 세 번:

| 설정 | 1패스 | CPU 시간 | GPU |
| --- | --- | --- | --- |
| 현행(MLProgram, ALL) | 32.6–42.0초, 중앙값 35.8초(5회) | 26–32초 | 83–86% |
| MLProgram, CPUAndNeuralEngine | 58.8초 | 84초 | 12% |
| MLProgram, CoreML CPU 전용 | 58.3초 | 84초 | 12% |
| MLProgram, CPUAndGPU | 44.3초 | 32초 | 86% |
| 현행 + fp16 누적 | 28.8–38.2초, 중앙값 30.1초(4회) | 27–31초 | 84–87% |
| 현행 + FastPrediction | 35.9초 | 29초 | 88% |
| NeuralNetwork, ALL | 29.3–33.4초, 중앙값 32.2초(4회) | 34–36초 | 13–36% |
| NeuralNetwork, CPUAndNeuralEngine | 33.5–36.7초, 중앙값 35.3초(4회) | 35–37초 | 12–21% |
| NeuralNetwork, CPUAndGPU | 101.6초 | 76초 | 42% |

fp16 누적의 첫 번째 반복값(38.2초)은 사진 세트를 받는 중이었다.

현행과 batch 고정을 번갈아 세 번(CLI 전체 실행을 먼저 세 번, 이어서 1패스 세 번). CoreML 캐시를 채우는 실행을 한 번씩 먼저 했다(현행 51.7초, batch 고정 44.1초).

| 설정 | CLI 전체 | 1패스 | CPU 시간(1패스) | GPU |
| --- | --- | --- | --- | --- |
| 현행 | 57.9 / 59.0 / 55.9초, 중앙값 57.9초 | 40.7 / 38.4 / 46.8초, 중앙값 40.7초 | 29–33초 | 84–88% |
| batch 고정 | 41.7 / 44.5 / 38.8초, 중앙값 41.7초(−28%) | 23.7 / 27.5 / 25.0초, 중앙값 25.0초(−39%) | 25–27초 | 87–89% |

이 측정 동안에는 load average가 5–22로 높았다. 현행 CLI가 앞선 기록(50.3초)보다 느린 것은 그 때문으로 보인다. 같은 조건에서 번갈아 쟀으므로 둘의 차이는 유효하다.

### 추론만

뷰 하나(1×3×640×640)의 `run` 시간, 200바퀴. 두 번 쟀고, 두 번째 실행에는 batch 고정 설정을 더했다. 두 번째는 다른 작업과 겹쳐 GPU 값이 전체로 느렸다.

| 설정 | 첫 번째 중앙값 (p10–p90) | 두 번째 중앙값 (p10–p90) | CPU EP 대비 출력 |
| --- | --- | --- | --- |
| CPU EP(10스레드) | 18.2 ms (17.9–19.0) | 20.9 ms (18.2–29.9) | — |
| 현행 | 11.5 ms (8.2–19.1) | 18.0 ms (11.3–23.1) | 최대 차 4.0e-4 |
| 현행 + fp16 누적 | 10.4 ms (8.3–15.4) | | 현행과 같음 |
| 현행 + FastPrediction | 8.4 ms (7.7–11.8) | | 현행과 같음 |
| CPUAndGPU | 8.0 ms (7.7–10.1) | 13.9 ms (10.4–21.8) | 현행과 같음 |
| CPUAndNeuralEngine | 21.8 ms (21.5–22.8) | | 최대 차 3.4e-4 |
| NeuralNetwork, ALL | 9.9 ms (9.6–10.5) | 9.9 ms (9.4–11.8) | 상자 최대 1.67 px, objectness 3.6e-3 |
| NeuralNetwork, CPUAndNeuralEngine | 11.0 ms (10.8–11.7) | 11.4 ms (10.8–13.3) | 위와 같음 |
| NeuralNetwork, CPUAndGPU | 39.9 ms (37.4–44.3) | | 상자 최대 6.3 px |
| batch 고정, 현행 | | 15.5 ms (10.1–20.2) | 현행과 같은 크기 |
| batch 고정, CPUAndGPU | | 11.9 ms (8.6–19.9) | 현행과 같은 크기 |
| batch 고정, CPUAndNeuralEngine | | 17.1 ms (15.2–25.4) | |
| batch 고정, NeuralNetwork ALL | | 9.2 ms (8.7–11.0) | NeuralNetwork와 같은 크기 |

GPU 설정은 뷰마다 8–23 ms로 흔들린다. 화면(WindowServer)과 다른 앱이 GPU를 함께 쓰기 때문으로 보인다. 앱도 UI를 GPU로 그린다. Neural Engine은 흔들림이 작지만 빠르지 않다.

### 사진 벤치마크

S3 세트 610장, YOLO5Face-n 0.5. 시간은 `benchmark` 예제 전체(디코드·전처리 포함)다.

| 설정 | 기본(타일 켬) 재현율 | 시간 | 타일 끔 재현율 | 시간 | CPU와 다른 사진 |
| --- | --- | --- | --- | --- | --- |
| CPU EP | 87.9% | 65.1초 | 83.9% | 16.5초 | — |
| 현행 | 87.9% | 33.3초 | 83.9% | 6.4초 | 0 / 1장(상자 하나 0.2 px) |
| 현행 + fp16 누적 | 87.9% | 35.5초 | 83.9% | 6.8초 | 현행과 같음 |
| batch 고정 | 87.9% | 25.9초 | 83.9% | 4.9초 | 현행과 같음 |
| NeuralNetwork, ALL | 87.9% | 38.1초 | 84.2% | 6.6초 | 327 / 311장 |
| NeuralNetwork, CPUAndNeuralEngine | 87.9% | 39.6초 | 84.2% | 7.0초 | 327 / 311장 |

NeuralNetwork 형식의 상자는 CPU와 IoU 중앙값 0.997, 최소 0.84다. 정밀도는 기본 설정 95.8%로 같고 타일 끔 97.2% → 97.1%다.

### 영상 벤치마크

얼굴 프레임 27,714 중 노출(50% 미만 덮임). 기준은 CPU 감지로 기록한 `expected.json`(타일 끔 5,322, 타일 켬 912)이다.

| 설정 | 타일 끔 | 타일 켬 | 게이트 | 1패스 합계 |
| --- | --- | --- | --- | --- |
| 현행 | 5,322 | 912 | 통과 | 154.9초 |
| batch 고정 | 5,322 | 912 | 통과 | 115.2초 |
| NeuralNetwork, CPUAndNeuralEngine | 5,637 | 882 | 타일 끔 실패 | 195.1초 |

1패스 시간(타일 끔 / 켬): 거리 17.4 / 56.0초 → batch 고정 13.2 / 35.6초 → NeuralNetwork 21.9 / 46.8초, 도로 7.5 / 23.4 → 8.3 / 22.0 → 7.5 / 41.8, 군중 13.4 / 37.2 → 9.6 / 26.5 → 22.4 / 54.7. batch 고정의 감지 기록은 현행과 바이트 단위로 같다. NeuralNetwork 형식의 track 수는 거리 68·151, 도로 65·180, 군중 212·350이다(현행 69·146, 65·174, 208·335).

## 구현

같은 날 브랜치 `coreml-compute-units`에서 구현했다.

- **YOLO5Face-n의 CoreML 세션에만 건다.** `yolo5face::FIXED_DIMENSIONS`(`batch` = 1)를 `OrtRuntime::session`이 CoreML 세션을 만들 때 건다. 번호판 출력 `[batch, 7]`의 `batch`는 감지 개수라 공통 빌더에 넣지 않았다. CPU에도 걸어 봤더니 사진 결과(상자 0.1 px, 점수 넷째 자리로 기록)는 같았지만, 영상 감지 기록 6개가 모두 달라졌다. 차이는 점수 최대 2.6e-6, 상자 최대 0.015 px였고 감지 개수가 바뀐 프레임은 없었다. 그래도 추적까지 이어져 타일 켬 노출이 912 → 906이 됐다. CPU에서는 속도 이득이 없어(사진 66.0초) 결정(2026-10-28)대로 CoreML에만 건다. DirectML은 재지 않아 바꾸지 않았다.
- **캐시 폴더:** batch를 고정한 세션은 `<ORT 빌드>/<모델 SHA-256>-batch1`에 캐시한다. 조각 이름은 고정 전후 모두 `0_dynamic_mlprogram`이고(고정 전에는 `1_dynamic_mlprogram`도 있음), 폴더를 나누지 않으면 고정한 세션이 batch가 열린 그래프의 첫 조각을 불러온다. `remove_unused_coreml_caches`는 같은 이름 규칙으로 이 빌드가 쓰는 폴더를 정하므로, batch를 고정하기 전의 `<모델 SHA-256>` 폴더는 지운다.
- **시험:** `detect_fixtures.rs`의 `yolo5face_runs_as_one_coreml_partition`은 캐시 폴더 이름과 컴파일된 조각이 하나임을 확인한다. 고정을 빼면 실패한다. 캐시 시험은 옛 폴더를 지우는지도 본다.
- **결과 확인(release, main 1d3462c와 비교):** CoreML 사진 610장(기본, YOLO5Face-n 0.5 타일 끔)과 영상 감지 기록 6개가 main과 바이트 단위로 같다. CPU 사진은 `engine/default.json`과, CPU 영상 감지 기록 6개는 기록된 `detections/*.jsonl.gz`와 바이트 단위로 같다. 영상 게이트는 CPU·CoreML 모두 5,322 / 912로 통과했다. 한 번씩 잰 시간은 CoreML 사진 기본 46.3 → 26.1초, 타일 끔 9.8 → 5.1초, 영상 1패스 합계 135.7 → 102.3초다.

## 남은 것

- 앱 안의 속도는 메인테이너가 다시 확인한다(UI가 GPU를 함께 씀).
- 해상도에 따라 영상 타일을 켜는 것, 노는 CPU에 세션을 함께 돌리는 것은 [M6 4단계 기록](M6_UX.md)의 "다음에 볼 것" 그대로다.

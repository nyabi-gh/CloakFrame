# M3 감지: 번호판 어댑터·종류별 임계값·감지 구성

실행일: 2026-10-19. 장치: Apple M4, macOS 27.0, 공식 ORT 1.30.0 CPU. [Rust 포팅 계획](RUST_PORT_PLAN.md) 8장 M3 2단계의 조사·결정·결과를 적는다. 코드는 `crates/cloakframe-engine/src/detect/`(`plate.rs`, `models.rs`)에 있다.

## 결론

- **번호판 어댑터:** 1.12.1 `PlateDetector`의 전처리(512 letterbox, 축소 크기 반올림, `round(pad - 0.1)` 위치, 회색 114, RGB 0..1)와 출력 해석(`[N, 7]`, score는 7번째 열, 모서리 순서 정리, 이미지 밖 자르기, 1 px 미만 버림)을 옮겼다. 감지 fixture 18건(6장 × score 0.1·0.25·0.5)이 1.12.1과 같다(최소 IoU 0.9999966, score 차 5.4e-7).
- **그래프 안의 NMS를 풀었다.** end2end 그래프의 `NonMaxSuppression`은 최대 **100개**, IoU **0.45**, score 하한 0.001이 상수 initializer로 들어 있다. engine은 `SessionBuilder::with_initializer`로 상한을 NMS 입력 box 수 전부(5,376)로, IoU를 1.0으로 바꾸고, core NMS를 번호판 임계값으로 직접 돌린다. 상한이 찾은 번호판을 버리는 일이 없어지고(D11, 사진은 NMS를 통과한 감지를 모두 가린다는 M2 결정과 같은 원칙), 번호판 NMS 설정이 실제로 적용된다(P-2). 1.12.1은 NMS 설정을 번호판에 쓰지 않았다.
- **기본값(2026-10-19 메인테이너 결정):** 얼굴 모델은 YOLO5Face-n, 번호판은 꺼짐(1.12.1과 같음), 임계값은 1.12.1이 실제로 쓴 값(얼굴 두 모델 score 0.5·NMS 0.4, 번호판 score 0.5·NMS 0.45). 값은 core `ModelId::default_thresholds`의 모델별 표에 둔다. 6단계 라벨 벤치마크(계획 9.2)에서 YOLO5Face-n 0.3·0.4·0.5를 비교한 뒤 0.5를 유지하기로 했다(2026-10-20, 0.4·0.3은 라벨 얼굴 재현율을 0.3–1.0%p만 올리고 오탐을 2–3배로 늘림, [기록](M3_BENCHMARK.md)).
- **감지 구성:** `DetectionModels`(얼굴 모델 하나 또는 없음, 번호판 켬·끔, 둘 다 없으면 만들 수 없음)가 처리기 설정에 들어가고 review key의 `models`가 된다. 선택한 모델은 추론 스레드 하나에서 차례로 돌고, 결과는 모델 사이 NMS 없이 합친다(얼굴과 번호판은 서로 지우지 않음, P-2). CLI는 `--models DIR`(upstream 파일 이름), `--faces yolo5face-n|yunet|off`, `--plates on|off`를 받고, `review`도 같은 선택을 받아야 결정을 적용한다.
- **YuNet의 NMS 전 후보 상한도 없앴다.** 1단계 어댑터는 OpenCV `FaceDetectorYN`처럼 NMS 전 후보를 점수 상위 5,000개로 잘랐다. 얼굴 하나에 후보가 6.5–11개라 대략 440–770명이 넘는 사진에서는 1.12.1 YOLO5Face-n의 2,000개 상한(D11)과 같은 방식으로 얼굴을 버리고, 버린 수는 `CappedCandidates`로도 남지 않았다. 메인테이너 결정(2026-10-19)으로 모든 후보를 NMS에 넣는다. fixture 18건은 그대로 1.12.1과 같다([아래](#yunet의-nms-전-후보-상한)).

## 번호판 그래프의 NMS

`onnx` 1.23.2로 그래프를 열어 확인했다. 입력 `images [1, 3, 512, 512]`, 출력 `output0 [batch, 7]`(batch index, x1, y1, x2, y2, class, score), opset 17.

| initializer | 값 | `NonMaxSuppression` 입력 |
| --- | --- | --- |
| `/end2end/Constant_24_output_0` | 100 | `max_output_boxes_per_class` |
| `/end2end/Constant_25_output_0` | 0.45 | `iou_threshold` |
| `/end2end/Constant_26_output_0` | 0.001 | `score_threshold` |

NMS 입력은 box `[1, 5376, 4]`, score `[1, 1, 5376]`(클래스 하나)이다. 5,376은 stride 8·16·32의 격자 64² + 32² + 16²다. engine은 상한을 5,376으로(어떤 출력도 잘리지 않음), IoU를 1.0으로(IoU가 1.0보다 클 수 없어 아무것도 지우지 않음) 바꾼다. score 하한 0.001은 그대로 둔다. 하한까지 풀면 매번 5,376개가 NMS에 들어가고, 하한 아래 box는 임계값을 넘는 box를 지울 수 없어(점수 순 greedy NMS) 결과가 달라지지 않는다.

같은 세션 옵션으로 원래 그래프와 바꾼 그래프를 비교했다(score 하한 0.001까지 모두, engine NMS 0.45):

| 입력 | 원래 그래프 | 바꾼 그래프 + engine NMS | 비교 |
| --- | --- | --- | --- |
| fixture 6장 | 0·0·3·4·5·7개 | 같음 | box·score 순서까지 같음 |
| `car_image` 2×2·3×3·4×4 타일 | 20·35·33개 | 같음 | box·score 순서까지 같음 |
| 3×3 타일, engine NMS 1.0 | — | 168개 | 상한 100과 그래프 IoU가 적용되지 않음 |

`detect_fixtures.rs`의 `the_plate_graph_limits_are_lifted`가 마지막 줄을 시험한다. 두 initializer 중 하나만 빼도 실패하는 것을 확인했다. engine NMS는 이미지 좌표(자르기 뒤)에서 하고 그래프는 모델 입력 좌표에서 했지만, 위 비교에서 차이는 없었다. 지연(전처리 포함, release)은 두 그래프 모두 512×512에서 약 29 ms로 같다.

자른 번호판만 회색 바탕에 격자로 붙인 그림(7×17, 8×16, 6×20)에서는 모델이 번호판을 거의 찾지 못해(score 0.001–0.25) 상한 시험에 쓸 수 없었다. 차 전체를 붙인 타일은 타일마다 번호판 하나를 찾는다(3×3, score 0.25·NMS 0.45에서 9개).

## 임계값 근거: 점수 분포

라벨 세트(9.2)가 없어 두 가지를 봤다. 기본값은 1.12.1 값으로 두고, 이 수치는 6단계 비교의 출발점으로 남긴다.

**얼굴·번호판이 없는 사진 179장의 오탐.** macOS에 들어 있는 사진(`/System/Library/Desktop Pictures`의 HEIC 정지 사진과 `/Library/User Pictures`의 동물·꽃·사물, `sips`로 PNG 변환)을 엔진 디코드 경로로 돌렸다. 괄호는 오탐이 하나 이상 나온 사진 수다.

| score | YOLO5Face-n | YuNet | 번호판 |
| --- | --- | --- | --- |
| ≥ 0.1 | 18 (9장) | 367 (72장) | 38 (29장) |
| ≥ 0.25 | 10 (6장) | 27 (20장) | 21 (16장) |
| ≥ 0.4 | 5 (3장) | 7 (7장) | 17 (12장) |
| ≥ 0.5 | 4 (3장) | 4 (4장) | 14 (10장) |
| ≥ 0.6 | 0 | 1 (1장) | 10 (7장) |

가장 높은 오탐: YOLO5Face-n은 Football 0.55·Baseball 0.54·Big Sur 0.51, YuNet은 Sandollar 0.69·Red Rose 0.56, 번호판은 Golf 0.82·Earth 0.69·Motion Yellow 0.68. 번호판 오탐 점수는 `car_image`의 실제 번호판(0.82)과 겹친다. 번호판을 기본으로 끈 근거다.

**실제 대상.** `car_image` 뒤쪽 군중의 작은 얼굴(12–18 px, 대부분 실제 얼굴)은 YOLO5Face-n이 score 0.5에서 2개, 0.4에서 5개, 0.25에서 10개, 0.1에서 14개 찾는다. YuNet은 0.5에서 0개, 0.25에서 7개다. `messi5`의 흐린 관중석에서 YuNet은 0.25에서 7개를 더 찾는데 얼굴인지 판단하기 어렵다. 크기 5단계 astronaut에서는 YOLO5Face-n이 세 크기(512·256·128 px), YuNet이 두 크기를 찾고 64·32 px는 둘 다 놓친다. 번호판은 `car_image`의 번호판 하나(0.82)뿐이고, 같은 사진의 0.118은 오탐이다.

YOLO5Face-n을 0.4로 낮추면 군중 얼굴이 2개에서 5개로 늘고 얼굴 없는 사진의 오탐은 4개에서 5개로 는다. 근거가 사진 한 장이라 6단계 벤치마크에서 다시 쟀고, 0.5를 유지했다([기록](M3_BENCHMARK.md)).

## YuNet의 NMS 전 후보 상한

YuNet 어댑터(1단계)는 1.12.1이 쓴 OpenCV `FaceDetectorYN`과 같게 score를 넘는 후보를 점수순으로 5,000개까지만 NMS에 넣는다(`TOP_K`). crowd fixture(같은 얼굴 280개)의 NMS 전 후보 수:

| score | 후보 | NMS 뒤 |
| --- | --- | --- |
| 0.1 | 3,200 | 318 |
| 0.25 | 2,393 | 280 |
| 0.5 | 1,820 | 280 |

얼굴 하나에 후보가 6.5–11개이므로 score 0.5에서는 약 770명, 0.1에서는 약 440명이 넘으면 상한에 걸린다. 잘린 후보에만 있던 얼굴은 가려지지 않고 `Found.over_limit`에도 남지 않았다. YOLO5Face-n은 같은 이유로 M2에서 NMS 전 상한을 없앴다(25,200행, 최악 약 0.3초).

**처리(2026-10-19):** 상한을 없앴다. 후보는 세 stride의 격자 수 8,400개(80² + 40² + 20²)를 넘을 수 없고, 서로 겹치지 않는 후보 8,400개의 NMS는 release에서 60 ms였다. 5,041개가 모두 남는지 보는 `nms_considers_every_candidate`를 먼저 넣어 5,000개에서 실패하는 것을 확인했다. fixture에는 5,000개를 넘는 경우가 없어 결과가 같다.

## 재현

조사용 프로그램은 저장소에 넣지 않았다. 작업 트리를 스크래치 폴더에 복사하고 engine에 예제 하나(감지 사본을 만들어 감지기를 돌리고 JSON 한 줄씩 출력)와 원래 그래프 세션을 만드는 생성자를 임시로 더해 별도 `CARGO_TARGET_DIR`에서 release로 빌드했다. 타일 그림은 Pillow로 `car_image.webp`를 이어 붙여 만들었다.

# M0 spike: ort + ONNX Runtime 모델 3종

실행일: 2026-10-16. 장치: Apple M4, macOS 27.0.1. [Rust 포팅 계획](RUST_PORT_PLAN.md) M0의 "ort/ORT/EP 쌍으로 세 모델 로드·추론" 항목의 첫 결과다. Windows DirectML과 Linux는 아직 시험하지 않았다.

## 결론

- `ort` 2.0.0-rc.14로 YOLO5Face-n, YuNet, 번호판 YOLOv9-t를 모두 로드·추론했고, CPU에서 **1.12.1 감지기와 같은 결과**를 냈다. 감지 개수, box, score, roll이 출력 정밀도(소수 4–5자리) 안에서 같다.
- YuNet은 OpenCV `FaceDetectorYN` 없이 ORT 원시 출력(`cls/obj/bbox/kps` × stride 8·16·32)을 직접 해석해 같은 결과를 냈다. 계획의 "YuNet raw 출력 해석" 위험은 해소됐다.
- 모델 전처리의 resize는 OpenCV `INTER_LINEAR`를 그대로 옮겨야 한다. 옮긴 구현은 시험한 8개 크기 조합 전부에서 OpenCV 출력과 바이트 단위로 같았다. 계획 4.3절을 이에 맞게 고쳤다.
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

지연시간(512×512 입력, 전처리 포함, 50회 p50)은 실행마다 크게 흔들렸다. 예를 들어 Rust CoreML YOLO5Face-n은 실행에 따라 12–23 ms, 번호판은 7.5–15.8 ms였다(MLProgram 기준). 아래는 한 번씩 잰 참고값이며, 성능 판정은 계획 10.3절대로 고정 장치에서 반복 측정해야 한다.

| 모델 | 1.12.1 CPU | Rust CPU | 1.12.1 CoreML | Rust CoreML |
| --- | --- | --- | --- | --- |
| YOLO5Face-n | 47.6 ms | 37.4 ms | 13.4 ms | 11.9–23.3 ms |
| YuNet | 6.8 ms(OpenCV) | 9.5 ms(ORT) | — | — |
| 번호판 | 40.2 ms | 25.5 ms | 7.2 ms | 7.5–15.8 ms |

Rust 전처리(512→640 resize + letterbox + blob)는 아직 SIMD가 없는 단순 루프라 p50 4.1 ms다. 최적화는 M2 범위다.

## 계획에 반영할 것

- 모델 전처리 resize는 core가 소유하는 OpenCV 호환 구현으로 한다(4.3절 반영).
- CoreML 옵션은 `MLProgram` + `ALL`을 명시하고, compiled-model cache 위치는 worker 전용으로 지정한다. 1.12.1 harness가 앱 이름 없이 실행되자 `~/Library/Caches/coreml-models`에 cache를 만들었다. Rust에서는 경로를 앱 cache 아래로 명시해야 한다.
- rc.14는 `Session::builder(&Environment)`로 Environment를 명시적으로 넘긴다. 계획의 "번들 절대 경로에서 ORT 로드, 전역 자동 설정에 맡기지 않음"과 맞는다.
- 남은 M0 추론 항목: Windows DirectML(같은 비교), Linux CPU, ORT 1.31 배포물과의 조합, 작은 얼굴·번호판 양성 샘플을 늘린 비교.

# M3 EP 정책과 CPU 폴백

실행일: 2026-10-19. 장치: Apple M4, macOS 27.0, 공식 ORT 1.30.0. [Rust 포팅 계획](RUST_PORT_PLAN.md) 8장 M3 3단계와 3장 EP 정책의 구현 기록이다. 코드는 `crates/cloakframe-engine/src/detect/`(`runtime.rs`, `fallback.rs`)에 있다.

## 결론

- **실행 위치:** 세션은 `Backend`(CPU·CoreML·DirectML) 중 하나에서 만든다. GPU를 켜면(기본) macOS는 YOLO5Face-n과 YuNet을 CoreML에, 번호판을 CPU에 두고, Windows는 세 모델을 DirectML에, Linux는 CPU에 둔다(`fallback::backend_for`). `--gpu off`면 모두 CPU다.
- **세션 옵션은 1.12.1 `OrtAcceleration`과 같다.** CPU는 모든 최적화·논리 코어 수 스레드, 가속기는 `EXTENDED`(ORT `Level2`)·스레드 1이다. CoreML은 `MLProgram`·`ALL`, DirectML은 memory pattern 끔·순차 실행·device 0이다. `MLProgram`을 빼면 accelerator fixture 시험이 실패한다(M0의 NeuralNetwork 결과 차이).
- **YuNet도 CoreML에서 돌린다(2026-10-19, macOS 시험 후 결정).** 1.12.1은 YuNet을 OpenCV로 CPU에서만 돌렸다. CoreML에서 fixture 18건이 1.12.1과 같고(최소 IoU 0.99999, score 차 3.9e-7) 2.5배 빠르다.
- **번호판은 macOS에서 CPU로 남는다.** 그래프 NMS를 풀어도(2단계) 후보가 하나도 없는 사진에서 CoreML이 빈 동적 텐서를 거부한다(`Input (_end2end_Gather_output_0) has a dynamic shape ({-1}) but the runtime shape ({0}) has zero elements`). fixture 18건 중 후보 0개인 6건이 실패하고 나머지 12건은 같다. 1.12.1은 로드 때 검은 그림으로 시험 실행을 해서 이 오류로 CPU로 바꿨으므로, 실제로도 CPU에서 돌았다.
- **CPU 폴백(계획 3장):** 가속기에서 세션을 만들지 못한 모델은 처음부터 CPU를 쓴다. 사진 하나가 가속기에서 실패하면(ORT 실행 오류, `DetectError::Failed`) 같은 사진을 CPU 세션으로 다시 분석하고, 그 실행의 나머지는 그 모델을 CPU로 돌린다. 모델·shape 오류(`UnexpectedModel`)와 CPU 실패는 다시 하지 않는다. CPU 세션은 처음 필요할 때 만들고, 모델 바이트는 그때까지 메모리에 둔다(파일을 다시 열지 않음).
- **표시:** 다시 분석한 항목에는 note "the GPU failed; analyzed again on the CPU"(`Note::AnalyzedAgainOnCpu`)가 붙는다. CLI는 시작할 때 모델마다 어디서 도는지("YOLO5Face-n runs on CoreML", 시작하지 못하면 "… runs on the CPU: it could not start on CoreML (이유)")를, 실행 중 옮겨 갔으면 끝에 "… moved to the CPU for the rest of the run: CoreML failed (이유)"를 stderr에 쓴다.
- **어디서 돌았는지는 review key에 들어가지 않는다.** 결과가 허용오차 안에서 같기 때문이다(fixture 시험이 실행 위치마다 확인). `--gpu off`로 분석한 세션도 기본값으로 `review`할 수 있다.
- **CoreML 컴파일 캐시:** `<캐시 폴더>/coreml/<ORT 빌드 정보 SHA-256 앞 16자>/<모델 SHA-256>`에 둔다. 캐시 폴더는 macOS `~/Library/Caches/CloakFrame`(`app_dirs::cache_dir`)이다. CoreML은 그래프 구조가 같으면 가중치나 분할한 런타임이 바뀌어도 컴파일된 모델을 다시 쓰므로 런타임 빌드와 모델마다 폴더를 나눈다. initializer를 바꾼 그래프(번호판)는 구조가 같아 원래 그래프의 컴파일 결과를 잘못 쓸 수 있으므로 캐시하지 않는다. 이전 폴더 정리는 4단계(모델 캐시)에서 정한다.
- **확인하지 못한 것:** Windows DirectML은 GitHub runner에 GPU가 없어, 그곳에서는 시작할 때 CPU로 가도 시험이 통과한다(macOS만 가속기에 남는지 확인한다). 번호판을 DirectML에서 후보 0개 입력으로 돌리는 확인(M0에서 남김)과 실기기 GPU 리셋은 Windows GPU 장치가 생기면 한다.

## CoreML과 CPU

fixture 18건(6장 × score 0.1·0.25·0.5)을 엔진 디코드 경로로 비교했다. 기준은 1.12.1 CPU 결과다.

| 모델 | CPU | CoreML |
| --- | --- | --- |
| YOLO5Face-n | 최소 IoU 0.999995, score 차 6.6e-7 | 최소 IoU 0.99999, score 차 8.0e-7 |
| YuNet | 최소 IoU 0.99999, score 차 7.5e-7 | 최소 IoU 0.99999, score 차 3.9e-7 |
| 번호판 | 최소 IoU 0.9999966, score 차 5.4e-7 | 후보 0개인 6건 실패, 나머지 12건 같음 |

지연(release, 전처리 포함, 40회 p50)과 세션 생성 시간:

| 모델 | CPU | CoreML | 세션: CPU / CoreML 첫 컴파일 / 캐시 |
| --- | --- | --- | --- |
| YOLO5Face-n, astronaut 512×512 | 27.9 ms | 9.9 ms | 57 / 415 / 149 ms |
| YuNet, astronaut 512×512 | 11.8 ms | 4.6 ms | 13 / 135 / 58 ms |
| YuNet, crowd 640×640 | 8.3 ms | 2.9 ms | |

CoreML 세션의 첫 실행은 캐시와 관계없이 약 30–60 ms 더 걸린다.

## 시험

| 시험 | 확인하는 것 |
| --- | --- |
| `detect::fallback` 단위 4건 | 가속기에서 실패한 그림을 CPU로 다시 분석하고 그 뒤로 CPU에 남음, 시작하지 못한 모델은 CPU로 시작, 모델 오류·CPU 실패는 다시 하지 않음, 플랫폼별 실행 위치 |
| `detect_fixtures.rs` `the_models_match_the_fixtures_where_a_run_puts_them` | 실행이 정하는 위치에서 세 모델 54건이 1.12.1과 같음. macOS는 얼굴 모델이 CoreML에 남아야 함 |
| `detect_fixtures.rs` `an_image_that_fails_on_coreml_is_analyzed_again_on_the_cpu`(macOS) | 실제 가속기 오류: 번호판을 CoreML에 강제로 두고 후보 없는 사진에서 CPU로 다시 분석, 결과가 CPU와 같고 이후 CPU에 남음 |
| `detect_fixtures.rs` `coreml_caches_compiled_models_per_runtime_build_and_model`(macOS) | 캐시 폴더 구조, initializer를 바꾼 그래프는 캐시하지 않음 |
| `photo_pipeline.rs` `a_photo_analyzed_again_on_the_cpu_says_so` | note, 정상 게시 |
| `cli_photo.rs` `the_models_run_where_the_gpu_setting_says` | `--gpu on|off`, 실행 위치 출력, macOS CoreML 캐시가 앱 캐시 폴더에 생김, 실행 위치가 review를 막지 않음 |

변형 확인: CoreML의 `MLProgram`을 빼면 accelerator fixture 시험이, 번호판을 CoreML에 두면 단위·fixture·CLI 시험이 실패한다.

# M5 0단계: core 추적

작성 2026-10-22. [Rust 포팅 계획](RUST_PORT_PLAN.md) 8장 M5 0단계(`m5-track`)의 기록이다. 코드는 core `tracking`(`scene`·`tracker`·`assign`·`merge`·`post`·`stream`), 시험은 `crates/cloakframe-core/tests/tracking_fixtures.rs`와 각 모듈의 단위 시험이다. 파일·FFmpeg·engine은 쓰지 않는다. engine 연결(레코드 파일, 종류별 분리, 취소)은 1단계다.

## 구성

1.12.1 `Tracking.cpp`·`SceneCut.cpp`를 옮기되, 모든 단계를 프레임 순서로 하나씩 받게 했다. 1단계는 레코드 파일을 앞·뒤로 읽으며 같은 함수를 부른다.

| 모듈 | 하는 일 | 들고 있는 것 |
| --- | --- | --- |
| `scene` | `Thumbnail`(48×27 회색, OpenCV BT.601 정수 luma와 `INTER_AREA` 면적 평균)과 `SceneCutDetector`, `SceneCuts` | 직전 썸네일, 최근 차이 8개, 확인 중인 후보 |
| `tracker` | 종류 하나의 ByteTrack. `update(frame, cut, detections)`가 어느 감지가 어느 track을 잇거나 시작했는지(`Assignment`)와 끝난 track을 돌려줌 | 살아 있는 track의 마지막 box·속도·시각 |
| `assign` | 프레임 안의 할당: IoU 합이 가장 큰 짝(Hungarian, 겹치는 무리마다 따로) | 없음 |
| `merge` | 양방향 병합. `Merger`가 앞·뒤 할당을 앞 방향 프레임 순서로 받아, 뒤 track이 끝나는 프레임(뒤 방향의 시작)에 합칠 앞 track을 정함. `Joins`가 프레임마다 최종 track의 box를 냄 | 진행 중인 뒤 track과 앞 track의 겹침 합계, 결정(뒤 track 수만큼) |
| `post` | 1.12.1 그대로의 전체 track 후처리(보간·평활화·끝 연장·장면 전환 경계). fixture 시험과 아래 `stream`의 기준 | 전체 track |
| `stream` | `Summaries`(track마다 처음·끝 프레임과 감지·강한 감지 수, 한 번 앞서 훑어 얻음)와 `PostProcessor`(프레임마다 최종 box를 받아 처리된 box를 프레임 순서로 돌려줌) | track마다 마지막 몇 box, 다음 장면 전환 앞뒤의 끝·시작, 아직 확정되지 않은 프레임의 출력 |

1단계의 흐름: 분석 1패스가 감지와 장면 전환을 레코드에 쓰고 → 앞 방향 `ByteTracker` → 뒤 방향 `ByteTracker`(장면 전환은 `reversed`) → `Merger`로 앞에서부터 → `Joins` + `Summaries` → `Joins` + `PostProcessor`. 종류는 추적기를 따로 두고 track 번호는 `ByteTracker::new(config, first_id)`로 겹치지 않게 한다(P-2).

## 1.12.1과 다른 점

- **보간 한도 1초**(M5 결정, [기록](M5_PLAN.md)): `TrackingWindows::default().max_interpolation`이 1.0초다. 30 fps에서 `PostProcessConfig::max_interpolation_gap` 기본값이 20 → 30. 다른 창은 1.12.1과 같고, `apply`는 유리수 속도로 받는다(23.976·29.97은 24·30과 같은 프레임 수).
- **평활화(P-6 a):** 보간 box도 관측 box처럼 평균과 원래 box를 함께 덮는 box로 바꾸고, 4분의 1 넘게 커지면 원래 box를 둔다. 1.12.1은 보간 box를 평균으로 바꿔 줄일 수 있었다. P-6 fixture의 입력으로 확인했다(프레임 10·11의 (80, 80, 80, 80)이 1.12.1에서는 (88, 88, 64, 64)·(84, 84, 72, 72)).
- **장면 전환(P-6 b):** 후보를 확인하는 2프레임 동안 들어온 새 spike도 평가한다. 새 spike가 오면 앞 후보를 확정하고 새 후보를 세운다. 1장면 삽입은 [10, 11], 1프레임씩 두 장면은 [10, 11, 12]. 같은 장면으로 돌아오는 섬광은 그대로 무시한다.
- **할당:** greedy 대신 IoU 합이 가장 큰 할당이다. 두 track 사이의 감지를 한 track이 가져가 다른 track이 새로 시작되던 경우를 시험으로 고정했다.
- **양방향 병합의 순서:** 1.12.1은 뒤 track을 번호 순으로 하나씩 합치면서, 앞서 합친 뒤 track의 box까지 포함한 앞 track과 비교했고 짝이 없는 뒤 track에 다음 뒤 track이 합쳐질 수 있었다. 여기서는 모든 뒤 track을 앞 방향 track하고만 비교한다. 결과가 순서에 기대지 않고 뒤 track이 끝나는 프레임에 정할 수 있어서다. 같은 앞 track에 둘 이상이 합쳐지면 앞 track에 없는 프레임은 번호가 작은 뒤 track의 box를 쓴다(1.12.1과 같음). fixture의 양방향 시험은 모두 같은 결과다.
- **내부 상한 없음:** 1.12.1의 프레임당 1,024개·track 65,536개 같은 상한은 옮기지 않았다(계획 0.1, `testTrackingSafetyLimitsRejectExcessiveData`는 dropped).
- **취소:** 1.12.1은 continue guard를 각 함수에 넘겼다. 여기서는 호출자가 프레임 사이에서 멈추면 된다. 1단계 engine이 프레임마다 취소 토큰을 본다.
- **자세(roll):** `hasPose`와 각도 대신 `Option<f32>`(60° 안의 유한한 값일 때만 `Some`)다. 1.12.1의 `kTwoPi`(6.2831853072F)는 f32 `TAU`와 같다.

## 메모리

`PostProcessor`는 프레임 p의 출력을 더 받을 수 없을 때 내보낸다: 진행 중인 track이 더 받을 수 있는 가장 이른 프레임(보간될 수 있는 동안은 마지막 감지 다음, 아니면 다음 입력 프레임), 아직 시작하지 않은 track의 첫 프레임에서 끝 연장만큼 앞, 결정하지 않은 다음 장면 전환에서 경계 거리만큼 앞, 장면 전환 쪽에서 당겨 온 시작. 장면 전환은 그 앞에서 끝나는 track이 모두 끝나고 경계 거리 안에서 시작할 수 있는 track이 모두 첫 box를 냈을 때 정한다. 그래서 들고 있는 box는 진행 중인 track 수 × (평활화 반경 + 보간 창)과 장면 전환 주변의 끝·시작뿐이다.

- 시험 `what_is_held_does_not_grow_with_the_video`: 한 대상이 7프레임마다 한 번 놓치고 1,000프레임마다 장면 전환이 있는 영상에서, 2,000프레임과 60,000프레임의 최대 보유 box 수가 같다(100개 미만).
- 시험 `frames_in_order_give_what_whole_tracks_give`: 무작위 3,000가지(프레임 수, 장면 전환, track 0–6개와 구멍·크기 변화·roll, 설정 전부)에서 프레임 순서 결과가 `post::post_process_tracks`와 box·보고서까지 같다. 출력 버퍼에 이미 내보낸 프레임이 들어오면 debug 빌드에서 실패한다.
- track 수만큼 드는 것: `Summaries`와 `Merger`의 결정(track마다 수십 바이트). 1단계 할당 시험이 10분·60분 레코드로 잰다.

## 시험

- 1.12.1 추적 시험 47개(`tests/fixtures/legacy/tracking_tests`의 fixture 46개 + 내부 상한 시험):
  - normative 28개 중 26개는 기록된 호출을 모두 다시 돌려 출력을 값까지 비교(`normative_tests_give_the_recorded_results`): `buildTracks`·`buildBidirectionalTracks`·`interpolateGaps`·`smoothTrack`·`extendTrackEnds`·`postProcessTracks`·`SceneCutDetector`. 허용오차는 box 1e-4(상대), score 1e-6.
  - normative 나머지 둘: `testSceneCutsContainer`는 `scene` 단위 시험, `testManualVideoTrackInterpolationAndBounds`는 수동 track이라 3단계(영상 검토).
  - intent 16개 중 15개는 C++ 시험의 assert를 Rust로 옮김. `testTrackingCancellationGuard`는 1단계.
  - changed 2개: `testTrackingWindowsFollowTheFrameRate`(보간 창, `tracking::tests`), `testSceneCutDetectorReportsSingleFrameShotAsOneBoundary`(P-6 b, `scene` 단위).
  - dropped 1개: 내부 상한.
  - `every_legacy_tracking_test_is_accounted_for`가 index.json의 모든 추적 시험이 위 중 하나에 있는지 본다.
- P-6 a(`smoothing_never_shrinks_a_box`, P-6 fixture 입력), P-6 b(`scene` 단위, expected.json 네 경우), P-2(`faces_and_plates_never_share_a_track`: IoU 0.6으로 겹친 얼굴·번호판이 종류별 추적기에서 track 둘).
- 단위: Hungarian이 5×5 이하 무작위 500가지에서 전수 탐색과 같은 합, 썸네일 면적 평균과 luma, 추적 창, 병합에서 앞 방향 box가 우선.
- core 시험 91개(단위 67, `tracking_fixtures.rs` 20, `serde_forms.rs` 4), fmt·clippy 두 조합 통과.

## mutation

작업 트리에서 한 곳씩 바꾸고 core 단위 시험과 `tracking_fixtures.rs`를 돌린 뒤 원본을 `cp`로 되돌렸다(끝에 원본과 `cmp`).

| 바꾼 것 | 결과 |
| --- | --- |
| 약한 감지 단계 끔 | 잡힘(6개) |
| 장면 전환이 track을 끝내지 않음 | 잡힘(fixture 재생) |
| 움직이는 track도 정지 track의 coast 한도 | 잡힘 |
| 속도 갱신 없음 | 잡힘 |
| 크기 변화 gating 없음 | 잡힘 |
| Hungarian 대신 greedy | 처음에는 놓침 → `every_track_that_can_continue_does`를 넣어 잡힘 |
| 병합에서 뒤 방향 box가 앞 방향 box를 덮음 | 처음에는 놓침 → merge 단위 시험을 넣어 잡힘 |
| 합칠 앞 track 없음 | 잡힘 |
| P-6 a를 1.12.1로 되돌림 | 처음에는 놓침(1.12.1 시험의 보간 box는 커지는 쪽이라 원래 box가 남음) → P-6 fixture 시험을 넣어 잡힘 |
| 크기·속도 gating된 gap도 선형 보간 | 잡힘 |
| 경계 거리까지 다리 놓기 | 잡힘 |
| P-6 b를 1.12.1로 되돌림 | 잡힘 |
| 섬광도 장면 전환 | 잡힘 |
| `stream`: 다리 box·당겨 온 시작·장면 전환 너머의 앞 연장·뒤 연장·보간·짧은 track 평활화를 각각 빼거나 바꿈 | 모두 잡힘(프레임 순서 비교) |
| `stream`: 장면 전환·시작 전 track의 준비 한계를 늦춤 | 잡힘(이미 내보낸 프레임에 box) |
| `stream`: 다리 후보의 하한에서 장면 전환을 뺌 | 같은 결과(등록부에 장면 전환 앞의 시작이 남지 않음) |

## 남은 것

- 완료: PR #49(2026-10-22, main `e9ba8a9`). 3개 OS CI와 탐지 벤치마크(Linux, 기본 설정 얼굴 재현율 87.9%·1%p 게이트 통과)가 통과했다. Windows·Linux에서는 이 PR이 core 추적 시험의 첫 실행이었다.
- 1단계(`m5-analysis`): 분석 디코드·감지·썸네일, 레코드 파일, 종류별 추적기와 위 흐름, 취소, 10분·60분 할당 시험, 영상 길이로 바꾼 추적 창(VFR은 원본 프레임 단위, 원본 속도로 환산).
- 미해결 gap의 근거 형식: `Problem::UnresolvedGap`은 track 번호를 갖지만 보고서는 1.12.1처럼 여러 track의 구간을 합친다. 검토 화면에 track별로 보일지 구간으로 보일지 1·3단계에서 정한다.
- 수동 track(keyframe 보간)과 그 시험: 3단계.

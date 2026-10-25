# M5 5단계: 완료 확인

작성 2026-10-25. [Rust 포팅 계획](RUST_PORT_PLAN.md) 8장 M5 5단계(`m5-verify`)의 기록이다. 장치는 Apple M4(10코어, 24 GB), macOS, release, 고정 FFmpeg 9.0.2·ORT 1.30.0. 도구는 `tools/benchmark/video_speed.sh`·`legacy/legacy_video_speed.cpp`, CI는 `.github/workflows/video-throughput.yml`이다.

## 결정(2026-10-25, 메인테이너가 추천대로)

시작할 때 실험하고 물었다([실험](#시작할-때의-실험)).

- **1시간 영상:** 이미 쓰던 Commons 영상 4개 전체(인터뷰 3:55·도로 1:20·거리 4:33·군중 3:25, 13.2분)를 1080p30 H.264·AAC 48 kHz로 맞춰 다시 인코드하고(군중 608×1080은 좌우를 채움, 소리 없는 영상은 무음), concat으로 반복해 stream copy로 10분과 60분을 자른다. 기본 설정(CoreML, 타일 켬, VideoToolbox)으로 CLI `run`을 끝까지 돌리고 CLI와 FFmpeg 자식의 RSS를 5초마다 따로 표본한다. 저장소에는 넣지 않는다.
- **처리량 입력·설정:** 1080p는 영상 벤치마크 세 클립(해시 확인), 4K는 거리 클립을 lanczos로 3840×2160으로 키운 것(쓸 수 있는 라이선스의 4K 원본을 받지 않음). 비교 설정 둘: 두 구현 모두 기본 GPU(CoreML·VideoToolbox), 두 구현 모두 CPU·libx264 `medium`. 엔진은 타일 끔으로 비교하고 타일 켬(기본)은 따로. 설정마다 3번, 전체 시간(프로세스 시작부터 끝)과 1패스를 따로 적는다.
- **1.12.1 실행:** v1.12.1 트리를 Homebrew 의존성으로 빌드하고 `processVideo`를 2패스까지 돌리는 하네스(`legacy_video_speed.cpp`). 같은 고정 FFmpeg를 쓴다. 릴리스 패키지와 다른 점은 ORT 판(Homebrew 1.31, 릴리스 1.29)과 OpenCV 빌드다.
- **spin의 release 측정:** 1단계에서 넘어온 3개 OS release 측정은 새 workflow `video-throughput.yml`(1패스·감지 경로가 바뀌는 PR과 수동 실행)이 맡고, 이 PR에서 처음 돈다.

처리량을 잰 뒤 물었다([찾은 것](#찾은-것-gpu-1패스의-직렬-전처리)):

- **GPU 처리량:** 기본 GPU 설정에서 엔진이 1.12.1보다 2–15% 느렸던 것은 이 단계에서 고친다. 모델 입력 만들기를 세션 실행과 겹치게 한다(별도 커밋, 감지 결과는 바이트까지 같아야 함).
- **P-4 비용:** 4K 출력의 P-4 전체 디코드(2.4–2.5초)는 M4 결정(손상된 출력을 게시하지 않음)의 비용으로 기록만 한다.

## 추적 fixture

1.12.1 추적 시험 47개(`tests/fixtures/legacy/tracking_tests`, 상태는 `index.json`)가 모두 새 시험에 대응한다. `every_legacy_tracking_test_is_accounted_for`가 index의 추적 시험이 아래 목록 밖에 없는지 본다.

| 상태 | 수 | 확인 |
| --- | --- | --- |
| normative | 28 | 27개는 기록된 호출을 값까지 다시 돌림(`tracking_fixtures.rs` `normative_tests_give_the_recorded_results`: `buildTracks`·`buildBidirectionalTracks`·`interpolateGaps`·`smoothTrack`·`extendTrackEnds`·`postProcessTracks`·`SceneCutDetector`, 수동 track의 `manualTrackRectAtFrame`·`materializeManualVideoTrack`). `testSceneCutsContainer`는 core `scene_cuts_container` |
| intent | 16 | 15개는 assert를 옮긴 `tracking_fixtures.rs` 시험(아래), `testTrackingCancellationGuard`는 engine `video::tracking` `tracking_stops_when_cancelled`와 `video_analysis.rs` `cancelling_the_first_pass_stops_it_and_removes_its_records` |
| changed | 2 | `testTrackingWindowsFollowTheFrameRate`(core `windows_follow_the_frame_rate`: 2초 창, 23.976·29.97은 공칭과 같은 프레임 수), `testSceneCutDetectorReportsSingleFrameShotAsOneBoundary`(core `hard_cuts_short_shots_and_flashes`, P-6 b) |
| dropped | 1 | `testTrackingSafetyLimitsRejectExcessiveData`: 내부 데이터 상한을 두지 않음(0.1 결정) |

intent 15개: `weak_detections_extend_but_never_start_tracks`, `coasting_on_weak_detections_ends`, `a_moving_track_coasts_longer_than_a_still_one`, `a_confirmed_weak_track_is_masked_whole`, `kept_weak_tracks_are_marked_low_confidence`, `a_short_strong_burst_is_kept`, `a_confident_single_detection_is_kept`, `crossing_subjects_keep_two_tracks`, `both_passes_over_one_subject_give_one_track`, `the_backward_pass_recovers_a_weak_start`, `smoothing_steadies_the_roll`, `smoothing_does_not_inflate_moving_boxes`, `smoothing_neither_inflates_nor_shrinks_interpolated_boxes`, `a_jump_in_size_starts_a_new_track`, `gradual_growth_keeps_one_track`.

core 추적을 레코드로 돌린 engine 결과도 같다: `video::tracking` `tracking_from_the_record_gives_what_whole_tracks_give`(무작위 영상 300개, 얼굴·번호판·장면 전환·세 속도, 영역·track·낮은 신뢰·gap까지)와 core `frames_in_order_give_what_whole_tracks_give`(무작위 3,000가지).

**고친 것:** index의 `testRetainedLowConfidenceTracksAreMarkedNotDropped` 메모가 "약한 track은 CoverageEvidence의 '약한 후보'로 남아 게시를 막는다"였다. M5 계획 결정(낮은 신뢰 track은 1.12.1처럼 통째로 가리고 표시만, 게시를 막지 않음)과 어긋나 메모만 결정대로 고쳤다. 시험과 코드는 이미 결정대로다.

## 시험 표

`tests/fixtures/defects`의 확인 항목별로 그것을 보는 시험이다. 모두 3개 OS CI(`ci.yml`, `--all-features`)에서 돈다.

| 항목 | 확인 | 시험 |
| --- | --- | --- |
| P-1 영상 | 30–64 놓침, 보간 1초 창 → gap 30–64·`NeedsReview`·출력 없음 | `video_analysis.rs` `an_unfilled_gap_holds_the_video_for_review_and_publishes_nothing` |
| P-1 영상 | 상한으로 잘린 후보 → `NeedsReview` | `video_analysis.rs` `capped_candidates_hold_the_video_for_review` |
| P-1 영상 | 기본 창(2초)은 같은 놓침을 보간해 모든 프레임을 가리고 게시 | `video_analysis.rs` `the_default_windows_mask_a_subject_missed_for_over_a_second` |
| P-1 영상 | 경고 포함 내보내기(gap 밖만 가림)·익명화 없이 내보내기, 정상 완료로 세지 않음 | `video_analysis.rs` `exports_past_a_gap_mask_what_was_tracked_or_nothing`, core `item` `explicit_export_with_warnings_publishes_but_is_not_a_normal_completion`·`export_without_anonymization_is_explicit_and_not_a_normal_completion` |
| P-1 영상 | gap은 수동 track이 모든 프레임을 덮을 때만 해결(CLI 검토) | `cli_video.rs` `a_manual_track_resolves_a_gap_only_over_all_of_its_frames` |
| P-1 공통 | 검토 끔에서도 미해결 문제는 게시 허가 없음 | core `item` `unresolved_problems_hold_the_item_in_review_with_review_off`, `cli.rs` `unresolved_problems_publish_nothing_until_resolved_or_explicitly_exported` |
| P-1 사진 | 상한 후보 | `photo_pipeline.rs` `capped_detections_hold_the_photo_until_it_is_exported_with_warnings`, `p1_photo.rs` 2건(M2) |
| P-2 1 | IoU 0.6으로 겹친 얼굴·번호판: NMS가 지우지 않음 | core `detection` `nms_keeps_overlapping_boxes_of_different_kinds`, engine `detect::models` `combined_results_keep_every_kind_and_each_models_limit` |
| P-2 1 | 같은 경우 track 2개 | `tracking_fixtures.rs` `faces_and_plates_never_share_a_track`, `video::tracking` `tracking_from_the_record_gives_what_whole_tracks_give`(종류별 추적기) |
| P-2 2 | 얼굴 0.5·번호판 0.3에서 0.4 후보 | core `detection` `score_thresholds_apply_per_kind` |
| P-2 3 | 결과 JSON의 모든 Detection·track·gap에 kind | `cli_photo.rs` `plates_join_the_faces_when_turned_on`, `session` `a_video_is_stored_with_the_kinds_of_its_tracks_and_gaps`, `cli_video.rs` `a_video_held_for_review_is_resumed_from_its_session`(세션의 track 종류) |
| P-6 a | 기록된 box를 20프레임까지 보간 후 반경 2 평활화: 모든 프레임에서 평활화 후 ⊇ 전 | `tracking_fixtures.rs` `smoothing_never_shrinks_a_box`, `smoothing_neither_inflates_nor_shrinks_interpolated_boxes` |
| P-6 b | A×10·B×1·C×10 → [10, 11], 1프레임씩 두 장면 → [10, 11, 12], B×2 → [10, 12], 섬광 → [] | core `scene` `hard_cuts_short_shots_and_flashes` |

## 처리량(9.3)

`tools/benchmark/video_speed.sh`, 설정마다 3번의 중앙값(괄호는 최대). 전체는 프로세스 시작부터 끝까지(모델 로드·조사·1패스·추적·2패스·출력 검사·게시), 1패스는 감지와 추적이다. 배속은 영상 길이 ÷ 전체. 아래는 [고친 뒤](#찾은-것-gpu-1패스의-직렬-전처리)이고, 1.12.1도 같은 시간대에 다시 쟀다.

| 입력 | 설정 | 1.12.1 전체 | 엔진 전체 | 1.12.1 1패스 | 엔진 1패스 | 엔진 배속 |
| --- | --- | --- | --- | --- | --- | --- |
| 거리 1080p60 19초 | GPU | 27.3초(28.3) | 25.6초(25.7) | 52 fps | 62 fps | 0.74 |
| | CPU | 54.5초(58.5) | 44.0초(45.8) | 29 fps | 42 fps | 0.43 |
| | 타일 켬 | | 63.9초(64.5) | | 20 fps | 0.30 |
| 도로 1080p25 20.4초 | GPU | 12.5초(12.6) | 10.8초(11.1) | 52 fps | 66 fps | 1.89 |
| | CPU | 26.4초(26.6) | 20.0초(20.0) | 29 fps | 47 fps | 1.02 |
| | 타일 켬 | | 28.1초(28.8) | | 20 fps | 0.73 |
| 군중 608×1080 60 12.5초 | GPU | 18.2초(18.6) | 14.3초(14.7) | 46 fps | 59 fps | 0.87 |
| | CPU | 28.9초(29.3) | 18.4초(18.6) | 29 fps | 46 fps | 0.68 |
| | 타일 켬 | | 39.9초(41.5) | | 19 fps | 0.31 |
| 거리 4K60(업스케일) 19초 | GPU | 39.0초(40.8) | 41.5초(46.8) | 57 fps | 62 fps | 0.46 |
| | CPU | 143.4초(147.3) | 115.3초(115.4) | 17 fps | 35 fps | 0.16 |
| | 타일 켬 | | 75.1초(75.9) | | 22 fps | 0.25 |

GPU는 두 구현 모두 CoreML과 VideoToolbox, CPU는 CPU 세션과 libx264 `medium`이다. 엔진의 GPU·CPU는 타일 끔(1.12.1과 같은 설정), 타일 켬은 기본 설정(CoreML·VideoToolbox)이다.

- **1080p:** 세 클립 모두 GPU·CPU에서 엔진이 1.12.1보다 빠르다(GPU 6–21%, CPU 19–36%).
- **4K:** 1패스는 엔진이 빠르지만, 전체는 GPU에서 6% 느리다. 차이는 P-4 출력 검사의 전체 디코드다(4K 출력 1,140프레임에 2.4–2.5초, 1.12.1에는 없음, 결정대로 기록만 함). 입력 SHA-256(0.2초)은 2패스와 나란히 돈다. CPU에서는 엔진이 20% 빠르다.
- **흔들림:** CoreML 값은 기기 상태에 따라 같은 실행도 10–30% 흔들렸다(1.12.1 군중 13.4–17.6초, 엔진 4K 40.3–46.8초). 그래서 두 구현을 같은 실행 안에서 번갈아 쟀다.
- **타일 켬:** 기본 설정은 타일 끔보다 2.5–2.8배 느리다(4K는 1.8배). 고치기 전보다 약 20% 빨라졌다.
- 9.3이 요구한 Windows DML·Linux x64 장치의 p50/p95는 M8 고정 장치에서 잰다. 이 Mac은 장치 하나이고 반복이 3번뿐이라 중앙값과 최대만 적었다.

### 찾은 것: GPU 1패스의 직렬 전처리

고치기 전 첫 측정에서는 GPU 설정의 엔진이 1.12.1보다 느렸다. 전체 기준으로 거리 31.3초 대 28.2초(+11%), 도로 +6%, 군중 +2%, 4K +15%였고, CPU 설정은 지금처럼 빨랐다.

- **배제한 것:** 두 구현의 세션 설정은 같다(CoreML MLProgram, 연산 장치 ALL, 최적화 EXTENDED, intra 스레드 1, 입력 [1, 3, 640, 640]). ORT 판(엔진을 Homebrew 1.31로 돌림)과 `AutoDevicePolicy::PreferCPU`도 차이가 없었다.
- **잰 것:** `sample`과 스크래치 복사본의 시간 측정으로 거리 클립의 프레임당 시간을 나눴다.
  - CoreML 실행: 17–19 ms
  - Rust 전처리(`resize_linear`와 f32 평면 변환): 2.5 ms
  - 장면 전환 썸네일: 0.7 ms
  - 스레드 넘김: 0.3 ms
- **원인:** 1패스는 프레임마다 썸네일 → 추론 스레드에서 전처리 → 세션 실행을 차례로 했다. 장치를 기다리는 세션 실행 동안 CPU가 다음 프레임에 할 일이 없었다. 1.12.1은 같은 단계를 OpenCV NEON resize와 `blobFromImage`로 해서 짧았다.
- **고친 것:** `Detector`에 세션 없이 하는 일인 `preparer`와 `detect_prepared`를 기본 구현과 함께 더했다. 세 감지기·조합·CPU 폴백이 이것을 넘긴다.
  - `DetectorHandle`은 전처리 스레드(`cloakframe-prepare`)가 다음 이미지의 입력을 미리 만들어 둔다(세션이 N을 도는 동안 N+1이 채널에, N+2가 보내기 대기). 세션은 여전히 추론 스레드 하나만 가진다(D8).
  - 영상 1패스는 `tiles::submit_views`로 다음 프레임을 먼저 넣은 뒤 앞 프레임의 결과를 받는다.
  - 사진은 `detect_views` 그대로다. worker 여럿이 요청을 쌓으므로 전처리 스레드만으로 겹친다.
- **확인:** 영상 벤치마크의 기록된 감지 여섯 개(세 클립 × 타일 끔·켬, CPU)를 새 빌드로 다시 만들어 바이트까지 같았다. 단위 시험 `inputs_are_made_on_their_own_thread_and_queued_images_keep_their_order`가 입력을 만든 스레드, 넣은 순서대로의 결과, 감지기 panic이면 Unavailable을 본다.

## 1시간 영상의 RAM

CLI `run`, 기본 설정(YOLO5Face-n CoreML, 타일 켬, VideoToolbox), `--review off`. RSS는 CLI 프로세스와 그 자식(FFmpeg)을 5초마다 따로 표본했다. 아래는 [ort 누수](#찾은-것-ort-rc14의-출력-누수)를 고친 뒤이고, 위의 전처리 겹침도 들어 있다.

| 영상 | 프레임 | track | 시간 | CLI RSS 최대 | 1패스 중 CLI RSS | FFmpeg 자식 최대 |
| --- | --- | --- | --- | --- | --- | --- |
| 10분 | 17,999 | 2,859 | 1,312초* | 161 MiB | 115–125 MiB, 평탄 | 876 MiB |
| 60분 | 107,987 | 19,808 | 5,932초 | 171 MiB | 156 MiB, 평탄 | 1,252 MiB |

\* 10분 실행의 2패스 시간대에 VideoToolbox 재현을 함께 돌려 시간은 비교에 쓰지 않는다.

- **RAM:** 1패스의 CLI RSS는 60분 내내 156 MiB에서 움직이지 않았다. 10분과 60분의 최대 차이 10 MiB는 track 요약(track마다 약 180 B, 2,859 → 19,808개)과 2패스에서 프로세스가 잡은 버퍼 수준이다. 영상 길이에 비례하지 않는다는 완료 기준을 만족한다. CI는 1단계 할당 시험(`tracking_an_hour_peaks_where_ten_minutes_do`, 합성 레코드 10분 241 KB → 60분 363 KB)이 지킨다.
- **FFmpeg 자식:** 2패스의 디코드·인코드 FFmpeg가 차지하고 길이와 무관하다. 1패스는 분석 크기 디코드 하나로 약 230 MiB다.
- **시간:** 60분은 1시간 39분(실시간의 0.61배)이다. 고치기 전 첫 실행은 9,292초였다. 그 실행은 하드웨어 실패로 2패스를 소프트웨어로 다시 했고, 전체 시험과 겹친 시간대가 있었다.
- **고치기 전(rc.14):** 10분 최대 191 MiB, 60분 최대 221 MiB. 60분의 1패스 동안 94 → 207 MiB로 꾸준히 늘었다(아래).

### 찾은 것: ort rc.14의 출력 누수

첫 실행(ort 2.0.0-rc.14)에서 60분 영상의 1패스 동안 CLI RSS가 94 MiB에서 207 MiB로 꾸준히 늘었다. 10분 실행은 160 → 184 MiB였다. 할당 시험이 재는 추적·영역 읽기 밖에서 프레임 수에 비례하는 무언가가 있었다.

- **좁히기:** `video_benchmark` 예제(같은 `first_pass`)를 10분 영상으로 돌리며 `vmmap --summary`를 여러 번 봤다. 타일 끔에서는 4분 동안 그대로였고, 타일 켬에서는 malloc 영역의 살아 있는 할당이 8분에 105 → 121 MB(블록 18만 → 26만 개)로 늘었다.
- **스택:** `MallocStackLogging=lite`와 `leaks`로 200초 뒤 누수 31,412개(2.85 MB)를 찾았다. 모두 `ort::value::Value::from_session_output_ptr` → `OrtApis::GetTypeInfo`가 만든 `OrtTypeInfo`(80 B + 형태 정보 192 B)였다. 세션 실행마다 하나씩이라 수가 실행 수(타일 켬은 프레임당 3)와 같다.
- **원인:** rc.14는 텐서 출력의 type info를 받기만 하고 `ReleaseTypeInfo`를 부르지 않는다. 실행 한 번에 272 B이고, 타일 켬 1시간(약 32만 번)이면 약 88 MB라 관측과 맞다. 사진도 실행마다 같은 양이 샌다.
- **고친 것:** upstream rc.15(2026-10-19)의 `984a2ccd` "only get type info if we need it"가 이 경로를 고쳤다. 릴리스 노트도 여러 누수·메모리 안전 문제 때문에 올릴 것을 권한다. 공개 2주 미만이지만 메인테이너 결정으로 올렸다(M5 완료 기준을 막는 결함).
  - rc.14를 쓴 이유를 다시 확인했다. M0에서 검증한 당시 최신 판이었을 뿐이다. 실제 제약은 Windows ML의 ORT 1.27.1이 정하는 `api-27`이고, rc.15에도 그대로 있다.
  - 우리가 쓰는 API는 rc.15의 깨지는 변경(`run_async`, `ValueRefMut`, `InMemorySession` 등)과 겹치지 않아 코드는 그대로다. `spikes/ort`도 함께 올렸다.
  - rc.15로 같은 측정을 다시 하자 `leaks`가 0개였다. 영상 벤치마크의 기록된 감지 여섯 개가 rc.15에서도 바이트까지 같았고, 전체 시험(감지 fixture·CoreML 세션 포함)이 통과했다.

### 찾은 것: 60분 출력의 하드웨어 인코더 실패

첫 60분 실행의 결과 줄에 "the hardware encoder failed; encoded again in software"가 나왔다. 소프트웨어로 다시 만든 출력은 P-4를 통과해 게시됐다. 10분 실행은 하드웨어로 끝났다.

- **기록이 없음:** `encode_with_fallback`이 하드웨어 실패의 이유를 버리고 note만 남겨서, 인코드가 실패했는지 P-4가 실패했는지 알 수 없다.
- **FFmpeg 재현(결론 없음):** 같은 1시간 영상을 rgb24로 디코드해 엔진 인자로 `h264_videotoolbox`에 넣었다.
  - 첫 번째(소리 복사와 `-t` 포함)는 종료 코드 0인데 출력이 107,981프레임이었다. pts가 두 칸 건너뛰는 자리가 6곳이었고, 그 자리의 원본 프레임은 앞뒤와 다른 내용이었다.
  - 두 번째(소리 없이)는 빈틈이 없었지만 108,002프레임이었다. 디코드 쪽이 CFR로 프레임을 복제해 엔진처럼 원본 프레임을 하나씩 넘기지 못했고, `-copyts`도 영상에 걸렸다. 이 재현은 엔진 경로와 달라 원인을 가리지 못한다.
- **겹친 작업:** 첫 60분 실행의 2패스 시간대에 전체 시험(VideoToolbox를 쓰는 영상 시험 포함)을 함께 돌렸다.

- **다시 실행:** 다른 VideoToolbox 사용 없이 60분을 다시 돌리자 하드웨어로 끝까지 인코드해 P-4를 통과했다. 그래서 첫 실패는 동시에 돈 영상 시험과 장치를 나눠 쓴 탓일 가능성이 크다. 어느 쪽이든 하드웨어 출력이 게시 전에 막히고 소프트웨어로 다시 만든 것은 M4 결정(하드웨어 인코드 실패와 P-4 실패 모두 소프트웨어로 한 번 더) 그대로다.
- **남김:** 하드웨어 실패의 이유를 결과나 로그에 남기는 것. 지금은 원인을 사후에 알 수 없다. 감지기 폴백처럼 이유를 note에 담는 변경은 M6의 결과 표시와 함께 정한다.

## spin의 3개 OS release 측정

`video-throughput.yml`이 release 1패스(1080p 5초, YOLO5Face-n CPU, spin 끔/켬/끔/타일)를 3개 OS에서 출력한다. 이 Mac의 release는 1단계에서 spin 끔 24.4 fps·켬 8.6 fps였다([기록](M5_ANALYSIS.md#처리량)). PR #54의 실행에서 runner 값은 이렇다(fps).

| OS | spin 끔 | 켬 | 끔 | 타일 |
| --- | --- | --- | --- | --- |
| macOS runner | 12.1 | 12.6 | 9.7 | 3.6 |
| Linux | 17.8 | 17.3 | 17.9 | 6.3 |
| Windows | 14.4 | 15.9 | 15.7 | 5.5 |

runner(코어 3–4개)에서는 release에서도 spin 차이가 runner 편차 안이다. spin 이득은 이 Mac(10코어)처럼 코어가 많을 때 보인다. 같은 실행의 ci.yml debug 1패스(macOS 10.5/10.7/12.2/4.4, Linux 10.6/7.1/10.5/4.0, Windows 17.1/13.5/17.1/6.5)는 전처리 겹침 뒤 release와 비슷하거나 높아, 이 workflow를 따로 두는 이유는 [M0–M5 재검토](M0_M5_REVIEW.md) 12번에서 다시 본다.

## 범위 조정 점검

계획 8장의 점검 규칙(M5 끝에 누적 소요를 전체 예산과 비교)을 따른다.

| 단계 | M1 | M2 | M3 | M4 | M5 | 합계 |
| --- | --- | --- | --- | --- | --- | --- |
| 예산 | 3주 | 4주 | 3주 | 4주 | 5주 | 19주 |
| 실제 | 2일 | 2일 | 2일 | 2일 | 4일(10-22–25) | 달력으로 9일(10-17–25, 단계 사이 같은 날이 겹침) |

- **단계별:** 어느 단계도 예산의 125%를 넘지 않았다.
- **누적:** 9일은 약 1.3주다. 32주 예산에서 약 30.7주가 남고, 남은 M6–M8은 13주라 들어간다. 범위 조정 순서(① 영상 미리보기 연속 재생, ② 앱 재시작을 넘는 검토 저장·재개)는 미루지 않는다.
- **주의:** M6(UI, 7주)부터는 실기기 확인(IME·스크린리더·Windows·Linux 데스크톱)이 사람 손을 거친다. 지금까지의 속도로 남은 예산을 다시 나누지 않는다.

## M5 완료 기준

| 기준 | 확인 |
| --- | --- |
| 추적 fixture 통과 | [위](#추적-fixture) 47개 |
| 1시간 영상에서 RAM이 길이에 비례하지 않음 | `allocations.rs` `tracking_an_hour_peaks_where_ten_minutes_do`(CI, 합성 레코드 10분 241 KB → 60분 363 KB), 실제 영상 전체 실행에서 CLI RSS 최대 10분 161 MiB·60분 171 MiB, 1패스 평탄([위](#1시간-영상의-ram), ort rc.15로 올린 뒤) |
| 처리량(9.3) | 1080p는 GPU·CPU 모두 1.12.1보다 빠름, 4K GPU는 P-4만큼 6% 느림([위](#처리량93)) |
| 미해결 문제가 게시를 막음 | P-1 행 |
| P-1(영상)·P-2·P-6 | [시험 표](#시험-표) |
| 범위 조정 점검 | [위](#범위-조정-점검) |

로컬 전체 시험(`cargo test --workspace --locked --all-features`, macOS)은 490개 통과(4단계 489개 + 전처리 스레드 단위 시험), fmt·clippy 두 조합 통과. 소요는 2026-10-22–25, 4일(예산 5주).

## 시작할 때의 실험

- **1시간 영상:** 네 영상을 1080p30으로 맞추는 다시 인코드는 약 5분이었다(libx264 `fast` crf 20). 이어 붙여 10분(609 MB, 17,999프레임)과 60분(3.0 GB, 107,987프레임)으로 자르는 데는 4초 걸렸다. 이음매가 들어간 2분 구간(인터뷰 끝 → 도로 → 거리)을 기본 설정으로 돌리니 엔진이 받아 끝까지 게시했고(P-4 통과) 243초 걸렸다. 그래서 60분은 약 2시간, 타일 끔이면 약 45분으로 봤다.
- **1.12.1 하네스:** 도로 클립 GPU에서 1.12.1 13.9초, 엔진(타일 끔) 13.6초. CPU에서는 1.12.1 26.2초, 엔진 19.2초였다. 4K 업스케일 클립에서는 엔진이 느렸다(44.5초 대 38.6초). 이것이 위 [찾은 것](#찾은-것-gpu-1패스의-직렬-전처리)의 시작이다.
- **4K 원본:** 로컬에 쓸 수 있는 4K 원본이 없었다. 거리 클립의 Commons 원본 이름은 4K지만, 받아 둔 앞부분은 1080p였다.

## 재현

```bash
clips=$(python3 -I tools/benchmark/video_fetch.py "$CLOAKFRAME_FFMPEG" ~/.cache/cloakframe-video-benchmark)
tools/benchmark/video_speed.sh "$CLOAKFRAME_ORT_LIBRARY" "$CLOAKFRAME_TEST_ASSETS" "$CLOAKFRAME_FFMPEG" "$CLOAKFRAME_FFPROBE" "$clips" <작업 폴더> 3
```

1시간 영상은 M5 계획의 Commons 영상 네 개(받기 전 허락, 저장소에 넣지 않음)를 다음처럼 만든다. 소리 없는 영상은 `-f lavfi -i anullsrc=r=48000:cl=stereo -map 1:a -shortest`로 무음을 넣는다.

```bash
vf="scale=-2:1080:flags=lanczos,pad=1920:1080:(ow-iw)/2:0,fps=30,setparams=colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv,format=yuv420p"
"$CLOAKFRAME_FFMPEG" -i <영상> -map 0:v:0 -map 0:a:0 -vf "$vf" -c:v libx264 -preset fast -crf 20 -g 60 \
  -c:a aac -b:a 160k -ar 48000 -ac 2 -video_track_timescale 30000 <부분>.mp4
# list.txt: 인터뷰·도로·거리·군중 순서를 다섯 번
"$CLOAKFRAME_FFMPEG" -f concat -safe 0 -i list.txt -map 0 -c copy -t 3600 -movflags +faststart hour-60.mp4
```

RSS는 CLI 프로세스와 그 자식(FFmpeg)을 `ps -o rss`로 5초마다 따로 읽었다.

## 남은 것

- 9.3의 고정 장치(Windows DML, Linux x64)에서 1.12.1 릴리스 패키지와의 처리량 p50/p95: M8.
- 4K 원본 클립과 HDR 원본: M9 전까지 HDR은 거부한다.
- P-4 디코드 비용(4K 2.4–2.5초): M4 결정대로 둔다. 하드웨어 디코드로 줄이는 것은 손상을 같은 방식으로 잡는지 확인한 뒤에만 한다.
- NVENC·QSV, Windows 4K 파이프 상한: M7·M8 실기기(M4에서 넘어옴).
- 하드웨어 인코드 실패의 이유를 결과에 남기기: M6 결과 표시와 함께([위](#찾은-것-60분-출력의-하드웨어-인코더-실패)).
- 9.3 목표 문구와 4K의 P-4 비용, 1패스 밖의 나머지 약 1.6초: [M0–M5 재검토](M0_M5_REVIEW.md) 12번.
- Windows ML DML 결과를 ort rc.15로 다시 확인: M8 실제 장치(M0는 rc.14로 확인, CI는 3개 OS CPU와 x86 ORT 비교만 돎).

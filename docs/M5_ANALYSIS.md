# M5 1단계: 분석 1패스

작성 2026-10-22. [Rust 포팅 계획](RUST_PORT_PLAN.md) 8장 M5 1단계(`m5-analysis`)의 기록이다. 코드는 engine `video::record`(레코드 파일), `video::analysis`(1패스), `video::tracking`(레코드에서 추적), `video::process`(처리기 연결), `detect::tiles::detect_views`, `detect::runtime`(spin)이고, core는 `UncoveredSpan`·`Problem::UnresolvedGap`에 track 목록을, `Joins::next_id`를 더했다. 시험은 `tests/video_analysis.rs`·`video_detect.rs`, `d7_sessions.rs`·`allocations.rs`에 더한 것과 각 모듈의 단위 시험이다.

## 결정(2026-10-22, 메인테이너가 추천대로)

- **미해결 gap 근거:** 종류마다 맞닿은 구멍을 합친 구간 하나가 근거 하나이고, 그 구간 양쪽의 track 목록을 함께 담는다(`Problem::UnresolvedGap { detection, tracks, first, last }`). 근거는 [아래](#미해결-gap).
- **중간에 해상도가 바뀌는 스트림:** 1.12.1처럼 첫 크기로 늘려 처리한다. 두 패스가 같은 필터를 거치므로 마스크는 인코드되는 pixel과 맞는다. 모양이 바뀌는 구간은 찌그러져 보인다. 근거는 [아래](#해상도가-바뀌는-스트림).
- **2단계 전의 영상:** 분석을 처리기에 연결하고, track이 하나라도 있는 영상은 렌더하지 않고 실패시킨다("N detected subjects cannot be masked in videos yet"). 감지한 영역을 가리는 2패스는 2단계다. 그때까지 감지한 대상이 가리지 않은 채 게시되지 않는다.

함께 정한 것(질문하지 않음):

- **레코드 위치:** 세션 저장소의 `<세션 id>/item-<항목 번호>/`(세션 JSON 옆). CLI는 세션 id를 실행 전에 정한다. 항목 폴더는 항목이 끝나면(분석 결과를 버리면) 지우고, 실행이 끝나면 CLI가 세션의 레코드 폴더를 지운다(영상은 아직 검토로 남기지 않음). 지우지 못하면 오류로 알리고 종료 코드를 실패로 한다.
- **타일 판정:** 영상은 score 0.1까지 감지하지만, 타일에서만 찾은 것이 점수만으로 남는 기준은 사진처럼 그 종류의 임계값 + 0.2다(`tiles::merge_judged`). 하한(0.1)을 기준으로 쓰면 0.3이 되어 M3의 타일 오탐 기준이 영상에서만 느슨해진다. M5_PLAN의 타일 실험은 하한을 기준으로 merge했으므로 그 수치는 이보다 tile track이 조금 많은 쪽이다.
- **추적 창의 속도:** 원본의 평균 속도(`avg_frame_rate`), 없으면 출력 속도.
- **진행률:** 항목의 단위는 원본 프레임 × 2. 1패스가 앞 절반, 2패스가 뒤 절반(2패스도 출력 프레임 대신 원본 프레임 단위로 바꿈).
- **`ProgressBoard::reporter` 공개:** `ItemContext`와 `ItemProcessor`가 공개이므로 runner 밖에서 처리기를 돌리는 시험(그리고 M6)이 쓸 수 있게 했다.
- **`--mask`:** 아직 필수(메시지를 "detected subjects are not masked in videos yet"로). 선택 사항으로 바꾸는 것은 2단계.

## 흐름

1. `source::open`, 원본 identity 비교, 고정 마스크 확인(M4와 같음).
2. **1패스(`analysis::first_pass`):** `FrameReader`(긴 변 960)가 디코드 스레드에서 4프레임 bounded 채널로 보낸다. 감지 루프는 프레임마다 48×27 썸네일을 `SceneCutDetector`에, 프레임을 `detect_views`(전체 보기와 타일, 요청 사이마다 취소 확인)에 주고, 감지를 표시 pixel로 늘려 레코드에 덧붙인다. 상한이 있는 모델이 잘라 낸 감지는 프레임 번호와 함께 `CappedCandidates`(고정 모델에서는 생기지 않음). 디코드 스레드는 끝에 "다 읽음"을 따로 보내므로, 스레드가 그냥 사라지면 끝이 아니라 오류다. 감지 루프가 먼저 멈추면 채널이 닫혀 디코드 스레드가 끝나고 FFmpeg도 끝난다.
3. 원본 identity를 다시 비교한다(1패스 중 바뀐 원본).
4. **추적(`tracking::track`):** 종류마다 `KindTracking`(추적 창을 원본 속도로, 시작·강한 점수 `min(t, max(0.35, t − 0.1))`, 약한 하한 0.1).
   1. 레코드를 뒤에서부터 읽어 종류별 뒤 방향 `ByteTracker`, 할당을 내림차순 scratch 레코드에.
   2. 레코드를 앞에서, scratch를 뒤에서(곧 프레임 순서로) 읽으며 앞 방향 추적기와 `Merger` → `Joins`.
   3. 같은 것을 다시 돌려 `Joins` → `Summaries`.
   4. 한 번 더 `Joins` → `PostProcessor`, 종류별 출력을 프레임마다 합쳐 영역 레코드에.

   추적은 결정적이라 앞 방향 세 번이 같은 track을 만든다. track 번호는 모델 순서로 종류마다 이어 붙인다(얼굴 1…N, 번호판 N+1…). 앞 방향 track을 다 센 뒤에야 다음 종류의 시작 번호를 알 수 있어, 계획의 `first_id` 대신 종류의 번호에 앞 종류의 수를 더한다. scratch는 끝나면 지운다.
5. 결과: `Analysis.evidence` = 잘린 후보 + 미해결 gap, 처리기의 `Analyzed`가 레코드 폴더와 track 요약(`TrackSummary`: 번호·종류·처음·끝 감지 프레임·감지 수·강한 감지 수·낮은 신뢰)을 든다. track이 없으면 결과에 "nothing detected", GPU 실패로 CPU에서 다시 했으면 그 표시.

## 레코드 파일

`video::record`. 머리 16 B(`CLOAKREC`, 판 1, 내용 종류, 덧붙인 순서), 프레임 블록(프레임 u32, 개수 u32, 항목들, 블록 길이 u32), 꼬리(1,024블록마다의 offset, 장면 전환 목록, 개수 셋, `RECORDEND`). 블록 끝의 길이로 뒤에서도 읽고, offset 색인으로 프레임 k부터 읽는다(3단계 미리보기). 꼬리가 없으면 끝나지 않은 파일로 거부한다. 읽는 쪽은 64 KiB 창 하나로 앞·뒤 어느 쪽으로든 읽는다.

| 내용 | 크기 | 담는 것 |
| --- | --- | --- |
| `Detected` | 25 B | 모델, score, box(표시 pixel), roll(없으면 NaN) |
| `Assigned` | 10 B | 종류, 감지 번호, track, 시작 여부(뒤 방향 scratch) |
| `TrackRegion` | 30 B | 종류, 보간 여부, track, score, box, roll |

프레임 번호가 차례가 아니거나, score가 0–1 밖이거나, roll이 60°를 넘거나, 모르는 모델·종류이거나, box가 유효하지 않으면 손상으로 본다. 판이 다르면 `UnsupportedVersion`.

## 미해결 gap

**기본 설정에서는 미해결 gap이 생기지 않는다.** 추적기는 track을 1초(`max_lost`)까지만 다시 이으므로 앞 방향 track 안의 구멍은 1초 미만이고, 보간 한도도 1초라 모두 채워진다. 양방향 병합은 앞 track과 같은 프레임에 box가 있는 뒤 track만 합치므로, 합친 track의 구멍은 두 track 모두의 구멍이라 더 길어지지 않는다. 장면 전환 경계는 1초까지 다리를 놓고(`max_interpolation_gap`) 그보다 멀면 보지 않으므로(`max_cut_boundary_gap`도 1초) 보고하는 구간이 비어 있다. 1초 넘게 놓친 대상은 새 track으로 시작하고, 이것은 M5 결정대로 gap이 아니다.

확인: core 추적(앞→뒤→병합→후처리)에 무작위 영상 20,000개(속도 5가지, 장면 전환 0–2개, 대상 1–5개, 프레임마다 빠짐)를 넣으면 기본 창에서 gap 0, 보간만 2/3초로 바꾸면 4,662개에서 gap이 나온다. CC 영상 네 개(타일 켬·끔)도 1초에서 0이다.

그래서 근거 형식은 기본이 아닌 창(시험·fixture)에서만 쓰인다. 2/3초로 잰 수:

| 영상 | track별 구멍 | 종류별로 합친 구간 | 두 track 이상인 구간 | 한 구간의 최대 track |
| --- | --- | --- | --- | --- |
| 인터뷰 | 2 | 2 | 0 | 1 |
| 도로(얼굴) | 3 | 3 | 0 | 1 |
| 군중 | 191 | 73 | 39 | 18 |
| 군중, 타일(앞 1분) | 174 | 15 | 12 | 53 |
| 거리 | 19 | 17 | 2 | 2 |

**P-1(영상)에 미치는 영향:** P-1 fixture의 영상 경우("얼굴 감지가 30–64프레임에서 끊김")는 실제 추적기로는 gap이 되지 않는다. 35프레임을 놓친 얼굴은 새 track으로 다시 시작한다. 이 fixture는 track을 직접 넣는 후처리 시험으로만 재현된다. 2단계가 P-1(영상) 게이트를 무엇으로 확인할지 정해야 한다.

## 해상도가 바뀌는 스트림

320×240 2초와 640×360 2초를 이어 붙인 H.264(TS를 그대로 붙인 것과 MP4로 다시 담은 것)로 조사했다. ffprobe는 첫 크기만 보고하고 패킷 목록에는 크기가 없다. `scale=W:H` 필터는 크기가 바뀌면 스스로 다시 설정해 모든 프레임을 첫 크기로 늘린다(120프레임, 27,648,000 B). `-reinit_filter 0`이어도 같고, 바뀜은 stderr의 한 줄로만 알 수 있다. 1.12.1도 `scale=W:H`였다. 1패스와 2패스가 같은 필터를 거치므로 감지한 자리와 인코드되는 pixel이 맞는다. 거부하려면 keyframe만 디코드하는 조사가 더 필요하고, VP9는 keyframe이 아닌 프레임에서도 크기를 바꿀 수 있어 놓친다.

시험 `a_stream_whose_size_changes_is_scaled_to_its_first_size_in_both_passes`가 이 파일로 분석(120프레임 모두 320×240)과 게시(320×240, 120프레임)를 확인한다.

## 처리량

`video_detect.rs` `first_pass_throughput_1080p`(무시되는 측정): 1080p 30 fps 5초(messi5.jpg), YOLO5Face-n CPU, 타일 끔. CI가 3개 OS에서 출력한다("Measure the first pass", debug 빌드). 이 Mac(Apple M4, load 5–6, 다른 시험 빌드와 겹침):

| 빌드 | spin 끔 | spin 켬 | spin 끔(다시) | spin 끔, 타일 |
| --- | --- | --- | --- | --- |
| release | 24.4 fps | 8.6 | 22.6 | 7.5 |
| debug | 14.4 | 6.5 | 7.8 | 2.5 |

debug는 Rust 전처리가 느리고 기기 부하에 따라 같은 설정도 두 배까지 달랐다. 비교는 같은 실행 안의 순서로 본다.

PR #50의 CI(2026-10-22, debug, GitHub runner):

| runner | spin 끔 | spin 켬 | spin 끔(다시) | spin 끔, 타일 |
| --- | --- | --- | --- | --- |
| macOS 15 arm64 | 6.2 fps | 7.2 | 6.5 | 2.3 |
| Ubuntu 24.04 x86_64 | 7.0 | 6.5 | 7.2 | 2.5 |
| Windows Server 2025 x64 | 6.5 | 5.2 | 5.1 | 2.2 |

runner에서는 spin을 끈 이득이 보이지 않는다. 끈 쪽이 확실히 느린 OS도 없다. runner는 코어가 3–4개이고 debug 빌드라 Rust 전처리가 시간 대부분을 차지해, spin이 디코드·전처리의 코어를 빼앗는 효과가 드러나지 않는 것으로 본다(이 Mac의 release는 2.8배 차이). M5_PLAN의 "영상 감지 1.6–2배"는 3개 OS에서 확인되지 않았고, release로 다시 재는 것은 5단계 처리량 측정(계획 9.3)에서 한다.

## 메모리

`allocations.rs` `tracking_an_hour_peaks_where_ten_minutes_do`: 30 fps, 동시에 4명이 움직이고 20초마다 새 사람으로 바뀌며, 7프레임마다 놓치고 2분마다 장면 전환인 합성 레코드. 추적과 영역 읽기의 peak heap(전역 할당자로 셈):

| 길이 | 레코드 | track | peak |
| --- | --- | --- | --- |
| 10분 | 1.76 MB | 135 | 241 KB |
| 60분 | 10.6 MB | 810 | 363 KB |

늘어난 것은 track 요약(track마다 약 180 B: `Summaries`·`Joins`·`Merger` 결정·`PostProcessor`의 track 목록)뿐이다. 시험은 60분 peak가 10분 peak + 늘어난 track × 512 B 이하인지 본다.

## 시험

- `video::record` 단위 6건: 오름·내림 순서 파일을 양쪽으로 2,053프레임(색인 두 칸 넘음, 읽기 창보다 큼), 색인으로 중간부터 읽기, 항목 크기, 잘리거나 바뀐 파일 7가지와 다른 내용 종류, 있는 파일을 덮지 않음, 빈 영상.
- `video::tracking` 단위 2건: 무작위 영상 300개(얼굴·번호판, 장면 전환, 속도 30·8·5, 절반은 보간을 다시 잇는 한도의 2/3로 줄여 gap이 생기게)에서 레코드로 한 추적이 core의 전체 track 기준(`build_bidirectional_tracks` + `post_process_tracks`)과 프레임마다의 영역·track·낮은 신뢰·gap(track 목록까지)이 같음, 취소.
- core: `merge_spans`가 합친 구간에 모든 track을 한 번씩, `Joins::next_id`, P-1 근거의 직렬화 형식.
- `video_analysis.rs` 4건(실제 FFmpeg, 흰 사각형을 찾는 가짜 감지기): 1280×720에서 움직이는 사각형이 960×540으로 감지되어 표시 pixel의 영역으로 모든 프레임에(놓친 40–49프레임은 보간), track 하나, 렌더는 거부, 끝나면 레코드 폴더가 빔; 10번째 프레임에서 취소하면 감지가 11번에서 멈추고 레코드가 지워짐; 1패스 중(5번째 프레임) 원본이 바뀌면 "the source changed while it was processed"; 해상도가 바뀌는 스트림.
- `d7_sessions.rs` `photos_and_videos_share_the_sessions`: 사진 4장과 얼굴 영상 하나를 한 실행에(YuNet + 번호판, GPU·CPU), 세션 수가 모델 수(2)와 같고 영상은 track이 있어 거부됨(감지가 돈 증거).
- `allocations.rs`·`video_detect.rs`: 위.

## mutation

작업 트리에서 한 곳씩 바꾸고 해당 시험을 돌린 뒤 원본을 `cp`로 되돌렸다(끝에 `cmp`). 9개 모두 잡혔다.

| 바꾼 것 | 잡은 시험 |
| --- | --- |
| 1패스 뒤 identity 비교 뺌 | `video_analysis` 원본 변경 |
| 뒤 방향 할당을 버림 | `video::tracking` 기준 비교 |
| 종류의 번호 이음 뺌 | 같은 것 |
| 뒤 방향 추적에 장면 전환을 주지 않음 | 같은 것 |
| 레코드의 프레임 번호 확인 뺌 | `video::record` 손상 |
| track이 있는 영상도 렌더 | `video_analysis` |
| 감지를 표시 pixel로 늘리지 않음 | `video_analysis` 영역 |
| 타일 판정을 하한으로 | `detect::tiles` 새 시험 |
| 합친 구간이 track을 잃음 | core `merge_spans` |

spin 설정은 결과를 바꾸지 않으므로 시험이 없고, CI 측정(켬·끔)으로만 본다. 로컬 전체(`cargo test --workspace --locked --all-features`, macOS) 462개 통과, fmt·clippy 두 조합 통과.

## 남은 것

- 완료: PR #50(2026-10-23, main `209a261`). 3개 OS CI와 탐지 벤치마크(core `detection.rs`가 바뀌어 돎)가 통과했다. Windows·Linux에서는 이 PR이 분석 1패스·레코드·추적 연결·D7 영상 경우의 첫 실행이었다. 같은 실행의 1080p 파이프: 디코드 Linux 88.6 fps·macOS runner 184·Windows 103, 인코드 Linux 77.2·macOS runner 109·Windows 84.5.
- spin 결정의 release 측정(위): 5단계.
- 2단계(`m5-render`, [기록](M5_RENDER.md)): 영역 레코드로 마스크, 렌더 거부를 마스크로 바꿈, `--mask` 선택 사항. P-1(영상)은 보간만 줄인 창의 실제 FFmpeg 경로와 상한 후보로 확인하고, 추적 창은 2초로 늘림. 결과 JSON의 track·gap 종류는 3단계 세션 형식 2로 넘김.
- 3단계([기록](M5_REVIEW.md)): 검토로 남기는 영상의 레코드 보존과 세션 형식 2, 비정상 종료로 남은 레코드 폴더 정리(세션 잠금 파일로 가림). 레코드 위치는 그대로 `<세션 id>/item-N`이고, 이제 분석을 마친 레코드는 항목과 함께 지워지지 않고 게시하거나 세션이 끝날 때 지운다.
- `Problem::FailedFrame`: 감지 실패는 GPU에서 CPU로 다시 한 뒤에도 실패하면 항목 실패다. 프레임 단위 근거로 남기는 경로는 아직 없다.

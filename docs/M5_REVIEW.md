# M5 3단계: 검토 세션과 결정

작성 2026-10-24. [Rust 포팅 계획](RUST_PORT_PLAN.md) 8장 M5 3단계(`m5-review`)의 기록이다. 코드는 core `review`(`ManualTrack`·`Keyframe`·`VideoEdits`·`Edits`), engine `session`(형식 2, `StoredMedia`, `SessionStore::lock`·`prune`·`sweep`), `review`(영상 결정), `batch`(영상 항목을 세션에), `video::process`(입력 digest, 레코드 보존, 저장한 분석에서 2패스 재개, 편집 반영), `video::preview`(미리보기 조회), `media::source`(`Source::timeline_digest`), `identity::file_digest`, `run`(`Analysis::video`, `RenderRequest::edits`, `ItemProcessor::fingerprint(input)`, `Note::RecordsLeft`), `media_processor`(종류별 fingerprint), CLI `run`·`review`다. 시험은 core `review` 단위와 `tracking_fixtures.rs`(수동 track fixture), engine `session`·`review`·`batch`·`identity`·`media::source` 단위, `video_analysis.rs`·`cli_video.rs`·`cli.rs`다.

## 결정(2026-10-24, 메인테이너가 추천대로)

- **영상 입력 identity:** 사진처럼 파일 전체의 SHA-256(`ReviewKey::input`)에 묶는다. 1패스와 나란히 다른 스레드가 파일을 한 번 읽어 digest를 만들고, 저장한 검토에서 재개할 때는 2패스와 나란히 다시 읽어 게시 전에 비교한다. 근거는 [아래](#입력-해시-비용).
- **비정상 종료로 남은 레코드:** 세션마다 잠금 파일(`<세션 id>.lock`, OS 파일 잠금)을 run·review가 도는 동안 잡는다. 프로세스가 어떻게 끝나든 OS가 푼다. run·review는 시작할 때 잠기지 않은 세션을 정리한다: 세션 JSON이 없으면 레코드 폴더와 잠금 파일을 지우고, 있으면 세션에 남지 않은 항목의 레코드 폴더를 지운다. 같은 세션을 두 review가 동시에 고치는 것도 막는다. 근거는 [아래](#파일-잠금).
- **제외한 track의 gap:** gap은 따로 해결해야 한다(오탐 지정, 또는 gap의 모든 프레임에 수동 track). gap 양쪽 track을 모두 제외해도 저절로 해결되지 않는다(계획 4.2: 명시적 결정만 해결).
- **CLI review의 영상 설정:** review가 run과 같은 영상 플래그(`--videos --ffmpeg --ffprobe`, `--mask`·`--video-codec`·`--video-quality`·`--hardware-encode`)를 받고, 영상 설정이 다르면 그 항목을 `SettingsChanged`로 거부한다(사진이 `--keep-metadata` 등을 다시 받는 것과 같은 규칙).

함께 정한 것(질문하지 않음):

- **세션 형식 2:** 판 1(사진만)은 읽지 않는다. 판 1을 쓴 빌드는 배포되지 않았다.
- **track 요약은 세션 JSON에:** 종류(P-2)·처음과 끝 감지 프레임·감지 수·강한 감지 수·낮은 신뢰. 군중 영상(약 11 track/초)이면 1시간에 track 약 4만 개, 약 4 MB다. box는 레코드에만 있다.
- **결정 파일의 영상 키:** `excluded_tracks`(track 번호)와 `added_tracks`(`first`·`last`·`keyframes`)로, 사진 키(`excluded`·`added`)와 따로 둔다. 다른 종류의 키를 주면 `WrongMedia`로 거부한다.
- **수동 track 규칙은 1.12.1과 같음:** keyframe은 프레임 순서대로 한 번씩, `first..=last` 안. keyframe 사이는 선형 보간(f64), 첫 keyframe 앞과 마지막 뒤는 그 box를 그대로. 프레임 경계로 자르고, 잘린 뒤 남는 것이 없는 프레임이 하나라도 있거나 영상 끝을 넘으면 track 전체를 거부한다. 프레임 번호는 원본 프레임 번호다(레코드·gap과 같음). 1.12.1의 개수 상한(수동 track 64개, keyframe 4,096개)은 옮기지 않았다(계획 0.1). 2패스는 프레임마다 keyframe에서 box를 계산하므로 수동 track을 레코드로 만들지 않는다.
- **수동 track의 모자이크 격자:** 자동 track처럼 그 track에서 가장 큰 box의 칸으로 영상 좌표에 고정한다(2단계 규칙).
- **미리보기:** 요청한 긴 변 크기로 디코드하고 영역·격자 칸을 같은 비율로 줄인다. 크기를 주지 않으면 표시 크기로, 2패스와 같은 함수(`Masks::frame` + `mask_frame`)로 그린다. 미리보기를 열 때는 원본 stamp와 timeline만 비교하고 바이트 digest는 다시 만들지 않는다. 미리보기에서는 아무것도 게시하지 않고, 게시하는 렌더가 바이트를 비교한다.
- **게시한 영상의 레코드:** 게시 직후 처리기가 지운다. 지우지 못하면 결과에 표시하고(`Note::RecordsLeft`) 다음 run의 정리가 지운다. 분석을 마친 레코드는 렌더가 실패해도 남고, 세션을 저장한 쪽이 정리한다(run 끝의 `prune`, 세션이 없으면 `remove`).
- **timeline digest:** `TimelineSpec`의 `Debug` 표현과 원본 프레임 수·끝 시각·모든 프레임의 pts. 같은 바이트를 같은 도구로 다시 읽으면 같다. 앱이 바뀌어 `TimelineSpec` 표현이 달라지면 저장한 영상 검토는 "바뀜"으로 거부된다(안전한 쪽).
- **영상 세션을 `--videos` 없이 review:** 사용법 오류(종료 코드 2).

## 실험

### 입력 해시 비용

이 Mac(Apple M4, SSD, 부하 1.7–2.3)에서 `sha2` 0.11(하드웨어 SHA)로 1 MiB씩 읽어 해시했다. 4 GiB 무작위 파일(방금 만들어 캐시에 있음) 세 번: 1.77·1.73·1.72초(약 2.5 GB/s).

| 입력 | 크기 | 해시 | 1패스(release, YOLO5Face-n CPU 약 24 fps) |
| --- | --- | --- | --- |
| 1080p30 1시간(8–20 Mbps) | 3.6–9 GB | 2–4초 | 약 75분 |
| 4K60 1시간(휴대폰, 약 100 Mbps) | 약 45 GB | 약 18초 | 분석은 긴 변 960 디코드라 디코드 속도에 달림 |
| 같은 4K 파일, 외장 HDD(약 100 MB/s) | 약 45 GB | 7분 넘음, FFmpeg 읽기와 겹침 | |

SSD에서는 1패스 시간의 1% 아래다. 해시를 패스와 나란히 돌리므로 디스크가 충분히 빠르면 시간이 늘지 않는다. 느린 디스크에서는 재개할 때도 같은 만큼 더 든다. 같은 크기·같은 수정 시각으로 내용만 바뀐 파일은 stamp만으로는 놓친다.

### 파일 잠금

`std::fs::File::lock`·`try_lock`(Rust 1.89부터 stable, 이 툴체인 1.98)으로, 한 프로세스가 잠근 파일을 다른 프로세스가 `try_lock`·`try_lock_shared`하면 둘 다 `WouldBlock`이었고, 잡은 프로세스를 `kill -9`로 죽이자 바로 잠글 수 있었다(macOS). 같은 프로세스 안에서도 다른 열기로는 잠기지 않는다(시험 `a_session_is_held_by_one_process_at_a_time`). Windows는 `LockFileEx`, 다른 Unix는 `flock`이고 CI가 첫 실행이다. 같은 기준을 나이(24시간, 게시 임시 파일 규칙)로 하면 하루 넘게 도는 실행의 레코드를 지울 수 있고, 비정상 종료 뒤 하루 동안 디스크에 남는다.

## 흐름

1. **run:** 세션 저장소를 정리하고(`sweep`) 새 세션 id를 잠근다. 영상 분석(`analyze`)은 1패스와 추적을 하는 동안 다른 스레드에서 파일 digest를 만든다(실패하거나 취소하면 읽기도 멈춤). 분석 결과에 `VideoAnalysis`(timeline digest, 원본 프레임 수, 표시 크기, track 요약)가 붙고, 레코드 폴더는 "완료"가 되어 항목과 함께 지워지지 않는다.
2. 미해결 근거가 있거나(P-1) 검토 필수이거나 이름이 충돌하면 `NeedsReview`. `session_from_report`가 영상 항목을 `StoredMedia::Video`로 넣고, `ReviewKey`는 파일 digest·모델·**영상** 설정·timeline digest다(`fingerprint(input)`가 종류별). 사진은 timeline이 빈 digest다.
3. run 끝: 세션을 저장하고 세션에 없는 항목의 레코드를 지운다(`prune`). 세션이 없으면 세션 폴더와 잠금 파일을 지운다. 잠금은 세션 JSON 옆에 남는다.
4. **review:** 정리, 세션 잠금(다른 프로세스가 쥐면 "in use"), 영상이 있으면 `--videos` 필요. `apply`가 항목 종류에 맞게 편집을 확인한다: 제외할 track이 있는지, 수동 track이 영상 안에 있는지(`ManualTrack::check`). "수동 마스크로 덮음"은 사진의 약한 후보면 box가 추가 영역에 덮일 때, 영상 gap이면 수동 track이 gap의 모든 프레임에 있을 때만(`VideoEdits::covers`). 나머지(상한 후보, 실패 프레임)는 오탐 지정만 된다.
5. **재개 렌더:** stamp 비교 → `source::open`(ffprobe 보고·패킷 목록) → timeline digest 비교 → 레코드 폴더(`item-N`)를 연다. 2패스는 프레임마다 영역 레코드의 그 프레임 영역 중 제외하지 않은 track, 그 프레임의 수동 track box, 고정 마스크를 그린다. 2패스와 나란히 digest를 다시 만들어 분석 때와 다르면 실패시킨다(출력은 게시하지 않음).
6. 게시 후 레코드 폴더 삭제. review 끝: 게시한 항목을 세션에서 빼고 저장 + `prune`, 남은 항목이 없으면 세션(JSON·레코드·잠금)을 지운다.

## 세션 형식 2

```json
{"version": 2, "output": "/out", "items": [
  {"item": 0, "input": "/in/clip.mp4", "destination": "clip.mp4", "conflict": "existing",
   "source": {...}, "revision": {"key": {"input": "…", "models": "…", "settings": "…",
   "timeline": "…"}, "number": 0},
   "media": "video", "frames": 90, "width": 641, "height": 361,
   "tracks": [{"id": 1, "kind": "face", "first": 0, "last": 89, "detections": 90,
               "strong": 90, "low_confidence": false}],
   "edits": {"excluded_tracks": [1], "added_tracks": [...]},
   "evidence": [{"kind": "unresolved_gap", "detection": "face", "tracks": [1],
                 "first": 30, "last": 64, "resolution": "unresolved"}]},
  {"item": 1, ..., "media": "photo", "detections": [{"model": "yolo5face_n", "kind": "face", ...}],
   "evidence": []}
]}
```

레코드는 저장소의 `<세션 id>/item-<항목 번호>/`(`detections`, `regions`)이고 세션 JSON에는 경로를 적지 않는다(저장소 밖을 가리킬 수 없음). 결정 파일의 영상 항목:

```json
{"item": 0, "revision": 0, "action": "approve", "rename_to": "clip (2).mp4",
 "excluded_tracks": [1],
 "added_tracks": [{"first": 30, "last": 64, "keyframes": [
   {"frame": 30, "bbox": {"x": 100, "y": 50, "width": 200, "height": 100}},
   {"frame": 64, "bbox": {"x": 150, "y": 50, "width": 200, "height": 100}}]}],
 "resolutions": [{"evidence": 0, "resolution": "covered_by_manual_mask"}]}
```

## 레코드 정리

`SessionStore::sweep`은 잠금 파일을 기준으로 돈다. 잠금을 못 잡으면(다른 run·review가 쥠) 건너뛴다. 잡으면 세션 JSON이 없을 때 세션 폴더와 잠금을 지우고, 있으면 `prune`, 읽을 수 없는 JSON이면 레코드를 그대로 둔다. 잠금 파일이 없는 레코드 폴더는 건드리지 않는다: run은 레코드 폴더보다 잠금 파일을 먼저 만들므로, 그런 폴더는 다른 프로세스가 막 만든 잠금 파일을 정리가 지운 경우에만 생기고 그 run은 아직 돌고 있을 수 있다. 그 경우 남는 것은 끝난 run이 지우지 못한 항목 폴더뿐이다.

## 시험

- core `review` 단위 3건: 수동 track이 프레임 경계에서 잘리고 남는 것이 없는 프레임에서 거부됨·keyframe 순서와 범위·영상 끝, gap을 덮는 수동 track(구간 합집합, 한 프레임 빈틈도 거부), 제외할 track 번호와 수동 track 오류. `tracking_fixtures.rs`가 1.12.1 `testManualVideoTrackInterpolationAndBounds`의 호출 6개(`manualTrackRectAtFrame` 4, `materializeManualVideoTrack` 2)를 값까지 다시 돌린다(normative 28개 중 27개 재생, 나머지 `testSceneCutsContainer`는 `scene` 단위).
- engine 단위: `file_digest`가 조각으로 읽어도 바이트의 digest와 같고 취소되면 멈춤, timeline digest가 프레임 시각·프레임 수·회전·시작에 따라 바뀜, 세션에 영상이 track·gap 종류와 함께 저장됨(P-2, JSON 키까지), 세션 잠금은 한 번에 하나, 정리가 끝난 run의 것만 지움(잠긴 세션·읽을 수 없는 세션·잠금 없는 폴더는 남김, 세션에 없는 항목은 지움), 판 1·3 거부, `session_from_report`가 영상을 timeline·영상 설정과 함께 넣고 분석 없는 영상은 `NotAnalyzed`, `apply`가 영상 편집(없는 track, 영상 끝을 넘는 수동 track, 한 프레임 빈 gap 덮음, 종류가 다른 편집)을 거부하고 받은 편집과 `VideoAnalysis`를 job에 넘김, 결정 파일의 영상 키와 모르는 키 거부.
- `video_analysis.rs` 3건 추가(실제 FFmpeg, 흰 사각형 감지기): 저장한 분석에서 재개하면 제외한 track은 모든 프레임에서 드러나고 직선 이동을 두 keyframe으로 따라간 수동 track은 모두 가림, 게시 후 레코드 삭제, digest가 파일 SHA-256과 같음; 다른 digest나 다른 timeline이면 "changed since it was analyzed"로 실패하고 게시 없음(레코드는 남음); 미리보기의 프레임 k가 전체 디코드의 k번째와 같음·영역(track, 수동 box와 보간 여부)·표시 크기 마스크(가림, 제외하면 원본과 같음, 수동 track이면 가림)·640 긴 변 마스크·마지막 다음 프레임 거부·원본이 바뀌면 거부. 기존 P-1 시험은 검토로 남는 영상의 레코드가 남는지 확인하도록 바꿈.
- `cli_video.rs` 3건: 이름 충돌로 검토에 남은 영상이 세션(영상, track 종류)과 레코드로 남고, `--videos` 없는 review는 사용법 오류, 영상 품질이 다른 review는 거부(레코드 남음), 같은 설정의 review가 제외 track은 드러내고 수동 track은 가린 채 새 이름으로 게시하고 세션 폴더를 비움; 저장된 gap을 한 프레임 모자라게 덮은 결정은 거부되고 모두 덮으면 게시됨; 2분 영상을 분석하는 run을 레코드가 생긴 뒤, 다른 run이 그 레코드를 건드리지 않는 것을 확인하고 kill(SIGKILL)한 다음 run이 남은 레코드와 세션 폴더를 지움. 사용법 시험에서 `--review off` 제한을 뺌.
- `cli.rs`: 세션 수는 JSON 수로 셈(잠금 파일이 옆에 있음), 세션 저장소가 파일이면 run이 시작할 때 "cannot lock the review session"으로 종료 코드 1.

로컬 전체(`cargo test --workspace --locked --all-features`, macOS) 489개 통과(2단계 475개), fmt·clippy 두 조합 통과.

## mutation

작업 트리에서 한 곳씩 바꾸고 해당 시험을 돌린 뒤 원본을 `cp`로 되돌렸다(끝에 바꾸기 전 사본과 `cmp`). 26개 모두 잡혔다. keyframe 표시 하나는 `cargo fmt`가 줄을 바꿔 처음에 패턴이 맞지 않아 다시 돌렸다.

| 바꾼 것 | 잡은 시험 |
| --- | --- |
| gap 덮음이 한 프레임 빈틈을 허용 | core `review` |
| 수동 box의 keyframe 여부를 뒤집음 | core `review`, 수동 track fixture 재생 |
| 수동 track이 프레임 수와 같은 번호에서 끝나도 됨 | core `review` 2건 |
| 어떤 수동 track이든 gap을 덮음 | engine `review`, `cli_video` gap |
| 제외한 track도 그림 | `video_analysis` 재개·미리보기, `cli_video` 재개 |
| 수동 track을 그리지 않음 | `video_analysis` 재개·미리보기 |
| 재개할 때 바이트 digest를 비교하지 않음 | `video_analysis` 바뀐 원본 |
| 재개할 때 timeline을 비교하지 않음 | 같은 것 |
| 게시한 영상의 레코드를 남김 | `video_analysis` 2건 |
| 분석을 마친 레코드를 항목과 함께 지움 | `video_analysis` 2건, `cli_video` 2건 |
| 정리가 잠금을 보지 않음 | `session` 정리, `cli_video` kill |
| 정리가 저장된 세션도 지움 | `session` 정리 |
| `prune`이 모든 항목을 남김 | 같은 것 |
| 영상을 사진의 빈 timeline으로 저장 | `batch` |
| 영상도 사진 fingerprint로 | `cli_video` 재개(다른 품질 거부) |
| timeline digest가 프레임 시각을 빼먹음 | `media::source` |
| 작은 미리보기가 마스크를 줄이지 않음 | `video_analysis` 미리보기 |
| run 끝에 남길 레코드도 지움 | `cli_video` 2건 |
| run이 세션을 잡지 않음 | `cli_video` kill |
| CLI가 정리하지 않음 | 같은 것 |
| 영상에 사진 편집을 받음 | engine `review` |
| 사진에 영상 편집을 받음 | 같은 것 |
| 없는 track 제외를 받음 | 같은 것 |
| 미리보기가 마지막 다음 프레임을 받음 | `video_analysis` 미리보기 |
| 분석이 다른 digest를 저장 | `video_analysis` 2건 |
| 파일 digest가 마지막 조각을 빼먹음 | `identity` |

## 남은 것

- 데스크톱 앱(M6)도 CLI처럼 세션을 잡고(`lock`) 시작할 때 정리해야 한다(`sweep`). 검토 화면은 미리보기 API(`VideoProcessor::preview`)를 쓴다. 연속 재생은 범위 조정 순서 ①.
- 재개한 렌더가 실패한 영상의 레코드는 세션이 끝날 때까지 남는다(다시 시도할 수 있게).
- Windows·Linux의 파일 잠금과 kill 뒤 정리는 이 PR의 CI가 첫 실행이다.
- CLI 사용자는 세션 JSON에서 track 번호를 읽는다. `sessions` 목록은 항목 수만 보인다.
- 4단계 `m5-benchmark`: 영상 탐지 품질(노출 프레임 수).

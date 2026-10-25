# M0–M5 재검토

실행일: 2026-10-25. 대상: main `1146100`(M5 완료, PR #54까지). 장치: Apple M4, macOS, 고정 FFmpeg 9.0.2·ORT 1.30.0. M5까지의 코드·문서·결정을 다시 보고 결함·위험·문서 오류를 찾은 기록이다. 코드는 고치지 않았다. 고칠 순서는 메인테이너가 정한다. 줄 번호는 `1146100` 기준이다.

## 방법

- 읽은 것: [계획](RUST_PORT_PLAN.md) 0–4·8·9장, [CHANGES](CHANGES_FROM_1.12.md), M3–M5 기록, CI workflow 전부, `manifests/`, `deny.toml`, `tests/fixtures/legacy/index.json`.
- 코드는 영역 넷(추적, 영상 파이프라인, 실행·검토·세션·게시, 감지·사진)으로 나눠 하위 검토에 맡겼다. 최근 변경(M5 5단계)과 문서·기록 대조는 직접 봤다. 하위 검토가 보고한 것 중 영향이 큰 것은 다시 재현하거나 코드로 확인했고, 확인 방법을 항목마다 적었다.
  - **재현**: 직접 실행해 확인
  - **mutation**: 코드를 바꿔 시험이 잡는지 확인
  - **코드**: 직접 읽어 확인
  - **하위 검토**: 하위 검토의 추적·스크립트만 있음
- 코드 실험은 `git clone --local` 복사본과 별도 `CARGO_TARGET_DIR`에서 했고, 끝난 뒤 지웠다. 실험 입력은 고정 FFmpeg로 스크래치 폴더에 만들었고 저장소에는 넣지 않았다.
- 회귀 확인:
  - 로컬: `cargo fmt --check`, clippy 두 조합(기본·`--all-features`), `cargo test --workspace --locked --all-features` 모두 통과했다. 시험은 490개 통과, 무시 3개(처리량 측정)로 [M5_VERIFY](M5_VERIFY.md)의 490개와 같다.
  - main push CI(`1146100`): CI, Benchmark, Video benchmark, ORT 모두 통과했다.

## 요약

| # | 심각도 | 영역 | 내용 | 확인 |
| --- | --- | --- | --- | --- |
| 1 | 결함 | 검토 | 게시에 실패한 항목을 다시 결정하면, 예전 "수동 마스크로 덮음" 해결이 그 마스크 없이 남아 가리지 않은 채 게시된다(P-1) | 재현 |
| 2 | 결함 | 영상 | 영상보다 먼저 시작하는 오디오를 AAC로 다시 인코드하면 그만큼 끝이 잘린다. P-4가 통과시킨다(P-5) | 재현 |
| 3 | 결함 | CLI | zsh에서 터미널을 닫으면 SIGHUP이 두 번 와서, 세션을 저장하기 전에 즉시 종료한다 | 재현 |
| 4 | 결함 | 추적 | 같은 장면으로 돌아오는 두 프레임 이상의 섬광을 장면 전환 여러 개로 본다(P-6 b) | 재현(Rust), 1.12.1 값은 하위 검토 |
| 5 | 결함 | 영상 | 1 fps 미만의 NTSC형 속도(예: 500/1001)에서 core가 panic해 "internal error"로 실패한다 | 재현 |
| 6 | 결함(낮음) | CLI | 빠진 항목·스캔 문제·레코드 정리 실패의 종료 코드 1이 검토 대기 3에 덮인다 | 코드 |
| 7 | 위험 | 감지 | 영상 타일 판정의 "성긴 보기 확인"이 점수 0.1짜리 감지로도 통과해, 사진보다 느슨하다 | 코드 |
| 8 | 시험 공백 | 감지 | 실제 감지기가 타는 `detect_prepared`의 CPU 폴백을 보는 시험이 없다 | mutation |
| 9 | 위험 | 검토 | 결정 파일은 항목 번호와 수정 번호에만 묶이고, 입력 키(`ReviewRevision::accepts`)를 쓰지 않는다 | 코드 |
| 10 | 위험 | 추적 | 2초 다시 잇기 창은 거의 정지한 대상에만 2초다. 움직이는 대상은 IoU 게이트로 더 일찍 새 track이 된다(1.12.1부터 있음) | 하위 검토 |
| 11 | 낮음 | 여러 곳 | panic·경쟁·정리 누락 등 드문 경우(아래 목록) | 항목별 |
| 12 | 판단 | 기록 | 9.3 목표("1.12.1보다 느려지지 않음")와 4K GPU 6% 느림의 처리, 그 원인 설명 | 계산 |
| 13 | 문서 | — | 문서·주석·메모리가 코드나 사실과 어긋난 곳 | — |

## 결정(2026-10-25, 메인테이너가 추천대로)

- **범위:** 결함 1–6, 시험 공백 8, 13번 문서·주석 오류를 main에서 만든 `m0-m5-fixes` 한 브랜치·PR에서 고친다. 항목마다 실패하는 시험을 먼저 넣고 커밋을 나눈다. 7·9·10·11번은 기록만 두고 M6에서 다시 본다. 9번은 문서만 사실대로 고친다.
- **순서(노출 위험 순):** 1 → 4 → 2 → 5 → 3 → 6 → 8 → 문서.
- **동작:**
  - 1번: 해결 표시도 편집처럼 결정마다 통째로 바꾼다. 재시도 결정에 없는 해결은 사라진다.
  - 4번: 확인 대기 중 후보를 이어 갈 때 첫 후보의 `before`와도 돌아옴을 비교한다.
  - 2번: 다시 인코드하는 오디오도 영상 구간 끝에서 정확히 자르고, P-4에 "원본 오디오가 영상 끝까지 있으면 출력도 끝까지 있다"를 더한다.
  - 5번: 1 fps 미만 속도에서 panic하지 않는다.
  - 3번: SIGHUP은 몇 번 와도 협조적 취소만 한다. 바로 종료는 두 번째 SIGINT·SIGTERM에만 둔다.
  - 6번: 실패(1)가 검토 대기(3)보다 앞선다.
  - 12번: 계획 9.3에 4K의 P-4 전체 디코드 비용을 예외로 적고, M8 고정 장치에서 2패스와 출력 검사를 따로 잰다.
- **turbojpeg:** 같은 수정의 열린 honzasp/rust-turbojpeg#43이 릴리스되기를 기다린다. 로컬 우회는 넣지 않는다.

## 수정(2026-10-25, `m0-m5-fixes`)

항목마다 실패하는 시험을 먼저 넣어 실패를 확인한 뒤 고쳤다. 커밋은 항목마다 하나다.

| # | 고친 것 | 시험 | 확인 |
| --- | --- | --- | --- |
| 1 | `check`가 결정마다 모든 해결 표시를 미해결로 되돌린 뒤 결정의 것만 적용한다 | `review` 단위 `a_retry_decides_the_resolutions_again`(사진의 수동 상자, 영상의 수동 track) | 고치기 전 `rejected=[]`로 실패 |
| 4 | 장면 전환 감지기가 확인을 기다리는 후보를 모두 들고, 어느 후보의 앞 장면으로 돌아오면 그 후보와 뒤의 후보를 버린다 | `scene` 단위 `a_flash_over_several_frames_that_goes_back_is_no_cut`(위 표의 넷과, 섬광 뒤 짧은 장면으로 돌아오는 경우) | 고치기 전 [10, 11, 12]로 실패. 1.12.1 fixture 시험은 그대로 통과. 영상 벤치마크는 기록된 전환을 쓰므로 CI 값은 그대로 |
| 2 | 다시 인코드하는 오디오는 `atrim=start=0:end=<길이>`로 디코드한 샘플을 자른다. P-4는 원본 오디오의 끝(`start_pts + duration_ts`)이 영상 끝에 닿으면 출력 오디오의 시작 + 길이도 영상 끝 − 1프레임에 닿아야 한다 | `video_export.rs` `pcm_sound_that_starts_before_the_video`(MOV, PCM 1초 앞섬), `verify` 단위, `media_probe` | 고치기 전 마지막 클릭이 3.8초(영상 5초)로 실패. atrim만 빼면 P-4가 "audio stream 1 lasts 4.000 s instead of 5.000 s"로 게시를 막음 |
| 5 | `integer_twin`은 N·1000/1001인 속도에만 정수 속도를 짝짓는다 | `timeline` 단위 `rates_slower_than_one_frame_a_second`(500/1001, 1/2) | 고치기 전 panic. 엔진 CLI로 500/1001 영상이 5프레임 그대로 게시됨 |
| 3 | Unix의 SIGHUP은 `ctrlc` 대신 자체 처리기(원자 저장과 `write(2)`만)가 받아 취소만 한다. 바로 종료는 ctrlc 처리기의 두 번째 호출(SIGINT·SIGTERM)에만 | `cli.rs` `hangups_only_cancel`(첫 hangup의 안내를 본 뒤 두 번째, 합성 `held` 항목이 취소와 상관없이 2초 렌더) | 고치기 전 130이지만 세션 0개로 실패. pty로 대화형 zsh를 닫는 재현(SIGHUP 2번)과 bash(1번)에서 두 번씩 세션 JSON이 남고 출력 폴더에 임시 파일이 없음 |
| 6 | 종료 코드를 `worse`로 합친다: 실패(1) > 검토 대기(3) > 완료(0). run의 레코드 정리·스캔 문제·빠진 항목과 review의 거부·재개 결과 | `cli.rs` `a_scan_problem_fails_the_run_even_with_items_waiting_for_review` | 고치기 전 3으로 실패 |
| 8 | 시험용 감지기에 preparer를 두고 `DetectorHandle`을 거쳐 가속기 실패를 시험 | `fallback` 단위 `a_prepared_image_that_fails_is_analyzed_again_on_the_cpu` | `detect_prepared`에서 폴백을 빼는 mutation을 잡음 |
| 13 | 위 13번 표의 문서·주석(M0_M2_REVIEW 10번은 6번을 고쳐 다시 맞음). M3_TILES의 36MP 타일 수는 `layout`으로 다시 셌다: 6000×6000·6016×6016 185개, 7360×4912 165개 | — | — |

구현 중 바꾼 것:

- **3번:** 처음에는 SIGHUP을 모든 스레드에서 막고 `sigwait` 스레드가 받게 했다. 그런데 `std::process`는 자식의 signal mask를 비우지 않아(작은 프로그램으로 확인: 자식이 SIGHUP을 막은 채 시작) FFmpeg 같은 자식이 막힌 SIGHUP을 물려받았다. 도구는 자기 프로세스 그룹에서 돌아 터미널의 hangup을 받지 않지만, 자식에 상태를 흘리지 않도록 처리기로 바꿨다. exec는 처리기를 건 신호를 기본 동작으로 되돌린다.
- **2번:** P-4의 오디오 끝 기준은 원본 오디오가 영상 끝에 정확히 닿거나 넘을 때만 건다. 원본이 조금이라도 먼저 끝나면 확인하지 않는다(거짓 거부를 피하는 쪽).

남긴 것: 7·9·10·11번(M6에서 다시 봄, 9번은 문서만 고침), 도구·CI의 두 항목(`video_speed.sh`·`record_video.sh`의 target 경로, `benchmark.yml` 제외 목록).

확인(macOS arm64): `cargo fmt --check`, clippy 두 조합, `cargo test --workspace --locked --all-features --no-fail-fast` 498개 통과(490 + 이번 8개), 무시 3개.

찾은 것: `video_analysis.rs` `cancelling_the_first_pass_stops_it_and_removes_its_records`가 전체 시험을 한 번 돌릴 때 감지 호출 12번으로 실패했다(혼자 20번은 모두 통과). 989b94d부터 1패스는 프레임 10의 결과를 기다리기 전에 프레임 11을 감지기에 넣는다. 부하로 감지기가 늦으면 프레임 10에서 취소할 때 11이 이미 채널에 있어 한 번 더 돈다. 이 시험의 "정확히 11번"은 그 전 동작의 기대값이다. 이번 수정과는 상관없고 main에도 있다. 메인테이너 결정(2026-10-25)으로 시험이 취소 때 이미 넣은 프레임 하나를 받아들인다(11–12번, 위 "확인했고 문제없는 것"의 "취소는 약 2프레임 안에 닿는다"와 같음).

## 1. 재시도 결정에서 남는 "수동 마스크로 덮음"

- **위치:** `crates/cloakframe-engine/src/review.rs:253-336`(`check`), `bin/cloakframe-cli.rs:836-843`.
- **동작:** `check`는 저장된 항목을 복제한다. 편집은 결정의 것으로 통째로 바꾸지만(`edits.clone_from`, `ItemDecision::edits` 주석 "it replaces any earlier one"), 해결 표시는 결정에 적힌 것만 덮어쓴다. 그래서 그것만 새 편집과 대조하고, 예전 `CoveredByManualMask`는 다시 보지 않는다.
- **상태가 생기는 길:** 받아들여진 결정은 게시 전에 세션 항목에 저장되고 revision이 하나 올라간다. 게시가 실패·취소되거나 이름이 그새 생기면(NameTaken), 항목은 그 상태로 세션에 남는다. 이것이 정상적인 재시도 경로다.
- **재현(단위 시험, 스크래치 복사본):** 약한 후보 하나가 있는 사진에 revision 0 결정 `approve`, `added:[후보를 덮는 상자]`, `resolutions:[{0, covered_by_manual_mask}]`를 넣었다. 받아들여진 뒤 게시는 실패한 것으로 두고, revision 1 결정 `approve`(편집·해결 없음)를 넣었다. 결과는 `rejected=[]`, 작업 1개, `added=[]`, 해결은 `CoveredByManualMask`로 남았다. 렌더하면 약한 후보를 가리지 않고 "published (reviewed)"가 된다.
- 영상도 같다. 수동 track으로 덮은 gap은 재시도가 `added_tracks`를 빼면 그 프레임들을 가리지 않고 게시된다.
- **방향:** 해결 표시를 편집처럼 결정마다 통째로 바꾸거나, 남아 있는 모든 `CoveredByManualMask`를 최종 편집과 다시 대조한다.

## 2. 먼저 시작하는 오디오를 다시 인코드하면 끝이 잘림

- **위치:** `crates/cloakframe-engine/src/media/encode.rs:302-348`(`-copyts -itsoffset -<영상 시작>`, 출력 `-t`, `setts`), `media/verify.rs:216-225`.
- **재현(엔진 CLI, `--fixed-detection`, 소프트웨어 인코드):** 입력은 MOV이고, H.264 영상이 1.0초에 시작해 4.0초(120프레임), `pcm_s16le` 오디오가 0–5초다. 출력은 영상 4.000초, AAC 오디오 3.000초인데 "published (automatic, unreviewed)"로 게시됐다.
- **원인(하위 검토가 FFmpeg로 확인):** 복사하는 오디오는 `-t`가 옮긴 시간축에서 잘려 맞다. 다시 인코드하는 오디오(PCM·FLAC·Opus·Vorbis 등)는 `-t`를 첫 디코드 프레임(−앞섬)부터 잰다. 그래서 앞선 만큼(위에서 1초) 끝이 짧다. 앞섬 0.3초면 3.7초이고, 영상보다 늦게 시작하는 오디오는 맞다. 시작 동기는 맞다.
- **P-4:** 오디오가 영상보다 길지 않은지만 보므로(소스가 짧을 수 있어서) 통과한다.
- **시험:** `sound_that_starts_before_the_video`는 AAC 복사이고, `webm_with_opus_sound`는 오디오와 영상이 함께 시작해서 잡지 못한다.
- **틀린 문서:** [M4_ENCODE](M4_ENCODE.md) "끝"("AAC로 다시 인코드하는 stream은 FFmpeg가 이미 정확히 끝내고"), CHANGES "영상 오디오 자르기"("영상과 오디오 길이·동기가 맞는다").

## 3. zsh에서 터미널을 닫으면 세션을 잃음

- **위치:** `bin/cloakframe-cli.rs:1050-1066`(`cancel_on_interrupt`: 어떤 신호든 두 번째면 `exit_now(130)`).
- **재현(pty, synthetic CLI, `--review off`):** 입력은 `a-gap.jpg`(검토 대기)와 `b-slow.jpg`(렌더 중 대기)이고, 대화형 셸에서 실행한 뒤 pty를 닫았다.
  - **zsh:** 세 번 모두 CLI가 "cancelling…"을 쓴 뒤 130으로 끝났다. 세션 JSON 없이 `.lock`만 남았고, 출력 폴더에 `.cloakframe-tmp-*.jpg`가 남았다. 한 번은 세션 폴더에 `.cloakframe-tmp-<id>.json`이 남았다.
  - 신호를 세는 감싸개로 보니 전경 프로세스 그룹이 SIGHUP을 **두 번** 받았다. 커널의 tty hangup과 zsh가 작업에 다시 보내는 것으로 본다.
  - **bash:** 세 번 모두 한 번만 받아 세션을 저장하고 임시 파일을 지웠다.
- macOS 기본 셸이 zsh이므로 이 Mac에서는 터미널을 닫으면 검토 대기 항목을 잃는다. 게시되는 것은 없다.
- **틀린 문서:** [M4_PROCESS](M4_PROCESS.md) "첫 신호"("터미널이 닫혀(SIGHUP) 쓰기가 실패해도"). 그 시험은 stdout만 닫는다.
- **방향:** "바로 종료"는 두 번째 SIGINT(또는 SIGTERM)에만 둔다. 남는 `.cloakframe-tmp-<id>.json`은 sweep이 지우지 않는다(11번).

## 4. 두 프레임 이상의 섬광이 장면 전환 여러 개가 됨

- **위치:** `crates/cloakframe-core/src/tracking/scene.rs:173-189`. 후보가 확인을 기다리는 중 새 spike가 오면 그 후보를 전환으로 확정하고, 새 후보의 `before`를 섬광 프레임으로 둔다. 그래서 "원래 장면으로 돌아옴" 비교가 섬광 전 장면과 하지 않는다.
- **재현(core 단위 시험, 스크래치 복사본):**

  | 입력(밝기) | Rust | 1.12.1(하위 검토의 Python 포트) |
  | --- | --- | --- |
  | 100×10, 255, 180, 100×10 | [10, 11, 12] | [] |
  | 100×10, 255, 200, 150, 100×10 | [10, 11, 12, 13] | [10, 12] |
  | 100×10, 255×2, 100×10 | [] | [] |
  | 그라디언트×10, 255, 180, 그라디언트×10 | [10, 11, 12] | — |

- **영향(하위 검토의 추적, 실행하지 않음):** 전환은 모든 track을 끝내고, 약한 감지(0.1 이상 시작 점수 0.4 미만)는 track을 시작하지 못한다. 강한 감지 하나 뒤 약한 감지만 이어지는 얼굴에 점점 어두워지는 섬광(카메라 플래시, 번개)이 끼면, 섬광 뒤 프레임이 보고 없이 드러날 수 있다. 섬광 뒤에 강한 감지가 있으면 전환 경계 처리가 이어 채운다.
- **시험:** P-6 fixture와 `hard_cuts_short_shots_and_flashes`에는 한 프레임 섬광과 같은 밝기의 두 프레임 섬광만 있다.
- **틀린 문서:** CHANGES "장면 전환(P-6)"·[M5_TRACK](M5_TRACK.md) "같은 장면으로 돌아오는 섬광은 그대로 무시".
- **방향:** 확인 대기 중에 후보를 이어 갈 때, 첫 후보의 `before`와도 돌아옴을 비교한다.

## 5. 1 fps 미만 NTSC형 속도의 panic

- **위치:** `crates/cloakframe-core/src/timeline.rs:393-395`(`integer_twin`: `Rational::new(num / 1000, 1).expect(...)`), `constant_rate`에서 부름.
- **재현:** `testsrc=rate=500/1001`, `-video_track_timescale 30000`(2.002초마다 한 프레임, ffprobe `r_frame_rate=avg_frame_rate=500/1001`)을 엔진 CLI에 넣었다. `timeline.rs:394:70`에서 panic했고, 결과는 "failed … internal error: a positive rate"이다.
- 실행기는 `catch_unwind`로 항목 실패로 바꾸지만, `VideoProcessor::preview`도 `source::open`을 부른다. M6에서 실행기 밖에서 부르면 호출자가 panic한다.

## 6. 종료 코드 우선순위

- **위치:** `bin/cloakframe-cli.rs:730,737`. `exit_for`는 실패(1)를 검토 대기(3)보다 앞세운다. 그런데 레코드 정리 실패, 스캔 문제, 세션에서 빠진 항목은 `code.max(FAILED)`로 더한다. 3 > 1이라 검토 대기가 하나라도 있으면 3이 된다.
- [M0_M2_REVIEW](M0_M2_REVIEW.md) 10번("세션에서 빠진 항목은 … 종료 코드 1")과 어긋난다.

## 7. 영상 타일 판정의 성긴 보기 확인

- **위치:** `crates/cloakframe-engine/src/detect/tiles.rs:170-180`(`Finding::stands`), `video/analysis.rs:256`.
- 영상은 모든 보기를 점수 하한 0.1로 감지한다. 타일 감지가 "점수만으로 남는" 기준은 사진처럼 임계값 + 0.2다(`merge_judged`의 `masking`). 그런데 "성긴 보기에서도 같은 것을 찾음"(IoU ≥ 0.3) 확인은 성긴 보기 감지의 점수를 보지 않는다.
- 예: 성긴 보기에서 40 px 넘게 보일 크기의 0.55점 타일 감지가 있고, 전체 보기에는 같은 자리에 0.12점 감지만 있다. 사진이면 버려지지만 영상이면 남아 track이 되고 가려진다.
- [M5_ANALYSIS](M5_ANALYSIS.md) "타일 판정"이 막으려던 것(M3의 타일 오탐 기준이 영상에서만 느슨해짐)이 이 경로로는 일어난다. 방향은 더 가리는 쪽이라 노출은 아니다. 영상 벤치마크는 노출만 재므로 오탐 영향은 재지 않았다.

## 8. `detect_prepared` 폴백 시험 공백

- **위치:** `crates/cloakframe-engine/src/detect/fallback.rs:171-179`.
- 989b94d 뒤 실제 감지기는 `DetectorHandle`을 거쳐 늘 `detect_prepared`를 탄다.
  - 단위 시험의 `Scripted`(fallback.rs:230)는 preparer가 없어 `detect`만 탄다.
  - CoreML 폴백 시험(`detect_fixtures.rs:171-208`)은 감지기의 `detect`를 직접 부른다.
- **mutation:** `detect_prepared`에서 `on_cpu_if_failed`를 빼도 전체 490개 시험이 통과했다.
- 코드는 지금 맞다(하위 검토 추적: 실패한 요청의 이미지로 CPU에서 다시 하고, 뒤의 입력은 같은 형식). 시험만 없다.

## 9. 결정 파일의 결속

- **위치:** `review.rs:260`. 결정은 `item`과 `revision` 번호만 대조한다. 모델·설정 지문과 원본 stamp는 보지만, 결정 파일 자체에는 입력 키가 없다. core `ReviewRevision::accepts`(키 + 번호)는 시험에서만 쓴다.
- 예: 예전 세션의 `decisions.json`을 새 세션에 쓰면, 입력 폴더에 파일이 늘어 번호가 밀린 경우 `item 3, revision 0, export_without_anonymization`이 다른 사진에 적용된다.
- 계획 4.2 `ReviewRevision` 행("입력·모델·설정·타임라인 해시에 묶인 수정 번호. 오래된 응답은 적용하지 않음")과 core 주석(`review.rs:94`)은 다른 입력의 응답도 막는 것처럼 읽힌다. M6의 앱 안 결정은 키를 들고 있겠지만, CLI 결정 파일은 그렇지 않다.

## 10. 움직이는 대상의 다시 잇기

- **위치:** `crates/cloakframe-core/src/tracking/tracker.rs:37-51`(예측 이동을 box 한 변으로 제한), `:169`(IoU 0.3 게이트). 1.12.1과 같은 규칙이다.
- **하위 검토 스크립트(40 px 얼굴):** 놓친 동안 약 1.54변 넘게 움직이면 다시 잇지 못한다.
  - 0.5–1 px/프레임: 59프레임(2초 전부)
  - 2 px/프레임: 29프레임
  - 3 px/프레임: 19프레임
  - 4 px/프레임: 14프레임
- 그보다 길게 놓치면 새 track이 되고, 그 사이 프레임은 1.12.1처럼 보고 없이 드러난다.
- [M5_RENDER](M5_RENDER.md)는 "전부는 아님"이라고 적었다. CHANGES "영상 추적 창"과 `tracking/mod.rs:152-153` 주석은 2초 넘게 놓친 대상만 그렇다고 읽힌다.

## 11. 낮음

| 위치 | 내용 | 확인 |
| --- | --- | --- |
| `identity.rs:44-49`, `session.rs:156` | 수정 시각이 1970년 전인 원본 하나가 있으면 `SystemTime` 직렬화가 실패해 세션 전체를 저장하지 못하고, 검토 대기 항목과 레코드를 모두 지운다 | 코드 |
| `output_plan.rs:44,55`, `review.rs:220-227,349-353` | 대소문자·정규화 규칙을 파일 이름에만 쓰고 폴더 이름은 문자열로 비교한다. 대소문자 무시 볼륨에서 `Trip/a.jpg`와 `trip/a.jpg`는 계획 단계에서 중복으로 잡히지 않고, 게시 때 NameTaken으로 실패한다(덮어쓰지는 않음, D10) | 하위 검토 |
| `session.rs:224-236`, `:290-301` | 잠금 파일을 만든 직후 `try_lock` 전에 다른 프로세스의 sweep이 그 파일을 잠그고 JSON이 없다고 지울 수 있다. 그러면 거짓 "in use"가 나거나, 지워진 inode를 잠근 실행이 죽었을 때 그 레코드가 영영 sweep되지 않는다. 저장 중 종료로 남은 `.cloakframe-tmp-<id>.json`도 아무도 지우지 않는다(3번에서 관찰) | 하위 검토, 임시 JSON은 재현 |
| `video/record.rs:583-600` | 뒤로 읽기에서 블록 길이 0–7이면 `u32_at`이 panic한다. 같은 분석이 쓴 파일만 읽으므로 실행 중 디스크 손상에서만 닿는다 | 하위 검토 |
| `video/process.rs:588-609` | `beside_a_digest`의 작업이 panic하면 digest 스레드를 취소하지 않아, 파일 전체를 다 읽을 때까지 panic 전달이 늦어진다 | 하위 검토 |
| `video/process.rs:311-319` | 오디오 쪽 실패(디코드할 수 없는 오디오, 언어 불일치)도 `encoders_fault`로 하드웨어 탓이 된다. 그러면 소프트웨어 재시도를 하고, 실행 나머지를 소프트웨어로 돌리고, `EncodedAgainInSoftware`를 붙인다 | 하위 검토 |
| core `review.rs:219-245` | `unchecked_rect_at`은 정렬되지 않은 keyframe에서 u64 뺄셈이나 `Rect` expect로 panic할 수 있다. 미리보기 API는 편집을 검사하지 않으므로, M6 호출자가 `VideoEdits::check`를 먼저 불러야 한다 | 하위 검토 |
| `detect/yunet.rs:145-162` | `nms_int`의 i32 면적 곱이 비현실적으로 큰 출력에서 넘친다(debug panic). 넓이 0인 두 box를 OpenCV와 달리 둘 다 남긴다(더 가리는 쪽) | 코드(넘침), 하위 검토(OpenCV 비교) |
| `tracking/stream.rs:323-325` | `PostProcessor::ended`가 마지막 전환 뒤 끝난 track마다 하나씩(약 56 B) 쌓인다. 긴 한 장면 끝에 전환이 있으면 track 수만큼이다. 시험 `what_is_held_does_not_grow_with_the_video`는 track 하나다 | 하위 검토 |
| `video/tracking.rs:241-244` | 남긴 track 찾기가 O(T²)다. 둘 다 id 순이다 | 하위 검토 |
| `tracking/merge.rs:120-123` | 병합할 track이 그 프레임에 이미 box가 있으면 뒤 방향 box를 버린다. 다른 대상의 약한 box일 수도 있다(1.12.1부터, 기록됨) | 하위 검토 |

## 12. 처리량 기록의 판단

- **9.3 목표:** 계획 9.3의 영상 처리량 목표는 "1.12.1보다 느려지지 않음"이다. [M5_VERIFY](M5_VERIFY.md)는 4K GPU 전체가 6% 느린 것을 M4 결정(P-4)의 비용으로 "기록만" 하기로 했다(메인테이너 결정). 그런데 9.3의 목표 문구에는 예외가 없어, M8 고정 장치 측정에서 같은 차이가 목표 미달로 남는다. 9.3에 P-4 예외를 적거나 M8 전에 다시 정해야 한다.
- **원인 설명:** M5_VERIFY는 "차이는 P-4 출력 검사의 전체 디코드다"라고 적었다. 4K 거리 클립(1,140프레임)의 기록값으로 계산하면 이렇다.
  - 1패스: 1.12.1 57 fps(20.0초), 엔진 62 fps(18.4초). 엔진이 1.6초 빠르다.
  - P-4만이 차이라면 전체는 약 +0.9초여야 한다. 기록은 +2.5초(41.5 대 39.0초)다.
  - 1패스 밖에서 P-4(2.4–2.5초) 말고도 약 1.6초가 더 느리다.
  - CoreML 흔들림(엔진 4K 40.3–46.8초) 안의 차이일 수 있지만, 기록은 원인을 다 밝힌 것처럼 쓰여 있다. 2패스·출력 검사를 따로 재야 가릴 수 있다.
- **runner 측정의 근거:** `video-throughput.yml` 주석은 "ci.yml의 debug 측정은 최적화하지 않은 전처리에 시간을 대부분 써서 spin 비용을 가린다"를 이유로 든다. 그런데 같은 PR(#54)의 전처리 겹침 뒤 debug 1패스가 release와 같거나 더 높다.

  | OS | debug(ci.yml) | release(video-throughput.yml) |
  | --- | --- | --- |
  | Windows | 17.1/13.5/17.1 | 14.4/15.9/15.7 |
  | macOS | 10.5/10.7/12.2 | 12.1/12.6/9.7 |

  release workflow의 이유를 다시 적거나 둘 중 하나로 줄일지 정할 일이다. 이 workflow의 경로 필터에는 1패스 디코드(`media/decode.rs`·`frames.rs`)가 없다.

## 13. 문서·주석 오류

| 위치 | 틀린 것 | 맞는 것 |
| --- | --- | --- |
| [M4_ENCODE](M4_ENCODE.md) "끝", CHANGES "영상 오디오 자르기" | 다시 인코드하는 오디오도 정확히 끝남 | 먼저 시작하는 오디오는 앞선 만큼 짧다(2번) |
| [M4_PROCESS](M4_PROCESS.md) "첫 신호" | 터미널이 닫혀도 세션을 저장 | zsh에서는 SIGHUP 두 번으로 저장 전에 끝난다(3번) |
| CHANGES "장면 전환(P-6)", [M5_TRACK](M5_TRACK.md) "장면 전환" | 같은 장면으로 돌아오는 섬광은 무시 | 점점 어두워지는 여러 프레임 섬광은 전환 여러 개(4번) |
| CHANGES "영상 추적 창", `tracking/mod.rs:152-153` | 2초 넘게 놓친 대상만 새 track | 움직이는 대상은 더 짧게(10번) |
| 계획 4.2 `ReviewRevision`, core `review.rs:94` | 다른 입력의 응답은 적용하지 않음 | CLI 결정 파일은 번호로만 묶인다(9번) |
| [M0_M2_REVIEW](M0_M2_REVIEW.md) 10번 | 빠진 항목은 종료 코드 1 | 검토 대기가 있으면 3(6번) |
| [M5_ANALYSIS](M5_ANALYSIS.md) "타일 판정" | 타일 오탐 기준이 영상에서 느슨해지지 않음 | 성긴 보기 확인 경로로는 느슨하다(7번) |
| `tracking/mod.rs:77,111` | `TrackerConfig`·`PostProcessConfig` 기본값이 30 fps의 `TrackingWindows` | 기본값은 1.12.1 프레임 수(30/30/45, 30/3/30)이고 `TrackingWindows::default()`는 30 fps에서 60/60/60, 60/3/60이다. 엔진은 `apply`로 창을 쓰므로 동작은 맞다 |
| `tracking/mod.rs:120` | `max_cut_boundary_gap`은 전환 뒤 얼마까지 | 앞 track 끝(연장 포함)부터 뒤 track 시작까지의 구멍 전체(`post.rs:279`, `stream.rs:387`) |
| [M5_TRACK](M5_TRACK.md) 기본 창·흐름, [M5_PLAN](M5_PLAN.md) 머리말, [M5_ANALYSIS](M5_ANALYSIS.md) 1초 수치 | 보간 1초·나머지 1.12.1 값, 1초 넘는 구멍은 게시를 막음, 앞 방향 먼저 | M5 2단계에서 모두 2초, 엔진은 뒤 방향을 먼저 돈다. 단계 기록이라 낡은 것은 자연스럽지만, 계획의 2026-10-22 결정 줄("보간 한도 … 1초")에도 2단계에서 바뀌었다는 표시가 없다 |
| [M5_TRACK](M5_TRACK.md) 메모리 절 | 후처리는 진행 중 track의 box와 전환 주변 끝·시작만 듦 | 마지막 전환 뒤 끝난 track도 하나씩 듦(11번) |
| `detect/mod.rs:143`, [M5_VERIFY](M5_VERIFY.md) "고친 것" | 입력을 하나 미리 만듦 | `sync_channel(1)`이라 실행 중 N 동안 N+1이 채널에, N+2가 보내기 대기로 둘이다 |
| `detect/mod.rs:31` | `Unavailable`은 감지기 panic | 전처리 스레드(`cloakframe-prepare`) panic도 |
| 계획 3장 "GPU 실패", [M3_EP](M3_EP.md), CHANGES "GPU 가속" | 현재 항목(그 사진)을 CPU로 다시 분석 | 실패한 요청(타일 하나, 영상 프레임 하나)과 `Combined` 안의 실패한 모델만 다시 하고, 앞 타일은 GPU 결과를 쓴다 |
| `video/process.rs:6-7` | digest 스레드가 패스마다 원본을 읽음 | 1패스와 재개한 2패스 옆에서만(같은 파일 774-775행 주석이 맞음) |
| `bin/cloakframe-cli.rs:490-492` | `println!`이 "세션 저장 전에" panic할 수 있음 | 저장이 출력보다 먼저다. 종료 코드 이유만 맞다 |
| `manifests/rust.toml` `[crates.ort]` 주석 | engine은 CPU만, coreml·directml은 M3에서 붙임 | M3에서 이미 붙였다(engine `Cargo.toml` 대상별 feature) |
| [M2_ENCODE_SPIKE](M2_ENCODE_SPIKE.md) "Windows의 LNK4098" | upstream honzasp/rust-turbojpeg#44로 고침 | #44는 2026-10-24 병합 없이 닫혔다. 같은 수정의 #43(다른 작성자, 2026-10-18)이 열려 있다. 다음 행동은 메인테이너가 정한다 |
| 계획 8장 M5 5단계 줄 | "문서**구현" | 다른 단계처럼 ". **구현" |
| 계획 M5 "결정(2026-10-22)" ORT spin, CHANGES "ORT CPU 세션 대기" | 영상 감지 1.6–2배 / 1.6–3배 | 측정 시점이 다르다(계획 실험 vs 1단계 release 24.4 대 8.6). 한쪽에 맞춘다 |
| [M3_TILES](M3_TILES.md) 8행 | 36MP 타일 184개 | 6016×6016과 6000×6000을 `layout`으로 다시 세면 16 + 169 = 185(어느 크기를 뜻했는지 확실하지 않음, 하위 검토) |

## 도구·CI

- `tools/benchmark/video_speed.sh:36-37`과 `record_video.sh:15`는 빌드 결과를 `$repo/target/release`로 고정한다. `CARGO_TARGET_DIR`를 쓰면 빌드한 것과 다른, 예전 바이너리를 잴 수 있다.
- `benchmark.yml`의 제외 목록에 `tools/benchmark/legacy/legacy_video_speed.cpp`와 `legacy/CMakeLists.txt`가 없다. 그래서 #54와 main push에서 사진 벤치마크가 돌았다. 결과에는 영향이 없고 CI 시간만 든다.

## 확인했고 문제없는 것

- **문서 수치:** [M5_VERIFY](M5_VERIFY.md)의 배속·백분율(1080p GPU 6–21%·CPU 19–36%, 4K +6%·−20%, 타일 2.5–2.8배), 1시간 실행 시간, rc.14 누수 계산(32만 번 × 272 B ≈ 88 MB)은 표의 값과 맞다.
- **추적 fixture:** `index.json`의 추적 시험은 normative 28, intent 16, changed 2, dropped 1, 합계 47로 문서와 같다. 문서에 인용된 시험·함수 이름은 모두 코드에 있다(M4_VERIFY·M5_* 기록).
- **고정 판:** `manifests/rust.toml`, `Cargo.toml`, `Cargo.lock`의 crate 판(ort rc.15 포함)과 툴체인 1.98.1이 일치한다. CONTRIBUTING의 CLI 플래그는 모두 파서에 있다. CLI와 두 예제는 ORT를 불러오기 전에 텔레메트리를 끈다.
- **#54 CI 값:** 메모리에 적어 둔 #54 CI 값(release·debug 1패스, 파이프)은 Actions 로그와 같다.
- **모델:** 다운로드(HTTPS, 호스트 allowlist, redirect마다 검사, 크기 상한)와 설치(스트리밍 SHA-256, 크기 초과·미달 거부, fsync 뒤 rename)는 문제를 찾지 못했다.
- **989b94d 전처리 겹침(하위 검토 추적):** 요청마다 응답 채널이 따로 있고 FIFO 하나라 결과가 바뀌지 않는다. `submit_views`/`finish`는 `detect_views`와 같은 순서·임계값으로 합친다. 1패스의 프레임 번호는 빠짐·중복이 없다. 취소는 약 2프레임 안에 닿는다. 어느 스레드가 panic해도 대기자는 `Unavailable`을 받고 교착이 없다.
- **게시:** 취소 확인 뒤에만 `PublishPermit`을 발급한다. 세 가지 덮지 않는 rename(대체 경로 포함)은 기존 파일을 바꾸지 않는다. 임시 파일은 모든 오류 경로에서 지운다. 영상 identity는 패스 전·사이·게시 전에 비교한다. 재개한 렌더는 바이트·timeline digest를 비교한다.
- **영상 프레임 대응·마스크(하위 검토 추적):** 원본 프레임마다 한 번 디코드·마스크한다. 영역 레코드의 프레임 번호를 대조한다. VFR 반복·padding·회전·분석→표시 배율·수동 track을 처리한다. 미리보기와 2패스의 제외 track·격자가 같다. stderr는 늘 읽고, protocol whitelist는 모든 입력에 걸린다.
- **추적(하위 검토 추적):** Hungarian(연결 성분 분할, 금지 쌍 0), 흘려 처리와 전체 처리의 일치, 병합된 track의 구멍이 `max_lost − 1` 이하(기본 창에서 gap 없음)를 확인했다.

## 고칠 때 볼 시험

- **1번:** `review` 단위 시험. 받아들여진 뒤 남은 항목에 편집을 뺀 재시도를 넣어 `UnresolvedProblems`가 나는지 본다(영상은 `added_tracks`).
- **2번:** `video_export.rs`. 영상보다 먼저 시작하는 PCM·FLAC 오디오를 넣어 오디오 길이와 클릭 위치를 본다. P-4에 "원본 오디오가 영상 끝까지 있으면 출력도 그렇다"를 더할지 정한다.
- **3번:** `cli.rs`. 같은 프로세스 그룹에 SIGHUP을 두 번 보내고 세션이 남는지 본다.
- **4번:** `scene` 단위 시험. 점점 어두워지는 2·3프레임 섬광이 원래 장면으로 돌아오는 경우.
- **5번:** core `timeline` 단위 시험. 500/1001 같은 속도.
- **8번:** preparer가 있는 `Scripted`로 `DetectorHandle`을 거친 GPU 실패.

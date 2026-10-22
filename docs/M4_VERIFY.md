# M4 5단계: 완료 확인

작성 2026-10-22. [Rust 포팅 계획](RUST_PORT_PLAN.md) 8장 M4 5단계(`m4-verify`)의 기록이다. 시험은 `crates/cloakframe-engine/tests/video_export.rs`, 함께 고친 것은 core `timeline::SourceFrames::constant_rate`의 NTSC 짝 고르기다. 실험은 모두 고정 FFmpeg 9.0.2(macOS arm64, Apple M4), debug 시험 빌드.

## 결정(2026-10-22, 메인테이너가 추천대로)

- **시험 구성:** engine runner(`run::start` + `VideoProcessor`, 감지기는 `NoDetections`)로 분석 → 2패스 → P-4 → 게시를 끝까지 거친다. CLI가 쓰는 처리기와 같고 FFmpeg만 있으면 된다(CLI 연결은 `cli_video.rs`가 이미 본다). 입력은 시험이 만든다. Rust가 번호를 그린 프레임을 rawvideo로 FFmpeg stdin에 넣고, 그 파일의 패킷 목록에서 읽은 프레임 시각에 클릭을 둔 WAV를 만들어 mux한다(입력 하나 0.1–0.2초). 저장소에는 넣지 않는다.
- **인코더:** 입력마다 두 번 내보낸다. H.264는 하드웨어 우선(기본값: 이 Mac은 VideoToolbox, 하드웨어 인코더가 없는 CI runner는 libx264), HEVC는 libx265.
- **허용오차:** 프레임 번호는 모든 출력 프레임에서 정확히 같아야 한다. 오디오는 영상 시작 기준 1 ms 안. 영상은 클릭이 난 원본 프레임을 처음 보이는 출력 프레임의 시작이 원본 시각과 반 출력 프레임 안(대응 규칙이 프레임을 옮기는 최대).
- **NTSC 짝:** VFR에서 29.97·30처럼 짝이 둘 다 모든 프레임을 보이면, 공칭 속도인 쪽을 고르고 아니면 원본 프레임 시작과 출력 칸 시작의 평균 차이가 작은 쪽을 고른다(아래 [찾은 것](#찾은-것-ntsc-짝)). 이 단계에서 별도 커밋으로 고친다.
- **시작 상한(PR CI 뒤):** fuzz가 찾은 시각 계산 overflow는 영상 시작 2^31초 상한과, 길이를 첫 프레임과 시작 중 이른 쪽부터 재는 것으로 고친다(아래 [CI fuzz가 찾은 것](#ci-fuzz가-찾은-것-시작-시각)). 이 단계에서 별도 커밋.

## 입력

320×180 회색 프레임의 위쪽 두 줄 블록 16개에 원본 프레임 번호의 비트를 그린다(`tests/common`의 `numbered_frame`, P-3 fixture와 같은 그림, `frame_number`로 읽음). 클릭은 2 ms 길이로, 세 번째 프레임부터 7프레임마다 그 프레임이 시작하는 순간에 둔다. 마스크는 오른쪽 아래 16×16으로, 번호와 겹치지 않는다.

| 시험 | 입력 | 출력 속도 |
| --- | --- | --- |
| `a_constant_rate_with_b_frames` | 30 fps 150프레임, libx264 `-bf 3`, AAC | 30 |
| `a_container_that_starts_late` | 29.97 fps, `-output_ts_offset 1.25`(컨테이너 시작 1.2287초: AAC가 영상보다 21 ms 앞) | 29.97 |
| `twenty_seconds_at_23976_with_44100_hz_sound` | 23.976 fps 480프레임(20초), 44.1 kHz AAC. 속도가 0.1% 틀리면 마지막 클릭이 20 ms 어긋남 | 23.976 |
| `a_rate_that_halves` | 2초 30 fps 뒤 4초 15 fps | 30(181프레임) |
| `a_rate_that_doubles_after_ten_seconds` | 10초 15 fps 뒤 1초 30 fps, FFmpeg 추정 15/1 | 30(330프레임) |
| `jittered_timestamps` | 30 fps 격자에서 ±5 ms 흔들린 시각(timescale 90000, FFmpeg 추정 90000/1) | 30 |
| `sound_that_starts_before_the_video` | 영상이 0.4초에 시작 | 30 |
| `sound_that_starts_after_the_video` | 오디오가 0.3초에 시작(첫 클릭은 소리 밖) | 30 |
| `a_turned_video` | 29.97 fps `-bf 2`, `-display_rotation 90` | 29.97 |
| `webm_with_opus_sound` | VP9 + Opus(→ AAC), WebM 시각은 ms 단위 | 30 |
| `the_output_shows_the_analysed_frame_in_every_frame` | P-3 fixture 다섯 개(소리 확인 없음) | 각 원본 |

## 확인하는 것

- **프레임(P-3 `analysis_equals_output`·`vfr`):** 분석 크기(긴 변 320) pass가 원본 프레임을 0..N 순서로 하나씩 읽고, 모든 원본 프레임이 `shown`에 적어도 한 번 나온다. 출력은 회전 없음·같은 표시 크기·원본과 같은 CFR이고, 출력 프레임 수가 `output_frames`, 모든 n에서 출력 n번째의 번호가 분석 pass의 `shown_at(n)`번째 번호와 같으며, 마스크 안이 검다.
- **소리:** 클릭 시각은 `aresample=async=1:first_pts=0`으로 FFmpeg가 파일에 주는 0(가장 이른 stream)부터 패딩해 디코드한 PCM에서, 프레임 시각은 패킷 목록에서 컨테이너 시작을 빼서 같은 기준으로 잰다. 원본의 클릭이 만든 시각과 1 ms 안에 있는지 먼저 확인한다(입력 확인). 출력은 영상 안의 클릭 수가 같고, 각 클릭의 영상 시작 기준 시각이 원본과 1 ms 안(오디오 오차)이며, 그 클릭이 난 원본 프레임 k를 처음 보이는 출력 프레임의 시작이 k의 원본 시각과 반 프레임 안(영상 이동)이다.

## 결과(실험, 2026-10-22)

인코더 네 조합(VideoToolbox H.264·HEVC, libx264, libx265)에서 결과가 같았다. 프레임은 모든 입력·모든 조합에서 전부 맞았다.

| 입력 | 오디오 오차 | 영상 이동 |
| --- | --- | --- |
| CFR 다섯 개(30·29.97 오프셋·23.976 20초·오디오 먼저·늦게) | 0 | 0 |
| 회전 | 0 | 0 |
| WebM VP9 + Opus | 0 | −0.33 ms(WebM의 ms 반올림) |
| VFR 30 → 15 | 0 | 0 |
| 지터 ±5 ms | 0 | 최대 5.0 ms |
| VFR 15 → 30 | 0 | 고치기 전 최대 10.9 ms(29.97 fps), 고친 뒤 0(30 fps) |

오디오 오차 0은 48 kHz 한 sample 안이라는 뜻이다. Opus를 AAC로 다시 인코드하면 클릭 시작이 0.33 ms 달라지지만, 원본도 같은 만큼 다르게 읽혀 상쇄된다. 시험 11개는 로컬에서 함께 약 9초(debug, 병렬) 걸린다. 내보내기 하나는 0.15–0.6초, 확인은 0.3초 안팎이다.

## 찾은 것: NTSC 짝

10초 15 fps 뒤 1초 30 fps인 입력은 프레임이 정확히 1/30초 격자에 있는데도 출력이 29.97 fps로 골라져, 출력 프레임이 원본보다 최대 10.9 ms 밀렸다. 반 프레임 안이라 동기 요건은 지켰다. 원인은 짝 고르기 규칙(재검토 결정, 2026-10-21)의 "평균 속도에 가까운 쪽"이었다. VFR의 평균은 늘 가장 빠른 구간보다 낮으므로, 공칭 속도가 짝이 아니면 평균이 29.985보다 낮은 한 거의 늘 29.97 쪽으로 기운다. 이 입력의 평균은 16.4로, 29.97과 13.61, 30과 13.64 떨어져 있었다.

고친 규칙: 공칭 속도가 짝 중 하나면 그대로 쓴다. 아니면 각 원본 프레임 시작과, 그 프레임을 처음 보이는 출력 칸 시작의 차이를 더해 평균이 작은 쪽을 고른다. 정확한 정수로 계산하고, 같거나 overflow면 낮은 쪽(NTSC)이다(`SourceFrames::starts_nearer`). 단위 시험 목록에서 잰 평균 차이(Python, 정확한 유리수):

| 목록 | 29.97 | 30 | 고르는 것 |
| --- | --- | --- | --- |
| 15 → 30(격자 1/30) | 5.89 ms | 0 | 30(전에는 29.97) |
| 지터 21·21·18(timescale 600) | 3.78 ms | 1.67 ms | 30(같음) |
| 30 → 24 혼합 | 5.00 ms | 4.55 ms | 30(전에는 29.97) |
| 29.97 격자, 느린 구간 | 0 | 2.14 ms | 29.97(같음) |

최대 차이는 어느 속도든 반 프레임 아래로 묶여 있어 평균을 쓴다(30 → 24 혼합은 최대로 보면 29.97이 14.6 ms·30이 16.7 ms로 뒤집힌다). 바뀐 기대값: core `the_output_rate_is_the_lowest_that_shows_every_frame`·`jitter_and_mixed_rates_keep_their_real_rate`의 혼합 목록, `media_decode.rs` `a_faster_stretch_after_the_first_seconds_keeps_every_frame`(29.97 → 30). 새 단위 시험 `of_two_ntsc_twins_the_one_on_the_sources_grid_is_taken`은 양쪽 방향(30 격자 → 30, 29.97 격자 → 29.97)을 공칭 속도 없이 본다. 기대값을 먼저 바꿔 세 시험이 실패하는 것을 확인한 뒤 고쳤다.

## CI fuzz가 찾은 것: 시작 시각

PR #47의 첫 CI에서 3개 OS 시험은 통과했고, `timeline.rs`가 바뀌어 돈 fuzz workflow의 `probe_fields`가 panic을 찾았다. 시간 기준 1577058301/2926592와 시작 PTS 약 4×10¹⁶이면 시작이 약 2.1×10¹⁹초가 되고, `frame_start`끼리 비교하는 `Seconds::cmp`의 교차 곱셈이 i128을 넘었다. 이번 변경과 무관하게 1단계부터 있던 결함이다(main `8c9aa21`에서 같은 입력으로 재현). 원인은 시작에 상한이 없던 것이다. 다른 상한(int 범위 유리수, 86,400초, 300만 프레임) 아래에서 비교의 곱은 시작 2^31초일 때 최대 125비트이고, 약 2^34초에서 넘친다(약분되지 않는 시간 기준 1/2147483647, 240 fps 바로 아래의 2147483629/8947849로 계산). 고친 것과 시험은 [조사 기록](M4_PROBE.md#fuzz)에 있다. 실패하는 단위 시험 둘을 먼저 넣었고, 고친 뒤 찾은 입력과 두 대상 150초씩이 통과했다.

## mutation

스크래치 복사본(별도 `CARGO_TARGET_DIR`)에서 한 곳씩 바꾸고 `video_export.rs`·`media_decode.rs`·core `timeline` 단위 시험을 돌렸다.

| 바꾼 것 | 실패한 시험 |
| --- | --- |
| 오디오를 10 ms 앞당김(`-itsoffset`) | 소리를 보는 10개 전부. P-5의 20 ms 기준이었다면 놓쳤다 |
| writer `-framerate`를 반올림한 정수로 | NTSC 입력 넷(23.976 20초, 29.97 오프셋, 회전, P-3 fixture) |
| 대응 규칙을 칸 가운데 대신 칸 시작으로 | `jittered_timestamps`·`webm_with_opus_sound`(늦게 찍힌 프레임이 출력에서 빠짐), core 단위 3개 |
| 오디오를 영상 시작 대신 컨테이너 시작 기준으로 옮김 | `sound_that_starts_before_the_video`(클릭 2개 손실)·`a_container_that_starts_late`(21 ms) |
| 짝 고르기를 평균 속도로 되돌림 | `a_rate_that_doubles_after_ten_seconds`(출력 속도), `media_decode.rs` 1개, core 단위 3개 |
| 출력 필터에 `shuffleframes=1 0`(두 프레임씩 바꿈, 수는 같음) | 11개 전부. P-4는 통과해 게시됐고 프레임 번호 비교가 잡음 |

처음 돌린 묶음에서는 복원을 `rsync -a`로 해 원래 수정 시각이 돌아왔고, cargo가 다시 빌드하지 않아 앞 mutation이 남은 crate로 두 개가 돌았다. 복원을 `cp`(새 수정 시각)로 바꿔 그 둘을 다시 쟀고, 위 표는 다시 잰 결과다. 바꾸지 않은 상태로 돌린 전후 기준 실행은 모두 통과했다.

## 시험 표

`expected.json`의 확인 항목별로 그것을 보는 시험이다. 모두 3개 OS CI(`ci.yml`, `--all-features`)에서 돈다.

| 항목 | 확인 | 시험 |
| --- | --- | --- |
| D1 | `output_tags_equal_source`·`center_rgb`(세 입력, 소프트웨어, 기본 품질) | `media_encode.rs` `the_output_keeps_the_sources_colour_description` |
| D1 | 내보낼 때마다 출력 태그가 spec과 같음(P-4가 값으로 비교) | `media::verify` 단위 `each_difference_from_the_expected_output_fails`·`colour_names_are_compared_as_values` |
| D1 | 디코드 쪽: 세 입력이 각자 matrix·range로 (200, 60, 40) ±3 | `media_decode.rs` `frames_decode_with_the_specs_matrix_and_range` |
| D1 | `untagged_input`: 너비 1280 이상·높이 576 초과면 BT.709, 아니면 SMPTE 170M, range tv. 모르는 태그·디코드할 수 없는 matrix는 거부 | core `untagged_fields_follow_the_frame_size`, `media::probe` 단위 `unspecified_colour_is_untagged_and_an_unknown_name_is_refused`·`colour_tags_are_kept_and_the_rest_assumed`, `media_probe.rs` `colour_tags_come_from_the_source_or_the_height_rule`·`every_tag_ffprobe_reports_is_read_or_refused`·`an_untagged_wide_hd_crop_is_taken_as_bt709` |
| P-3 | `analysis_equals_preview`(다섯 fixture, 모든 k, 바이트까지) | `media_decode.rs` `the_preview_of_a_frame_is_the_passs_frame` |
| P-3 | `analysis_equals_output`(다섯 fixture, 모든 n, 두 인코더) | `video_export.rs` `the_output_shows_the_analysed_frame_in_every_frame`, 합성 입력 10개의 시험 |
| P-3 | `cfr_no_loss` | `media_decode.rs` `a_pass_gives_every_source_frame_once_in_order` |
| P-3 | `vfr`: CFR로 내보내고 원본 프레임을 하나도 버리지 않음 | `media_decode.rs` `the_variable_rate_output_shows_every_source_frame`·`a_faster_stretch_after_the_first_seconds_keeps_every_frame`, `video_export.rs` VFR 셋과 P-3 `vfr-30-15`(출력 쪽) |
| P-4 | 정상: 프레임 수 = spec, 길이 차 1프레임 안 | `media_verify.rs` `the_writers_output_passes`, 단위 `what_the_writer_makes_passes` |
| P-4 | 인코더가 한 프레임 덜 씀 → 게시 안 함 | `media_verify.rs` `an_output_a_frame_short_is_refused`(`FrameCount`) |
| P-4 | mux 뒤 끝 4 KiB 자름 → 게시 안 함 | `media_verify.rs` `an_output_cut_short_after_muxing_is_refused`(`Decode`) |
| P-4 | 금지 metadata, stream 구성 | `media_verify.rs` `an_output_with_metadata_is_refused`, 단위 `each_difference_from_the_expected_output_fails`(11가지) |
| P-4 | 검사 실패는 항목 실패(하드웨어면 소프트웨어로 한 번 더) | `video::process` 단위 `a_failing_hardware_encoder_is_replaced_by_software_for_the_item_and_the_run`·`a_software_retry_that_fails_too_fails_the_item` |
| P-5 | 641×361 → 642×362, 같은 좌표의 원본 pixel, 오른쪽·아래 가장자리 복제, 오디오 − 영상 1 ms 이하, 클릭 0·0.5·1.0·1.5초 | `media_encode.rs` `an_odd_size_is_padded_and_the_longer_audio_is_cut` |
| P-5 | 결과 표시 `padded` | `cli_video.rs` `a_video_and_a_photo_are_exported_in_one_run_with_the_video_masked`("odd size padded to 642×362") |
| P-5 | 오디오 선행: 영상·오디오 시작 0, 클릭 0.1·0.6·1.1·1.6초 ±0.02 | `media_encode.rs` `audio_that_starts_early_is_cut_at_the_videos_start`, 더 좁게 `video_export.rs` `sound_that_starts_before_the_video`(1 ms) |

## M4 완료 기준

- **합성 영상의 frame 일치·A/V 동기:** `video_export.rs` 11건(위). CFR·VFR·시작 오프셋·회전을 포함한다.
- **D1·P-3·P-4·P-5 시험 통과:** 위 표. 로컬 전체 시험(`cargo test --workspace --locked --all-features`, macOS)은 414개 통과(4단계 402개), 시작 상한을 고친 뒤 415개, fmt·clippy 두 조합 통과. 3개 OS CI는 이 단계의 PR에서.
- **소요:** 2026-10-21–22, 2일(예산 4주).

## 남은 것

- 이 단계의 PR CI: 첫 실행(`4adaa2b`)에서 3개 OS 시험·lint는 통과, fuzz는 위 결함으로 실패. 고친 커밋의 CI는 진행 중
- M4 밖으로 넘어간 것:
  - NVENC·QSV 실제 인코드와 Windows 4K 파이프 상한: M7·M8 실기기
  - 영상 검토 세션, 분석 해상도 디코드와 감지, ORT spin 측정: M5
  - Windows 콘솔 창(`CREATE_NO_WINDOW`)과 Job 시점: M6 GUI 전
  - 1패스 중 원본이 바뀌는 경우의 시험, 중간에 해상도가 바뀌는 스트림

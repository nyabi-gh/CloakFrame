# M4 4단계: 영상 처리기

작성 2026-10-22. [Rust 포팅 계획](RUST_PORT_PLAN.md) 8장 M4 4단계(`m4-video`)의 기록이다. 코드는 engine `video::process`(`VideoProcessor`), `media::verify`(P-4), `media_processor`(사진·영상을 한 실행에), CLI `run --videos`, 시험은 `media_verify.rs`·`cli_video.rs`와 `video::process`·`media::verify` 단위 시험이다. 실험은 모두 고정 FFmpeg 9.0.2(macOS arm64, Apple M4).

## 결정(2026-10-22, 메인테이너가 추천대로)

- **검토 모드:** `--videos`는 `--review off`일 때만. 영상의 검토 세션(입력 해시, `TimelineSpec`을 묶은 키, 트랙 결정)은 감지와 함께 M5에서 설계한다.
- **수동 마스크:** `--mask X,Y,W,H`(여러 번)를 각 영상의 표시 방향 pixel 좌표로 받아 모든 프레임에 고정한다(사진 검토의 추가 영역과 같은 좌표계). 프레임과 전혀 겹치지 않는 마스크는 그 항목을 실패시키고, `--videos`에 `--mask`가 없으면 사용법 오류다. M5 전에는 감지가 없어 아무것도 가리지 않은 영상이 "자동 게시"로 나가는 것을 막는다.
- **하드웨어 인코더:** `--hardware-encode on|off`(기본 on)로 감지 GPU(`--gpu`)와 따로 켠다(1.12.1도 따로였다). 하드웨어로 인코드한 항목이 인코드 실패(FFmpeg 실패·파이프 끊김)나 P-4 검사 실패로 끝나면 그 항목의 2패스를 소프트웨어로 다시 하고 실행 나머지도 소프트웨어다. 디코드 실패, 출력 파일을 만들지 못함, 취소는 다시 하지 않는다.
- **P-4 기준과 동시 처리:** 아래 [검사](#검사p-4). 영상 2패스는 한 번에 하나(사진 worker는 계속).

함께 정한 것(질문하지 않음): FFmpeg·ffprobe는 `--ort-library`처럼 `--ffmpeg FILE`·`--ffprobe FILE`로 받는다(번들 위치는 M7). 코덱 `--video-codec h264|hevc`(기본 H.264), 품질 `--video-quality high|balanced|space-saver`(기본 high, 1.12.1과 같음). 마스크 방식은 사진 기본값(검은 채우기).

## 흐름

1. **분석(1패스):** 원본 identity(파일 ID·크기·수정 시각, `SourceStamp`)를 기록 → `media::source::open`(조사·패킷 목록·출력 속도) → identity를 다시 비교 → 마스크가 프레임과 겹치는지 확인. M4에는 감지가 없어 프레임을 디코드하지 않는다. M5가 여기에 분석 해상도 디코드와 감지를 넣는다.
2. **2패스:** 인코더 잠금을 잡고 identity를 비교(패스 사이) → 인코더 고르기(`Encoders::pick`, 하드웨어 3프레임 시험) → `TempOutput`(출력 폴더 안 `.cloakframe-tmp-*.mp4`)에 `FrameWriter`(`Destination::Made`: 이미 만든 임시 파일에 FFmpeg가 씀) → 원본 해상도 `FrameReader`로 원본 프레임을 읽어 프레임마다 한 번 마스크(core 렌더러, 사진과 같은 함수)하고 `SourceFrames::shown` 순서로 쓴다(VFR 반복 프레임은 같은 마스크 결과를 다시 씀) → 남은 원본 프레임까지 읽어 디코드 수 확인 → `finish` → P-4 검사. 실패하면 위 규칙대로 소프트웨어로 한 번 더. 끝나면 identity를 다시 비교하고 홀수 크기면 결과에 "odd size padded to 642×362".
3. **게시:** runner가 `PublishPermit`을 발급한 뒤 identity를 마지막으로 비교하고 `TempOutput::publish`(fsync → no-replace rename, 사진과 같은 코드). 이미 같은 이름이 있으면 덮지 않고 항목 실패.

원본 사본은 어디에도 만들지 않는다(계획 0.1). identity가 바뀌면 "the source changed while it was processed"로 항목이 실패하고 임시 파일은 지워진다.

## 검사(P-4)

`media::verify::check`가 임시 파일에 대해:

- **ffprobe**(`-show_streams -show_format -show_chapters`): stream이 정확히 [영상, 원본 오디오 수만큼 오디오], chapter 없음. 영상 코덱(h264·hevc), 크기(패딩한 크기), D1 네 태그(이름이 아니라 값으로 비교: ffprobe가 `gamma28`로 낼 수 있는 것을 `bt470bg`와 같게 봄). 영상 길이는 프레임 수 ÷ 속도와 1프레임 안. 오디오는 영상 + 1프레임을 넘지 않음(원본 오디오가 먼저 끝나면 짧을 수 있음). 원본 오디오가 영상 끝까지 있으면(`start_pts + duration_ts`) 출력 오디오도 시작 + 길이가 영상 끝 − 1프레임에 닿아야 함([M0–M5 재검토](M0_M5_REVIEW.md) 2번). 언어는 spec과 같음. 태그는 allowlist만: format `major_brand·minor_version·compatible_brands·encoder`, stream `language·handler_name·vendor_id·encoder`.
- **전체 디코드:** `ffmpeg -xerror -i out -map 0 -fps_mode passthrough -f null -progress pipe:1 -`로 오디오까지 디코드하고, 마지막 `frame=`이 기대 프레임 수와 같아야 한다.

실험:

| 확인 | 결과 |
| --- | --- |
| 제목·위치·creation_time·Apple make/model을 넣은 원본을 writer 인자로 다시 인코드 | 남는 태그는 allowlist의 키뿐 |
| 끝 4 KiB를 자른 출력, `-xerror` | 종료 코드 183("Invalid NAL unit size") |
| 같은 파일, `-xerror` 없이 | 종료 코드 0, 1,799/1,800프레임 |
| 60초 1080p 전체 디코드(오디오 포함) | 0.56초(프레임 스레드). `ffprobe -count_frames`는 단일 스레드라 2.4초 |
| 한 프레임 모자란 출력 | 길이는 1프레임 허용 안이라 통과, 디코드 수에서 걸림 |

## 동시 처리

20초 1080p 두 개(Apple M4, load 4–8): libx264 `medium` 차례로 6.2초·함께 5.7초, VideoToolbox 5.3초·5.2초. 이득이 2–8%이고 인코더 메모리(4K libx264 lookahead)는 두 배라, `VideoProcessor`가 인코더 상태(`Encoders`)를 담은 잠금을 2패스 내내 잡아 영상은 하나씩 렌더한다. 분석(1패스)과 사진은 worker 수대로 돈다. 실행 단위로 하나뿐인 `Encoders`의 하드웨어 실패 표시도 같은 잠금으로 보호된다.

## 시험

- `media_verify.rs` 4건(P-4 fixture의 세 경우 + metadata): writer 출력 통과, 한 프레임 모자란 출력은 `FrameCount`, mux 뒤 끝 4 KiB를 자른 출력은 `Decode`, `location` 태그를 더한 출력은 `Metadata`.
- `media::verify` 단위 4건: 보고서의 항목마다 하나씩 바꾼 11가지가 모두 실패, 이름이 다른 같은 색 값, 먼저 끝나는 오디오.
- `video::process` 단위 6건: 하드웨어 인코드 실패·P-4 실패 → 소프트웨어로 한 번 더·실행 나머지 소프트웨어·결과 표시, 디코드 실패·취소·출력 파일 실패는 다시 하지 않음, 소프트웨어 실패, 다시 한 소프트웨어도 실패, 프레임 밖 마스크, 패스 사이·게시 직전에 원본이 바뀌면 실패하고 출력 폴더가 빔(실제 FFmpeg).
- `cli_video.rs` 4건: 사진과 P-5 영상을 한 실행에(영상은 642×362·"padded" 표시·오디오 유지·모든 프레임에서 마스크 안은 검정·밖은 원본과 가까움, 출력 폴더에 임시 파일 없음; 이 Mac에서는 VideoToolbox), 회전 fixture를 HEVC·소프트웨어로(회전이 pixel에 반영됨), 프레임 밖 마스크는 그 영상만 실패, 사용법 오류 6가지.
- mutation: 게시 직전·패스 사이 identity 비교를 빼면, 디코드의 `-xerror`를 빼면(잘린 파일이 프레임 수 확인에서 대신 걸려 기대 오류가 달라짐), 디코드 수 비교를 빼면 시험이 실패한다.
- 재시도 흐름은 FFmpeg 없이 함수(`encode_with_fallback`)로 시험한다. 실제 FFmpeg로도 재현할 수 있음을 확인했다: `-frames:v 3`을 준 인코더는 3프레임 시험을 통과하고 실제 인코드에서 파이프가 끊기거나(EPIPE) 프레임 수 검사에 걸린다. 장치가 없는 NVENC·QSV의 실제 실패는 M7·M8 실기기에서 본다.
- 로컬 전체(`cargo test --workspace --locked --all-features`, macOS): 402개 통과(3단계 384개).

## 남은 것

- 완료: PR #46(2026-10-22, main `8c9aa21`), 3개 OS CI와 탐지 벤치마크 통과. Windows·Linux에서는 이 PR이 영상 처리기·P-4·`cli_video.rs`의 첫 실행이었고, 하드웨어 인코더가 없는 runner라 소프트웨어로 골라졌다. 같은 실행의 1080p 파이프(debug 시험): 디코드 Linux 179 fps(1,116 MB/s)·macOS runner 236(1,470)·Windows 101(631), 인코드 Linux 164(1,017)·macOS runner 151(942)·Windows 82(507). 3단계 때(인코드 Linux 99·macOS 113·Windows 84)보다 Linux·macOS가 높은데 인코드 경로는 그대로라 runner 편차로 본다.
- 1패스 중(조사·목록 사이)에 원본이 바뀌는 경우는 같은 비교 함수를 쓰지만 따로 시험하지 않았다.
- 영상 검토 세션, 분석 디코드와 감지, 트랙별 모자이크 격자: M5.
- 5단계(`m4-verify`): 프레임 번호·오디오 임펄스 합성 영상으로 frame 일치·A/V 동기, D1·P-3·P-4·P-5 시험 표, P-3 `analysis_equals_output`([기록](M4_VERIFY.md)).

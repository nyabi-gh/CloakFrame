# M4 0–2단계 재검토

실행일: 2026-10-21. 대상: `m4-decode` `c70f2f6`(main `c1b7772` 위의 M4 2단계 커밋 2개, PR #43). 장치: Apple M4, macOS 27.0.1, 고정 FFmpeg 9.0.2. [계획](RUST_PORT_PLAN.md) M4의 0–2단계 작업과 문서·결정을 다시 보고 결함·위험·문서 오류를 찾은 기록이다. 코드는 고치지 않았다. 고칠 순서는 메인테이너가 정한다. 줄 번호는 `c70f2f6` 기준이다.

## 방법

- 읽은 것: 계획 3·4.4·8장 M4·9·11장, [M4_PROCESS](M4_PROCESS.md)·[M4_PROBE](M4_PROBE.md)·[M4_DECODE](M4_DECODE.md), [M0 파이프 기록](M0_FFMPEG_PIPE.md), [CHANGES](CHANGES_FROM_1.12.md), fixture D1·P-3·P-4·P-5의 `expected.json`, 0–2단계 코드와 시험, 1.12.1의 `VideoIo.cpp`·`VideoProcessor.cpp`(`v1.12.1` 태그).
- 2단계는 직접 봤고, 0단계(`media::process`, CLI 신호)와 1단계(`media::probe`)는 하위 검토 둘에 나눠 맡겼다. 보고된 항목 중 영향이 큰 것은 다시 재현하거나 코드로 확인했고, 확인 방법을 항목마다 적었다.
- 실험 입력은 고정 FFmpeg로 스크래치 폴더에 만들었고 저장소에는 넣지 않았다. 코드 실험은 `git clone --local` 복사본과 별도 `CARGO_TARGET_DIR`에서 하고 지웠다.
- PR #43의 CI(Linux·macOS·Windows 시험, fuzz workflow)는 모두 통과했고, 로컬 전체 시험도 351개가 통과했다.

## 요약

| # | 심각도 | 단계 | 내용 | 확인 |
| --- | --- | --- | --- | --- |
| 1 | 결함 | 2 | open GOP를 edit list 없이 자른 파일에서 디코더가 버리는 선행 프레임이 목록에 남아 항목 실패 | 재현 |
| 2 | 결함 | 1(결정) | VFR 출력 속도로 쓴 `r_frame_rate`가 원본 프레임을 버리거나 CFR을 크게 부풀림 | 재현 |
| 3 | 결함 | 1 | 길이가 적히지 않은 WebM을 "읽을 수 없음"으로 거부(1.12.1은 처리) | 재현 |
| 4 | 결함 | 1 | ffprobe의 색 이름 일부를 못 읽고, 모르는 태그를 높이 기본값으로 바꿈(D1) | 재현 |
| 5 | 결함 | 0 | nohup 등으로 무시된 SIGHUP·SIGINT를 CLI가 다시 받아 실행을 취소 | 코드, 하위 검토 재현 |
| 6 | 결함 | 0 | 터미널이 닫히면 결과 출력에서 panic해 검토 세션을 저장하지 못함 | 코드, 하위 검토 재현 |
| 7 | 위험 | 1·2 | 길이·프레임 수·해상도 상한을 헤더 값으로 피할 수 있음 | 재현 |
| 8 | 위험 | 1 | 옛 `rotate` 태그, 정수가 아닌 각도, 반전 행렬에서 spec과 디코드 크기가 다름 | 일부 재현 |
| 9 | 위험 | 0 | 두 번째 Ctrl+C의 종료가 드물게 SIGABRT | 하위 검토 재현 |
| 10 | 위험 | 2 | Windows에서 4K 내보내기의 파이프 상한을 아직 모름 | CI 수치 |
| 11 | 위험 | 0 | 읽지 않는 stdout 파이프에서 `wait`가 멈춤(지금 호출부 없음) | 하위 검토 재현 |
| 12 | 위험 | 0 | Windows: Job 지정 시점, 콘솔 창, Ctrl+Break | 읽기만 |
| 13 | 관찰 | — | ORT CPU 세션의 spin 대기가 같은 일에 CPU를 2–3배 씀 | 측정 |
| 14 | 관찰 | 1 | 태그 없는 영상의 HD 판정을 높이만으로 함 | 읽기만 |
| 15 | 문서 | — | 문서끼리 어긋나거나 낡은 곳 | — |

로컬 전체 시험에서 `tiles` 시험이 30분 넘게 걸린 일은 코드나 시험 문제가 아니었다(아래 [조사](#로컬-tiles-시험이-30분-걸린-이유)).

## 결함

### 1. 선행 프레임이 있는 파일의 프레임 수

`media::frames::list`(frames.rs:48)는 패킷 목록으로 원본 프레임을 정하고, `FrameReader`(decode.rs:170)는 디코드한 수가 목록과 다르면 항목을 실패시킨다. 디코더가 패킷을 받고도 프레임을 내지 않는 경우가 있다: open GOP의 keyframe(HEVC CRA, H.264 recovery point)에서 시작하는 스트림은 그 keyframe보다 먼저 보이는 선행 프레임(RASL 등)이 이전 GOP를 참조하므로 디코더가 버린다.

| 파일(고정 FFmpeg로 만듦: 30 fps, keyint 30, open GOP, 1초에서 `-c copy`로 자름) | 버림 아닌 패킷 | 디코드한 프레임 |
| --- | --- | --- |
| HEVC(libx265), 자르지 않음 | 120 | 120 |
| HEVC, edit list 있음(FFmpeg 기본) | 90 | 90 |
| HEVC, edit list 없음(`-use_editlist 0`) | 94 | 90 |
| H.264(libx264 `open-gop=1`), edit list 있음 | 90 | 90 |
| H.264, edit list 없음 | 91 | 90 |

edit list가 있으면 컨테이너가 선행 프레임을 버림(`D`)으로 표시해 목록과 디코드가 맞는다. edit list 없이 쓴 파일(다른 도구의 자르기, 방송·스트림 녹화의 다시 담기)은 "the video has 94 frames but 90 could be decoded"로 실패한다. 1.12.1은 디코드한 프레임만 `fps` 필터로 맞췄으므로 이런 파일도 처리했을 것이다(1.12.1로 실행해 보지는 않았다). `-xerror`는 1.12.1도 썼고, 이 경우 FFmpeg는 오류 없이 끝난다.

디코드로 목록을 만들면(ffprobe `-show_entries frame=…`) 두 파일 모두 정확히 90개가 나오지만, 1080p H.264 1분(1,800프레임)에 4.03초로 패킷 목록(0.03초)보다 훨씬 느리다. 제안: 시작이 깨끗하지 않을 때만(첫 패킷이 keyframe이 아니거나, 첫 GOP에 첫 keyframe보다 이른 pts의 패킷이 있음) 첫 GOP를 디코드해(ffprobe `-read_intervals %+#N`, `frame=best_effort_timestamp`) 실제로 나오는 프레임을 확인하고, 디코더가 버리는 패킷을 목록에서 뺀다. 수 확인은 나머지 경우의 안전장치로 둔다. 스크래치 스크립트로 확인한 결과 표의 다섯 파일, 자르지 않은 H.264, 1080p 1분 파일 모두에서 보정한 목록이 전체 디코드 목록과 같았고(94 → 90, 91 → 90), 시작이 깨끗한 파일은 추가 디코드가 필요 없었다. 시험은 고정 FFmpeg로 두 파일을 만들어 순서·미리보기 일치를 본다(세 OS 빌드 모두 libx264·libx265가 있음).

### 2. VFR 출력 속도

1단계 결정은 VFR 입력을 공칭 속도 `r_frame_rate`의 CFR로 바꾸면 원본 프레임을 버리지 않는다고 봤다(probe.rs:197–205, [CHANGES](CHANGES_FROM_1.12.md)의 VFR 줄). 그런데 `r_frame_rate`는 공칭 속도가 아니라 FFmpeg가 앞부분 타임스탬프로 추정한 값이다. 고정 ffprobe 결과(첫 줄은 이 재검토에서 만든 파일, 나머지는 하위 검토가 만든 파일을 다시 조사; 버려지거나 늘어나는 수는 하위 검토가 2단계 `SourceFrames`로 셈):

| 입력 | `r_frame_rate` | 결과 |
| --- | --- | --- |
| 앞 10초 15 fps, 뒤 2초 30 fps(MP4) | 15/1 | 뒤 구간 원본 60프레임 중 30프레임이 출력에 없음 |
| timescale 600, 간격 21·21·18 반복(평균 30 fps) | 200/1 | 원본 300프레임이 CFR 1,999프레임으로 늘어남 |
| timescale 600, 30 fps·24 fps가 섞임 | 120/1 | 132프레임 → 600프레임 |
| timescale 600, 30 fps·20 fps가 섞임 | 60/1 | 2–3배로 늘어남 |
| timescale 600, 간격 20–30(어두운 곳 촬영형) | 600/1 | 240을 넘어 평균 속도로 바뀌고, 빠른 구간 200프레임 중 10프레임을 버림(1.12.1과 같음) |
| 90 kHz, 큰 지터 | 90000/1 | 같음(300프레임 중 16프레임) |

VP9 VFR을 WebM으로 다시 담으면 DefaultDuration 때문에 `r_frame_rate`가 평균과 같아져 VFR로 보지도 않는다(하위 검토, 210프레임 중 25프레임을 버림). 제안: 출력 속도를 2단계의 패킷 목록으로 정한다. 표준 속도(23.976·24·25·29.97·30·48·50·59.94·60·90·100·120·144·240)와 원본 평균 속도 가운데 모든 원본 프레임이 적어도 한 칸에 나오는 가장 낮은 속도를 쓰고(칸 가운데 규칙으로 목록 전체를 한 번 훑으면 됨), 29.97과 30처럼 짝이 둘 다 되면 평균에 가까운 쪽, 그런 속도가 없으면 240. 스크래치 스크립트로 이 규칙을 실제 패킷 목록에 돌리면 위 표의 입력과 VP9 WebM, P-3 VFR fixture는 모두 29.97 또는 30 fps가 되어 버리는 프레임도 부풀림도 없고, CFR 입력(P-3 23.976·29.97 fixture, 1080p 30 fps 1분)은 원래 속도를 지킨다. 계획 8장 1단계 결정과 CHANGES의 VFR 줄은 이 결정에 맞춰 고친다.

### 3. 길이가 적히지 않은 WebM

`ffmpeg -f webm pipe:1`로 쓴 VP8·VP9 파일은 stream·format 길이가 모두 없어 `Unreadable`로 거부된다(probe.rs:218–221, 이 재검토에서 재현). 브라우저 MediaRecorder가 만든 WebM도 같은 구조다(하위 검토가 읽기로 확인). 1.12.1은 길이가 없으면 프레임 수 추정만 건너뛰고 처리했다(`VideoIo.cpp:603–617`). 제안: 길이가 없으면 패킷 목록의 마지막 시각으로 정하고, 길이·프레임 수 상한은 목록에서 잰다(7번과 함께).

### 4. 색 태그(D1)

- ffprobe는 transfer 4·5를 `bt470m`·`bt470bg`로 출력하는데 `Transfer`는 FFmpeg 옵션의 별칭인 `gamma22`·`gamma28`로 찾는다(timeline.rs:199–200). 그래서 세 값을 모두 bt470bg로 태그한 PAL 파일도 transfer를 smpte170m으로 가정하고 `assumed`가 된다(`h264_metadata`로 만든 파일로 재현).
- enum에 없는 태그는 태그가 없는 것으로 보고 높이 기본값으로 바꾼다(probe.rs:264–278): primaries `smpte432`(Display P3, 재현), matrix `gbr`·`ycgco`·`bt2020c`(하위 검토). D1(원본 태그를 그대로 디코드·인코드에 씀)과 M4_PROBE의 "원본이 태그하지 않은 필드만 가정"과 어긋난다.

제안: ffprobe가 출력하는 이름으로 읽고, 색역 변환을 하지 않는 primaries·transfer는 FFmpeg가 아는 값이면 그대로 넘긴다. swscale이 디코드할 수 없는 matrix(`ycgco`·`bt2020c`·`ictcp` 등)는 이유와 함께 거부한다.

### 5. 무시된 신호를 다시 받음

`ctrlc::set_handler`(cloakframe-cli.rs:817)는 덮어쓰기 모드라, 실행 전에 무시(`SIG_IGN`)로 설정된 SIGHUP·SIGINT에도 처리기를 건다(ctrlc 3.5.2 `init_os_handler(overwrite = true)`, 코드로 확인). `nohup cloakframe-cli run … &`은 터미널을 닫으면 취소되고 130으로 끝나며, 비대화형 셸의 백그라운드 작업은 SIGINT로 취소된다. 하위 검토가 SIGHUP을 무시한 채 띄운 CLI에 SIGHUP을 보내 재현했다. 제안: 처리기를 걸기 전에 신호의 현재 설정을 보고, 무시된 신호는 그대로 둔다.

### 6. 터미널이 닫힌 실행

터미널이 닫히면 SIGHUP으로 실행은 취소되지만, 결과를 쓰는 `println!`(cloakframe-cli.rs:426)이 EIO로 panic해 101로 끝나고, 그 뒤의 세션 저장(cloakframe-cli.rs:549–550)을 하지 못한다. 그 실행에서 검토를 기다리던 항목이 세션에서 사라진다. 처리기의 `eprintln!`(cloakframe-cli.rs:822)도 같은 이유로 panic하면 그 뒤의 신호로 즉시 종료되지 않는다. [M4_PROCESS](M4_PROCESS.md)의 "SIGHUP도 받아 결과를 출력한 뒤 130"과 다르다. 하위 검토가 pty를 닫아 재현했다(101, 세션 파일 없음; 같은 pty에서 SIGINT를 보내면 130과 세션 저장). 제안: 세션을 먼저 저장하고, 출력 오류는 panic 없이 넘긴다.

## 위험

### 7. 상한을 헤더 값으로 피함

조사 단계의 길이(86,400초)·프레임 수(300만) 상한은 컨테이너가 적은 길이와 프레임 수로 잰다(probe.rs:218–238). 출력 프레임 수 `SourceFrames::output_frames`는 패킷 목록의 실제 pts를 따르므로 둘이 다른 파일에서는 상한을 지나친다. 하위 검토가 mdhd 길이를 1로 고친 3프레임 MP4는 ffprobe 길이가 0.000065초인데 pts가 100,000초에 걸쳐 있다(이 재검토에서 다시 조사). 30 fps CFR이면 출력이 300만 프레임을 넘는다. 해상도도 첫 SPS로만 재므로 중간에 7000×6000(42 MP)으로 바뀌는 스트림이 통과한다(하위 검토; 2단계 축소 필터가 출력 크기를 고정하므로 이 프로세스의 메모리는 늘지 않는다). 아직 내보내기가 `output_frames`를 쓰지 않으므로(시험에서만 씀) 지금 결함은 아니다. 제안: 패킷 목록을 얻은 뒤 시각 범위와 출력 프레임 수를 상한과 다시 비교한다.

시각 계산의 overflow도 확인했다: 실제 ffprobe가 낼 수 있는 값(약분되지 않는 시간 기준 1/4294967291, 2^63에 가까운 pts, 240 fps 근처의 큰 분자·분모)에서 `Seconds` 비교와 `output_frames`가 debug·release 모두 맞는 값을 냈다. ffprobe가 내지 않는 u32 범위의 시간 기준 분자(`4294967295/1`)를 넣으면 `frame_start`가 넘친다(하위 검토). 해석기가 ffprobe처럼 int 범위만 받게 하면 막힌다.

### 8. 회전

- 옛 `rotate` 태그를 표시 행렬 값과 같은 부호로 읽는다(probe.rs:329–344, 시험은 `"270"` → `Quarter`). 옛 태그는 시계 방향 값이었고, FFmpeg 9는 이 태그를 디코드할 때 적용하지 않는다: `rotate=90` 태그만 있는 160×96 WebM을 고정 FFmpeg로 디코드하면 160×96 그대로인데(재현) spec은 96×160이 되고 2단계 `scale`이 그 크기로 늘린다.
- 각도 허용 ±1°: ffprobe는 각도를 정수로 잘라 출력하고, FFmpeg는 반올림한 각도가 정확히 90°의 배수일 때만 transpose한다. 90.6°이면 spec은 옆으로 눕지만 FFmpeg는 `rotate` 필터로 크기를 그대로 둔다(하위 검토 재현).
- 좌우 반전만 있는 행렬은 −180°로 보여 `Half`가 되지만 FFmpeg는 hflip만 한다(하위 검토).
- 회전한 프레임의 SAR은 원본의 역수가 된다(4:3 → 3:4). 3단계 `setsar`에서 쓴다.

제안: 옛 태그는 무시하고, 각도는 FFmpeg와 같은 규칙으로 정확한 90° 배수만 받으며, 반전이 있는 행렬은 이유와 함께 거부한다.

### 9. 두 번째 Ctrl+C

즉시 종료가 `std::process::exit`(cloakframe-cli.rs:819)라 C++ 정적 소멸자(ORT·CoreML)가 추론 스레드가 살아 있는 채로 돈다. 하위 검토가 CoreML로 SIGINT 두 번을 137번 보냈더니 1번이 SIGABRT("Pure virtual function called")로 끝났다(SIGINT 한 번 25번, CPU 12번에서는 없음). 제안: 즉시 종료는 `_exit(130)`(Windows는 `TerminateProcess`)로 정리 없이 끝낸다. 지금은 CLI가 도구를 띄우지 않지만, 나중에 stdin으로 받는 인코더가 있으면 그 도구는 EOF를 받고 임시 출력을 마저 쓴다.

### 10. Windows 파이프 상한

2단계 결정(모든 OS stdout 파이프)에 따라 CI가 디코드 파이프 처리량을 잰다: Windows 125 fps(775 MB/s), Linux 170 fps(1,060 MB/s), macOS runner 199 fps(1,238 MB/s), 1080p rgb24, debug 시험([기록](M4_DECODE.md#처리량)). 분석 크기는 30 fps에 47 MB/s면 되므로 충분하다. 4K 원본 내보내기는 디코드 쪽과 인코드 쪽에 각각 30 fps당 약 750 MB/s가 오가는데, 이 측정은 디코드·변환·파이프를 함께 잰 것이라 파이프만의 상한은 모른다. M0의 Linux runner 파이프 상한(FFmpeg 없음)은 약 1 GB/s였다([M0 기록](M0_FFMPEG_PIPE.md#linux-x86_64-github-actions-소프트웨어만)). 3단계 CI에서 인코드 방향도 같은 방식으로 재고, Windows 실기기(NVENC)의 4K 2패스 처리량은 M0 [실행 절차](M0_PLATFORM_RUNBOOK.md)로 잰다.

### 11. 읽지 않는 stdout과 `wait`

`reap`은 stdin만 닫고 stdout은 둔다(process.rs:335–337). stdout을 파이프로 띄우고 읽지 않은 채 `wait`를 부르면 파이프가 차서 멈춘다(하위 검토가 0.9 MB 출력으로 재현, 취소해야 풀림). 지금 호출부(`output`, `frames::list`, `FrameReader`)는 모두 끝까지 읽거나 kill하거나 놓는다. 3단계 writer에서 주의한다.

### 12. Windows(읽기만)

- Job 지정이 `CreateProcess` 뒤라(process.rs:461, `CREATE_SUSPENDED` 없음) 그 사이에 띄운 손자는 Job 밖에 남을 수 있다. FFmpeg는 자식을 띄우지 않아 낮다.
- `CREATE_NEW_PROCESS_GROUP`은 Ctrl+C만 막는다. Ctrl+Break와 콘솔 닫기는 FFmpeg에도 가고, FFmpeg가 스스로 실패로 끝나면 `wait`는 감시 스레드가 먼저 `killed`를 세웠는지에 따라 취소나 실패로 보고한다.
- `CREATE_NO_WINDOW`가 없어 M6 GUI에서 도구마다 콘솔 창이 열린다.

## 관찰

### 13. ORT CPU 세션의 spin 대기

CPU 세션은 1.12.1처럼 intra-op 스레드를 논리 코어 수(이 Mac 10개)로 두고 ORT 기본값대로 대기 중 spin한다(runtime.rs:169–177). `tiles.rs`의 astronaut 시험을 load 6에서 재면:

| 설정 | 시간 | CPU 시간 |
| --- | --- | --- |
| 기본(스레드 10, spin) | 3.5초 | 9.1초 |
| spin 끔 | 3.1초 | 3.7초 |
| 스레드 1 | 3.2초 | 3.0초 |

debug 시험이라 Rust 쪽이 대부분이고, macOS 백그라운드 QoS에서는 spin이 시간도 약 33% 늘렸다(15.7초 vs 11.8초). 1.12.1과 같은 설정이라 회귀는 아니다. 영상 1패스에서는 추론과 FFmpeg 디코드가 동시에 돌므로 M5 성능 측정(계획 9.3)에서 release 빌드로 다시 재고 정한다.

### 14. 태그 없는 영상의 HD 판정

태그가 없으면 높이 720 이상만 BT.709로 본다(timeline.rs:227). HD를 2.35:1로 잘라 낸 1280×544는 smpte170m이 되고 출력에도 그렇게 태그된다. mpv는 너비 1280 이상 또는 높이 576 초과를 BT.709로 본다. 1단계에서 D1 fixture의 제안대로 정한 규칙이라, 바꿀지는 메인테이너가 정한다.

## 문서

앞의 다섯 개는 이 재검토에서 고쳤다(커밋 전).

- 계획 8장 M4의 "결정(2026-10-21)"에 "파이프 형식은 bgr24로 시작"이 남아 있었다. 2단계 결정이 rgb24로 바꿨다는 표시가 없었다.
- [M4_PROBE](M4_PROBE.md) "프레임 n의 시각"이 "2단계의 디코드·미리보기는 이 한 함수(`frame_start`)로 프레임을 고른다"고 적었지만, 2단계는 칸 가운데 시각(`frame_middle`)과 `SourceFrames::shown_at`으로 고른다.
- [M4_DECODE](M4_DECODE.md) 처리량 표의 "조사·목록 포함"은 틀렸다. 시험은 조사·목록 뒤에 시간을 재기 시작한다.
- 계획 3장 동시성의 "작은 Tokio runtime은 자식 프로세스 감독(M4)에 필요할 때 추가": M4 0단계는 감시 스레드로 감독해 Tokio를 쓰지 않았다(Cargo.lock에 없음).
- 계획 4.2 표의 `TimelineSpec`에 "분석 해상도 변환"이 있었지만 크기는 `media::decode::frame_size`가 따로 정한다. 2단계가 더한 시간 기준과 원본 프레임 목록도 없었다.
- M0 기록이 M4로 미룬 결정 두 가지(Linux 소프트웨어 프리셋, VAAPI)가 계획 M4 3단계 줄에 없다. 3단계 결정과 함께 넣는다.
- [M4_PROBE](M4_PROBE.md)의 fuzz 불변식 "상한, 시작 ≥ 0, 시각 단조": `probe_fields`는 시각을 보지 않고(probe_fields.rs:86–91), `probe_report`는 `frame_start` 비교 하나만 하며 길이 상한은 보지 않는다(probe_report.rs:11–17).
- [M4_PROCESS](M4_PROCESS.md)의 "Windows 콘솔 닫기도 받음": 처리기가 TRUE를 돌려도 Windows가 프로세스를 끝내므로 취소 경로는 돌지 않고 정리는 Job Object가 한다. process.rs:309의 "`wait`는 취소로 보고"도 늦은 kill 규칙(이미 성공으로 끝난 도구는 성공)과 맞지 않는다.
- 1단계 자잘한 것(하위 검토): AV1·H.264가 든 WebM의 오류가 "MP4, MOV, WebM만"이라 WebM 사용자에게 헷갈림, 문서의 검사 순서(컨테이너 먼저)와 코드(영상 stream 먼저), 8비트 형식 목록 문장이 rgb24를 받는 것처럼 읽힘, WebM의 BCP-47 언어(`en-US`)는 버리고 두 글자 코드는 남지만 MP4 muxer가 나중에 버림(3단계에서 확인).

## 로컬 tiles 시험이 30분 걸린 이유

2단계 마무리의 전체 시험(2026-10-21 14:13 KST 시작)에서 `tiles` 시험 바이너리가 `ps`로 15분·29분째 CPU 290–330%로 돌고 있었고, 전체는 15:03에 끝났다. CI는 시험 전체가 2–4분이다.

- `pmset -g log`: 13:47:41 덮개를 닫아 잠들었고(Clamshell Sleep) 15:36에 열 때까지 15분마다 10–62초씩만 깨어났다(DarkWake). `tiles`는 14:18:48에 시작했고, 그 뒤 깨어 있던 시간은 14:33:47(20초), 14:47:37(10초), 15:02:53(약 16초)뿐이다. 지난 세션이 `ps`를 본 시각이 모두 이 DarkWake 안이다. 벽시계 30분은 거의 다 잠든 시간이었고, 실제로 돈 시간은 많아야 약 46초다.
- 같은 바이너리를 다시 재면 load 4에서 18.4초(CPU 61초), 바쁜 프로세스 12개로 load 17–27을 만들면 41초, macOS 백그라운드 QoS로 묶으면 84초였다. 당시 load 15–16에서 깨어 있던 약 46초와 맞는다.
- 같은 전체 시험(`cargo test --workspace --locked --all-features`)을 깨어 있는 상태에서 다시 돌리면(load 7–10) 351개가 통과하고 전체 84초, 그중 `tiles`는 18.4초였다.
- 따라서 코드·시험 결함은 없다. 긴 작업 중 덮개를 닫으면 같은 일이 생긴다(`request_keep_awake`는 덮개 잠자기를 막지 못함). 시험이 느려 보이면 먼저 `pmset -g log`로 잠든 구간을 본다.

## 판단 재검토

- 1단계 "VFR은 공칭 속도(`r_frame_rate`)의 CFR": 전제가 틀렸다(2번). 원본 프레임을 버리지 않는다는 목표는 맞고, 속도를 정하는 방법을 바꿔야 한다.
- 1단계 "태그 없는 필드의 기본값": 규칙 자체(14번)보다, 태그가 있는데 못 읽은 경우까지 기본값으로 바꾸는 것(4번)이 D1과 어긋난다.
- 2단계 결정 다섯 가지(Rust 대응 규칙, rgb24, matrix·range 명시와 색역 변환 없음, 정확한 축소, 모든 OS stdout 파이프)는 근거와 결과가 맞는다. x86 Linux·Windows에서 축소 해시가 같아 "CPU와 무관한 pixel"이 확인됐다. 다만 "디코드 수 ≠ 목록이면 실패"는 1번 경우를 고려하지 않았다.
- 0단계 결정(첫 Ctrl+C 취소, 두 번째 즉시 종료, Windows gyan essentials, FFmpeg 9.0.2)은 맞다. 구현이 5·6·9번에서 결정의 의도와 다르게 동작한다.
- 0.1절 결정과 어긋나는 곳은 없다(FFmpeg는 외부 프로세스, 감독은 정리 목적, protocol allowlist 유지).

## 문제없음으로 확인한 영역

- 0단계(하위 검토): kill과 `try_wait`가 한 잠금 아래에 있고 그룹 id가 회수되지 않은 leader의 pid라 재사용된 pid에 신호가 가지 않는다. stderr는 자기 스레드가 비우고, 취소는 막힌 읽기에도 닿으며, 오류·panic 때 drop이 도구 트리를 죽이고 회수한다. Job 핸들은 상속되지 않고 breakaway 제한이 없다. 실제 입력은 모두 `input_file`(probe.rs:99, frames.rs:62, decode.rs:126·237)을 거쳐 `-`나 `concat:`으로 시작하는 경로도 파일로만 읽힌다. M4_PROCESS의 옵션·`data:` 시험·8 KiB·늦은 kill·시험 수가 코드와 맞는다.
- 1단계(하위 검토): 표시 행렬의 부호(`rotated-90`의 블록 위치), 시작 시각(오프셋 fixture 5/4, edit list 없는 B-frame은 첫 pts), 8비트 형식 목록이 네이티브 디코더 출력을 덮고 10비트는 거부, `format_name`, 조각난 MP4의 길이, 표지 그림과 여분 stream, ffprobe가 `unknown`·`reserved` 색을 출력하지 않는 것, 상한 안의 산술, 시험·씨앗 수.
- 2단계: `-xerror`는 1.12.1도 같은 옵션을 썼다(`VideoIo.cpp:746`). 미리보기의 컨테이너 시작을 뺀 `-copyts -ss`와 정확한 pts 선택이 P-3 다섯 fixture의 모든 프레임에서 패스와 바이트까지 같다(3개 OS CI). edit list로 자른 파일(FFmpeg 기본 `-c copy` 자르기)은 HEVC·H.264 모두 목록과 디코드 수가 같다. 패킷 목록 줄 해석(`Packet::parse`)은 `?`로만 실패하고 색인·`unwrap`이 없다.

## 처리 (2026-10-21)

메인테이너가 추천대로 정했다(계획 8장 M4의 "결정(2026-10-21, 재검토)"). #43을 main `c70f2f6`에 넣은 뒤 `m4-review`에서 항목마다 실패하는 시험을 먼저 넣고 고친다.

- 2번(VFR 속도): core `SourceFrames::constant_rate`가 패킷 목록으로 출력 속도를 고르고, engine `media::source::open`(조사 + 목록 + 속도)이 앞으로 패스들의 진입점이다. `TimelineSpec`은 원본의 공칭·평균 속도를 따로 담는다. 시험: core 4건, `media_decode.rs` `a_faster_stretch_after_the_first_seconds_keeps_every_frame`(고치기 전에는 30 fps 구간의 15프레임이 빠져 실패). "없으면 240"은 "가장 많이 보이는 가장 낮은 속도"로 바꿨다([M4_DECODE](M4_DECODE.md)).
- 1번(선행 프레임): `media::frames::list`가 시작이 깨끗하지 않은 stream만 첫 GOP를 디코드해 디코더가 내지 않는 패킷을 목록에서 뺀다. 시험: `media::frames` 단위 1건(판단), `media_decode.rs` `leading_pictures_the_decoder_drops_are_not_listed`(고정 FFmpeg로 만든 H.264·HEVC open GOP를 edit list 없이 잘라 90프레임, 고치기 전에는 목록 91개로 실패).
- 3번·7번(길이 없는 WebM, 헤더로 피하는 상한): 조사 단계는 길이가 없으면 비워 두고, `media::source::open`이 패킷 목록으로 길이와 출력 프레임 수를 정한 뒤 길이(86,400초)·프레임 수(300만) 상한을 다시 잰다. 시험: `media::probe` 단위 1건, `media::source` 단위 2건, `media_decode.rs` `webm_without_a_duration_opens_from_its_frames`(고치기 전에는 `Unreadable`). 중간에 해상도가 바뀌는 스트림은 그대로다: 2단계 축소 필터가 출력 크기를 고정하므로 이 프로세스의 메모리는 늘지 않고, 해상도 변경을 찾으려면 전체를 디코드해야 한다.
- 4번·14번(색 태그, HD 판정): core 색 enum이 ffprobe 이름으로 읽고(옛 옵션 이름도), FFmpeg가 아는 primaries·transfer를 모두 담는다. `media::probe`는 태그가 있는데 모르는 이름이거나 디코드할 수 없는 matrix면 `Unsupported::Color`로 거부하고, 태그 없는 필드는 너비 1280 이상 또는 높이 576 초과를 HD로 본다(D1 fixture의 규칙 문장도 고침). 시험: core 단위 2건(크기 판정, 이름과 별칭), `media::probe` 단위 1건, `media_probe.rs` 2건(`h264_metadata`로 태그한 PAL·Display P3·YCgCo, 태그 없는 1280×544). mutation 3개(높이 720 판정, transfer 이름 `gamma28`만, 모르는 이름을 태그 없음으로) 모두 잡힘.
- 8번(회전): `media::probe`가 ffprobe의 표시 행렬 9개 값으로 FFmpeg와 같이 판단한다(반올림한 각도가 정확한 90° 배수일 때만, 행렬식이 음수인 반전은 `Unsupported::Mirrored`, 옛 `rotate` 태그 무시). 시험: `media::probe` 단위 1건(행렬 텍스트로 90°·−90°·180°·90.4°·90.6°·반전·0 행렬·태그), `media_probe.rs` `rotation_follows_what_ffmpeg_applies`(`-display_rotation 90.4`·`90.6`, `-display_hflip`, `rotate=90` 태그의 WebM; 고치기 전에는 90.6°를 받아 실패). 회전한 프레임의 SAR 역수는 3단계에서 쓴다.
- 5번(무시된 신호): CLI가 ctrlc 처리기를 걸기 전에 SIGINT·SIGTERM·SIGHUP의 설정을 읽고, 무시였던 것은 건 뒤 다시 무시로 둔다. 시험: `cli.rs` `interrupt::signals_ignored_at_start_stay_ignored`(`nohup`으로 띄운 실행에 SIGHUP을 보내도 1.5초 안에 끝나지 않고 SIGINT에는 130; 고치기 전에는 SIGHUP에 취소되어 130).
- 6번(터미널이 닫힌 실행): `run`과 `review`가 세션을 결과보다 먼저 저장하고, 결과·안내는 쓰기 실패를 버리는 함수로 쓴다(`println!`처럼 panic하지 않음). 시험: `cli.rs` `a_closed_output_still_keeps_the_review_session`(stdout을 바로 닫아도 종료 코드 3과 세션 1개; 고치기 전에는 101). M4_PROCESS의 "Windows 콘솔 닫기도 받음"도 고쳤다.
- 9번(두 번째 Ctrl+C): 즉시 종료를 `exit_now`(Unix `_exit`, Windows `TerminateProcess`)로 바꿨다. 경쟁이라 결정적인 시험은 없고, `cli.rs` `interrupt::a_second_ctrl_c_exits_at_once`가 종료 코드 130을 확인한다. Windows 쪽 코드는 CI가 처음 컴파일한다.

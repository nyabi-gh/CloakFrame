# M4 1단계: 조사와 TimelineSpec

작성 2026-10-21. [Rust 포팅 계획](RUST_PORT_PLAN.md) 8장 M4 1단계(`m4-probe`)의 기록이다. 코드는 core `timeline`(`TimelineSpec`과 시간 규칙), engine `media::probe`(ffprobe 실행과 보고 해석), fuzz는 `fuzz/`와 `.github/workflows/fuzz.yml`이다.

## TimelineSpec

ffprobe 보고에서 한 번 만들고 분석·미리보기·내보내기가 같이 쓴다(P-3). 담는 것:

- 처리할 영상 stream의 색인, 코덱, 코딩된 크기, 회전(시계 방향 0·90·180·270°), SAR.
- **속도:** 조사 단계는 공칭 속도(`r_frame_rate`, 없거나 240 fps를 넘으면 평균 속도)를 임시 속도로 두고 상한과 추정 프레임 수에 쓴다. 모든 패스가 쓰는 CFR 속도는 2단계가 패킷 목록으로 정한다([M4_DECODE](M4_DECODE.md)). `r_frame_rate`는 파일이 적은 값이 아니라 FFmpeg가 앞부분 타임스탬프로 추정한 값이라, 그대로 쓰면 원본 프레임을 버리거나 CFR이 부풀었다([재검토](M4_REVIEW.md) 2번).
- **시작:** 첫 영상 프레임의 표시 시각을 `start_pts × time_base`로 정확히(유리수) 둔다. 음수면 0.
- **프레임 n의 시각:** `start + n / rate`(정확한 유리수, `TimelineSpec::frame_start`). 2단계는 이 시각 대신 칸 가운데 시각(`frame_middle`)에 화면에 있던 원본 프레임을 고른다([M4_DECODE](M4_DECODE.md)).
- 길이, 추정 프레임 수(CFR이면 `nb_frames`, 아니면 `ceil(길이 × 속도)`; 상한과 진행률용). 흘려 쓴 WebM(`-f webm pipe:1`, 브라우저 녹화)은 stream·format 길이가 모두 없어 비워 두고, 2단계 `media::source::open`이 패킷 목록으로 길이와 출력 프레임 수를 정한다(처음 판은 이런 파일을 "읽을 수 없음"으로 거부했다, [재검토](M4_REVIEW.md) 3번).
- **색(D1):** matrix·primaries·transfer·range를 ffprobe가 출력하는 이름으로 읽는다(transfer 4·5는 `bt470m`·`bt470bg`, 옛 옵션 이름 `gamma22`·`gamma28`도 읽음). 인코더 옵션도 같은 이름을 받는다(고정 FFmpeg로 확인). 원본이 태그하지 않은 필드(없음, `unknown`·`reserved`)는 너비 1280 이상이거나 높이 576을 넘으면 bt709, 아니면 smpte170m, range는 tv로 채우고 `assumed`로 표시한다(mpv와 같은 크기 판정, 1280×544 같은 HD 크롭도 HD). 태그가 있는데 이 빌드가 모르는 이름이거나 디코드할 수 없는 matrix(`ycgco`·`bt2020c`·`ictcp` 등)이면 기본값으로 바꾸지 않고 이유와 함께 거부한다. `gbr`은 RGB로 코딩된 영상이라 디코드에 matrix가 필요 없다. 3단계 인코드가 같은 값을 태그로 쓴다. 처음 판은 높이 720 기준이었고, 모르는 이름을 태그 없음으로 봤다([재검토](M4_REVIEW.md) 4·14번).
- 오디오 stream(색인, 코덱, 소문자 언어 코드만: 두 글자는 3단계부터 세 글자로 바꿈, [기록](M4_ENCODE.md#언어)), 처리하지 않는 다른 영상 stream 수.

## 받는 입력과 거부

검사 순서는 1.12.1과 같다(컨테이너를 앞에 더함): 컨테이너 → 읽을 수 있음 → 해상도 → 속도 → 길이 → 프레임 수 → 코덱 → pixel 형식 → HDR → 회전.

- **스트림:** `attached_pic`이 아닌 첫 영상 stream. 1.12.1은 표지 그림이 앞에 있으면 그것을 처리했다.
- **컨테이너(내용으로 판정):** `format_name`에 `mov`·`mp4`가 있으면 MP4·MOV 계열. `matroska`·`webm`은 영상이 VP8·VP9일 때만(WebM). 그 밖(H.264 MKV, AVI 등)은 거부.
- **코덱:** H.264·HEVC·VP8·VP9.
- **pixel 형식:** 디코더가 내는 8비트 형식 목록(yuv420p·yuvj420p·yuv422p·yuvj422p·yuv444p·yuvj444p·yuv440p·yuvj440p·yuv411p·yuva420p·gray·gbrp·nv12·nv21). 1.12.1처럼 이름 끝 숫자로 비트 수를 읽으면 `nv12`·`rgb24`도 거부된다.
- **HDR:** transfer가 `smpte2084`(PQ)·`arib-std-b67`(HLG)이면 거부(M9 전까지).
- **회전:** FFmpeg가 디코드할 때 쓰는 규칙(fftools)을 따른다: 표시 행렬(ffprobe의 `displaymatrix` 9개 값)에서 각도를 구해 정수로 반올림하고, 정확히 90°의 배수일 때만 돌린다(시계 방향 90°·180°·270°). ffprobe의 `rotation`은 반시계 각도를 정수로 잘라 보여 주므로 90.6°도 90으로 나오지만 FFmpeg는 이를 임의 각도로 돌리며 크기를 그대로 둔다. 그런 각도와 좌우·상하 반전이 있는 행렬(행렬식이 음수, ffprobe에는 −180으로 보임)은 이유와 함께 거부한다. 옛 `rotate` 태그는 FFmpeg 9가 적용하지 않으므로 읽지 않는다(처음 판은 태그를 읽고 ±1°를 허용해 spec과 디코드 크기가 어긋날 수 있었다, [재검토](M4_REVIEW.md) 8번). `rotated-90.mp4`를 FFmpeg 자동 회전으로 디코드해 원래 좌상단의 프레임 번호 블록이 좌하단(x 0–59, y 400–639)에 오는 것으로 방향을 확인했다(시계 방향 270°).
- **상한(1.12.1과 같음, 4.7):** 한 변 16,384, 3,600만 pixel, 240 fps, 86,400초, 추정 300만 프레임. 파일 크기 상한은 없다(파이프로 흘려 읽음). 길이와 프레임 수는 패킷 목록을 읽은 뒤 다시 잰다: 컨테이너 헤더의 길이는 프레임과 다를 수 있다(mdhd 길이를 1로 고친 파일, 재검토 7번). 영상 시작 2^31초(약 68년) 상한과 목록 범위를 첫 프레임부터 재는 것은 5단계에 더했다(아래 찾은 결함).
- ffprobe 인자: `-v error -hide_banner -print_format json -show_streams -show_format -protocol_whitelist file -i file:<경로>`(0단계 `ToolCommand`).

## fuzz

계획 9.1의 fuzz 대상 두 곳 중 하나(다른 하나 EXIF는 같은 틀에 나중에). cargo-fuzz 0.13.2, libFuzzer, nightly-2026-10-01(공개 2주 넘은 날짜), `-s none`(파서가 안전한 Rust라 sanitizer 대신 빠른 실행). `fuzz/`는 workspace 밖이고 배포하지 않는다. libfuzzer-sys에는 LLVM의 NCSA 라이선스가 들어 있어 workspace의 cargo-deny 대상이 아니다. lockfile은 workspace의 판을 그대로 쓰고 libfuzzer-sys 0.4.13·arbitrary 1.5.0만 더한다.

- `probe_report`: fixture 9개의 ffprobe 보고(경로만 지움, `fuzz/seeds/`)와 JSON 키 사전으로 원시 바이트를 변형. 파서가 panic하지 않는지와 결과의 불변식(상한, 시작 ≥ 0, 길이 상한, 프레임 시각이 늘어남)을 본다.
- `probe_fields`: 필드값(크기·속도·시작 PTS·회전·형식 이름 등)을 `arbitrary`로 뽑아 문법이 맞는 보고를 만든다. 같은 불변식을 본다. 처음 판은 두 대상 모두 문서의 불변식을 다 보지 않았다(`probe_fields`는 시각을 보지 않음). 프레임 시각을 보게 하면서, ffprobe가 C `int`로 출력하는 유리수를 u32 범위까지 받던 것도 int 범위로 줄였다: 시간 기준 `4294967295/1`이면 `frame_start`가 i128을 넘었다([재검토](M4_REVIEW.md) 7번).
- **왜 둘인가:** 해상도 상한 검사를 지운 mutation을 `probe_report`는 사전이 있어도 2분(약 230만 번) 안에 찾지 못했다. 바이트 변형은 대부분 JSON을 깨뜨려 판단할 값에 닿지 않는다. `probe_fields`는 같은 mutation을 바로 찾았다.
- **찾은 결함:** `probe_fields`가 첫 실행에서 회전값이 아주 클 때(`1e300` 등) 회전 횟수를 i64로 바꾼 뒤 부호를 뒤집다 overflow로 panic하는 것을 찾았다. 값은 입력 파일이 정하므로 조작된 영상 하나로 처리 스레드가 죽을 수 있었다. 실수 상태에서 4로 나눈 나머지를 구하도록 고치고 단위 시험을 넣었다.
- **찾은 결함(2026-10-22, 5단계 PR #47의 CI):** `probe_fields`가 시간 기준 1577058301/2926592(한 tick 539초)와 시작 PTS 약 4×10¹⁶으로 시작이 약 2.1×10¹⁹초인 보고를 만들었고, `frame_start` 비교(`Seconds::cmp`의 교차 곱셈)가 i128을 넘어 panic했다(release에서는 틀린 비교). 1단계부터 있던 결함이다(main `8c9aa21`에서도 같은 입력으로 재현). 시작에 상한이 없었다. 조사가 시작 2^31초를 넘는 영상을 이유와 함께 거부하고(`Unsupported::Start`, 정확한 유리수로 비교), `media::source`는 길이 상한을 첫 프레임과 시작 중 이른 쪽부터 잰다(시작보다 훨씬 앞선 패킷 시각도 같은 계산을 넘길 수 있었음). int 범위의 시간 기준·속도에서 비교의 곱은 시작 2^31초에서 최대 125비트이고 약 2^34초에서 넘친다. 두 fuzz 대상의 불변식에 시작 상한을 더했다. 시험: `media::probe` `a_start_past_the_limit_is_refused`(찾은 보고, 상한 바로 위, 바로 아래의 약분되지 않는 시간 기준과 240 fps에 가까운 속도에서 프레임 시각 비교), `media::source` `limits_hold_on_the_frames_whatever_the_header_says`(시작 하루 전 프레임).
- CI(`fuzz.yml`): 파서·timeline·fuzz 파일이 바뀌는 main 대상 PR에서 Linux가 두 대상을 150초씩. 실패하면 입력을 artifact로 남긴다.

## 확인

macOS arm64(2026-10-21): core `timeline` 단위 5건, engine `media::probe` 단위 12건, `media_probe.rs` 11건(P-3·D1·P-5 fixture, 고정 FFmpeg로 만든 표지 그림·영상 둘·WebM·H.264 MKV·10비트 HEVC·PQ 태그 입력, 읽을 수 없는 파일, fuzz 씨앗이 모두 해석됨). 로컬 fuzz(회전 수정 뒤): `probe_report` 60초 116만 번(사전 사용), `probe_fields` 120초 158만 번 실패 없음. fuzz crate는 nightly clippy `-D warnings` 통과. workspace는 fmt·clippy 두 조합·전체 시험·cargo-deny 0.20.2 통과.

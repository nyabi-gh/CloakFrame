# M4 3단계: 인코드

작성 2026-10-22. [Rust 포팅 계획](RUST_PORT_PLAN.md) 8장 M4 3단계(`m4-encode`)의 기록이다. 코드는 engine `media::encode`(`FrameWriter`·`Encoders`), `media::language`, core `TimelineSpec::output_matrix`, 시험은 `crates/cloakframe-engine/tests/media_encode.rs`다. 실험은 모두 고정 FFmpeg 9.0.2(macOS arm64).

## writer

- **입력:** rgb24 원시 프레임을 stdin 파이프로, 표시 방향·표시 크기(홀수면 패딩한 크기)와 spec의 CFR 속도로 넣는다. stdout은 파이프로 두지 않는다(`Stdio::null`, [재검토](M4_REVIEW.md) 11번: 읽지 않는 stdout이 차서 `wait`가 멈추는 일이 없음). 쓰기가 실패하면 stdin을 닫고 FFmpeg의 종료 상태와 stderr로 원인을 알린다. 시작할 때 정한 프레임 수와 다르게 쓰거나 크기가 다른 프레임을 주면 실패한다.
- **출력 파일:** Rust가 `File::create_new`로 먼저 만들고 FFmpeg에는 `-y`로 그 파일에 쓰게 한다. 고정 FFmpeg 9는 출력 파일이 이미 있으면 "already exists. Exiting."을 내고도 **종료 코드 0**으로 끝난다(`-n`을 줘도 같음, 실험). 프레임이 파이프 버퍼에 다 들어가는 짧은 영상이면 writer가 파일을 쓰지 않고도 성공으로 볼 수 있었다. 시험: `an_existing_file_is_never_replaced`.
- **메타데이터:** `-map_metadata -1 -map_chapters -1 -movflags +faststart`(1.12.1과 같음), 형식은 MP4, HEVC는 `-tag:v hvc1`.

## 인코더

- **품질:** 1.12.1과 같다. CRF 고품질 18·균형 21·용량 절약 24, HEVC는 +5. NVENC `-preset p6 -rc vbr -cq N -b:v 0`, QSV `-preset slow -global_quality N`, VideoToolbox `-q:v (100 − 2 × CRF)`, 소프트웨어 libx264·libx265 `-preset medium -crf N`(모든 OS).
- **고르기(`Encoders::pick`):** 하드웨어를 켜면 후보(Windows·Linux NVENC → QSV, macOS VideoToolbox)마다 이 항목의 크기·속도·색 필터 그대로 검은 프레임 3장을 파이프로 인코드해 보고(`-f null`), 처음 성공한 것을 쓴다. 모두 실패하면 소프트웨어. 1.12.1은 같은 시험을 lavfi 입력으로 했다. 여기서는 실제 writer 경로라 필터·pixel 형식까지 같이 확인된다.
- **실패 뒤:** `note_hardware_failure` 뒤에는 그 실행의 나머지가 소프트웨어다(M3 GPU 규칙과 같음). 실패한 항목의 2패스를 소프트웨어로 다시 하는 일과 결과 표시는 4단계 처리기가 한다.
- 로컬(Apple silicon)에서는 VideoToolbox가 H.264·HEVC 모두 골라진다. NVENC·QSV는 장치가 없어 인자만 단위 시험으로 확인한다(M7·M8 실기기).

## 색(D1)

- 필터: `setsar`, `scale=out_color_matrix=M:out_range=R:flags=accurate_rnd+full_chroma_int+bitexact`, `format=yuv420p`, `setparams=colorspace=M:color_primaries=P:color_trc=T:range=R`.
- 태그는 `setparams`로 프레임에 붙인다. 인코더 옵션 `-colorspace -color_primaries -color_trc -color_range`만 주면 matrix·range만 남고 primaries·transfer는 `unknown`이 된다(libx264·libx265, 실험). FFmpeg 9 인코더가 프레임의 값을 쓰기 때문이다. VideoToolbox도 `setparams`로 네 값과 full range(`yuvj420p`)가 모두 남는다(실험).
- RGB로 부호화된 원본(matrix `gbr`)은 4:2:0으로 내보내므로 태그 없는 원본과 같은 크기 규칙의 matrix(너비 1280 이상 또는 높이 576 초과면 BT.709, 아니면 SMPTE 170M)를 쓰고 primaries·transfer·range는 원본 값(2026-10-22 결정).
- 시험: `the_output_keeps_the_sources_colour_description`(D1 세 입력의 출력 태그가 원본과 같고, 각자 태그로 디코드한 중앙 RGB 차이가 3 이하).

## 크기와 회전

- 홀수 크기는 Rust가 오른쪽 열·아래 행을 복제해 짝수로 만든 뒤 넘긴다(641×361 → 642×362, `encoded_size`). FFmpeg에 알리는 크기도 패딩한 크기다. 처음 구현은 표시 크기를 알려 libx264가 "width not divisible by 2"로 실패했고, VideoToolbox는 프레임을 잘못 읽어 640×360을 냈다(시험이 잡음).
- 원시 프레임에는 비율 정보가 없으므로 `setsar`를 늘 붙인다. 회전으로 가로세로가 바뀐 프레임은 원본 SAR의 역수(4:3 → 3:4, 재검토 8번).
- 시험: `an_odd_size_is_padded_and_the_longer_audio_is_cut`(642×362, 원본 좌표의 pixel이 같고 오른쪽·아래가 가장자리 복제), `a_turned_frame_takes_the_inverse_aspect`.

## 오디오

정책은 1.12.1과 같다: spec의 오디오 stream을 모두 순서대로, MP4에 담을 수 있는 코덱(AAC·MP3·AC-3·E-AC-3·ALAC)은 복사, 그 밖(Opus·Vorbis 등)은 AAC 192 kb/s.

### 자르기

- **시작:** 오디오 입력에 `-copyts -itsoffset -<영상 시작 시각>`을 줘 영상의 첫 프레임이 0이 되게 옮기고, `noise=drop='lt(pts*tb,-0.25)'`로 그보다 0.25초 넘게 앞선 패킷을 버린다. 0 앞의 패킷은 MP4 edit list가 가리고, 디코더는 그것으로 첫 프레임을 맞게 복원한다.
- **입력 `-ss`를 쓰지 않는 이유:** 계획은 "FFmpeg가 쓰는 edit list"였고 처음 구현은 `-ss`(영상 시작 − 컨테이너 시작)였다. 자르는 지점이 AAC 패킷 경계와 정확히 맞으면 FFmpeg는 그 앞 패킷을 복사하지 않는다. AAC는 앞 패킷과 겹쳐 더해야 프레임을 복원하므로 첫 프레임(21 ms)이 망가져 0초의 클릭이 사라졌다(컨테이너 시작 9.978667초·영상 시작 10초 시험 입력). `-copypriorss 1`이나 탐색 여유를 줘도 FFmpeg가 탐색 지점 앞 패킷을 버려 같았다(실험). 경계가 아닌 곳(P-5 0.4초)에서는 FFmpeg가 앞 패킷을 남겨 맞았다.
- **0.25초:** 8 kHz AAC 한 패킷(128 ms)과 MP3·AC-3 한 패킷보다 길다. 원본에서 영상보다 앞선 오디오는 이만큼만 파일에 남고(재생되지 않음), 나머지는 버린다. `-itsoffset`만 쓰면 영상 앞의 오디오가 전부 숨은 채 남았다(실험).
- **컨테이너 시작:** 1.12.1은 오디오에 `-ss <영상 stream 시작>`을 줬는데, FFmpeg가 `-ss`에 컨테이너 시작을 더하므로 컨테이너가 0에서 시작하지 않는 파일은 오디오가 어긋나거나(위 시험 입력에서는 오디오 0초) 사라졌다(재검토에서 넘어온 항목). `-copyts -itsoffset`은 원본 시각을 그대로 쓰므로 컨테이너 시작과 무관하다.
- **끝:** 출력 `-t`(프레임 수 ÷ 속도)는 끝을 덮는 패킷까지 남기므로 복사한 AAC는 한 프레임(최대 21 ms)까지 길어진다(641×361 시험 입력: 2.005초). `setts=duration='if(gt(PTS+DURATION,E/TB),E/TB-PTS,DURATION)'`로 마지막 패킷의 길이를 줄여 디코드한 길이가 영상과 같다(2.000초). AAC로 다시 인코드하는 stream은 `-t`가 그 stream의 첫 디코드 프레임부터 재므로, 영상보다 먼저 시작하면 앞선 만큼 짧았다([M0–M5 재검토](M0_M5_REVIEW.md) 2번). 그래서 디코드한 샘플을 `atrim=start=0:end=<길이>`로 자른다. 같은 bsf는 그대로 둬도 값이 같다.
- 시험: P-5 두 입력(오디오 − 영상 길이 1 ms 이하·클릭 0, 0.5, 1.0, 1.5초 / 영상이 0.4초에 시작하는 입력의 클릭 0.1, 0.6, 1.1, 1.6초), `the_audio_is_cut_from_the_videos_start_when_the_container_starts_elsewhere`, `audio_long_before_the_video_is_dropped_but_for_the_decoders_packets`(영상이 1.5초에 시작, 첫 오디오 패킷 −0.3초 이후·클릭 0초부터), `webm_audio_is_reencoded_with_its_language`.

### 언어

FFmpeg MP4 muxer는 세 글자 코드(`eng`·`ger`·`deu`·`qaa`)는 남기고 두 글자(`en`)는 조용히 버린다(실험). WebM·Matroska는 두 글자를 쓸 수 있으므로 `media::language`가 ISO 639-1 두 글자 184개를 ISO 639-2/T 세 글자로 바꾼다(2026-10-22 결정). 표에 없는 두 글자, `und`, 대문자, BCP-47(`en-US`)은 버린다. 조사 단계가 바꾸므로 spec의 언어는 늘 세 글자다.

## 처리량

`encode_throughput_1080p`(무시되는 시험, CI가 3개 OS에서 출력): 1080p rgb24를 stdin으로 libx264 `medium`에 300장. 같은 프레임만 넣어 인코더 일이 적고 파이프가 대부분을 차지한다. macOS arm64 로컬 374 fps(2,328 MB/s), debug 시험 빌드. PR #45의 CI(2026-10-22): Linux 99 fps(618 MB/s), macOS runner 113 fps(703 MB/s), Windows 84 fps(525 MB/s). 같은 실행의 디코드 파이프는 Linux 131 fps(818 MB/s), macOS runner 222 fps(1,381 MB/s), Windows 102 fps(637 MB/s)다. 분석 크기 30 fps에 필요한 47 MB/s보다 넉넉하다. 4K 30 fps 내보내기는 양쪽에 약 750 MB/s가 필요해 Windows runner 수치로는 모자랄 수 있으나, runner는 CPU가 적고 시험이 debug 빌드라 실기기 측정이 필요하다. Windows 실기기 NVENC의 4K 2패스는 M0 [실행 절차](M0_PLATFORM_RUNBOOK.md)로 잰다.

## 남은 것

- 완료: PR #45(2026-10-22, main `2e35d1f`), 3개 OS CI와 fuzz workflow 통과.

- NVENC·QSV의 색 태그와 full range: 장치 없음(인자는 단위 시험). 하드웨어 인코드 실패 뒤 소프트웨어 재시도, P-4, 게시, CLI 끝까지 내보내기는 4단계([기록](M4_VIDEO.md)).

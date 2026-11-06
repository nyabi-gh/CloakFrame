# M7: FFmpeg 직접 빌드

작성 2026-11-06, 브랜치 `m7-ffmpeg`(main `1d22153` 위). [ROADMAP](ROADMAP.md) M7 "Video in releases"와 [계획](RUST_PORT_PLAN.md) 10.1이다. 지금까지 고정한 제3자 FFmpeg는 대응 소스를 낼 수 없어(macOS Riedl 빌드는 x264 `master`를 빌드할 때마다 받음) preview는 사진만 가렸다. 고정한 소스 tarball로 직접 빌드하고, 그 소스와 스크립트를 대응 소스로 낸다.

## 결정(2026-11-06, 메인테이너가 추천대로)

- **빌드 위치:** 새 `ffmpeg.yml`이 세 OS 바이너리와 대응 소스 묶음을 고정 pre-release `ffmpeg-<FFmpeg 판>-<revision>`에 올리고, `native.toml`이 그 파일의 URL·해시를 고정한다. CI와 release workflow는 지금처럼 받기만 한다.
- **구성:** 엔진이 쓰는 것만 켠다([`tools/ffmpeg/components.sh`](../tools/ffmpeg/components.sh)). 시험은 입력을 만들고 출력을 재는 데 배포하지 않는 FFmpeg(`CLOAKFRAME_TEST_FFMPEG`, 지금 고정한 제3자 빌드)를 따로 쓴다.
- **하드웨어 인코더:** macOS VideoToolbox, Windows NVENC·QSV, Linux NVENC. Linux QSV는 VA-API 의존이 필요해 Linux 패키지 단계에서 다시 본다. 엔진은 없는 인코더면 시험 인코드가 실패해 소프트웨어로 간다(`media/encode.rs` `Encoders::pick`).
- **대응 소스:** 소스 묶음을 FFmpeg 고정 릴리스와, FFmpeg를 담은 모든 앱 릴리스에 함께 올린다. GPLv2 3조의 "같은 곳에서 소스를 받을 수 있게"를 가장 단순하게 지킨다.

## 함께 정한 것(질문하지 않음)

- **판:**
  - FFmpeg 9.0.2(9.0의 최신). `ffmpeg-9.0.2.tar.xz.asc`를 FFmpeg 릴리스 키 `FCF986EA15E6E293A5644F10B4322F04D67658D8`로 검증했다.
  - x264는 stable 브랜치 `b35605a`. VideoLAN이 릴리스 tarball을 내지 않아 GitLab이 만든 압축 파일을 해시로 고정했다. 같은 날 두 번 받아 해시가 같았다. GitLab이 압축 파일을 다시 만들면 해시가 달라져 빌드가 실패하는데, 내용은 커밋이 정하므로 다시 고정하면 된다.
  - x265 4.2(최신). videolan 미러와 bitbucket의 파일 해시가 같다.
  - NVENC 헤더(nv-codec-headers)는 12.1.14.0. FFmpeg 9.0이 받는 판 중 드라이버 요구가 가장 낮다(Windows 531.61·Linux 530.41.03). 12.2는 551.76, 13.0은 570 이상을 요구한다.
  - libvpl 2.17.0(최신). QSV dispatcher이고, Intel 드라이버의 런타임은 실행 중 찾는다.
- **라이선스:** `--enable-gpl`과 x264·x265로 GPL-2.0-or-later. `--enable-version3`는 켜지 않는다.
- **링크:** 모두 정적으로 링크한다. Windows는 `-static`으로 libgcc·libstdc++·winpthreads까지 넣어 Windows DLL만 가져온다.
- **빌드 환경:** macOS는 macos-15에서 최소 15.0. Linux는 ubuntu-24.04에서 빌드해 glibc 2.39(지원 하한)를 따른다. Windows는 ubuntu-24.04에서 mingw-w64(GCC 13 posix)로 크로스 빌드한다. Windows runner의 MSYS2보다 빠르고, Linux와 같은 도구로 만든다.
- **이름과 판:** `tools/ffmpeg/revision`이 같은 FFmpeg 판의 빌드 번호다. `ffmpeg -version`은 `9.0.2-cloakframe1`로 나온다. 릴리스 파일은 `CloakFrame-FFmpeg-9.0.2-1-<platform>.tar.xz`와 `…-source.tar`다. 다운로드 배지는 `-windows-x64.zip`로 끝나는 파일을 세므로 Windows 묶음도 `.tar.xz`로 해서 배지에 섞이지 않게 했다.

## 구성

| 종류 | 켜는 것 | 근거 |
| --- | --- | --- |
| protocol | file, pipe | 엔진의 `-protocol_whitelist`와 같다(`media/process.rs`) |
| demuxer | mov, matroska, rawvideo | 입력 MP4·MOV·M4V·WebM(계획 1.1), 인코드 파이프의 rgb24 |
| muxer | mp4, null, rawvideo | 출력 MP4, 시험 인코드와 출력 검사, 디코드 파이프 |
| video decoder | h264, hevc, vp8, vp9, rawvideo | 입력 코덱(계획 1.1) |
| audio decoder | AAC, MP3, AC-3, E-AC-3, ALAC, Opus, Vorbis, FLAC, DTS, TrueHD, AMR, IMA ADPCM, PCM 16종 | 복사할 수 없는 오디오는 디코드해 AAC로 다시 인코드한다 |
| encoder | libx264, libx265, aac, rawvideo, wrapped_avframe, pcm_s16le + OS별 하드웨어 | `-f null`은 wrapped_avframe·pcm_s16le로 인코드한다(출력 검사) |
| filter | setsar, scale, format, aformat, aresample, select, setparams, trim, atrim, null, anull, crop, transpose, hflip, vflip, rotate | 엔진이 쓰는 것과, ffmpeg 명령이 스스로 넣는 것(회전, 컨테이너 crop, `-ss`·`-t`, 형식 변환; `fftools/ffmpeg_filter.c`) |
| bsf | setts, noise, aac_adtstoasc, null | 오디오 끝(setts)과 앞부분(noise) 자르기, ADTS AAC 복사 |

- **처음 목록에서 빠졌던 것:** 출력 필터의 `setsar`와 오디오 앞부분을 버리는 `noise` bsf. 엔진 영상 시험을 lean 빌드로 돌려 찾았다.
- **점검:** [`tools/ffmpeg/check.sh`](../tools/ffmpeg/check.sh)가 그 OS에서 protocol이 file·pipe뿐인지, demuxer가 위 셋뿐인지, OS별 인코더가 있는지 본다. 이어서 rgb24 세 프레임을 엔진의 출력 필터와 두 소프트웨어 인코더로 MP4로 만들고 끝까지 다시 디코드한다. 지금 쓰는 제3자 빌드는 protocol 검사에서 실패한다(의도대로).

## 측정과 확인(2026-11-06, 이 Mac과 Docker)

| | macOS arm64 | Windows x64 | Linux x86_64 |
| --- | --- | --- | --- |
| 빌드한 곳 | 이 Mac(M4) | Docker arm64 Ubuntu 24.04에서 크로스 | Docker amd64 에뮬레이션 Ubuntu 24.04 |
| 빌드 시간 | 58초 | 컨테이너에서 apt 포함 3분(참고만) | (에뮬레이션이라 의미 없음) |
| ffmpeg / ffprobe | 12.5 / 12.3 MB | 20.0 / 19.8 MB | 18.4 / 18.2 MB |
| 가져오는 라이브러리 | libSystem, libc++, VideoToolbox 등 시스템 프레임워크, 최소 macOS 15.0 | KERNEL32, msvcrt, bcrypt, ole32, ADVAPI32, SHELL32, USER32 | libc, libm, libmvec, libgcc_s, libstdc++(glibc 2.38·GLIBCXX_3.4.32 이상) |
| check.sh | 통과 | CI(Windows runner)에서 | 통과(에뮬레이션) |

- 지금 쓰는 macOS Riedl 빌드는 ffmpeg·ffprobe가 각각 66 MB다.
- Linux는 libstdc++와 libgcc_s를 시스템 것으로 쓴다. 요구 판(GCC 13의 GLIBCXX_3.4.32)은 glibc 2.39를 담은 배포판들이 이미 넘는다. Linux 패키지 단계에서 AppImage가 함께 담을지 다시 본다.
- 대응 소스 묶음만으로 README대로(`CLOAKFRAME_NATIVE_CACHE`에 `sources`) 네트워크 없이 macOS 빌드가 되고 점검을 통과했다. 같은 소스로 두 번 빌드한 바이너리의 바이트가 같은지는 보지 않았다.
- macOS에서 우리 빌드를 엔진에 쓰고 입력은 시험용 FFmpeg로 만들어, 영상 시험 72개(`media_*`, `video_export`, `video_analysis`), `cli_video` 9개, `d7_sessions` 3개, 앱 시험 82개가 통과했다. VideoToolbox H.264·HEVC와 libx264·libx265로 1080p 인코드도 확인했다.
- 소스 묶음은 27.8 MB이고, 두 번 만들어 해시가 같았다.
- 이 PR의 CI 설정(두 변수 모두 지금의 제3자 빌드)으로 워크스페이스 시험 605개가 통과했고, fmt와 clippy 두 조합이 통과했다.

## 시험의 생성기 분리

시험은 lavfi 소스, libvpx·libopus, WAV·TS 입력, `setpts`, 출력 오디오를 재는 `-f s16le`로 입력을 만들고 출력을 잰다. 배포 빌드에는 이것들이 없다. 그래서 시험이 직접 돌리는 ffmpeg 명령은 `CLOAKFRAME_TEST_FFMPEG`(`tests/common` `test_ffmpeg()`, 앱 시험은 같은 변수)로, 엔진에 넘기는 경로만 `CLOAKFRAME_FFMPEG`로 했다. 입력을 다른 빌드가 만드니, 배포 빌드가 다른 인코더의 출력을 읽는지도 함께 본다.

- `inputs_open_only_local_files`는 엔진의 `-protocol_whitelist`가 HLS 재생 목록의 `data:` 조각을 막는지 본다. 배포 빌드에는 HLS도 다른 protocol도 없으니, 모든 protocol이 있는 시험용 FFmpeg로 돈다. 개발 빌드에 다른 FFmpeg를 줄 때 이 화이트리스트가 막는다.
- `ffprobe_reports_on_a_local_file`의 입력은 WAV에서 M4A(AAC)로 바꿨다.
- 이 PR에서는 아직 두 변수가 같은 제3자 빌드를 가리킨다(`ci.yml`, `video-throughput.yml`, CONTRIBUTING). 고정 릴리스를 `native.toml`에 넣는 다음 PR에서 `CLOAKFRAME_FFMPEG`가 우리 빌드로 바뀐다. `ffmpeg.yml`의 시험 job은 이미 그렇게 나눠 돈다.

## 빌드의 함정

- **x264의 상대 경로:** x264 configure는 소스를 빌드 폴더에서 본 상대 경로로 찾는다. macOS의 `/var`가 `/private/var`의 symlink라 경로가 틀어져서, 작업 폴더를 `pwd -P`로 잡는다.
- **x265의 판:** x265 CMake는 git을 찾았을 때만 `x265Version.txt`에서 판을 읽는다. git이 없으면 판이 비어 Windows 리소스 파일 단계에서 실패한다. 그래서 `build.sh`가 git을 요구한다.
- **x265의 `-lgcc_s`:** x265는 컴파일러의 암묵 라이브러리를 `x265.pc`에 적는다. mingw의 `libgcc_s.a`는 `-static`에서도 `libgcc_s_seh-1.dll`의 import library라서 지운다.
- **libvpl의 C++ 런타임:** libvpl 2.17의 `vpl.pc`는 라이브러리 끝을 `${CXX_LIB}`로 두는데 CMake가 그 변수를 정하지 않는다. 그래서 C로 정적 링크하면 C++ 런타임이 빠져 FFmpeg configure가 "libvpl >= 2.6 not found"로 멈춘다. `-lstdc++`를 덧붙인다. upstream 결함으로 보이지만 보고하지 않았다.
- **최소 macOS:** `MACOSX_DEPLOYMENT_TARGET`이 `make`를 돌리는 셸에 없으면 바이너리의 최소 OS가 빌드한 OS(27.2)가 된다. configure에 `-mmacosx-version-min=15.0`을 준다.
- **configure 실패:** `build.sh`는 FFmpeg configure가 실패하면 `ffbuild/config.log` 끝을 출력한다. 임시 폴더는 지워지므로 CI에서 원인을 보려면 이 출력이 필요하다.

## 병합(2026-11-06, PR #99)

- **CI:** `fa692d3`에서 6개 workflow가 통과했다(CI run 42182700651, FFmpeg run 42182700559). 탐지 벤치마크 기본 설정 얼굴 재현율은 87.9%로 기록과 같다. CI의 1080p 파이프(아직 제3자 빌드)는 디코드 macOS runner 184 fps·Linux 174·Windows 102, 인코드(libx264 `medium`) 86·153·83이었다.
- **FFmpeg workflow:** 빌드는 macOS 3분 반, Linux 5분, Windows 7분. 세 OS 모두 `check.sh`가 통과했고, 우리 빌드를 엔진 FFmpeg로 쓴 영상 시험 72개가 통과했다. Windows 빌드는 Windows Server 2025 runner에서 처음 실행됐다.
- **실패와 재실행:**
  - 첫 push `f9bd64d`: Windows 빌드가 ffmpeg.org에서 소스를 받다 TLS 연결이 끊겨 실패했다(`Connection reset by peer`).
  - `fa692d3`: Windows 빌드의 apt 설치가 45분 제한에 걸렸다. 메인테이너 승인으로 그 job만 다시 돌려 통과했다.
- 메인테이너 "fast-forward"로 main = `fa692d3`.

## 첫 태그 실행(2026-11-06)

`ffmpeg-9.0.2-1`을 `fa692d3`에 달았지만(run 42210180882) 게시하지 못하고 취소했다. 실패한 job이 있어 Publish job은 돌지 않았고, 릴리스는 만들어지지 않았다.

- **macOS:** x265를 설치한 직후 ffmpeg.org에서 소스를 받다 TLS 연결이 끊겼다. 다른 runner에서 같은 실패가 두 번째였다. 빌드 job 셋과 소스 묶음 job이 실행마다 각자 upstream에서 받으니, ffmpeg.org가 가끔 끊는 연결을 매번 맞을 수 있다.
- **Windows:** apt가 멈춘 것이 아니라 Ubuntu 미러가 초당 수십 KB로 느렸다(cmake 11 MB에 5분). 게다가 `mingw-w64` 메타 패키지는 i686과 win32 변형까지 받고, runner에 이미 있는 cmake·pkg-config도 apt로 다시 받았다. 컨테이너에서 잰 받을 양은 그 목록이 307 MB, nasm과 x86_64 posix 컴파일러만이면 81.5 MB다.
- **고친 것(`m7-ffmpeg-ci`, 메인테이너가 추천대로):**
  - 소스는 소스 묶음 job만 받는다. 받은 파일은 `actions/cache`(키는 `native.toml`의 해시)에 남기고, cache에서 꺼낸 것도 해시를 확인한다.
  - 빌드 job들은 그 묶음을 풀어 묶음 안의 스크립트로 네트워크 없이 빌드한다. 그래서 게시하는 바이너리가 함께 게시하는 대응 소스에서 만들어졌다는 것도 보장된다.
  - apt로는 nasm과, Windows는 x86_64 posix 컴파일러만 설치한다. CMake와 pkg-config는 runner에 이미 있다.
  - 컨테이너에서 이 패키지들만 설치하고 `unshare --net`으로 네트워크를 끊은 채 묶음으로 Windows 빌드를 해, 같은 결과를 확인했다.
  - cache는 브랜치마다 따로라 PR의 cache를 태그 실행이 쓰지 못한다. 태그 실행은 소스 묶음 job에서 한 번 upstream에 닿는다. 거기서 끊기면 그 job만 다시 돌린다.
- 지금 태그는 릴리스를 만들지 않았으니 지우고, 고친 main에 같은 이름으로 다시 단다. 빌드 내용은 그대로라 revision은 1로 둔다.

## 남은 것

- 병합 뒤 태그 `ffmpeg-9.0.2-1`로 고정 릴리스를 게시하고, 다음 PR에서 할 것:
  - `native.toml`에 고정하고 `CLOAKFRAME_FFMPEG`를 우리 빌드로 바꾸며, 제3자 빌드는 시험 전용 항목으로 옮긴다.
  - macOS 번들과 Windows 설치본에 넣고, release workflow가 소스 묶음을 함께 올린다.
  - THIRD_PARTY_NOTICES에 FFmpeg, x264, x265, NVENC 헤더, libvpl, mingw-w64 런타임을 넣는다.
  - PREVIEW_NOTES·README의 "사진만"을 고친다.
  - 위 오디오 디코더 밖의 코덱(옛 QuickTime의 QDM2, 휴대폰의 QCELP 등)은 다시 인코드할 수 없어 항목이 실패할 것으로 본다(확인 안 함). 1.12.1의 전체 빌드와 다른 동작이니 시험으로 확인하고 CHANGES_FROM_1.12에 적는다.
- 고정 릴리스를 내는 절차는 [RELEASING](RELEASING.md#ffmpeg-builds)에 있다.
- 메인테이너 Windows PC(RTX 5080, Intel 내장 GPU)에서 NVENC·QSV로 실제 인코드.
- ROADMAP의 "Video in releases"는 영상이 든 릴리스가 나가야 닫는다.

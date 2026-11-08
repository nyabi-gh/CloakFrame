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

## 병합(2026-11-06, PR #100)

- **CI:** `5d054ee`에서 CI(run 42214542334)와 FFmpeg workflow(run 42214542127)가 통과했다. 세 빌드 job 모두 소스 묶음을 풀어 묶음의 스크립트로 오프라인 빌드했고, upstream에서 받은 것은 소스 묶음 job뿐이었다.
- **시간:** Windows apt 설치는 13초(전에는 25초에서 45분 넘게까지)였다. 빌드는 Windows 6분 20초, Linux 4분 15초, macOS 2분 30초.
- 메인테이너 "fast-forward"로 main = `5d054ee`. #99 뒤 main push CI(run 42189495589)도 `fa692d3`에서 통과했다.

## 게시(2026-11-06)

릴리스가 없던 태그 `ffmpeg-9.0.2-1`(`fa692d3`)을 지우고 `5d054ee`에 다시 달았다(run 42217214136). 모든 job이 통과해, pre-release `CloakFrame-FFmpeg-9.0.2-1`이 12:34 UTC에 게시됐다. Latest는 1.12.1 그대로다.

| 파일 | 바이트 | SHA-256 |
| --- | --- | --- |
| `CloakFrame-FFmpeg-9.0.2-1-macos-arm64.tar.xz` | 6,742,724 | `e83d87db006c0bf9a176fc7adfdab2ab4a542450d6e7c4dd083f51fbef287258` |
| `CloakFrame-FFmpeg-9.0.2-1-linux-x86_64.tar.xz` | 8,949,876 | `3da24fbe766302eb2f398d957938b5596f051e375af6a854776d0b30f4921e72` |
| `CloakFrame-FFmpeg-9.0.2-1-windows-x64.tar.xz` | 9,646,092 | `e57b63eb17e2ffdbe5627e9ae7e537f2e81a56e745bf5df44cfeb335cc0e2cae` |
| `CloakFrame-FFmpeg-9.0.2-1-source.tar` | 27,832,320 | `61423fd99d866169236644d9d4aafbb7eef1bdae92f58aee49b405d5782f032b` |

- 올라간 파일을 받아 `SHA256SUMS.txt`와 해시가 맞는 것을 확인했다.
- 게시된 소스 묶음은 main에서 이 Mac으로 만든 묶음과 바이트까지 같다.
- macOS 묶음을 풀어 이 Mac에서 `check.sh`가 통과했다.

## 패키지에 넣기(2026-11-06, `m7-ffmpeg-bundle`)

위 게시 묶음을 `native.toml`에 고정하고 macOS 번들과 Windows 설치본에 넣었다. 영상은 이 PR부터 패키지에서 가려진다.

### 결정(2026-11-06, 메인테이너)

- **구성 밖 오디오(추천대로):** 항목을 분석 전에 실패시키고 "오디오 제거"를 켜면 처리된다고 알린다. 빌드는 그대로 작게 둔다. 아래 "구성 밖 오디오" 절.
- **`updates` 릴리스의 소스(추천대로):** 업데이트 패키지(nupkg)도 FFmpeg를 담아 `updates` 릴리스에서 내려받으니, 대응 소스 묶음을 거기에도 올린다.
- **순서:** 영상이 든 preview.3를 먼저 내고, 메인테이너 Windows PC의 NVENC·QSV 확인은 그 다음에 한다(추천은 반대 순서였다).

### 고정과 시험

- `native.toml`의 `ffmpeg`(`use = "bundle"`)는 이제 세 OS 모두 게시 묶음이다. ffprobe·`BUILDINFO.txt`·`licenses/`가 ffmpeg 옆에 있어 macOS용 ffprobe 항목이 따로 없다. 대응 소스 묶음은 `ffmpeg-corresponding-source`(`use = "attach"`, 새 값)다.
- 제3자 빌드는 `test-ffmpeg`(`use = "test"`, 새 값)로 옮겼다. 시험이 입력을 만들고 출력을 재는 데만 쓴다. macOS 제3자 ffprobe는 쓰는 곳이 없어 뺐다.
- `ci.yml`과 `video-throughput.yml`은 엔진에 우리 빌드를, `CLOAKFRAME_TEST_FFMPEG`에 `test-ffmpeg`를 준다. `ffmpeg.yml` 시험 job도 `test-ffmpeg`를 받는다. CONTRIBUTING의 변수 예도 같다.
- 영상 벤치마크 클립(`tools/benchmark/video_fetch.py`)은 시험용 FFmpeg로 자른다. 다시 인코드한 클립의 해시는 그 빌드의 x264 출력이고, 우리 빌드의 x264는 다른 판이다.

### 패키지

- **macOS:** `Contents/MacOS`에 ffmpeg·ffprobe를 두고 dylib과 함께 ad-hoc 서명한 뒤 앱을 서명한다. 라이선스와 `BUILDINFO.txt`는 `Contents/Resources/FFmpeg`. 서명 뒤 번들 안의 ffmpeg·ffprobe가 실행되는지 본다.
- **Windows:** 앱 폴더에 ffmpeg.exe·ffprobe.exe, `FFmpeg\`에 라이선스와 `BUILDINFO.txt`. `release.yml`은 설치한 뒤 설치된 ffmpeg.exe·ffprobe.exe를 실행해 본다.
- **릴리스:** `release.yml`의 초안 job이 `ffmpeg-corresponding-source`를 해시로 받아 FFmpeg pre-release의 이름 그대로(`CloakFrame-FFmpeg-9.0.2-1-source.tar`) 첨부한다. `publish-feed.yml`은 게시된 릴리스의 `*-source.tar`를 받아 nupkg보다 먼저 `updates`에 올린다. 둘 다 태그·게시 때만 도는 단계라 PR CI로는 확인하지 못한다. 다운로드 배지는 `-source.tar`를 세지 않는다.

### 고지

THIRD_PARTY_NOTICES에 FFmpeg 절을 넣었다: 번들 방식과 GPL-2.0-or-later, 대응 소스가 있는 곳, FFmpeg·x264·x265의 저작권 줄, NVENC 헤더와 libvpl의 MIT 전문, MinGW-w64 런타임 고지, GPL-2.0 전문. 정보 창이 이 파일을 보여 준다.

- **MinGW-w64 런타임:** Windows ffmpeg.exe에 winpthreads가 정적으로 들어 있다(바이너리에 `mingw-w64-libraries/winpthreads/src/rwlock.c` 문자열). 고지 원문은 빌드에 쓴 Ubuntu 24.04 소스 패키지 mingw-w64 11.0.1의 `COPYING.MinGW-w64-runtime.txt`와 winpthreads `COPYING`을 그대로 넣었다. libgcc·libstdc++는 GCC Runtime Library Exception이라 그 사실만 적었다.
- 런타임과 컴파일러 라이브러리의 소스는 대응 소스 묶음에 넣지 않았다. 컴파일러와 함께 배포되는 구성 요소로 보았고(GPLv2 3조의 예외), 판은 고지에 적었다. 법률 검토는 하지 않았다.

### 구성 밖 오디오

엔진은 AAC·MP3·AC-3·E-AC-3·ALAC를 복사하고 나머지를 AAC로 다시 인코드한다. 복사하는 오디오도 출력 검사에서 디코드한다. 우리 빌드에 디코더가 없는 오디오를 실험했다.

| 입력(MOV, 시험용 FFmpeg로 만듦) | ffprobe(우리 빌드) | ffmpeg 디코드(우리 빌드) |
| --- | --- | --- |
| iLBC(`ilbc_at`), Nellymoser, ADPCM MS | 코덱 이름을 알아봄 | "Decoding requested, but no decoder found" |
| PCM의 `stsd` 네 글자를 `QDM2`·`QDMC`로 바꾼 파일 | `qdm2`·`qdmc` | 같은 오류 |
| 같은 방법의 `Qclp`·`MAC3` | demuxer가 "unaligned chunk"로 거부 | (열리지 않음) |

- 마지막 줄은 실제 QCELP·MACE 파일이 아니라 PCM 자리에 넣은 가짜라서 거부된 것이다. 실제 파일로는 확인하지 않았다.
- 고치기 전 엔진은 첫 패스(분석)를 마치고, 검토가 필요하면 검토까지 마친 뒤 내보낼 때 "cannot encode the video: ffmpeg failed … no decoder found for: qdm2"로 실패했다(앱 문구 "The output could not be encoded.").
- **고친 것:** 오디오를 남기는 설정이면 분석 전에 `ffmpeg -codecs`로 디코드할 수 있는 오디오 코덱을 읽어(이 Mac에서 10 ms 안) 남길 오디오가 모두 들어 있는지 본다. 없으면 새 실패 종류 `UnreadableAudio`로 실패하고, 앱은 "이 영상의 오디오는 CloakFrame이 읽을 수 없는 형식입니다. 소리 없이 처리하려면 고급 옵션에서 '영상에서 오디오 제거'를 켜세요."(영·일 같은 뜻)를 보인다. 빌드에 고정 목록을 두지 않고 실행하는 FFmpeg에 묻는다.
- **시험:** `video_export.rs` `sound_ffmpeg_cannot_decode_fails_before_the_first_pass`가 ADPCM MS 오디오의 MOV로, 오디오를 남기면 감지기가 프레임을 하나도 받지 않고 `UnreadableAudio`로 실패하며 "오디오 제거"면 내보내지는 것을 본다. 확인 호출을 빼면 `Encode`로 실패하는 것을 확인했다. `-codecs` 목록 읽기는 단위 시험이 따로 본다.
- 1.12.1과 다른 동작이라 [CHANGES_FROM_1.12](CHANGES_FROM_1.12.md)에 적었다.

### 확인

- 이 Mac에서 엔진 FFmpeg를 게시된 macOS 묶음으로, 시험 FFmpeg를 `test-ffmpeg`로 두고 `cargo test --workspace --all-features` 607개가 통과했다(무시 5개). fmt와 clippy 두 조합도 통과했다.
- `fetch_native.py`로 macOS `ffmpeg`, `test-ffmpeg`, `ffmpeg-corresponding-source`를 받아 해시가 맞는 것을 확인했다.
- `macos_preview.sh`로 release 앱의 zip을 만들어 풀었다. `codesign --verify --strict --deep`이 통과했고, 번들 안의 ffmpeg·ffprobe는 ad-hoc 서명이며 실행된다. zip은 44.4 MB다(preview.1은 34.9 MB).
- Windows 설치본은 이 Mac에서 만들 수 없어 PR CI로 확인했다(아래 "PR CI" 절). 릴리스 초안의 소스 첨부와 `updates`로 올리는 단계는 태그·게시 때만 돈다.

### PR CI의 소스 job(2026-11-06)

- PR #101의 FFmpeg workflow(run 42223898512)에서 소스 묶음 job이 두 번 실패했다. 첫 시도는 ffmpeg.org 연결 시간 초과(Errno 110), 메인테이너 승인으로 다시 돌린 둘째 시도는 연결 끊김(Errno 104)이었다. 같은 시각 이 Mac에서도 ffmpeg.org는 여섯 번 중 세 번 20–30초 안에 응답하지 않았다.
- 이 PR은 소스를 바꾸지 않았는데도 upstream에서 받았다. cache 키가 `native.toml` 전체의 해시라 이 PR에서 달라졌고, 있던 cache는 PR #100과 태그의 ref에 있어 이 PR이 읽을 수 없었다. 이 workflow는 main push에서 돌지 않아 main의 cache도 없다. 키를 소스 항목만으로 좁혀도 새 ref마다 처음에는 받아야 한다.
- **고친 것:** 소스 job이 먼저 `native.toml`에 고정한 게시 대응 소스 묶음(GitHub 릴리스)을 받아 그 안의 upstream 원본을 받은 파일 자리에 푼다. 원본은 묶을 때 `native.toml`의 해시로 다시 확인되고, 바뀐 소스만 upstream에서 받는다. 다음 revision의 태그 실행도 바뀌지 않은 소스는 ffmpeg.org에 닿지 않는다.
- 확인: 네트워크를 끊은 컨테이너(GNU tar 1.35)에서 이 단계와 `source_bundle.py`가 묶음을 만들었고, 원본·스크립트는 게시된 묶음과 바이트가 같다(다른 것은 바뀐 `native.toml`뿐).

### PR CI(2026-11-06–07, `0633595`)

- **통과:** FFmpeg workflow(run 42226261312)는 소스 job이 게시 묶음에서 원본을 꺼내 10초 만에 끝났고, 세 OS 빌드와 점검·엔진 영상 시험이 통과했다. Release(run 42226261256)는 macOS zip과 Windows 설치본을 만들었고, 설치한 ffmpeg.exe·ffprobe.exe가 `9.0.2-cloakframe1`로 실행됐으며 앱 창이 열리고 제거됐다. CI의 Test(macOS·Ubuntu), 영상 처리량, 탐지 벤치마크, ORT 비교도 통과했다.
- **실패(코드와 무관, 외부 받기):**
  - Format, lint and licenses: `apt-get install nasm`이 20분 동안 끝나지 않아 job 제한 시간에 취소됐다(첫 시도, 11-06).
  - Test (windows-2025): 시험 전 네이티브 파일 받기 단계에서 `HTTP Error 503: Service Unavailable`. 첫 시도(11-06 13:32)와 메인테이너 승인으로 그 job만 다시 돌린 둘째 시도(11-07 12:37) 모두 4초 만에 같은 곳에서 실패했다. 같은 단계가 3d36050(run 42223898518)에서는 7초 만에 통과했고, 그 뒤 바뀐 것은 `ffmpeg.yml`과 문서뿐이다. `fetch_native.py`가 실패한 URL을 출력하지 않아 nuget·GitHub 릴리스·gyan.dev 중 어느 곳의 503인지 모른다. 이 Mac에서는 셋 다 정상 응답했다.

### PR CI 녹색과 병합(2026-11-07, PR #101)

- `614abfd` push의 CI(run 42382343605)에서도 Test (windows-2025)가 받기 단계에서 5초 만에 같은 503으로 실패했다. 이 Mac에서는 그 단계가 받는 세 URL(nuget의 Windows ML, 게시 FFmpeg 묶음, gyan.dev의 `test-ffmpeg`)이 모두 200이었다.
- `7a7fe11`에서 `fetch_native.py`가 받기에 실패하면 `Downloading <URL> failed: <오류>`로 끝나게 했다(httpbin 503으로 확인). 그 push의 CI(run 42382627428)에서는 같은 단계가 통과해 503이 다시 나지 않았고, **어느 서버였는지는 아직 모른다.** 다시 나면 로그의 URL로 원인을 본다. Format, lint and licenses의 `apt-get install nasm`도 이번에는 멈추지 않았다(2분 10초).
- `7a7fe11`에서 CI·FFmpeg(run 42382627420)·Release(run 42382627498)·처리량·벤치마크·ORT가 모두 통과했다(태그 전용 job 셋은 건너뜀). 메인테이너 "fast-forward"로 main = `7a7fe11`, PR #101은 merged.
- 이 세션의 Auto-fix는 앱이 "이 세션이 연 PR이 아니다"로 켜지 못하게 했다.

## 영상이 든 첫 릴리스(2026-11-08)

- 2.0.0-preview.3가 우리 FFmpeg 9.0.2-1을 담아 나갔고, 릴리스와 `updates` 릴리스에 대응 소스가 올라갔다. 받은 macOS zip은 앱을 허용한 뒤 번들 안의 ffmpeg·ffprobe가 따로 막히지 않고 VideoToolbox로 인코드했다([기록](M7_RELEASE.md#세-번째-릴리스2026-11-08-영상)).

## 남은 것

- 메인테이너 Windows PC(RTX 5080, Intel 내장 GPU)에서 NVENC·QSV로 실제 인코드.
- 고정 릴리스를 내는 절차는 [RELEASING](RELEASING.md#ffmpeg-builds)에 있다.

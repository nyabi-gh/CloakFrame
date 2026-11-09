# M7: Linux 패키지

작성 2026-11-09, 브랜치 `m7-linux-package`(main `af2be6e` 위). [ROADMAP](ROADMAP.md) M7 "Installers"·"Updates"의 Linux 부분이다.

## 결정(2026-11-09, 메인테이너가 추천대로)

- **nasm:** `ci.yml`과 같은 Ubuntu 패키지. 설치가 멈추면 그 job만 다시 돌린다.
- **libstdc++:** AppImage에 넣지 않는다. 번들 FFmpeg·ORT가 요구하는 GLIBCXX_3.4.32는 지원 하한(glibc 2.39) 배포판이 이미 넘는다([M7_FFMPEG](M7_FFMPEG.md)의 남은 질문).
- **업데이트 확인:** 시험 키로 GitHub runner에서 한 번 실험하고 결과를 여기에 남긴다. 시험 코드는 저장소에 넣지 않는다.
- **릴리스 범위:** 다음 릴리스부터 AppImage를 macOS·Windows와 같은 초안에 올리고, 게시할 때 서명한 `releases.linux.json`도 `updates` 릴리스에 올린다.

함께 정한 것(질문하지 않음):

- `packId`는 Windows와 같은 `CloakFrameApp`, 표시 이름은 `CloakFrame 2`, 채널은 `linux`다. 1.12.1의 `packId`는 `CloakFrame`이라 서로의 패키지를 건드리지 않는다.
- 릴리스 파일 이름은 1.x와 같은 꼴인 `CloakFrame-<판>-Linux-x86_64.AppImage`다.

## 구성

Velopack의 Linux 패키지는 AppImage 하나다. Windows처럼 Setup이 따로 있지 않고, `vpk`가 AppImage, full 패키지(`CloakFrameApp-<판>-linux-full.nupkg`), `releases.linux.json`을 쓴다. [`tools/package/linux.sh`](../tools/package/linux.sh)가 실행 파일, `libonnxruntime.so`, ffmpeg·ffprobe, 고지를 모아 묶는다.

- 파일은 AppImage의 `usr/bin`에 들어가고, 앱은 실행 파일 옆에서 찾으므로 `tools.rs`를 바꿀 필요가 없다.
- AppImage 런타임(type 2)은 정적 링크라 libfuse2가 없어도 되고, PATH의 `fusermount`만 쓴다. `APPIMAGE_EXTRACT_AND_RUN=1`이면 FUSE 없이 풀어서 돈다.
- 런타임은 libfuse(LGPL), musl, squashfuse, libzstd, zlib을 정적으로 담고 자기 고지를 파일 안에 갖고 있다. THIRD_PARTY_NOTICES에 같은 목록을 더했다. 1.12.1의 고지에는 없던 항목이다.
- `packaging/linux/cloakframe.desktop`은 쓰지 않는다. `vpk`가 AppImage 안의 `.desktop`을 만든다.

## 시험(GitHub runner `ubuntu-24.04`, 2026-11-09)

임시 브랜치에서 시험 키로 preview.3(A)과 판만 올린 preview.9(B)를 만들었다(run 42561840572, 42563572865, 42565051183). Xvfb에서 실제 FUSE로 AppImage를 띄우고 시험 feed(`CLOAKFRAME_UPDATE_FEED`)를 읽혔으며, 화면은 OCR로 읽고 버튼은 xdotool로 눌렀다. 끝난 뒤 임시 브랜치를 지웠다.

| 확인 | 결과 |
| --- | --- |
| AppImage 크기, 실행 파일이 요구하는 glibc | 55.6 MB, 2.39까지 |
| runner에서 FUSE로 실행 | `wgpu, vulkan, llvmpipe`로 창을 그림. Vulkan을 막으면 `OpenGL, llvmpipe` |
| Ubuntu 24.04, Debian 13, Fedora 43, Arch 컨테이너 | ffmpeg·ffprobe 실행, 창 뜸(FUSE 없이 풀어서 실행) |
| B, 시험 키로 서명 | "2.0.0-preview.9 is available" → 받기 → Restart Now. AppImage 파일이 그 자리에서 B로 바뀌고 새 창이 뜸 |
| 패키지 저장 위치 | `~/.cache/cloakframe/updates`. `/var/tmp/velopack`은 생기지 않음 |
| B 설치 상태에서 같은 판, A(서명됨) | 아무것도 표시하지 않음 |
| 다른 키로 서명, `.sig` 없음, feed의 Version만 바꿈 | 거부, 패키지를 남기지 않음 |
| 패키지 1바이트 바꿈 | Update를 누르면 거부, AppImage 그대로 |
| 실제 feed(HTTPS, 시스템 인증서) | AppImage에서 GitHub에 연결됨. `the feed has no releases.linux.json`(아직 없음)을 받았고, 다른 실행에서는 GitHub가 HTTP 504를 돌려줌([M6_UX](M6_UX.md#설정과-화면-배치-재검토2026-11-08-ui-settings)의 504와 같은 현상, 같은 시각 이 Mac에서도 Windows feed까지 504) |

`release.yml`의 Linux job 자체, 서명 job의 `linux` 채널, 초안과 `publish-feed.yml`의 Linux 경로는 태그나 PR에서 처음 돈다. 실험은 같은 스크립트와 서명 도구를 썼다.

## 찾은 결함: 업데이트 줄이 오른쪽 패널 뒤로 밀림

기본 창 크기(1100)에서 "다운로드했습니다" 줄이 가운데 칸보다 길어 "Restart Now"가 잘리고 "Later"가 패널 뒤로 숨었으며, 칸 전체가 넓어져 아래 내용도 밀렸다. OS와 관계없다. Windows의 실제 업데이트 확인(preview.3)은 UI Automation으로 버튼을 눌러 드러나지 않았다. 업데이트 줄을 `horizontal_wrapped`로 바꿨다. 재현 시험은 `the_update_bar_keeps_its_buttons_in_the_middle_column`이고 고치기 전에 실패했다(버튼 오른쪽 831.6, 패널 시작 812). 고친 뒤 실제 AppImage 화면에서 두 버튼이 다 보이는 것도 확인했다.

## 남은 것

- 실제 Linux 데스크톱(Wayland 포함)에서 실행과 업데이트. 지금까지는 runner의 Xvfb와 컨테이너다.
- AppImage를 데스크톱에 등록하면 `.desktop`의 `StartupWMClass=CloakFrameApp`이 앱의 창 이름과 맞지 않아 작업 표시줄 아이콘이 묶이지 않을 수 있다. 앱은 app id와 창 아이콘을 정하지 않는다.
- README의 preview 절은 Linux를 담은 릴리스를 낼 때 고친다.
- AppImage 런타임이 정적으로 담은 libfuse(LGPL)를 고지만으로 충분히 다루는지 법률 검토 때 같이 본다.

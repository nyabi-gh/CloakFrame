# M7: 릴리스 workflow(macOS preview·Windows)

작성 2026-10-30, 브랜치 `m7-release-windows`(main `a3e4c5f` 위). [ROADMAP](ROADMAP.md) M7 "Installers"·"Updates"의 릴리스 job 부분이다. 절차는 [RELEASING](RELEASING.md#preview)에 있다.

## 결정(2026-10-30, 메인테이너가 추천대로)

- **workflow 하나:** `preview.yml`을 `release.yml`로 바꾸고 macOS zip과 Windows 설치본을 한 초안에 올린다. Linux 설치본·DMG도 이 파일에 더한다.
- **feed는 두 번째 승인 뒤:** 설치된 앱은 feed가 바뀌는 즉시 받으므로, 초안을 확인하고 게시한 뒤 "Publish the update feed" job을 따로 승인해야 `updates` 릴리스가 바뀐다. 그 job은 릴리스가 아직 초안이면 실패한다.
- **portable zip은 올리지 않는다:** 지원하고 시험할 형태를 설치본 하나로 줄인다.
- **preview.1에 Windows를 더한다:** `preview.yml`이 만든 macOS만의 초안은 지우고 새 workflow로 다시 만든다.
- **Authenticode 없음:** 비용 때문에 Windows 설치본은 서명하지 않는다. SmartScreen이 처음 실행에서 경고한다.

함께 정한 것(질문하지 않음):

- **태그가 먼저:** `release` environment는 `v*` 태그에서만 배포를 허락한다. 그래서 `preview.yml`처럼 초안 게시가 태그를 만드는 순서로는 서명 job이 돌 수 없다. 태그를 푸시하면 workflow가 돌고, Version job이 태그와 `Cargo.toml` 판이 같은지 먼저 확인한다.
- **태그 밖에서는 빌드와 시험만:** 패키지에 닿는 PR(`release.yml`, `tools/package/**`, `tools/ci/app_window.ps1`, `manifests/native.toml`, `packaging/**`)과 수동 실행은 두 패키지를 만들고, Windows 설치본을 설치해 창을 열고 제거한다. 서명·초안·feed job은 태그에서만 돈다.
- **빌드 job에는 비밀이 없다:** 공개 키는 저장소 변수로 넘기고, 개인 키는 `release` environment의 서명 job만 읽는다. 서명 job은 runner의 Python·OpenSSL과 저장소의 `sign_updates.py`만 쓴다.
- **feed 순서:** 패키지와 `.sig`를 먼저 올리고 `releases.win.json`을 마지막에 바꿔, feed가 아직 없는 패키지를 가리키는 순간이 없게 한다. 앞 판의 패키지는 지우지 않는다.
- **채널 하나:** preview도 `win` 채널 하나에 올라가므로 설치된 preview는 다음 preview로 업데이트된다.
- **설치본 이름:** 릴리스에는 `CloakFrame-<판>-Windows-x64-Setup.exe`로 올린다(1.x와 같은 꼴). vpk의 `CloakFrameApp-win-Setup.exe`는 저장소 밖에서 쓰지 않는다.
- **다운로드 배지:** 지운 `CloakFrame-updates` 저장소를 읽던 부분을 뺐다. 2.0의 업데이트 패키지는 이 저장소의 `updates` 릴리스에 있어 그대로 센다.

## 시험

- 이 PC에서 `windows.ps1`이 쓴 파일 이름(`CloakFrameApp-win-Setup.exe`, `CloakFrameApp-<판>-full.nupkg`, `releases.win.json`, 그리고 올리지 않는 `-Portable.zip`·`assets.win.json`·`RELEASES`)이 workflow가 고르는 이름과 같다.
- PR #82의 Release 실행(run 41017059120): Version, macOS zip, Windows 설치본 job 통과. 서명·초안·feed job은 태그가 아니라 건너뜀. `windows-2025` 러너에 vpk가 쓰는 .NET 10 runtime이 있었고, 빌드는 cargo-about 설치 3분, 패키지 10분이었다.
- 설치 시험: `Setup.exe --silent`로 설치한 `%LocalAppData%\CloakFrameApp\current\cloakframe.exe`가 software dx12로 창을 열었다. 아직 없는 feed에는 "the feed has no releases.win.json"이라고 알리고 계속 떴다. wgpu를 막은 실행은 "CloakFrame cannot start" 창을 보였다. `Update.exe --uninstall --silent` 뒤 `current`가 없다. 창 검사는 자기가 띄운 프로세스의 창만 보므로, Setup이 설치 뒤 앱을 스스로 띄웠는지는 이 시험으로 알 수 없다.

## libjpeg-turbo SIMD(2026-11-01)

빌드 출력에 turbojpeg-sys의 "NASM does not seem to be installed, so turbojpeg will be compiled without SIMD extensions" 경고가 있었다. workspace가 turbojpeg를 `default-features = false`로 가져와 `require-simd`가 꺼져 있고 release workflow에 nasm이 없어, Windows 설치본도 SIMD 없이 빌드됐을 것이다(cargo가 빌드 스크립트 출력을 숨겨 workflow 로그로는 확인하지 못함). macOS arm64는 NEON이라 해당 없다.

- **바꾼 것:** engine·app 기능 `require-simd`(turbojpeg의 같은 기능)를 두고, `windows.ps1`과 macOS 빌드가 켠다. 이 기능은 nasm이 없으면 빌드를 실패시킨다. `windows.ps1`은 `native.toml`에 해시로 고정한 nasm 3.01(nasm.us 공식 win64 zip)을 받아 PATH에 넣는다. CI는 모든 job이 `--all-features`라 Windows job은 같은 nasm을, Linux job은 Ubuntu의 `nasm` 패키지를 설치한다. 그래서 CI 시험도 패키지와 같은 SIMD 빌드로 돈다.
- **시험(이 PC, Core Ultra 7 270K Plus, release):** nasm 없이 `--features require-simd`는 turbojpeg-sys 빌드에서 실패했다. 고정 nasm이 PATH에 있으면 `WITH_SIMD_INT=1`로 빌드됐다. 12 MP JPEG(4032×3024, 4:2:0, 7.0 MB)의 engine `decode`·`encode` 12회 p50은 SIMD 없이 234.3 ms·525.1 ms, SIMD로 115.1 ms·216.8 ms였다(디코드 2.0배, 인코드 2.4배).

## 첫 릴리스(2026-11-01)

`preview.yml`이 만든 macOS만의 초안을 지우고 main `c55bcc1`에 `v2.0.0-preview.1`을 태그했다(run 41310016529).

- Version, macOS zip, Windows 설치본, 서명, 초안 job이 첫 시도에 통과했다. 초안에는 `CloakFrame-2.0.0-preview.1-Windows-x64-Setup.exe`(50,534,838바이트)와 `CloakFrame-Preview-2.0.0-preview.1-macOS-arm64.zip`(35,014,201바이트)이 올라갔다.
- feed 승인이 초안 게시보다 먼저 들어가 feed job이 "Publish the v2.0.0-preview.1 draft first"로 실패했고, `updates` 릴리스는 바뀌지 않았다. 게시 뒤 다시 돌린 job이 통과했다.
- `updates` 릴리스에 `CloakFrameApp-2.0.0-preview.1-full.nupkg`(sha256 `8f3037a0…547a7e9`), `.sig`, `releases.win.json`이 이 순서로 올라갔다. 올라간 파일을 받아 SHA-256이 feed와 같고 서명이 저장소 변수 `CLOAKFRAME2_UPDATE_PUBLIC_KEY`로 검증되는 것을 확인했다.
- 릴리스는 pre-release로 게시했고 Latest는 1.12.1 그대로다.

## 남은 것

- Linux release job을 만들 때 nasm을 소스로 빌드할지 Ubuntu 패키지를 쓸지 정한다.
- 설치된 preview.1이 preview.2를 이 feed로 받는 것.
- Linux 설치본·AppImage, DMG·Sparkle, 1903 VM.

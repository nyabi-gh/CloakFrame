# M7: Windows 설치본

작성 2026-10-30, 브랜치 `m7-windows-installer`(main `1f96e53` 위). [ROADMAP](ROADMAP.md) M7 "Installers"의 Windows 부분과 [재검토](M0_M6_REVIEW.md) 10번의 남은 확인(1.12.1이 설치된 PC에서 설치·제거)이다.

## 결정(2026-10-30, 메인테이너가 추천대로)

- **Velopack SDK:** 안정판 `velopack` 1.2.161을 쓰고 vpk도 같은 판으로 맞춘다. SDK는 설치·업데이트·제거 hook 말고도 작업 표시줄 AppUserModelID 지정, 받아 둔 업데이트 적용, 오래된 패키지 정리를 하므로 직접 구현하지 않는다. cargo-deny에 걸리는 두 가지는 근거를 적어 예외로 둔다.
  - zip 8.6의 권고 3건(RUSTSEC-2026-0367·0368·0369)은 모두 `ZipArchive::extract`·`ZipStreamReader::extract`의 Unix 권한·symlink 문제다. SDK는 extract를 부르지 않고 항목 하나(`Update.exe`)를 직접 만든 파일에 복사한다(`bundle.rs`의 `extract_zip_idx_to_path`). derivative(RUSTSEC-2024-0388, unmaintained)는 SDK의 컴파일 시점 proc-macro다. zip 9를 쓰는 1.3은 아직 사전판(1.3.39-g8d2844f)이라, 그 안정판이 나오면 올리고 예외를 지운다.
  - SDK가 ureq 기본 feature를 켜고 끌 방법이 없어 webpki-roots(CDLA-Permissive-2.0, Mozilla 루트 인증서 목록)가 들어온다. `deny.toml`과 `tools/about/about.toml`에 이 crate만 예외로 두고, 라이선스 전문은 cargo-about이 `RUST_CRATE_NOTICES.txt`에 넣는다. engine의 모델 다운로드는 `RootCerts::PlatformVerifier`를 명시하므로 OS 인증서 저장소로 확인하는 것은 그대로다.
- **표시 이름 `CloakFrame 2`:** vpk의 `packTitle`이 바로 가기 파일 이름과 앱 및 기능 목록 이름이 된다. 1.12.1과 같은 `CloakFrame`이면 아래 시험대로 2.0이 1.12.1의 바로 가기를 덮어쓰고, 2.0을 제거하면 1.12.1은 바로 가기 없이 남는다. 창 제목과 앱 안의 이름은 CloakFrame 그대로다.

함께 정한 것(질문하지 않음):

- `packaging/windows/CloakFrame.manifest`의 `activeCodePage` UTF-8은 둔다. 앱의 호출은 모두 wide 문자 API지만, 링크한 C 라이브러리의 narrow 호출이 비ASCII 경로를 깨지 않게 한다.
- `packAuthors`는 1.12.1과 같은 `Nyabi`다. vpk 1.2에는 `--noPortable`이 없어 portable zip도 만들어진다. 릴리스에는 올리지 않는다([M7_RELEASE](M7_RELEASE.md)).
- 설치본은 서명하지 않는다(메인테이너 결정 2026-10-30, Authenticode 비용). SmartScreen이 처음 실행에서 경고한다.

## 바꾼 것

- **Velopack hook:** `main`이 텔레메트리 끄기 바로 다음에 `VelopackApp::build().run()`을 부른다(Windows·Linux). macOS는 Sparkle이라 넣지 않는다.
- **실행 파일 리소스:** `crates/cloakframe-app/build.rs`가 winresource 0.1.34로 아이콘, `packaging/windows/CloakFrame.manifest`(긴 경로, DPI, UTF-8, Windows 10 supportedOS), 버전 정보를 넣는다. 지금까지는 이 manifest가 2.0 실행 파일 어디에도 들어가지 않았다. `mt.exe -inputresource`로 꺼내 세 설정이 들어 있음을 확인했다. Windows SDK의 `rc.exe`가 필요하다.
- **DLL 검색 제한:** `main`이 `SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_DEFAULT_DIRS)`를 불러, 현재 폴더와 PATH를 검색에서 뺀다. ORT가 정적으로 import하는 `DirectML.dll`은 실행 파일 폴더에서 찾는다(계획 10장).
- **패키지 스크립트:** `tools/package/windows.ps1 <out>`이 `+crt-static`으로 release 빌드하고, 실행 파일에 `VCRUNTIME140`이 남아 있으면 멈춘다. 실행 파일, Windows ML의 `onnxruntime.dll`·`DirectML.dll`, LICENSE·THIRD_PARTY_NOTICES, Windows ML의 `license.txt`·`ThirdPartyNotices.txt`, cargo-about의 crate 고지를 모아 vpk로 묶는다. vpk는 `native.toml`에 해시로 고정한 NuGet 패키지를 `fetch_native.py`로 받아 `dotnet`으로 돌린다(.NET 10 runtime 필요). `--runtime win10.0.18362-x64`로 Setup이 Windows 10 1903 미만에서 설치를 거부한다. FFmpeg는 아직 넣지 않는다(계획 10.1).

## 시험(이 PC, Windows 11 Pro 26300, 2026-10-30)

1.12.1은 릴리스의 `CloakFrame-1.12.1-Windows-x64-Setup.exe`(SHA-256 `928174411d43…`, GitHub digest와 같음)를 썼다. 두 설치본 모두 `--silent`로 설치하고 `Update.exe --uninstall --silent`로 제거했다. 시험 전에는 두 설치와 `CloakFrameData`가 모두 없었고, 시험 뒤 같은 상태로 되돌렸다.

| 단계 | 결과 |
| --- | --- |
| 1.12.1 설치 | `%LocalAppData%\CloakFrame` 76개 파일, 바로 가기 `CloakFrame.lnk` 2개 |
| 2.0 설치 | `%LocalAppData%\CloakFrameApp`. 1.12.1 폴더는 파일 수·크기가 그대로. 앱 및 기능에 두 줄(`CloakFrame` 1.12.1, `CloakFrame 2` 2.0.0) |
| 2.0 실행 | 창이 뜨고 정상 종료(0), `CloakFrameData\settings.toml`을 씀 |
| 2.0 사진 실행 | 1.12.1의 모델 폴더에 넣어 둔 `yolov5n_face.onnx`를 해시 확인 뒤 `CloakFrameData\models`로 복사했다고 표시. 프로세스의 `onnxruntime.dll`·`DirectML.dll`이 `CloakFrameApp\current`에서 로드됨(System32 아님). 검토 대기 세션 하나가 `CloakFrameData\sessions`에 남음 |
| 2.0 제거 | `CloakFrameApp`만 지워짐. 1.12.1(모델 포함)과 `CloakFrameData`(설정·모델·세션) 그대로 |
| 2.0 재설치 | 앱이 앞 실행의 검토 대기 세션을 목록에 보임 |
| 1.12.1 제거 | `CloakFrame`만 지워짐. 2.0과 `CloakFrameData` 그대로 |

바로 가기는 `packTitle`에 따라 달랐다.

- `CloakFrame`(1.12.1과 같음): 2.0 설치가 `CloakFrame.lnk` 두 개를 2.0을 가리키게 덮어썼고, 2.0 제거가 그것을 지워 1.12.1은 바로 가기 없이 남았다. 1.12.1 제거는 2.0을 가리키는 바로 가기를 건드리지 않았다.
- `CloakFrame 2`: 두 버전의 바로 가기가 따로 생기고, 어느 쪽을 제거해도 다른 쪽 바로 가기가 남았다.

1.12.1의 모델은 1.12.1에서 받은 것이 아니라 시험 자산의 고정 모델 파일을 1.12.1이 받는 자리(`%LocalAppData%\CloakFrame\models`)에 넣은 것이다. 앱 및 기능의 2.0 버전은 Velopack이 `2.0.0`으로 적는다(사전판 표시 없음).

## 남은 것

- 깨끗한 Windows 10 1903 VM에서 설치·오프라인 실행·제거.
- 업데이트(ROADMAP M7 "Updates"): 새 키로 서명한 feed, 다운그레이드 거부, 이 저장소의 `updates` 릴리스.
- 릴리스 workflow의 Windows job은 [M7_RELEASE](M7_RELEASE.md).
- Microsoft 약관 3b의 설치 시 동의 필요 여부(ROADMAP "Notices and terms").

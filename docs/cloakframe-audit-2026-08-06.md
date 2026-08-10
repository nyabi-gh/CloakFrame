# CloakFrame 전면 감사 보고서 — 2026-08-06

감사 기준 revision: `4f3d6da8b96f68799a0871ec6899570e95dd675f` (`main`)

> 본문의 파일 경로, 줄 번호, LOC는 모두 위 revision 기준이다. 이후 수정으로
> 코드가 이동했고 `scripts/package_linux.sh`, `scripts/package_macos.sh`,
> `scripts/package_windows.ps1`은 삭제되었으므로, 각 finding을 다시 확인할 때는
> §0의 현재 상태를 먼저 보고 본문은 원인 분석과 재현 절차로 읽어야 한다.

## 0. Finding 현재 상태 (2026-08-10 기준)

재확인 revision: `9edfa0ce2713e2dd09edf8ddd0e545cf6de7ed06` (`main`). 아래 줄
번호도 그 revision 기준이며, 본문의 감사 시점 줄 번호와는 다르다. 본문 §9의
분류와 심각도는 감사 시점 기록이므로 바꾸지 않았다.

**이 절이 남은 작업의 단일 목록이다.** 감사 밖에서 발견한 항목도 §0.2와 §0.3에
함께 적는다. finding 상태가 바뀌면 여기서 갱신한다.

재확인 방법은 각 finding의 결함을 만드는 코드 경로를 다시 읽는 것이었다.
자료구조나 필드가 생겼다는 사실은 근거로 쓰지 않았고, 그 값을 실제로 읽어
사용자에게 도달시키는 caller가 있는지까지 확인했다 — `CF-004`가 그 구분 때문에
한 번 잘못 닫혔다(§0.4).

### 0.1 감사 finding

| ID | 상태 | 근거 (2026-08-10 재확인) |
|---|---|---|
| CF-001 | Fixed | client가 적용 가능한 모든 asset의 SHA-256에 대한 Ed25519 서명을 build에 고정된 key로 검증하고(`SelfUpdaterVelopack.cpp`의 `updateIsTrusted`, `UpdateSignature.cpp`), release job이 `.github/scripts/sign_update_packages.sh`로 서명한다. keypair는 `CLOAKFRAME_UPDATE_PUBLIC_KEY` 변수와 `CLOAKFRAME_UPDATE_PRIVATE_KEY` secret으로 등록되었다 (2026-08-10). Authenticode는 이 finding과 무관하며 여전히 없다 — §0.2 |
| CF-002 | Open | `cmake/CloakFrameVelopack.cmake:3`의 Velopack 1.2.0 고정과 공유 `/var/tmp` cache 그대로. 다만 `CF-001`의 서명 검증이 선점된 package를 거부하므로 남는 것은 검증과 Velopack의 재읽기 사이 좁은 TOCTOU다. `UpdateManager`에 locator를 넘기면 auto-locate가 통째로 꺼지므로(`manager.rs:198`) client에서 `PackagesDir`만 바꿀 수 없다. 상류가 `package_dir_override`를 C API로 노출하는 것이 정공법이다 |
| CF-003 | Fixed | `DetectionResult::omitted`(`Yolo5FaceDetector.cpp:319-338`, `ScrfdFaceDetector.cpp:610-624`)가 버린 후보를 전달하고 `RunSummary::uncovered`가 clean `Completed`를 막는다 |
| CF-004 | Fixed | `VideoProcessor.cpp:665`가 `droppedTracks`를 결과에 싣고 `ProcessorWorker.cpp:1500,1527`이 이를 사용자 경고와 `outcome.uncovered`로 올린다 (2026-08-09 늦게, `e9ea380`) |
| CF-005 | Fixed | `VideoProcessor.cpp:663-664`가 `uncoveredFrames`와 `uncoveredSpans`를 올리고, review timeline이 그 구간을 별도 행으로 표시한다 (`3135dcb`) |
| CF-006 | Fixed | `VideoInfo::startTimeSeconds`가 정규화된 timeline을 정의하고 preview/encode가 같은 mapping을 쓴다 |
| CF-007 | Fixed | 서명 job이 `environment: release`를 선언하고, 저장소의 `release` 환경이 `v*` tag 배포 제한과 필수 리뷰어로 설정되었다 (2026-08-09). 단 secret 자체는 아직 repository scope다 — §0.3 |
| CF-008 | Fixed | 디코드 패스가 `-xerror`(`VideoIo.cpp:747`)로 실행되어, 복구된 오류가 나면 짧아진 결과를 게시하지 않고 실패한다 (2026-08-09) |
| CF-009 | Fixed | `VideoInfo::sarNum/sarDen`을 probe하고 writer가 `setsar`를 적용한다 |
| CF-010 | Fixed | probe가 모든 audio stream을 읽고 writer가 `-map 1:a?`로 전부 map하며, 호환되지 않는 stream만 재인코딩하고 언어 태그를 보존한다 (2026-08-09) |
| CF-011 | Fixed | 실제 encode 실패가 encoder 원인일 때 software encoder로 pass 2를 한 번 재실행한다 (2026-08-09) |
| CF-012 | Fixed | `process()` 진입 시의 cancel flag reset 제거 |
| CF-013 | Fixed | thread 생성 실패 시 이미 시작한 worker를 stop/join한 뒤 rethrow한다 (`OrderedParallel.hpp`의 `StopAndJoin`, `VideoProcessor.cpp`의 `MaskWorkerPool` 생성자 catch) |
| CF-014 | Fixed | estimate가 budget을 넘으면 decode 전에 거부한다 (`ProcessorWorker.cpp:875`, 2026-08-09) |
| CF-015 | Fixed | 원자적 primitive가 없는 filesystem에서는 `.cloakframe-partial`(`ImageIo.cpp:1120,1184`)로 복사한 뒤 rename하며, 복사 중 guard를 polling한다 (2026-08-09) |
| CF-016 | Open | settings에 `customModelPath`만 저장한다(`MainWindow.cpp:1923,2060`). 동의가 여전히 path에만 묶이고 parser가 같은 process에서 실행된다 |
| CF-017 | Obsolete | `scripts/package_linux.sh`가 삭제되고 AppImage는 `vpk pack`이 만든다 |
| CF-018 | Fixed | keypair를 생성해 `SPARKLE_ED_PUBLIC_KEY` 변수와 `SPARKLE_ED_PRIVATE_KEY` secret으로 등록했다 (2026-08-10). keychain private key의 public half가 저장소 변수 및 번들의 `SUPublicEDKey`와 일치함을 대조했다. appcast에 서명이 실제로 실리는지는 다음 release job의 `grep -q 'edSignature='`가 확인한다 |
| CF-019 | Open | `ModelDownloader.cpp:105`가 고정 `.part` 이름을 일반 `QFile`로 연다 |
| CF-020 | Open | crash 후 남은 source snapshot(`ProcessorWorker.cpp:788`, `VideoProcessor.cpp:433`)을 회수하는 startup scavenger가 없다 |
| CF-021 | Open | `OutputPlan.cpp:15`의 `destinationKey`가 compile-time OS 가정으로 collision을 판단한다. README의 시작 전 충돌 보장 문구도 그대로다 |
| CF-022 | Fixed | `Mosaic.cpp:116`의 fill이 `opaqueAlphaFor(roi)`로 alpha를 채운다 |
| CF-023 | Fixed | 96개 문구를 번역하고 세 locale 모두에 strict gate를 적용했다. 근본 원인인 `trVideo`/`trVideoProcessor` 간접 호출도 `QT_TRANSLATE_NOOP`으로 노출했다 (2026-08-09) |
| CF-024 | Fixed | release workflow의 `verify-version` job이 tag/project/release-notes 일치를 강제한다 |
| CF-025 | Open | `.github/workflows/release.yml:95`의 `brew install`이 버전을 고정하지 않고 SBOM이 없다 |
| CF-026 | Open | `SceneCut.cpp:112`의 `push`에서 확정 분기가 여전히 early return한다 |

감사에 없던 항목 두 가지도 함께 처리했다. Windows manifest
(`activeCodePage`/`longPathAware`)를 추가해 non-ASCII 경로에서 metadata 보존이
꺼지던 문제를 막았다(Windows CI가 검증 수단이다). 그리고 `lupdate`가
`trVideo`/`trVideoProcessor` 간접 호출을 인식하지 못해 video 관련 문구 36개가
obsolete로 표시되고 `lrelease`가 이를 제외하면서, 모든 언어에서 영어로 표시되고
있었다. 이는 `CF-023`보다 넓은 범위의 결함이었다.

2026-08-09 늦게 시작한 별도 세션이 Windows/Linux 패키징 결함, webm(VP8/VP9)
입력, uncovered region 보고와 pass 1 검출 성능을 다뤘다. 그 세션의 측정값,
기각한 접근과 환경 메모는 `docs/open-work-2026-08-09.md`에 있다. 상태는 그
문서가 아니라 이 절이 관리한다.

### 0.2 감사 밖에 남은 작업

감사 범위 밖에서 발견했지만 아직 열려 있는 항목이다. `Mosaic.cpp`,
`ImageScanner.cpp`, `ModelDownloader.cpp`, `OutputPlan.cpp`, `SceneCut.cpp`,
`OnnxGraphPatch.cpp`는 2026-08-09 이후 한 줄도 바뀌지 않았다.

| 항목 | 위치 | 비고 |
|---|---|---|
| pass 1 검출 성능의 남은 절반 | `VideoProcessor.cpp` 검출 루프 | 측정과 tensor 재사용까지 끝났다. scene-cut 워커 분리(8.6s)와 prepare/infer 겹치기(14.3s)가 남았고, 후자는 `Detector`를 두 단계로 나눠야 한다. `docs/open-work-2026-08-09.md` §2 |
| Resize scale 하드코딩 | `OnnxGraphPatch.cpp:382` | 원본 `sizes` initializer를 읽지 않고 2배를 가정한다 |
| GUI 스레드 블로킹 | `MainWindow.cpp:242` | 썸네일 추출이 GUI 스레드에서 3초 `waitForStarted`/`waitForFinished`. 모델 SHA-256도 같은 스레드 |
| 영상 리뷰 UX | `VideoReviewDialog.cpp:720` | Esc가 `CancelAll`(이미지 다이얼로그의 Esc는 한 장만 건너뛴다). undo 없음, "여기를 시작/끝으로"가 keyframe 전체를 지울 수 있다 |
| soft-mask 전역 직렬화 | `Mosaic.cpp:451` | `g_maskComputationMutex`가 마스크 계산을 전 스레드에 걸쳐 직렬화한다 |
| YOLO5Face 점수 | `Yolo5FaceDetector.cpp:270` | objectness만 쓰고 class score와 곱하지 않는다 |
| 기타 Low | — | 1px blur no-op, 타원 모서리 미커버, 스캐너 에러 처리 비대칭, 대소문자 무시 입력 dedupe |
| DMG만 stapling | `.github/workflows/release.yml:263` | app bundle 자체는 staple되지 않는다 |
| per-test timeout 없음 | `tests/CMakeLists.txt` | `TIMEOUT` 속성이 없어 멈춘 테스트가 CI 전체 한도까지 간다 |
| Windows 설치본 미서명 | `.github/workflows/release.yml` | signtool 단계가 없어 SmartScreen 경고가 난다. `CF-001`은 app-pinned key로 닫혔고 Authenticode는 그것과 별개의 비용 문제다 |
| Windows Qt 6.11.1 핀 | `.github/workflows/*` | 저장소 밖 차단. aqtinstall이 Windows의 아키텍처별 저장소 경로를 아직 못 읽는다 (issues #959, #1000는 master에만 반영) |

### 0.3 사람이 해야 하는 것

코드로 끝낼 수 없고 비밀값 접근 권한이 필요한 작업이다. 하나 남았다.

1. **Apple 서명 secret 7개를 `release` 환경으로 옮긴다.** `release` 환경은
   존재하지만 `gh secret list --env release`가 비어 있고, 7개 secret은 여전히
   repository scope다. 즉 어떤 workflow의 어떤 job이든 아직 읽을 수 있다
   (`CF-007`의 남은 절반).

2026-08-10에 update signing key 두 개를 만들어 등록하면서 이 목록의 두 번째
항목은 닫혔다 — macOS의 Sparkle EdDSA key (`CF-018`)와 Windows/Linux의 Ed25519
key (`CF-001`). 두 private key 모두 이 저장소가 아니라 만든 사람의 keychain과
오프라인 사본에 있다. 어느 쪽이든 잃어버리면 그 key로 서명된 build를 받은
client가 이후 모든 update를 영구히 거부하므로, 기계를 옮길 때 함께 옮겨야 한다.

`CF-001`의 서명 경로는 local round trip과 CI build까지 확인했지만, 실제
release에서 동작하는 것은 다음 `v*` tag가 처음이다. 그때 release job 로그에
`signed <package> <digest>`가 찍히고 release asset에 `.nupkg.sig`가 올라오는지
확인한다.

### 0.4 이 표가 한 번 틀렸던 곳

2026-08-09 판 표는 `CF-004`를 "`TrackCoverageReport::droppedTracks`가 삭제된
track을 결과로 올린다"는 근거로 Fixed로 적었다. 그 시점(`b3beb25`)에
`droppedTracks`는 `Tracking.cpp:783`에서 **쓰이기만 하고 읽는 곳이 없었다.**
값이 사용자에게 도달한 것은 `e9ea380`부터다. 필드가 생겼다는 사실을 근거로
쓰면 이런 오판이 나온다 — 근거는 항상 그 값을 읽는 caller까지 적는다.

이 보고서는 정적 코드 추적, 두 번의 독립적인 sweep, 기존 테스트 검토, 별도 sanitizer/경고 빌드, 합성 이미지·영상과 production library에 연결한 임시 harness, 그리고 현재 CI/패키징 설정 검증을 결합한 결과다. 소스의 주석이나 README를 구현의 증거로 사용하지 않고 caller에서 최종 output/update sink까지 실제 경로를 확인했다. 외부 플랫폼이나 안전하게 재현할 수 없는 조건은 명시적으로 **Not dynamically verified**라고 표시했다.

## 1. Executive Summary

감사 결과는 다음과 같다.

| Classification | Count | Severity breakdown |
|---|---:|---|
| Confirmed defect | 13 | Critical 1, High 6, Medium 6 |
| Probable defect | 2 | Medium 2 |
| Hardening opportunity | 5 | High 1, Medium 3, Low 1 |
| Documentation/build inconsistency | 6 | Medium 1, Low 4, Info 1 |

가장 중대한 결론은 다음과 같다.

1. Windows/Linux 자동 업데이트는 GitHub release feed와 package가 같은 신뢰 영역에 있고, 앱에 고정된 독립 서명 public key가 없다. release 권한 탈취자는 자신이 선택한 hash와 악성 package를 함께 게시해 업데이트로 실행시킬 수 있다 (`CF-001`, Critical).
2. 고정된 Velopack 1.2.0 Linux 경로는 공유 `/var/tmp` cache를 사용하고 이미 존재하는 package를 hash 재검증 없이 적용한다. 다른 로컬 사용자가 cache tree와 향후 package filename을 선점하면 victim 사용자 권한으로 임의 AppImage를 실행시킬 수 있다 (`CF-002`, High).
3. 이미지 YOLO detector는 NMS 후 300개를 초과하는 검출을 조용히 버리며 worker는 일부만 가린 파일을 `Done`으로 확정한다 (`CF-003`, High).
4. 영상에서는 low-confidence track 전체 삭제, retained track 내부의 보간 거부 gap 방치, nonzero-start review timeline 불일치가 각각 검출된 얼굴/번호판 또는 사용자가 수동으로 선택한 얼굴을 정상 결과에서 노출시킬 수 있다 (`CF-004`–`CF-006`, High).
5. 정상 CTest, current-head 3-platform CI, ASan/UBSan가 모두 통과했지만 위 결함을 잡지 못했다. 기존 unit test 일부는 오히려 low-confidence 삭제와 over-limit gap을 의도된 값으로 고정하면서 최종 픽셀과 `Done` 상태를 검증하지 않는다.

반대로 중요한 방어도 확인했다. 일반적인 local filesystem에서 source snapshot, rooted directory traversal, no-replace publication, source identity 재검사로 원본 덮어쓰기와 output symlink escape가 방어된다. 투명 custom overlay 아래에는 먼저 mosaic가 적용된다. built-in model은 download 시와 native runtime session 생성 직전에 고정 SHA-256으로 다시 검증된다. image/video bytes를 network request에 넣는 경로와 FFmpeg shell command injection 경로는 찾지 못했다. 무검출 결과도 현재는 `Review required`로 끝난다.

감사의 잔여 위험은 Windows/Linux runtime과 실제 release artifact를 로컬에서 실행하지 못한 점, 실제 detector model 파일이 로컬에 없어 crowded-image inference를 재현하지 못한 점, LSan이 현재 macOS runtime에서 지원되지 않은 점, 대형 이미지 OOM·disk-full·power-loss를 안전상 실제로 유발하지 않은 점이다.

## 2. Audit Scope

### 2.1 포함 범위

- `src/`와 `include/cloakframe/`의 모든 production source/header
- `tests/`, 최상위·하위 `CMakeLists.txt`
- `.github/workflows/`, `.github/scripts/`, `scripts/`, `cmake/`, `tools/`
- `README.md`, `RELEASE_NOTES.md`, `release-notes/`, `THIRD_PARTY_NOTICES.txt`, `LICENSE`
- 한국어·일본어·중국어 translation catalog와 안전 관련 UI text
- pinned Velopack 1.2.0의 update source 중 CloakFrame이 실제 호출하는 locator/download/apply 경로
- 현재 HEAD에 대한 GitHub Actions CI 결과와 repository signing-secret/environment 구성의 read-only metadata

### 2.2 방법

**Pass 1 — data-flow 중심:** image, video, model, update 입력을 최초 discovery에서 최종 publish/apply까지 추적했다. decode/transform/detect/review/redact/metadata/temp/publication 및 FFprobe/decode/track/interpolate/encode/audio/container 흐름을 caller-to-sink로 확인했다.

**Pass 2 — state/ownership 중심:** Pass 1의 판정에 의존하지 않고 GUI/worker state machine, QObject affinity, callback lifetime, cancel/finish 경쟁, `std::thread` construction/destruction, temporary file lifetime, path identity, platform branch, release branch를 다시 조사했다. 중복 원인은 새 finding으로 세지 않고 기존 finding의 증거를 강화했다.

### 2.3 제외·가정

- 실제 개인 사진이나 외부에서 받은 신뢰할 수 없는 ONNX model은 사용하지 않았다.
- 합성 입력과 harness는 `/private/tmp`에만 만들었다.
- 법률 결론을 내리지 않았다. license 평가는 repository와 관찰한 package dependency closure의 기술적·배포상 위험으로 한정한다.
- 제품 코드는 수정하지 않았다. 감사 보고서 이외의 tracked file은 변경하지 않았다.
- 현재 지원 표기는 macOS Apple Silicon, Windows x64, Linux x86_64로 해석했다. 다른 architecture branch는 hardening 관점에서만 평가했다.

## 3. Repository Revision and Environment

### 3.1 Revision과 초기 상태

| Item | Value |
|---|---|
| Branch | `main` |
| HEAD | `4f3d6da8b96f68799a0871ec6899570e95dd675f` |
| Upstream | `origin/main` |
| Initial working tree | clean (`## main...origin/main`) |
| Final working tree | only new audit report: `?? docs/cloakframe-audit-2026-08-06.md`; no tracked product/build/document file modified |
| Audit date/time zone | 2026-08-06, Asia/Seoul |

### 3.2 Host와 toolchain

| Component | Observed version/status |
|---|---|
| OS | macOS 26.6, build `25G72`; Darwin 25.6.0 |
| Architecture | arm64 (`T8132`) |
| Compiler | Apple clang 21.0.0 (`clang-2100.1.1.101`), target `arm64-apple-darwin25.6.0` |
| CMake / CTest | 4.4.2 / 4.4.2 |
| Ninja | 1.13.2 |
| Qt | 6.11.1 |
| OpenCV | 5.0.0 |
| ONNX Runtime | 1.28.0 |
| Exiv2 | 0.28.8 |
| spdlog / fmt | 1.17.0 / 12.2.0 |
| FFmpeg / ffprobe | 8.1.2 / 8.1.2; local build has `--enable-gpl`, libx264/libx265 |
| Sparkle linked by normal build | 2.9.4 |
| Velopack pinned for Windows/Linux | 1.2.0 |

Versions were taken from executable output, CMake cache, and `otool -L build/CloakFrame.app/Contents/MacOS/CloakFrame`; `pkg-config` alone could not resolve the Homebrew OpenCV package and was not treated as authoritative.

### 3.3 Analysis-tool availability

| Tool | Status |
|---|---|
| AddressSanitizer | available; executed |
| UndefinedBehaviorSanitizer | available; executed with no-recover |
| LeakSanitizer | compiler flag accepted, but runtime reports `detect_leaks is not supported on this platform` |
| ThreadSanitizer | available; executed; external symbolizer unavailable |
| compiler `-Werror` | available; executed |
| `-Wpedantic -Wconversion -Wsign-conversion -Wshadow` | available; executed |
| `clang-tidy` | unavailable in PATH/Xcode toolchain |
| `cppcheck` | unavailable |
| `scan-build` / Infer / IWYU / Valgrind | unavailable |
| `llvm-symbolizer` | unavailable |

### 3.4 Repository size

`git ls-files` 기준 104 tracked files를 대상으로 했다. Binary assets 4개를 제외한 tracked text는 총 34,725 lines였으며, 겹치지 않는 구성은 production C++ source/header 17,461, tests와 test CMake 2,929, CMake/workflow/script/tooling 2,063, translation catalogs 5,109, root/release/license documents 7,084, 기타 text assets와 `.gitignore` 79 lines다. Generated build files와 이 보고서는 baseline count에서 제외했다.

주요 개별 LOC는 coverage matrix에 병기한다. 큰 파일은 `src/MainWindow.cpp` 2,492, `src/ImageIo.cpp` 1,990, `tests/test_core.cpp` 1,431, `src/ProcessorWorker.cpp` 1,367, `src/VideoIo.cpp` 1,143, `src/VideoReviewDialog.cpp` 1,090, `src/VideoProcessor.cpp` 928, `src/ReviewDialog.cpp` 772, `src/Tracking.cpp` 764, `src/Mosaic.cpp` 696, `THIRD_PARTY_NOTICES.txt` 6,061 lines다.

## 4. Commands Executed

아래는 build/test/analysis 명령의 재현 가능한 inventory다. 반복적인 source 열람은 실제 사용한 command family와 대상 범위를 함께 기록했다.

### 4.1 Baseline, inventory, environment

```text
git branch --show-current
git rev-parse HEAD
git status --branch --short
git ls-files
git ls-files | wc -l
git ls-files src include/cloakframe | xargs wc -l
git ls-files tests | xargs wc -l
git ls-files translations | xargs wc -l
git ls-files README.md RELEASE_NOTES.md THIRD_PARTY_NOTICES.txt LICENSE release-notes | xargs wc -l
uname -a; uname -m; sw_vers
clang++ --version; cmake --version; ctest --version; ninja --version
qmake6 -query QT_VERSION
ffmpeg -version; ffprobe -version
otool -L build/CloakFrame.app/Contents/MacOS/CloakFrame
command -v clang-tidy cppcheck scan-build infer include-what-you-use valgrind llvm-symbolizer
xcrun --find clang-tidy; xcrun --find llvm-symbolizer
```

### 4.2 Source/data-flow inspection

```text
rg -n '<QProcess/update/download/path/cancel/thread/metadata/timestamp/license patterns>' src include tests cmake scripts .github README.md RELEASE_NOTES.md release-notes translations
nl -ba <every production/test/workflow/script file> | sed -n '<target ranges>p'
cmake --help-variable CMAKE_PREFIX_PATH
rg -n 'assert\(' tests
rg -n 'NDEBUG|UNDEBUG' CMakeLists.txt src tests .github scripts
nm -u build/tests/cloakframe_tests
```

`rg`/`nl`은 coverage matrix의 모든 production file과 관련 tests, CMake, workflow, script, documentation에 적용했다. 외부 update 경로는 pinned Velopack commit `f2edcbcafb81da5b3c884aaea330e225ad91d8b6`의 `sources/github.rs`, `sources/mod.rs`, `manager.rs`, `locator.rs`, Windows/Linux apply implementation을 read-only로 확인했다.

### 4.3 Builds and tests

```text
cmake -S . -B /private/tmp/cloakframe-audit-debug -G Ninja -DCMAKE_BUILD_TYPE=Debug -DCLOAKFRAME_SELF_UPDATE=OFF -DCMAKE_EXPORT_COMPILE_COMMANDS=ON
cmake --build /private/tmp/cloakframe-audit-debug --parallel 4
ctest --test-dir /private/tmp/cloakframe-audit-debug --verbose

cmake --build build --parallel 4
ctest --test-dir build --output-on-failure

cmake -S . -B /private/tmp/cloakframe-audit-asan -G Ninja -DCMAKE_BUILD_TYPE=Debug -DCLOAKFRAME_SELF_UPDATE=OFF -DCMAKE_CXX_FLAGS='-fsanitize=address,undefined -fno-omit-frame-pointer -fno-sanitize-recover=all' -DCMAKE_EXE_LINKER_FLAGS='-fsanitize=address,undefined'
cmake --build /private/tmp/cloakframe-audit-asan --parallel 4
env ASAN_OPTIONS=detect_leaks=0 UBSAN_OPTIONS=print_stacktrace=1 ctest --test-dir /private/tmp/cloakframe-audit-asan --output-on-failure
env ASAN_OPTIONS=detect_leaks=1 ctest --test-dir /private/tmp/cloakframe-audit-asan -R cloakframe_tests --output-on-failure

cmake -S . -B /private/tmp/cloakframe-audit-tsan -G Ninja -DCMAKE_BUILD_TYPE=Debug -DCLOAKFRAME_SELF_UPDATE=OFF -DCMAKE_CXX_FLAGS='-fsanitize=thread -fno-omit-frame-pointer' -DCMAKE_EXE_LINKER_FLAGS='-fsanitize=thread'
cmake --build /private/tmp/cloakframe-audit-tsan --parallel 4
ctest --test-dir /private/tmp/cloakframe-audit-tsan --output-on-failure
env OPENCV_FOR_THREADS_NUM=1 TSAN_OPTIONS=halt_on_error=1:second_deadlock_stack=1 /private/tmp/cloakframe-audit-tsan/tests/cloakframe_tests

cmake -S . -B /private/tmp/cloakframe-audit-werror -G Ninja -DCMAKE_BUILD_TYPE=Debug -DCLOAKFRAME_SELF_UPDATE=OFF -DCMAKE_COMPILE_WARNING_AS_ERROR=ON
cmake --build /private/tmp/cloakframe-audit-werror --parallel 4

cmake -S . -B /private/tmp/cloakframe-audit-warnings -G Ninja -DCMAKE_BUILD_TYPE=Debug -DCLOAKFRAME_SELF_UPDATE=OFF -DCMAKE_CXX_FLAGS='-Wpedantic -Wconversion -Wsign-conversion -Wshadow'
cmake --build /private/tmp/cloakframe-audit-warnings --parallel 4

python3 scripts/check_translations.py
python3 scripts/check_translations.py --strict-source-equality translations/cloakframe_ja.ts
```

### 4.4 Synthetic/harness verification

Production `cloakframe_core`에 직접 link한 harness는 `/private/tmp/cloakframe-audit-harness`와 `/private/tmp/cloakframe-video-audit.*`에만 생성했다.

```text
cmake -S /private/tmp/cloakframe-audit-harness -B /private/tmp/cloakframe-audit-harness-build -G Ninja
cmake --build /private/tmp/cloakframe-audit-harness-build
/private/tmp/cloakframe-audit-harness-build/cloakframe_audit_harness cancel
/private/tmp/cloakframe-audit-harness-build/cloakframe_audit_harness qfile-symlink
/private/tmp/cloakframe-audit-harness-build/cloakframe_audit_harness video-tools <synthetic-input> <output> /private/tmp/cloakframe-audit-harness/ffmpeg_hardware_then_fail.sh
/private/tmp/cloakframe-video-audit.DFGoIB/tracking_harness <synthetic-input> <output> <mixed|gap>
/private/tmp/cloakframe-video-audit.DFGoIB/process_harness <synthetic-input> <output>
ffmpeg -v error -y -i cfr.mp4 -map 0:v:0 -c copy -bsf:v noise=amount=-1 corrupt-packets.mp4
ffprobe -v error -count_packets -show_streams -show_format -of json <input-or-output>
```

FFmpeg lavfi/color/testsrc/anullsrc를 이용해 30000/1001 CFR, VFR, SAR 16:15, video-start 2초/audio-start 0초, no-audio, two-audio, rapid scene cuts를 합성했다. 각 input/output은 `ffprobe` stream/frame/duration/SAR로 비교하고 필요한 frame은 decoded BGR hash 또는 ROI luma average로 비교했다.

### 4.5 Remote/release metadata and final integrity

```text
gh run list --commit 4f3d6da8b96f68799a0871ec6899570e95dd675f --limit 20 --json ...
gh run view 30779346869 --json jobs,conclusion,status,url
gh run view 29895011765 --job 88843209002 --log | rg -n '<Exiv2/metadata/build/test patterns>'
gh secret list --repo nyattic/CloakFrame
gh api repos/nyattic/CloakFrame/environments
dnx vpk@1.2.0 -- pack -h
git diff --check
git diff --no-index --check /dev/null docs/cloakframe-audit-2026-08-06.md
rg -n '[[:blank:]]+$' docs/cloakframe-audit-2026-08-06.md
git status --short
```

Secret 값은 조회하지 않았고 이름과 존재 여부만 확인했다. `git diff --no-index --check`는 새 file이 `/dev/null`과 다르므로 status 1이지만 whitespace diagnostic은 없었고, 별도 trailing-whitespace `rg`도 match가 없었다.

## 5. Build and Test Results

### 5.1 Build/test matrix

| Configuration | Result | Evidence/notes |
|---|---|---|
| Existing Release build, self-update ON | Pass | Sparkle 2.9.4 포함 rebuild 성공; final CTest 6/6, 2.95s |
| Fresh Debug, self-update OFF | Pass | configure/build 성공; final CTest 6/6, 4.23s |
| ASan + UBSan | Pass | leak detection OFF에서 final CTest 6/6, 9.55s; sanitizer diagnostic 없음 |
| LSan request | Unsupported | `AddressSanitizer: detect_leaks is not supported on this platform`; **Not dynamically verified** |
| TSan build | Build pass, suite 5/6 | core test가 report로 abort; 나머지 translation 2, tracking, parallel, video test pass |
| TSan focused rerun | External-sync report | `Qt::BlockingQueuedConnection` return storage의 Qt 내부 semaphore를 TSan-instrumented project가 보지 못한 report; §19 참조 |
| Project warnings as errors | Pass | clean rebuild 성공 |
| Pedantic/conversion/shadow warnings | Build pass with diagnostics | 주로 Qt/OpenCV size/int conversion; finding으로 승격할 독립 오동작 증거 없음 |
| `clang-tidy`, `cppcheck`, `scan-build` | Skipped | tool unavailable |
| Current HEAD GitHub CI | Pass | run 30779346869; macOS, Windows, Linux jobs 모두 build/model-fetch/test 성공 |
| Translation normal policy | Pass with warning | ko 313 OK, ja 313 OK + source-equality warnings 96, zh_CN 313 OK |
| Japanese strict policy | Expected fail | 96 errors; `CF-023` |

Local core test는 외부 model path가 없어 fixed/dynamic SCRFD patch 및 YOLO5Face/YuNet/plate inference 계열을 skip했다. Current-head CI는 pinned checksum으로 model/astronaut assets를 내려받아 세 플랫폼에서 해당 test를 수행했고 성공했다. 그 성공은 crowded detection, final-pixel coverage 또는 update authenticity의 증명이 아니다.

Tests는 `assert`를 543회 사용하지만 `tests/CMakeLists.txt:1-7`이 Release에서도 `/UNDEBUG` 또는 `-UNDEBUG`를 강제한다. 따라서 Release CI에서 assertion이 제거된다는 초기 의심은 non-issue로 판정했다.

### 5.2 Adversarial verification matrix

| Case | Result |
|---|---|
| Empty/not-a-video/corrupt container | 기존 test가 reject 경로 확인; recoverable H.264 packet corruption은 120→100 frames로 조용히 성공하여 `CF-008` 재현 |
| Extension/content mismatch | probe/decode가 content를 확인하는 경로 점검; shell extension만으로 decode 성공 처리하는 경로 없음 |
| Huge declared dimensions/decompression bomb | header-level dimension/file cap 확인; 실제 multi-GiB valid decode/OOM은 안전상 **Not dynamically verified** (`CF-014`) |
| Space/Unicode path | `공백 path 30000_1001.mp4` production round trip 60→60 frames 성공 |
| Quote/newline/other metacharacters | QProcess argument-vector 사용을 정적으로 확인; newline path의 모든 플랫폼 실행은 **Not dynamically verified** |
| Input/output symlink escape | existing rooted-publication tests pass; normal output escape/overwrite 방어 확인 |
| Model `.part` symlink | QFile semantics harness가 victim content를 `verified-model`로 덮어씀 (`CF-019`) |
| Output creation race | concurrent no-replace existing test에서 정확히 one winner; POSIX unsupported-rename mid-copy crash는 **Not dynamically verified** (`CF-015`) |
| Read-only destination / disk full | error cleanup trace 확인; distinct filesystem fault injection은 **Not dynamically verified** |
| FFmpeg immediate failure | production wrapper로 hardware probe 성공 후 real encode exit 42; CPU retry 없이 Failed (`CF-011`) |
| FFmpeg hang/cancel | timeout/kill/cancel code와 existing tests 확인; OS-level permanent hang fault injection은 **Not dynamically verified** |
| 30000/1001 CFR | Unicode/space path 포함 input/output 모두 60 frames, 2.002s, rational rate 유지 |
| VFR | input 70 packets (`avg=1050/47`, `r=30`) → output 67-frame CFR (`1050/47`), 2.999s; intended conversion 범위 |
| Rotation metadata | existing real-FFmpeg test pass; rotation baked, output rotation 0 |
| SAR/DAR | 720×576 SAR 16:15/DAR 4:3 → output SAR/DAR 없음; `CF-009` |
| No audio | audio 없는 output 정상 |
| Two audio streams | two AAC → one AAC; `CF-010` |
| Differential stream start | video start 2s/40 frames + audio start 0s/6s → output 60-frame video/6s + audio 4.01s; preview 20-frame offset; `CF-006` |
| Rapid consecutive scene cuts | red→green(2 frames)→blue에서 두 cut 중 하나만 기록; `CF-026` |
| Crossing faces | tracker matching code/tests 조사; detector-backed realistic crossing identity test는 **Not dynamically verified** |
| Low-confidence mixed track | known strong frames가 포함된 second track을 전부 제거하고 `Completed`; ROI unchanged; `CF-004` |
| 21-frame internal gap | gap frames unchanged while both ends redacted, `Completed`; `CF-005` |
| Edge face | clipping/finite guards와 synthetic rectangles 확인; real detector model edge-face inference는 **Not dynamically verified** |
| Transparent custom overlay | existing production-path test에서 mosaic fallback과 blend 방어 확인 |
| Early cancel/new run | `cancel()` 후 `process()`가 output을 만들고 outcome 1; `CF-012` |
| Partial batch failure | summary aggregation test/trace 확인; privacy-coverage-specific mixed batch는 missing coverage로 보강 필요 |

## 6. Architecture and Data-Flow Map

### 6.1 Image pipeline

```text
[External file/folder paths]
  -> MainWindow validation/drop/file dialog
  -> ImageScanner canonical de-dup + recursive scan
  -> OutputPlan lexical collision preflight
  -> ProcessorWorker/QThread
  -> source identity capture
  -> private source snapshot
  -> QImageReader dimension/frame-count gate
  -> OpenCV imdecode(IMREAD_UNCHANGED)
  -> EXIF orientation apply
  -> BGR detector view
  -> ONNX/OpenCV detector(s)
  -> optional BlockingQueued GUI ReviewDialog
  -> finite/positive box validation + padded clipping
  -> mosaic/blur/fill/custom-overlay (mosaic safety layer first)
  -> in-memory encode
  -> metadata removal or selected EXIF allowlist rewrite
  -> hidden private rooted staging file
  -> no-replace final publication
```

- **Trust boundaries:** path/file bytes, image decoder, optional user ONNX, detector output, review edits, destination filesystem.
- **Ownership/lifetime:** GUI가 immutable `ProcessingRequest`와 detector cache를 worker에 넘긴다. image workers는 `processOrdered` threads에서 item 결과를 만들고 worker thread가 ordered consume한다. detector 호출은 mutex로 serialize된다.
- **Temporary data:** item source snapshot `QTemporaryDir`; output root 아래 mode 0700/Windows-private hidden stage; encoded bytes와 review preview in memory.
- **Failure points:** scan/path conflict, source identity change, dimension/multiframe/file-size cap, decode/model/review/encode/metadata/publish failure. Item counters가 final UI outcome을 결정한다.
- **Privacy exposure points:** source snapshot crash residue, GUI preview memory, local rotating log의 filename/path, output before user review. Network sink는 없다.

### 6.2 Video pipeline

```text
[External MP4/MOV/M4V]
  -> ffprobe JSON (first video + first audio metadata only)
  -> codec/8-bit SDR/dimension/fps/duration/frame-count gates
  -> same-destination source snapshot (POSIX owner-only; Windows parent ACL inherited)
  -> FFmpeg pass 1: fps filter -> raw BGR frames
  -> scene-cut detector + low-threshold detections
  -> forward/backward ByteTracker merge
  -> low-confidence filter -> interpolation -> smoothing -> end extension
  -> optional BlockingQueued VideoReviewDialog
       -> independent FFmpeg image2pipe preview timeline
       -> track exclusion/manual keyframes
  -> per-frame region materialization
  -> FFmpeg pass 2: same snapshot/fps timeline at native resolution
  -> parallel frame masking batches
  -> FFmpeg stdin encoder + source first audio stream
  -> metadata/chapter strip + MP4 staging
  -> rooted no-replace move/copy publication
  -> snapshot/encode-stage cleanup
```

- **Trust boundaries:** ffprobe JSON and FFmpeg behavior, timestamps/time bases, detector/tracker results, GUI manual coordinates, encoder capability, audio/container streams, filesystem.
- **Thread/process boundaries:** worker QThread; detector call; mask worker pool; FFmpeg child processes; GUI review uses blocking queued call and its own asynchronous preview QProcess.
- **Timeline ownership:** `VideoInfo` preserves rational fps, rotation, duration, estimated count, but not stream start/time-base or SAR. Main pass uses raw-frame index; review independently reconstructs frames. 이 불완전한 contract가 `CF-006`과 `CF-009`의 원인이다.
- **Temporary data:** output-root `.cloakframe-snapshot-*` contains a full source copy; `.cloakframe-encode-*` contains `video.mp4`; both normal error/cancel paths clean up한다.
- **Final state:** `processVideo`의 `Completed`와 `trackCount>0`만으로 worker가 redacted count를 증가시킨다. coverage hole/drop 정보는 result에 없어 `Done`으로 승격된다.

### 6.3 Network/update/model pipeline

```text
Built-in model:
  fixed HTTPS GitHub URL -> Qt redirect policy -> bounded in-memory reply
  -> fixed catalog SHA-256 -> predictable .part QFile -> rename to cache
  -> run-start full-file SHA-256 -> detector re-read + SHA-256 -> ORT/OpenCV session

Generic update notification:
  api.github.com releases/latest -> semver compare -> trusted github.com page URL

Windows/Linux in-app update:
  Velopack GithubSource -> releases.<channel>.json
  -> package URL/size/SHA from same release trust domain
  -> Velopack cache -> apply/restart

macOS in-app update:
  Info.plist SUFeedURL -> GitHub appcast -> Sparkle
  -> Apple code-signing validation; EdDSA key/signature optional in workflow
```

이미지/영상 content가 network request body/query에 포함되는 경로는 찾지 못했다. Model URL은 고정되고 hash는 앱 binary/source에 고정된다. 반면 update hash는 같은 online release feed가 공급하므로 독립 authenticity가 아니다.

### 6.4 GUI/worker state machine

```text
Idle
  -> validate/configure/snapshot request
  -> Processing + "Starting" (Stop enabled)
  -> QThread started -> ProcessorWorker::process
  -> scan/preflight/load model/process items
  -> optional blocking image/video review
  -> encode/publish
  -> Completed | CompletedWithWarnings | Cancelled | Failed
  -> GUI thread quit/wait/cache return/UI state
```

Stop/close는 atomic cancel을 set하고 worker/FFmpeg loops가 poll한다. 그러나 worker entry가 flag를 false로 reset해 start 직후 요청을 잃는다 (`CF-012`). 이후 callback은 `QPointer`, disconnect, thread wait와 Qt receiver auto-disconnect로 대체로 방어된다. 이전 run이 끝나기 전 새 run을 시작하는 UI path는 비활성화되어 있다.

## 7. Security Boundaries and Product Invariants

| Invariant | Verdict | Evidence / exception |
|---|---|---|
| Media content stays local | Maintained in audited code | network paths are fixed model/update metadata/assets; no media upload sink |
| Logs/network requests contain no file contents | Maintained | logs may contain names/paths and UI discloses this; pixel/file bytes are not logged or sent |
| Original is never modified/replaced | Maintained for audited normal paths | source is opened read-only/copied to snapshot; rooted no-replace destination; identity rechecks |
| Existing output is not silently replaced | Maintained at final sink | O_EXCL/CREATE_NEW/no-replace; preflight identity mismatch is `CF-021` but final overwrite is blocked |
| Different inputs cannot silently map to one output | Partially maintained | lexical preflight differs from actual filesystem, but second final publication fails rather than overwrites (`CF-021`) |
| Failed/cancelled work does not resemble complete output | Violated/probable | POSIX fallback can expose a partial final name on crash/cancel (`CF-015`); early cancel is lost (`CF-012`) |
| Missing/unsafe redaction cannot become `Done` | Violated | `CF-003`–`CF-006` |
| Transparent custom overlay never reveals original pixels | Maintained | ROI mosaicked first; existing transparent-overlay test passes |
| Invalid/clipped boxes do not expose an accepted original region | Mostly maintained | finite/positive filtering and padded clipping exist; silent detector/track omission remains separate |
| Metadata policy matches current UI | Maintained for current image/video UI | image default strip; allowlisted EXIF only when enabled; video strips metadata/chapters |
| Downloaded built-in model has strong integrity before use | Maintained | fixed SHA at download and detector byte load; corrupt cache cannot create session |
| Incomplete/tampered model cannot become valid cache | Integrity maintained; cache hardening incomplete | invalid bytes rejected; predictable symlink-following `.part` is `CF-019` |
| Update authenticity has an app-pinned trust anchor | Violated | Windows/Linux `CF-001`; macOS extra EdDSA absent `CF-018` |
| Update cache/replace cannot be pre-seeded | Violated on Linux | `CF-002` |
| Platform/architecture/version select is exact | Partially maintained | official artifacts target documented arches; tag/version mismatch `CF-024`; non-x86 tool path `CF-017` |
| Custom ONNX native-runtime risk is safely isolated | Not maintained, but disclosed | explicit trust warning exists; same-process execution and non-digest-bound consent `CF-016` |

## 8. Audit Coverage Matrix

### 8.1 Production source/header coverage

`✓ P1/P2`는 두 sweep에서 직접 확인, `✓ focused`는 작은 support file 전체와 caller를 함께 확인했다는 뜻이다.

| Production unit | LOC | Coverage | Main concerns checked |
|---|---:|---|---|
| `src/ImageIo.cpp`, `include/cloakframe/ImageIo.hpp` | 1,990 + 88 | ✓ P1/P2 | decode/encode, EXIF, rooted publish, symlink/TOCTOU, POSIX/Windows branches |
| `src/ImageScanner.cpp`, `include/cloakframe/ImageScanner.hpp` | 146 + 23 | ✓ P1/P2 | recursion, canonical de-dup, output-inside-input |
| `src/MainWindow.cpp`, `include/cloakframe/MainWindow.hpp` | 2,492 + 255 | ✓ P1/P2 | validation, state, QObject lifetime, UI claims, updater, review bridge |
| `src/MemoryBudget.cpp`, `include/cloakframe/MemoryBudget.hpp` | 68 + 12 | ✓ P1/P2 | physical-memory discovery, caps/platform branches |
| `src/ModelCatalog.cpp`, `include/cloakframe/ModelCatalog.hpp` | 89 + 35 | ✓ P1/P2 | URLs, digests, cache lookup, licenses |
| `src/ModelDownloader.cpp`, `include/cloakframe/ModelDownloader.hpp` | 225 + 16 | ✓ P1/P2 | redirect/size/hash/cache/temp/custom consent |
| `src/Mosaic.cpp`, `include/cloakframe/Mosaic.hpp` | 696 + 30 | ✓ P1/P2 | clipping, alpha, fill, masks, rotation, overlap, Mat ownership |
| `src/OnnxGraphPatch.cpp`, `include/cloakframe/OnnxGraphPatch.hpp` | 530 + 11 | ✓ P1/P2 | protobuf bounds/shape patch, malformed graph limits |
| `src/OrtAcceleration.cpp`, `include/cloakframe/OrtAcceleration.hpp` | 137 + 20 | ✓ focused | provider setup/fallback/session configuration |
| `src/OutputPlan.cpp`, `include/cloakframe/OutputPlan.hpp` | 75 + 31 | ✓ P1/P2 | mapping, extension, collisions, case/Unicode |
| `include/cloakframe/PathSafety.hpp`, `PathUtil.hpp` | 69 + 31 | ✓ P1/P2 | lexical escape, platform path conversion |
| `src/PlateDetector.cpp`, `include/cloakframe/PlateDetector.hpp` | 286 + 40 | ✓ P1/P2 | model digest, tensor bounds, resize mapping/NMS |
| `src/ProcessorWorker.cpp`, `include/cloakframe/ProcessorWorker.hpp` | 1,367 + 161 | ✓ P1/P2 | orchestration, summary state, cancellation, memory, image/video sinks |
| `src/ReviewDialog.cpp`, `include/cloakframe/ReviewDialog.hpp`, `ReviewTypes.hpp` | 772 + 54 + 24 | ✓ P1/P2 | preview scaling, manual boxes, skip/copy, dialog lifetime |
| `src/SceneCut.cpp`, `include/cloakframe/SceneCut.hpp` | 170 + 48 | ✓ P1/P2 | confirmation, reverse cuts, consecutive transitions |
| `src/ScrfdFaceDetector.cpp`, `include/cloakframe/ScrfdFaceDetector.hpp` | 624 + 71 | ✓ P1/P2 | tensor shapes, candidates, resize reverse-map, hash |
| `src/Yolo5FaceDetector.cpp`, `include/cloakframe/Yolo5FaceDetector.hpp` | 326 + 42 | ✓ P1/P2 | output schema, NMS/caps, hash, pose |
| `src/YuNetFaceDetector.cpp`, `include/cloakframe/YuNetFaceDetector.hpp` | 213 + 32 | ✓ P1/P2 | channel/box/landmark mapping, hash |
| `include/cloakframe/Detector.hpp`, `DetectionGeometry.hpp`, `FaceDetection.hpp` | 20 + 60 + 41 | ✓ focused | API contracts, finite/positive guards, letterbox geometry |
| `include/cloakframe/OrderedParallel.hpp` | 108 | ✓ P1/P2 | cancellation, ordering, exception/thread construction |
| `src/Tracking.cpp`, `include/cloakframe/Tracking.hpp` | 764 + 127 | ✓ P1/P2 | IoU matching, IDs, bidirectional merge, gap/angle/cuts, caps |
| `src/VideoIo.cpp`, `include/cloakframe/VideoIo.hpp` | 1,143 + 150 | ✓ P1/P2 | probe/timebase/SAR/audio, QProcess, encoder, staging/publication |
| `src/VideoProcessor.cpp`, `include/cloakframe/VideoProcessor.hpp` | 928 + 81 | ✓ P1/P2 | snapshot, two passes, detection memory, tracking, mask pool, status |
| `src/VideoReviewDialog.cpp`, header | 1,090 + 81 | ✓ P1/P2 | preview process, timeline, exclusions, HiDPI/manual coordinates |
| `src/VideoReviewTypes.cpp`, header | 133 + 75 | ✓ P1/P2 | manual ranges/keyframes/interpolation/validation |
| `src/SelfUpdaterVelopack.cpp`, `SelfUpdaterSparkle.mm`, `SelfUpdaterNull.cpp`, header | 175 + 53 + 13 + 47 | ✓ P1/P2 | trust/update/apply/thread lifetime/platform modes |
| `src/UpdateChecker.cpp`, header | 101 + 31 | ✓ focused | API URL, semver, redirect/release URL restriction |
| `src/SettingsDialog.cpp`, header | 160 + 58 | ✓ focused | safety/log/GPU/update wording and persistence |
| `src/Theme.cpp`, header | 564 + 26 | ✓ focused | GUI-only lifetime/HiDPI/icon paths |
| `src/main.cpp` | 133 | ✓ P1/P2 | startup hooks, logging, settings migration, meta-types |

`src/CMakeLists.txt`, root CMake, four CMake modules, all packaging/notarization scripts, both workflows, test-model fetch script, four C++ test files, translation checker, model-inspection tool도 직접 조사했다.

### 8.2 Tracked file inventory at audited HEAD

<details>
<summary>104 tracked files</summary>

```text
.github/scripts/fetch_test_models.sh
.github/workflows/ci.yml
.github/workflows/release.yml
.gitignore
CMakeLists.txt
LICENSE
README.md
RELEASE_NOTES.md
THIRD_PARTY_NOTICES.txt
assets/CloakFrame.icns
assets/CloakFrame.rc
assets/MacOSXBundleInfo.plist.in
assets/cloakframe-512.png
assets/cloakframe.desktop
assets/cloakframe.ico
assets/cloakframe.png
cmake/CloakFrameHarden.cmake
cmake/CloakFrameSparkle.cmake
cmake/CloakFrameVelopack.cmake
cmake/FindOnnxRuntimeLocal.cmake
include/cloakframe/DetectionGeometry.hpp
include/cloakframe/Detector.hpp
include/cloakframe/FaceDetection.hpp
include/cloakframe/ImageIo.hpp
include/cloakframe/ImageScanner.hpp
include/cloakframe/MainWindow.hpp
include/cloakframe/MemoryBudget.hpp
include/cloakframe/ModelCatalog.hpp
include/cloakframe/ModelDownloader.hpp
include/cloakframe/Mosaic.hpp
include/cloakframe/OnnxGraphPatch.hpp
include/cloakframe/OrderedParallel.hpp
include/cloakframe/OrtAcceleration.hpp
include/cloakframe/OutputPlan.hpp
include/cloakframe/PathSafety.hpp
include/cloakframe/PathUtil.hpp
include/cloakframe/PlateDetector.hpp
include/cloakframe/ProcessorWorker.hpp
include/cloakframe/ReviewDialog.hpp
include/cloakframe/ReviewTypes.hpp
include/cloakframe/SceneCut.hpp
include/cloakframe/ScrfdFaceDetector.hpp
include/cloakframe/SelfUpdater.hpp
include/cloakframe/SettingsDialog.hpp
include/cloakframe/Theme.hpp
include/cloakframe/Tracking.hpp
include/cloakframe/UpdateChecker.hpp
include/cloakframe/VideoIo.hpp
include/cloakframe/VideoProcessor.hpp
include/cloakframe/VideoReviewDialog.hpp
include/cloakframe/VideoReviewTypes.hpp
include/cloakframe/Yolo5FaceDetector.hpp
include/cloakframe/YuNetFaceDetector.hpp
release-notes/v1.10.0.md
release-notes/v1.10.1.md
release-notes/v1.10.2.md
release-notes/v1.11.0.md
release-notes/v1.9.1.md
scripts/check_translations.py
scripts/entitlements.plist
scripts/notarize_macos.sh
scripts/package_linux.sh
scripts/package_macos.sh
scripts/package_windows.ps1
src/CMakeLists.txt
src/ImageIo.cpp
src/ImageScanner.cpp
src/MainWindow.cpp
src/MemoryBudget.cpp
src/ModelCatalog.cpp
src/ModelDownloader.cpp
src/Mosaic.cpp
src/OnnxGraphPatch.cpp
src/OrtAcceleration.cpp
src/OutputPlan.cpp
src/PlateDetector.cpp
src/ProcessorWorker.cpp
src/ReviewDialog.cpp
src/SceneCut.cpp
src/ScrfdFaceDetector.cpp
src/SelfUpdaterNull.cpp
src/SelfUpdaterSparkle.mm
src/SelfUpdaterVelopack.cpp
src/SettingsDialog.cpp
src/Theme.cpp
src/Tracking.cpp
src/UpdateChecker.cpp
src/VideoIo.cpp
src/VideoProcessor.cpp
src/VideoReviewDialog.cpp
src/VideoReviewTypes.cpp
src/Yolo5FaceDetector.cpp
src/YuNetFaceDetector.cpp
src/main.cpp
tests/CMakeLists.txt
tests/test_core.cpp
tests/test_parallel.cpp
tests/test_tracking.cpp
tests/test_video_io.cpp
tools/CMakeLists.txt
tools/inspect_model.cpp
translations/cloakframe_ja.ts
translations/cloakframe_ko.ts
translations/cloakframe_zh_CN.ts
```

</details>

## 9. Findings Summary Table

> 감사 시점(2026-08-06) 기록이다. 대부분은 이후 수정되었으므로 현재 상태는
> §0.1을 본다.

| ID | Classification | Severity | Confidence | Component | Platforms | Summary |
|---|---|---|---|---|---|---|
| CF-001 | Confirmed defect | Critical | High | Velopack updater/release | Windows, Linux | Update feed와 package에 독립 서명 trust anchor가 없어 release 권한 탈취가 임의 code update로 이어짐 |
| CF-002 | Confirmed defect | High | High | Velopack cache/apply | Linux | 공유 `/var/tmp` cache 선점과 existing-package 검증 생략으로 다른 사용자 권한 code execution |
| CF-003 | Confirmed defect | High | High | YOLO5Face/image worker | All | 300개 이후 검출을 조용히 버리고 부분 redaction을 `Done` 처리 |
| CF-004 | Confirmed defect | High | High | Video tracking/review | All | Low-confidence track을 통째로 버려 실제 strong detection frames도 노출 |
| CF-005 | Confirmed defect | High | High | Tracking/interpolation | All | Retained track 안의 interpolation-rejected gap을 가리지 않고 `Done` 처리 |
| CF-006 | Confirmed defect | High | High | Video timeline/review/audio | All | Nonzero stream start가 preview와 encode frame을 어긋나게 하고 audio를 잘못 자름 |
| CF-007 | Confirmed defect | High | High | Release CI | Hosted CI/macOS release | 임의 workflow_dispatch branch가 signing/notarization secrets를 사용 가능 |
| CF-008 | Confirmed defect | Medium | High | FFmpeg decode/status | All | Recoverable decode error로 20 frames가 사라져도 정상 output으로 게시 |
| CF-009 | Confirmed defect | Medium | High | Video geometry | All | Sample aspect ratio를 버려 output DAR와 detection/review geometry 왜곡 |
| CF-010 | Confirmed defect | Medium | High | Video audio mux | All | 여러 audio stream 중 첫 stream만 남기고 나머지를 조용히 삭제 |
| CF-011 | Confirmed defect | Medium | High | Hardware encoder | All | Probe 후 실제 hardware encode 실패 시 software fallback 없이 작업 실패 |
| CF-012 | Confirmed defect | Medium | High | Cancellation/state | All | Worker start 직전 cancel을 `process()`가 reset하여 작업/output이 계속됨 |
| CF-013 | Confirmed defect | Medium | High | Thread construction | All | N번째 `std::thread` 생성 실패가 catch 가능한 오류 대신 `std::terminate` 유발 |
| CF-014 | Probable defect | Medium | High | Image memory budget | All | Budget보다 큰 단일 image estimate를 budget으로 축소 예약해 OOM 가능 |
| CF-015 | Probable defect | Medium | High | POSIX publication | macOS, Linux | rename/link 불가 filesystem에서 final 이름에 직접 copy하여 crash/cancel partial output 가능 |
| CF-016 | Hardening opportunity | Medium | High | Custom ONNX | All | 승인한 path의 bytes가 바뀌어도 재동의 없이 same-process native runtime에 로드 |
| CF-017 | Hardening opportunity | High | High | Linux packaging | Linux non-x86 | Mutable `continuous` linuxdeploy binary를 hash 없이 내려받아 실행 |
| CF-018 | Hardening opportunity | Medium | High | Sparkle | macOS | Apple code signing만 trust anchor로 사용하고 EdDSA key/signature가 optional |
| CF-019 | Hardening opportunity | Low | High | Model cache | All | Predictable `.part`를 일반 QFile로 열어 symlink target을 overwrite 가능 |
| CF-020 | Hardening opportunity | Medium | High | Image/video source snapshots | All | Crash/SIGKILL 후 system temp/output root에 full original snapshot이 남을 수 있고 startup scavenger 없음 |
| CF-021 | Documentation/build inconsistency | Low | High | OutputPlan/README | All, FS-dependent | Preflight case/Unicode identity가 실제 volume과 달라 README의 start-time collision guarantee와 불일치 |
| CF-022 | Documentation/build inconsistency | Low | High | RGBA fill/UI | All | “opaque black” fill이 4-channel image에서 alpha 0 transparent pixels 생성 |
| CF-023 | Documentation/build inconsistency | Low | High | Japanese translation/CI | All | 일본어 지원을 표방하지만 안전 문구 포함 96개가 영어이고 CI는 pass |
| CF-024 | Documentation/build inconsistency | Medium | High | Release versioning | All artifacts | `v*` tag와 CMake application version 일치 검사가 없어 잘못된 feed/artifact version 게시 가능 |
| CF-025 | Documentation/build inconsistency | Info | Medium | macOS notices | macOS | Unversioned Homebrew dependency closure와 고정 notice가 drift하며 local bundle에서 미기재 libs 관찰 |
| CF-026 | Documentation/build inconsistency | Low | High | Scene cuts/release notes | All | 빠른 연속 두 cut 중 두 번째를 잃지만 release notes는 every cut boundary를 보장 |

## 10. Detailed Confirmed Defects

### CF-001 — Windows/Linux update에 독립된 authenticity trust anchor가 없다

- **분류:** Confirmed defect
- **심각도:** Critical
- **신뢰도:** High
- **영향받는 플랫폼:** Windows x64 installer/portable update, Linux x86_64 AppImage update
- **위치:** `src/SelfUpdaterVelopack.cpp:16-32,47-81` (`VelopackWorker::check/download/apply`), `src/MainWindow.cpp:1857-1865,1920-1995` (update UI/apply), `cmake/CloakFrameVelopack.cmake:3-13`, `scripts/package_windows.ps1:263-294`, `scripts/package_linux.sh:154-175`, `.github/workflows/release.yml:97-188,190-272,274-305`
- **위반되는 불변조건:** unsigned update, 동일 online origin이 선택한 digest만으로 검증되는 update, 변조된 feed/package가 허용되지 않아야 한다.
- **사전 조건:** 공격자가 repository/release write token, GitHub account, release automation 또는 그와 동등한 release 게시 권한을 탈취한다. 단순 network MITM이나 package asset만 바꾸는 공격은 전제하지 않는다.
- **정확한 실행 순서:** (1) 공격자가 `releases.win.json`/`releases.linux.json`과 malicious package를 같은 release에 게시한다. (2) `GithubSource(kRepoUrl)`가 release feed를 읽는다. (3) feed가 package URL, size, SHA를 제공한다. (4) `DownloadUpdates`는 공격자가 함께 제공한 SHA와 malicious bytes가 일치하므로 성공한다. (5) 사용자가 UI의 Update/Restart를 승인한다. (6) `WaitExitThenApplyUpdates`가 package를 설치하고 공격자 code가 실행된다.
- **기대 동작:** GitHub release 계정/online feed가 변조되어도 앱에 미리 고정된 offline public key 또는 OS signer identity로 인증되지 않은 update는 거부해야 한다.
- **실제 동작:** package의 hash는 package와 같은 GitHub release trust domain에 있는 feed가 선택한다. Windows `vpk pack`과 Linux `vpk pack`은 signer argument 없이 실행되고, app에 검증 public key가 없다. 같은 job이 만드는 `SHA256SUMS`는 updater의 독립 신뢰 입력이 아니다.
- **코드 증거:** pinned Velopack 1.2.0의 [GitHubSource](https://github.com/velopack/velopack/blob/f2edcbcafb81da5b3c884aaea330e225ad91d8b6/src/lib-rust/src/sources/github.rs#L68-L131)와 [feed 병합](https://github.com/velopack/velopack/blob/f2edcbcafb81da5b3c884aaea330e225ad91d8b6/src/lib-rust/src/sources/mod.rs#L107-L159)이 online metadata를 신뢰한다. [manager download/apply](https://github.com/velopack/velopack/blob/f2edcbcafb81da5b3c884aaea330e225ad91d8b6/src/lib-rust/src/manager.rs#L384-L545)는 그 metadata의 hash를 사용하고, [Windows apply](https://github.com/velopack/velopack/blob/f2edcbcafb81da5b3c884aaea330e225ad91d8b6/src/bins/src/commands/apply_windows_impl.rs#L36-L136)에 CloakFrame 고유 signature verification이 없다.
- **동적 검증 결과:** **Not dynamically verified.** 실제 malicious Windows/Linux update를 적용하는 검증은 안전·플랫폼 제약상 수행하지 않았다. `dnx vpk@1.2.0 -- pack -h`, local package command, pinned upstream source를 대조하여 trust path를 논리적으로 완결했다.
- **사용자 영향:** 공식 UI가 제안하는 update를 승인하는 정상 행동만으로 arbitrary code가 사용자 계정 권한으로 지속 실행될 수 있다.
- **보안·개인정보 영향:** update supply-chain takeover이며 local media, model cache, output, credentials에 대한 전면 접근으로 이어진다. 제품의 “media stays local” 보장을 application 내부에서 우회할 수 있다.
- **기존 테스트가 잡지 못한 이유:** build/CTest는 updater authenticity나 compromised-feed threat model을 실행하지 않는다. HTTPS와 SHA 일치만 성공 조건으로 간주한다.
- **최소 수정 방향:** app binary에 offline public key를 고정하고 canonical signed manifest와 full/delta package signature를 verify한다. Windows는 Authenticode signer pin/필수 서명을 추가하되 Linux에도 동일 수준의 signed metadata가 필요하다. signature가 없거나 invalid하면 download/apply 모두 fail closed해야 한다.
- **필요한 regression test:** 정상/서명 누락/다른 key/manifest tamper/package tamper/replay/downgrade/wrong channel·arch를 포함한 update integration test. Release CI는 unsigned feed/package 생성 시 실패해야 한다.
- **관련 finding:** `CF-002`는 independent signature 유무와 별개로 existing local package 검증 자체를 건너뛰는 Linux cache 취약점이다. `CF-007`, `CF-017`, `CF-018`, `CF-024`는 release authority와 platform별 trust를 보강한다.

### CF-002 — Linux 공유 `/var/tmp` update cache를 선점해 다른 사용자 권한으로 code를 실행할 수 있다

- **분류:** Confirmed defect
- **심각도:** High
- **신뢰도:** High
- **영향받는 플랫폼:** Linux AppImage self-update
- **위치:** `cmake/CloakFrameVelopack.cmake:3-13` (Velopack 1.2.0 pin), `scripts/package_linux.sh:165-174` (`vpk pack`, `packId CloakFrame`), `src/SelfUpdaterVelopack.cpp:47-81` (`VelopackWorker::download/apply`); pinned upstream `locator.rs:463-553` (특히 package directory `:537-542`), `manager.rs:384-468,600-650` (download/apply selection), `apply_linux_impl.rs:7-83` (Linux apply)
- **위반되는 불변조건:** update cache가 다른 사용자에게 선점·변조되지 않고, 이미 존재하는 package도 사용/apply 직전에 size/hash/signature를 재검증해야 한다.
- **사전 조건:** multi-user Linux host에서 `/var/tmp/velopack/CloakFrame/packages`를 attacker가 먼저 소유하거나 writable하게 만들 수 있고, victim이 self-update 가능한 CloakFrame AppImage를 실행한다. 공격자는 공개 feed로 target filename을 안다.
- **정확한 실행 순서:** (1) attacker가 `/var/tmp` 아래 `velopack/CloakFrame/packages` tree를 먼저 만들고 victim이 접근할 수 있게 한다. (2) 공개될 full/delta package의 정확한 filename으로 malicious nupkg를 둔다. (3) victim의 update check는 정상 GitHub feed를 받는다. (4) manager는 final package path가 이미 존재하면 download 및 feed SHA/size 검증 없이 성공으로 반환한다. (5) restart/apply path는 존재 여부만 확인한다. (6) Linux apply가 nupkg의 AppImage를 추출해 victim AppImage를 `mv -f`로 교체한다. (7) 재시작 시 attacker AppImage가 victim 권한으로 실행된다.
- **기대 동작:** cache는 해당 사용자 전용 0700 directory여야 하며 owner/mode/symlink를 검증해야 한다. existing file도 feed hash와 independent signature를 검증하고 apply 직전 다시 검사해야 한다.
- **실제 동작:** Velopack 1.2.0 [Linux locator](https://github.com/velopack/velopack/blob/f2edcbcafb81da5b3c884aaea330e225ad91d8b6/src/lib-rust/src/locator.rs#L463-L553)는 `/var/tmp/velopack/{app.id}/packages`를 선택한다. [download manager](https://github.com/velopack/velopack/blob/f2edcbcafb81da5b3c884aaea330e225ad91d8b6/src/lib-rust/src/manager.rs#L384-L468)는 existing final path의 hash를 재검증하지 않고, [apply selection](https://github.com/velopack/velopack/blob/f2edcbcafb81da5b3c884aaea330e225ad91d8b6/src/lib-rust/src/manager.rs#L600-L650)도 재검증하지 않는다. [Linux apply](https://github.com/velopack/velopack/blob/f2edcbcafb81da5b3c884aaea330e225ad91d8b6/src/bins/src/commands/apply_linux_impl.rs#L7-L83)는 package에서 AppImage를 꺼내 교체한다. `/var/tmp` sticky bit는 attacker 소유 하위 tree 내부를 victim에게 안전하게 만들지 않는다.
- **코드 증거:** CloakFrame은 이 exact version과 ID를 package/runtime 양쪽에서 사용하며 custom per-user package directory를 지정하지 않는다.
- **동적 검증 결과:** **Not dynamically verified.** 별도 두 사용자와 실제 malicious AppImage 적용은 수행하지 않았다. Path choice, skip condition, apply sink를 pinned source에서 완결 추적했다.
- **사용자 영향:** 같은 Linux machine의 다른 low-privilege user가 victim의 다음 CloakFrame restart에서 code를 실행할 수 있다.
- **보안·개인정보 영향:** sandbox가 없는 desktop process 권한으로 private media와 output에 접근한다. System privilege escalation은 입증하지 않았으므로 Critical 대신 High로 판정했다.
- **기존 테스트가 잡지 못한 이유:** repository tests는 AppImage locator/cache/apply를 포함하지 않고 single-user temporary directories만 사용한다.
- **최소 수정 방향:** patched upstream으로 교체하거나 per-user XDG cache 아래 mode 0700 directory를 강제하고, component마다 no-follow/owner/mode check를 수행한다. Existing file도 size/hash/signature를 다시 검증하고 open descriptor 또는 immutable handle을 apply까지 연결한다.
- **필요한 regression test:** 두 UID가 공유 `/var/tmp`에서 attacker-owned parent, symlink, pre-existing correct-name/wrong-hash package, download/apply 사이 swap을 시도하고 모두 거부되는 integration test.
- **관련 finding:** `CF-001`의 signed manifest가 있더라도 existing-file skip이 signature 확인을 우회하면 이 finding은 남는다.

### CF-003 — YOLO5Face가 300개 이후 검출을 조용히 버리고 부분 비식별화를 `Done` 처리한다

- **분류:** Confirmed defect
- **심각도:** High
- **신뢰도:** High
- **영향받는 플랫폼:** 모든 image-processing platform
- **위치:** `src/Yolo5FaceDetector.cpp:25-31,310-324` (`Yolo5FaceDetector::detect`), `src/ProcessorWorker.cpp:856-877,990-1037,609-619` (`processItem`, `process`)
- **위반되는 불변조건:** 알려진 detector output의 일부가 누락되거나 safety cap에 도달한 결과가 정상 redaction/`Done`으로 저장되지 않아야 한다.
- **사전 조건:** valid model output에서 threshold와 NMS를 통과한 서로 독립적인 얼굴 후보가 301개 이상 존재한다. Large crowd/contact sheet 또는 adversarially dense synthetic output이 예다.
- **정확한 실행 순서:** detector가 candidates를 만들고 NMS 후 301개 이상을 얻는다 → `resize(kMaxDetections)`가 score 순서의 첫 300개만 남긴다 → API는 overflow bit 없이 정상 vector를 반환한다 → worker는 그 300개만 `finalFaces`로 익명화한다 → output publication 성공 → `finalFaces.empty()==false`이므로 redacted count가 증가한다 → 다른 warning이 없으면 `RunOutcome::Completed`와 `src/MainWindow.cpp:1553-1559`의 UI `Done`.
- **기대 동작:** cap 도달은 fail-closed overflow 상태여야 한다. 모든 region을 안전하게 처리하거나 item을 실패/Review Required로 분류해야 한다.
- **실제 동작:** 301번째 이후 원본 pixel이 그대로 남아도 truncation이 caller/status에 전달되지 않는다. SCRFD도 `src/ScrfdFaceDetector.cpp:31,597-608`에서 pre-NMS 2,000 후보를 silent truncate하므로 detector result contract에 공통 overflow 표현이 없다.
- **코드 증거:** `kMaxDetections=300`; `detections.resize(300)` 후 즉시 return. `ProcessorWorker`에는 original candidate count 또는 truncation 상태를 받을 field가 없다.
- **동적 검증 결과:** **Not dynamically verified.** local model asset이 없고 실제 개인 crowd image를 사용하지 않았다. Unconditional reachable resize와 caller/status trace로 결함을 확정했다.
- **사용자 영향:** 수백 명이 있는 한 장의 image에서 일부 얼굴이 완전히 노출된 채 제품이 공유 가능한 `Done`으로 표시할 수 있다.
- **보안·개인정보 영향:** 핵심 비식별화 보장을 직접 위반하는 광범위한 개인정보 노출이다.
- **기존 테스트가 잡지 못한 이유:** `tests/test_core.cpp:1310-1355`의 optional model test는 blank/nonempty/pose만 확인한다. crowded tensor, cap, final pixels, outcome은 확인하지 않는다.
- **최소 수정 방향:** detector API를 `{detections, truncated/overflow}`로 바꾸고 overflow item을 fail closed한다. 필요하면 tiling/streaming NMS로 모든 검출을 처리하되 최대치 초과를 사용자에게 숨기지 않는다.
- **필요한 regression test:** `[1,25200,16]` constant-output ONNX fixture가 301개 이상의 non-overlapping high-score boxes를 만들게 하고, 모두 가려지거나 output이 게시되지 않으며 `Done`이 아님을 production worker path로 검증한다.
- **관련 finding:** `CF-004`, `CF-005`도 coverage 상태를 result에 전달하지 않아 `Done`이 되는 같은 상위 설계 결함이지만, 누락을 만드는 알고리즘 원인은 서로 다르다.

### CF-004 — Low-confidence video track 삭제가 strong detection frames까지 노출시킨다

- **분류:** Confirmed defect
- **심각도:** High
- **신뢰도:** High
- **영향받는 플랫폼:** 모든 video-processing platform
- **위치:** `src/ProcessorWorker.cpp:1151-1168,1252-1255,1316-1329,582-619`, `src/VideoProcessor.cpp:600-615`, `src/Tracking.cpp:683-725` (`postProcessTracks`), `src/VideoReviewDialog.cpp:474-483,510-535`, `src/MainWindow.cpp:1553-1567`
- **위반되는 불변조건:** detector가 실제 반환한 얼굴/번호판 후보를 policy상 버릴 때 그 pixel이 원본으로 남은 output을 clean `Done`으로 확정하지 않아야 한다.
- **사전 조건:** 한 track에 strong detection 수가 low-confidence retention rule보다 적고, 다른 accepted track이 하나 이상 존재한다. Review가 꺼져 있거나 review UI의 default unchecked 상태를 사용한다.
- **정확한 실행 순서:** detector threshold가 tracker low threshold까지 낮아진다 → ByteTracker가 후보를 하나의 track으로 만든다 → review 없음: `retainLowConfidenceTracks=false`, `postProcessTracks`가 전체 track을 erase한다; review 있음: track은 `lowConfidence=true`로 표시되지만 UI가 default unchecked/excluded한다 → erase된 track의 strong frames도 per-frame regions에서 사라진다 → 다른 track 때문에 `trackCount>0` → worker redacted count → `Done`.
- **기대 동작:** privacy-first default는 모든 detector-supported track을 포함해야 한다. Filter할 필요가 있으면 unresolved coverage로 기록하여 적어도 `Review required`가 되어야 한다.
- **실제 동작:** false-positive 억제를 위한 track-level filter가 known positive frames까지 제거하며 status에는 dropped track/detection count가 없다.
- **코드 증거:** `isLowConfidence`가 track 전체 boolean을 계산하고 `std::erase_if(tracks, isLowConfidence)`한다. Review dialog도 low-confidence item을 unchecked로 만들고 `excludedTrackIds_`에 넣는다.
- **동적 검증 결과:** 40-frame 160×120 synthetic video에서 first ROI는 모든 frame score 0.9, second ROI는 frame 3–4만 0.9이고 나머지는 0.2로 반환했다. Production `processVideo()`는 `Completed`, 40 frames, `trackCount=1`. First ROI frame 10 luma average는 15.9956으로 fill되었지만 second ROI는 strong frame 3과 frame 10 모두 40으로 unchanged였다.
- **사용자 영향:** 잠깐만 선명하게 보이는 얼굴/번호판이 명백히 검출됐어도 전체 track이 사라져 공유 output에 노출된다.
- **보안·개인정보 영향:** 중요한 검출 누락과 잘못된 `Done` 상태의 직접적인 privacy breach다.
- **기존 테스트가 잡지 못한 이유:** `tests/test_tracking.cpp:195-203,212-239`는 low-confidence track 삭제 자체를 기대값으로 고정하지만 final video pixels와 worker/UI outcome을 확인하지 않는다.
- **최소 수정 방향:** low-confidence track을 default include하거나 strong detection boxes만이라도 보수적으로 유지한다. Exclusion을 허용하면 explicit user decision과 unresolved count를 result/summary에 전달해 clean `Done`을 금지한다.
- **필요한 regression test:** mixed accepted/low-confidence production video에서 삭제 후보의 known-detection pixel coverage와 `Review required`/failure 상태를 함께 검증한다. Review default도 included여야 하거나 명시적 위험 확인을 요구해야 한다.
- **관련 finding:** `CF-005`는 retained track 내부 coverage hole, `CF-003`은 detector 단계 silent truncation이다.

### CF-005 — Retained track의 interpolation-rejected gap이 원본으로 남고 timeline/상태가 이를 숨긴다

- **분류:** Confirmed defect
- **심각도:** High
- **신뢰도:** High
- **영향받는 플랫폼:** 모든 video-processing platform
- **위치:** `include/cloakframe/Tracking.hpp:46-56,89-99` (`TrackerConfig`, `TrackPostProcessConfig`), `src/Tracking.cpp:271-336` (`ByteTracker::update`), `:505-537` (`interpolateGaps`), `:683-735` (`postProcessTracks`), `src/VideoProcessor.cpp:609-615,650-687,790-795,925` (`processVideo`), `src/VideoReviewDialog.cpp:418-431` (timeline paint)
- **위반되는 불변조건:** accepted track의 알려진 시간 범위 안에 unredacted original frames가 남는데도 complete coverage/`Done`으로 표시하지 않아야 한다.
- **사전 조건:** 같은 identity의 두 detection이 tracker에서는 한 track으로 연결되지만 그 사이가 (a) 21–29 missing frames이거나, (b) 더 짧더라도 interpolation motion/size guard를 통과하지 못한다. Scene cut으로 track이 분리되지 않은 경우다.
- **정확한 실행 순서:** tracker의 `maxFramesLost=30`은 재검출 frame과 마지막 detection frame 차이가 30 이하인 track을 active로 유지한다 → later detection이 같은 track에 붙는다 → `interpolateGaps`는 missing-frame count가 20을 넘거나 `gapMotionTooFast`/`sizeJumpTooLarge`가 true이면 box를 만들지 않는다 → track은 양쪽 box를 가진 채 retained된다 → review timeline은 first-to-last를 연속 bar로 그린다 → redaction pass는 `boxAtFrame`이 있는 frames만 가린다 → `trackCount=1`, `Completed`, `Done`.
- **기대 동작:** track 내부 hole은 conservative mask로 채우거나 track을 분리하고 unresolved interval을 Review Required로 표시해야 한다. Timeline은 실제 coverage hole을 보여야 한다.
- **실제 동작:** parameter mismatch로 21–29 missing-frame window가 명시적으로 uncovered이고, 1–20 frame gap도 matching guard보다 엄격한 interpolation motion/size guard에서 거부되면 같은 상태가 된다.
- **코드 증거:** active expiration은 `frame-lastFrame > 30`이므로 최대 missing-frame count는 29다. Interpolation은 `gap > 20`뿐 아니라 motion/size guard에서도 continue한다. 예를 들어 한 missing frame을 사이에 두고 같은 중심의 정사각형 크기가 1→1.45배가 되면 tracker의 gap-adjusted match는 허용될 수 있지만 interpolation의 area ratio limit은 거부한다. Timeline은 `boxes.front().frame`부터 `boxes.back().frame`까지 한 rectangle을 그린다.
- **동적 검증 결과:** frame 0–4와 26–39에 같은 box를 주어 21-frame gap을 만들었다. Production result는 `Completed`, 40 frames, `trackCount=1`. ROI luma는 frame 3 = 15.9456, frame 10 = 40 unchanged, frame 30 = 16.0933이었다.
- **사용자 영향:** 30fps에서 약 0.7초 동안 얼굴/번호판이 완전히 노출될 수 있으며 review timeline도 공백을 시각적으로 감춘다.
- **보안·개인정보 영향:** retained/accepted track의 부분 원본 노출과 잘못된 공유 가능 상태다.
- **기존 테스트가 잡지 못한 이유:** `tests/test_tracking.cpp:117-129`는 over-limit gap이 interpolation되지 않는 것을 정답으로 assert하지만 output pixel/status/timeline을 검사하지 않는다.
- **최소 수정 방향:** tracker matching과 interpolation의 coverage contract를 통합하거나 거부된 gap을 conservative predicted/union mask로 채운다. 불확실하면 split track + explicit unresolved interval로 처리하고 clean completion을 금지한다.
- **필요한 regression test:** 20/21/29/30 missing-frame boundary, short motion/size-guard rejection, multiple holes와 scene-cut 인접 case에서 every output frame coverage 및 status를 검증한다. Timeline rendering test도 hole을 표시해야 한다.
- **관련 finding:** `CF-004`와 같이 track count가 coverage completeness를 대신하는 status-model 결함을 공유한다.

### CF-006 — Nonzero stream start가 review frame과 encoded frame을 다르게 만들고 audio도 잘못 자른다

- **분류:** Confirmed defect
- **심각도:** High
- **신뢰도:** High
- **영향받는 플랫폼:** FFmpeg video pipeline을 쓰는 모든 플랫폼
- **위치:** `include/cloakframe/VideoIo.hpp:52-71` (`VideoInfo`), `src/VideoIo.cpp:421-540` (`probeVideo`), `src/VideoIo.cpp:603-652` (`VideoFrameReader::open`), `src/VideoIo.cpp:815-908` (`VideoFrameWriter::open`), `src/VideoReviewDialog.cpp:56-101,708-805` (`previewFrameArguments`, preview lifecycle), `src/VideoProcessor.cpp:497-576,694-752`, `src/VideoReviewTypes.cpp:90-130`
- **위반되는 불변조건:** review preview와 실제 redaction/encoding의 같은 frame index는 같은 displayed content여야 하고, 원 audio interval과 sync가 보존되어야 한다.
- **사전 조건:** video stream start가 format/audio start와 다르거나 negative/nonzero timestamp normalization이 필요한 input. 재현 input은 format/audio start 0s, video start 2s, video packets 40@10fps/4s, audio 6s였다.
- **정확한 실행 순서:** ffprobe JSON에는 start/time_base가 있지만 `VideoInfo`가 저장하지 않는다 → main reader의 `fps` filter와 rawvideo muxer가 format timeline 시작까지 duplicate/padding frames를 만들어 60-frame index space를 만든다 → review preview는 작은 index에서 no-seek `fps,select`를, 큰 index에서는 조건부 `-ss` + `fps:start_time=0,select`를 사용해 다른 normalization을 만든다; 아래 frame-10 재현은 no-seek branch였다 → 사용자가 preview frame N에 manual box/keyframe을 둔다 → encoder는 main reader frame N에 그 box를 적용한다 → writer는 video stream interval이 아니라 `info.durationSeconds`를 `-t` input option으로 first audio source의 시작부터 자른다.
- **기대 동작:** probe가 stream/format start, duration, time-base를 모델링하고 하나의 explicit normalized CFR timeline을 detection, preview, manual tracks, encoding, audio 모두 공유해야 한다.
- **실제 동작:** 같은 integer frame이 preview와 production에서 2초 다른 content를 가리켰으며 output audio interval도 video와 불일치했다.
- **코드 증거:** `VideoInfo`에는 start/time-base가 없고 probe가 해당 JSON fields를 무시한다. Reader와 preview의 filter argument가 서로 다르고, writer는 `-t <video duration>`을 second input `-i audioSource` 앞에 둔 뒤 `1:a:0?`을 map한다.
- **동적 검증 결과:** production raw frame 10 BGR MD5는 `12e50944df614adfc0e8cdeecebefbbb`, preview-request frame 10은 `961b13242a282c5c1ed7b5c8c4598e27`이었다. Preview frame 10은 production raw frame 30과 일치해 정확히 20-frame offset이었다. Input은 video 40 frames/start 2/duration 4, audio start 0/duration 6; output은 video 60 frames/duration 6, audio duration 4.010이었다.
- **사용자 영향:** review에서 missed face를 정확히 가린다고 생각해도 다른 encoded frame이 가려져 원래 face가 노출될 수 있다. Audio도 앞/뒤가 잘리거나 video와 drift한다.
- **보안·개인정보 영향:** manual privacy correction을 무효화하는 직접 노출 경로다. Audio data loss/sync corruption도 동반한다.
- **기존 테스트가 잡지 못한 이유:** `tests/test_video_io.cpp:398-434` 등의 fixtures는 zero-start이며 preview argument와 main reader의 per-frame pixel identity를 비교하지 않는다.
- **최소 수정 방향:** normalized timeline type에 start PTS/time-base/fps mapping을 포함하고 main reader/preview가 같은 extraction implementation을 사용하게 한다. `setpts=PTS-STARTPTS` 등 명시적인 policy를 양쪽에 동일 적용하고 audio도 선택한 video interval로 `atrim/asetpts` 또는 timestamp-aware map을 수행한다.
- **필요한 regression test:** positive/negative/differential starts, VFR, B-frame time base에 대해 모든 review frame hash와 production reader hash가 같고 output A/V endpoints/sync가 허용오차 내인지 검증한다. Manual box가 exact intended pixel을 가리는 end-to-end test가 필요하다.
- **관련 finding:** `CF-009`도 `VideoInfo`가 display geometry metadata를 충분히 모델링하지 않는 문제다. 두 main passes는 같은 snapshot과 동일한 fps/timeline filter를 사용하고 count equality를 검사한다. Pass 1은 analysis scaling을 추가하고 pass 2는 native resolution이므로 command가 완전히 동일하다는 주장은 하지 않는다.

### CF-007 — 임의 workflow_dispatch branch가 macOS signing/notarization secret을 사용할 수 있다

- **분류:** Confirmed defect
- **심각도:** High
- **신뢰도:** High
- **영향받는 플랫폼:** GitHub-hosted release CI와 macOS distribution signing authority
- **위치:** `.github/workflows/release.yml:3-8,30-67` (`workflow_dispatch`, macOS job), 특히 checkout `:39`, certificate import `:44-49`, branch-controlled package/notary scripts `:51-63`; repository environment configuration
- **위반되는 불변조건:** release signing/notarization authority는 reviewed/tagged immutable source와 protected environment에서만 접근 가능해야 한다.
- **사전 조건:** 공격자 또는 실수한 collaborator가 repository write 및 Actions workflow-dispatch 권한을 가진다. Default branch workflow는 존재한다.
- **정확한 실행 순서:** collaborator가 unmerged branch에서 `scripts/package_macos.sh` 또는 `scripts/notarize_macos.sh`를 바꾼다 → Actions UI/API에서 그 ref로 Release workflow를 dispatch한다 → checkout은 selected ref를 받는다 → job이 repository-level P12/password/Developer ID와 Apple ID/team/app-password를 제공한다 → branch script가 secret을 유출하거나 attacker payload를 signing identity로 서명한다.
- **기대 동작:** secret-bearing job은 version-matched protected tag, protected production environment, independent approval 후에만 실행되어야 하며 untrusted branch script를 실행하면 안 된다.
- **실제 동작:** workflow dispatch에 ref restriction이나 job `environment:`가 없다. Publish job만 tag 조건이므로 branch artifact가 자동 release되지는 않지만 signing authority 노출은 이미 발생한다.
- **코드 증거:** read-only `gh secret list`에서 `MACOS_CERTIFICATE_P12`, certificate/keychain passwords, Developer ID, Apple ID/team/app password 7개가 존재했다. `gh api .../environments`는 `total_count: 0`이었다. Secret 값은 조회하지 않았다.
- **동적 검증 결과:** metadata configuration은 동적으로 확인했다. 실제 secret exfiltration/임의 signing은 안전상 **Not dynamically verified**.
- **사용자 영향:** 공식 개발자 identity로 서명된 malicious binary가 배포·사회공학에 사용될 수 있고 certificate/Apple credentials 교체가 필요해질 수 있다.
- **보안·개인정보 영향:** release trust와 notarization authority compromise다. 자동 publish까지는 별도 tag/release 권한이 필요하므로 Critical 대신 High로 판정했다.
- **기존 테스트가 잡지 못한 이유:** CTest와 build는 Actions permission/environment/ref policy를 검사하지 않는다. Action pinning과 top-level `contents: read`는 이 secret exposure를 막지 않는다.
- **최소 수정 방향:** secret-bearing jobs를 protected `production` environment에 넣고 required reviewers/self-review 금지를 설정한다. `workflow_dispatch`는 unsigned build만 수행하거나 trusted immutable tag/commit과 CMake version을 검증한 후 별도 reusable workflow가 environment secret을 사용하게 한다.
- **필요한 regression test:** policy-as-code로 non-tag dispatch가 secret-bearing job을 건너뛰는지, unprotected ref가 environment approval을 받지 못하는지, checkout SHA가 approved tag SHA와 같은지 검사한다.
- **관련 finding:** `CF-001`은 Windows/Linux online release authority 자체가 update root인 문제이고, `CF-024`는 tag/version 결합 부재다.

### CF-008 — FFmpeg가 복구한 decode 오류로 영상이 짧아져도 정상 완료로 게시된다

- **분류:** Confirmed defect
- **심각도:** Medium
- **신뢰도:** High
- **영향받는 플랫폼:** 모든 video-processing platform
- **위치:** `src/VideoIo.cpp:421-550` (`probeVideo`), `src/VideoIo.cpp:702-779` (`VideoFrameReader::readFrame`), `src/VideoProcessor.cpp:497-590,735-769,869-887,901-925` (`processVideo`)
- **위반되는 불변조건:** decode 오류나 ffprobe-declared timeline과 실질적으로 다른 output이 정상 `Completed`/`Done`으로 게시되지 않아야 한다.
- **사전 조건:** container/stream metadata는 유효하지만 일부 H.264 packet이 손상되어 FFmpeg decoder가 error를 stderr에 쓰고 해당 frames를 버린 뒤 exit 0으로 복구한다.
- **정확한 실행 순서:** ffprobe는 120 frames/12s를 보고한다 → pass 1 FFmpeg는 damaged access units를 버리고 100 complete raw frames만 내보낸 뒤 exit 0 → reader는 partial frame도 nonzero exit도 아니므로 `error_`를 설정하지 않는다 → pass 2도 같은 snapshot/decoder로 100 frames를 내보낸다 → two-pass count equality가 통과한다 → writer가 100-frame/10s output을 게시하고 `Completed` → detector track이 하나 이상이면 worker는 redacted/`Done`.
- **기대 동작:** decode error 또는 declared timeline과 의미 있는 차이가 있으면 output을 게시하지 않거나 명시적인 Review Required로 끝나야 한다.
- **실제 동작:** FFmpeg stderr의 recoverable error와 ffprobe count/duration discrepancy는 final status에 반영되지 않는다.
- **코드 증거:** `readFrame`은 mid-frame, process nonzero exit, I/O timeout만 error로 설정한다. `processVideo`는 두 production passes 서로의 count만 비교하며 `estimatedFrameCount`/duration과 actual count를 completion gate로 사용하지 않는다.
- **동적 검증 결과:** 정상 120-frame/12s H.264를 `-c copy -bsf:v noise=amount=-1`로 손상했다. ffprobe/decode가 `Invalid NAL unit size`와 `Error splitting the input into NAL units`를 출력했지만 production result는 `Completed`, 100 frames, output 10s였다. Detection harness에서는 `trackCount=2`인 상태로도 `Completed`였다.
- **사용자 영향:** 영상의 2초가 조용히 사라지고 audio/event chronology가 변할 수 있지만 사용자는 정상 처리로 오인한다.
- **보안·개인정보 영향:** 직접 원본 pixel 노출은 재현하지 않았으나, 손상/악의적 media가 safety review 대상 frames를 제거하고 완료 상태를 얻을 수 있다. 주 영향은 반복 가능한 data loss와 상태 무결성이다.
- **기존 테스트가 잡지 못한 이유:** `tests/test_video_io.cpp:386-396`은 ffprobe 자체가 실패하는 “not a video”만 검사하고 recoverable packet corruption이나 declared-vs-decoded count를 검사하지 않는다.
- **최소 수정 방향:** privacy mode에서 FFmpeg decode에 strict error policy(`-xerror` 등)를 사용하거나 stderr/error counters를 구조적으로 감시한다. `nb_frames`가 신뢰 가능한 경우 actual count와, 그렇지 않으면 duration endpoint와 합리적인 tolerance를 비교해 discrepancy를 warning/failure로 승격한다.
- **필요한 regression test:** valid container 안의 recoverable corrupted packet, truncated tail, damaged B-frame/GOP fixtures가 clean completion으로 게시되지 않는지 검증한다.
- **관련 finding:** `CF-006`은 손상이 없는 input에서도 timestamp contract 때문에 timeline이 달라지는 별도 원인이다.

### CF-009 — Sample aspect ratio를 버려 영상 geometry와 display aspect ratio가 바뀐다

- **분류:** Confirmed defect
- **심각도:** Medium
- **신뢰도:** High
- **영향받는 플랫폼:** 모든 video-processing platform
- **위치:** `include/cloakframe/VideoIo.hpp:52-71` (`VideoInfo`), `src/VideoIo.cpp:453-524` (`probeVideo`), `src/VideoIo.cpp:406-418,603-652` (`displayWidth/Height`, reader), `src/VideoIo.cpp:815-908` (raw writer)
- **위반되는 불변조건:** source의 display geometry가 detection, review, redaction, encoded output에서 일관되게 보존되거나 명시적으로 square-pixel geometry로 bake되어야 한다.
- **사전 조건:** H.264/HEVC stream이 `sample_aspect_ratio != 1:1`인 anamorphic video다.
- **정확한 실행 순서:** ffprobe는 SAR/DAR을 반환하지만 parser가 버린다 → `displayWidth/Height`는 rotation만 적용한다 → decoder는 raw sample raster를 BGR로 보낸다 → detector/review는 squeezed raster를 본다 → writer의 rawvideo input은 square pixels로 해석되고 `setsar`/aspect metadata 없이 MP4로 encode된다.
- **기대 동작:** source SAR/DAR을 보존하거나 `scale`로 square-pixel display raster를 만든 뒤 그 exact geometry를 detector/review/output 모두 사용해야 한다.
- **실제 동작:** pixel dimensions만 유지되고 display aspect ratio가 바뀐다.
- **코드 증거:** `VideoInfo`에 SAR field가 없고 ffprobe loop가 `sample_aspect_ratio`/`display_aspect_ratio`를 읽지 않는다. Writer arguments에도 `setsar`/`-aspect`가 없다.
- **동적 검증 결과:** 720×576, SAR 16:15, DAR 4:3, 50-frame input을 production 처리했다. Output은 720×576/50 frames였지만 SAR와 DAR가 모두 N/A여서 player는 5:4 square-pixel display로 해석한다.
- **사용자 영향:** 사람과 번호판이 찌그러지고 review overlay/visual quality가 source display와 달라진다. Detection accuracy와 padding perception도 변할 수 있다.
- **보안·개인정보 영향:** 이 재현에서 accepted box의 원본 pixel 노출은 입증하지 않았으므로 Medium으로 제한했다. Geometry-sensitive small/edge detection의 residual privacy risk가 있다.
- **기존 테스트가 잡지 못한 이유:** rotation test는 있지만 non-square SAR fixture와 output DAR assertion이 없다.
- **최소 수정 방향:** SAR rational을 probe/model에 추가한다. 한 policy를 선택해 (a) source SAR을 output에 보존하거나 (b) analysis 전 square-pixel display dimensions로 scale하고 output SAR=1을 명시한다.
- **필요한 regression test:** common PAL/NTSC anamorphic ratios와 rotation+SAR 조합에서 preview/detection/output display dimensions 및 box alignment를 확인한다.
- **관련 finding:** `CF-006`과 같이 FFprobe가 제공하는 essential media metadata를 `VideoInfo`가 표현하지 못하는 문제다.

### CF-010 — 여러 audio stream 중 첫 번째만 남고 나머지는 조용히 삭제된다

- **분류:** Confirmed defect
- **심각도:** Medium
- **신뢰도:** High
- **영향받는 플랫폼:** 모든 video-processing platform
- **위치:** `include/cloakframe/VideoIo.hpp:52-71`, `src/VideoIo.cpp:455-464` (`probeVideo` first audio), `src/VideoIo.cpp:862-903` (`VideoFrameWriter::open`, `-map 1:a:0?`)
- **위반되는 불변조건:** 지원 입력의 유효한 audio streams가 정책 설명 없이 손실되지 않아야 하며, unsupported mapping은 작업 전 사용자에게 알려야 한다.
- **사전 조건:** input MP4/MOV에 commentary, alternate language, accessibility track 등 두 개 이상의 audio stream이 있다.
- **정확한 실행 순서:** probe loop가 첫 audio stream을 발견한 뒤 `hasAudio`가 true가 되어 이후 audio metadata를 무시한다 → writer가 source를 second input으로 열고 `-map 1:a:0?` 하나만 선택한다 → metadata/chapter를 제거하고 output을 정상 게시한다.
- **기대 동작:** 모든 compatible audio stream과 disposition/language를 보존하거나 명시적인 selection UI/policy와 Review Required warning을 제공해야 한다.
- **실제 동작:** first stream 외 모든 audio가 사라지고 success status에 data-loss indication이 없다.
- **코드 증거:** `VideoInfo`는 단일 `audioCodec`; mapping은 hard-coded first audio only다.
- **동적 검증 결과:** two AAC stream synthetic MP4를 production 처리했다. Input stream indices 1,2의 audio 두 개가 output에서는 index 1 하나로 줄었고 processing은 `Completed`였다.
- **사용자 영향:** alternate language, director commentary, descriptive audio 등이 복구 불가능하게 output에서 누락된다. 원본은 보존되지만 결과물을 원본 audio 보존으로 오인할 수 있다.
- **보안·개인정보 영향:** 추가 노출은 없고 data integrity/제품 설명 영향이 주다.
- **기존 테스트가 잡지 못한 이유:** `tests/test_video_io.cpp:224,275`는 단일 AAC stream만 assert한다.
- **최소 수정 방향:** probe에 audio stream vector를 추가하고 compatible streams를 모두 map한다. Container compatibility에 따라 stream별 copy/transcode하고 language/default/forced disposition을 보존하거나 명시적으로 경고한다.
- **필요한 regression test:** 0/1/2+ audio, mixed codec, language/disposition, commentary, incompatible stream 조합의 count/codec/tags/sync를 검사한다.
- **관련 finding:** `CF-006`은 보존된 first audio조차 잘못된 interval로 자르는 문제다.

### CF-011 — 실제 hardware encode 실패 시 software encoder로 재시도하지 않는다

- **분류:** Confirmed defect
- **심각도:** Medium
- **신뢰도:** High
- **영향받는 플랫폼:** Windows NVENC/QSV, Linux NVENC/QSV, macOS VideoToolbox
- **위치:** `src/VideoIo.cpp:230-325` (`encoderWorks`, `selectVideoEncoder`), `src/VideoIo.cpp:815-919` (`VideoFrameWriter::open`), `src/VideoIo.cpp:929-996,999-1033` (`writeFrame`, `finish`), `src/VideoProcessor.cpp:705-732,901-917`; `README.md:50`
- **위반되는 불변조건:** advertised hardware-to-software fallback이 실제 full encode failure에도 작동하거나 문서가 probe-only fallback임을 정확히 말해야 한다.
- **사전 조건:** 3-frame black test encode는 성공하지만 real frames/length/options/resource state에서 hardware encoder가 시작 후 실패한다. GPU reset, session limit, unsupported real content, driver error가 예다.
- **정확한 실행 순서:** `encoderWorks`가 3-frame lavfi probe를 성공한다 → encoder name이 static cache에 available로 저장된다 → real writer가 hardware encoder로 시작한다 → process가 frame write 또는 finalize 중 실패한다 → writer returns false and removes stage → `processVideo` returns Failed; source snapshot을 다시 decode해 software encoder로 재시도하는 path는 없다.
- **기대 동작:** advertised policy대로 hardware attempt가 실제 encode에서 실패하면 clean stage를 폐기하고 full second pass를 software x264/x265로 재실행해야 한다.
- **실제 동작:** preflight 실패에만 software 선택이 적용되고 real failure는 전체 item 실패다.
- **코드 증거:** selected `encoderName_`는 writer lifetime 동안 고정된다. `writeFrame`/`finish` 오류는 caller에 반환될 뿐 fallback loop가 없다.
- **동적 검증 결과:** FFmpeg wrapper가 `color=black` VideoToolbox probe는 libx264로 성공시키고 real VideoToolbox encode는 exit 42로 만들었다. Production trace는 `Hardware video encoder h264_videotoolbox available`, `Video encoder: h264_videotoolbox`, 이어 `status=2`, `Encoding failed: Error writing to process`; software retry/output은 없었다.
- **사용자 영향:** 장시간 detection/review 후 encode 단계에서 전체 작업이 실패해 CPU/GPU 시간과 사용자의 review 작업을 잃는다.
- **보안·개인정보 영향:** failed final output은 게시되지 않아 직접 노출은 확인하지 않았다. Availability와 문서 신뢰의 문제다.
- **기존 테스트가 잡지 못한 이유:** encoder probe 성공 후 real encode만 실패하는 stateful fake FFmpeg가 없다.
- **최소 수정 방향:** processVideo orchestration이 encoder attempt를 명시적으로 관리하고 failure 시 fresh hidden stage와 fresh second-pass reader로 software encode를 한 번 재시도한다. Retry 중 status/progress/codec/color policy를 명확히 유지한다.
- **필요한 regression test:** fake FFmpeg로 probe success + N번째 frame failure/finalize failure를 만들고 software output, cleanup, codec/pixel format/audio가 기대값인지 검사한다.
- **관련 finding:** README의 “falling back to CPU ... otherwise” 문구 불일치는 이 finding에 포함했으며 별도 count하지 않았다.

### CF-012 — Worker 시작 직전의 Stop/창 닫기 요청이 취소 flag reset으로 사라진다

- **분류:** Confirmed defect
- **심각도:** Medium
- **신뢰도:** High
- **영향받는 플랫폼:** 모든 GUI platform
- **위치:** `src/MainWindow.cpp:942-963` (`~MainWindow`), `src/MainWindow.cpp:1446-1496` (`startProcessing`, `stopProcessing`), `src/ProcessorWorker.cpp:366-369` (`process`), `src/ProcessorWorker.cpp:1359-1366` (`cancel`)
- **위반되는 불변조건:** 사용자가 보낸 cancel은 같은 run이 시작되기 전/중 어느 시점에도 잃어서는 안 되고, 닫기 후 output write가 새로 시작되어서는 안 된다.
- **사전 조건:** GUI가 Stop을 활성화한 뒤 `QThread::start()`는 호출됐지만 worker의 queued `process()`가 아직 scheduling되지 않은 짧은 window에 Stop 또는 창 닫기가 발생한다.
- **정확한 실행 순서:** new worker의 flag는 false → GUI setProcessing/Stop enabled → thread start → GUI가 `worker_->cancel()`하여 true → worker thread가 `process()` entry → line 368이 false로 덮어씀 → scan/decode/output이 정상 실행 → close destructor는 thread completion을 기다린다.
- **기대 동작:** pre-start cancel을 process entry가 관측하고 즉시 `Cancelled`로 끝내며 output을 만들지 않아야 한다.
- **실제 동작:** cancel을 잃고 작업이 진행된다. 창 닫기는 해당 작업을 기다리며 보이지 않는 processing/output publication이 계속될 수 있다.
- **코드 증거:** worker는 run마다 새로 생성되므로 entry reset은 초기화 목적에도 필요하지 않다. `cancel()`의 release store 바로 뒤에 unconditional relaxed false store가 온다.
- **동적 검증 결과:** production `ProcessorWorker` harness에서 `worker.cancel(); worker.process();`를 호출했다. 결과 `outcome=1` (`CompletedWithWarnings`)이고 `output_exists=1`이었다.
- **사용자 영향:** Stop이 무시되고 큰 batch/영상이 계속 돌아 UI 종료가 지연되며 원하지 않은 output이 생긴다.
- **보안·개인정보 영향:** 사용자가 취소로 막으려던 local data processing/publication이 계속된다. 기존 원본 덮어쓰기는 아니지만 user intent와 lifecycle safety를 위반한다.
- **기존 테스트가 잡지 못한 이유:** `tests/test_parallel.cpp:69` 계열은 20개 item 처리 후 mid-run cancel만 검사하며 pre-entry ordering을 만들지 않는다.
- **최소 수정 방향:** `process()`의 reset을 제거한다. 재사용 가능한 worker가 필요하다면 generation-tagged state machine과 start/cancel handshake를 사용하고 GUI는 thread started acknowledgement 전에도 cancel generation을 보존해야 한다.
- **필요한 regression test:** cancel-before-process, start/stop race 반복, close immediately after Start, cancel then new run generation isolation을 검사하며 첫 run output이 없어야 한다.
- **관련 finding:** 다른 cancel loops의 atomic visibility와 normal cleanup은 확인되어 별도 race finding으로 만들지 않았다. `CF-015`는 publication fallback 중 cancel responsiveness 문제다.

### CF-013 — 부분적인 `std::thread` 생성 실패가 예외 처리 대신 process를 terminate한다

- **분류:** Confirmed defect
- **심각도:** Medium
- **신뢰도:** High
- **영향받는 플랫폼:** 모든 platform, 특히 thread/resource limit 또는 대규모 load 환경
- **위치:** `include/cloakframe/OrderedParallel.hpp:72-77,103-106` (`processOrdered`), caller `src/ProcessorWorker.cpp:551-563`; `src/VideoProcessor.cpp:191-217` (`MaskWorkerPool`), construction caller `src/VideoProcessor.cpp:705-732`
- **위반되는 불변조건:** resource exhaustion은 item/run failure로 정리되어야 하며 abrupt `std::terminate`로 process와 temporary data를 남겨서는 안 된다.
- **사전 조건:** 여러 worker 중 첫 thread들은 생성됐지만 N번째 `std::thread` constructor가 `std::system_error`를 throw한다.
- **정확한 실행 순서:** vector에 joinable threads가 이미 존재한다 → 다음 `emplace_back`가 throw → `processOrdered` stack unwinding이 vector를 destroy한다 → join/detach되지 않은 `std::thread` destructor가 `std::terminate` 호출. `MaskWorkerPool`에서는 object construction이 완료되지 않아 class destructor/join loop 자체가 실행되지 않고 member vector destruction에서 같은 terminate가 발생한다.
- **기대 동작:** 이미 시작한 threads를 stop/join하고 exception을 worker의 outer catch로 전달해 Failed/cleanup을 수행해야 한다.
- **실제 동작:** C++ standard의 joinable-thread destructor semantics상 catch에 도달하지 못하고 application이 즉시 종료된다.
- **코드 증거:** 두 loops 모두 construction 후에만 join logic이 존재하며 constructor-failure guard가 없다. Video path는 writer/source stage를 연 뒤 pool을 만든다.
- **동적 검증 결과:** **Not dynamically verified.** OS thread limit을 고의 소진하거나 constructor fault injection을 하지 않았다. Standard-mandated control flow로 확정했다.
- **사용자 영향:** resource pressure에서 repeatable crash, batch 중단, crash-residual snapshots/stages가 발생한다.
- **보안·개인정보 영향:** 직접 노출은 없지만 `CF-020`의 원본 snapshot residue와 `CF-015` fallback partial final을 악화시킬 수 있다.
- **기존 테스트가 잡지 못한 이유:** `tests/test_parallel.cpp`는 normal creation/cancel/order만 검사하고 thread constructor injection seam이 없다.
- **최소 수정 방향:** `std::jthread`와 stop token을 사용하거나 construction loop를 try/catch로 감싸 이미 생성된 workers에 stop을 알리고 join한 후 rethrow한다. `MaskWorkerPool`도 two-phase RAII factory로 구성한다.
- **필요한 regression test:** injectable thread factory가 2번째/N번째 creation에 `system_error`를 던지게 하고 process가 abort하지 않으며 Started threads가 join되고 item이 Failed 처리되는지 검증한다.
- **관련 finding:** `CF-020`은 crash가 남길 수 있는 private source copy를 다룬다.

## 11. Detailed Probable Defects

### CF-014 — 단일 대형 image가 global memory budget보다 커도 admission된다

- **분류:** Probable defect
- **심각도:** Medium
- **신뢰도:** High
- **영향받는 플랫폼:** 모든 image-processing platform
- **위치:** `src/ProcessorWorker.cpp:45-50,76-80,151-215,791-856,990-1003` (`imageMemoryBudget`, `imageParallelism`, `processItem`), `src/MemoryBudget.cpp:42-68`, `src/ImageIo.cpp:1463-1476,1549-1604` (`imreadUnicode`, encode path), `src/Mosaic.cpp:637-668`
- **위반되는 불변조건:** memory budget은 parallelism만 제한하는 hint가 아니라 single-item peak allocation도 enforce해야 하며, budget보다 큰 작업을 decode 전에 거부해야 한다.
- **사전 조건:** header 검사상 512M pixels 이하이지만 decoded bit depth/channels와 processing copies를 합친 estimate가 physical-memory-derived budget을 넘는 valid compressed image다.
- **정확한 실행 순서:** dimension/file-size inspection이 512M-pixel/2GiB cap을 통과한다 → `pixels*16+fileSize` estimate가 budget을 넘는다 → reservation amount가 `min(imageMemoryBudget(), estimate)`로 budget 값에 잘린다 → available budget 전부를 예약한 것으로 처리하고 admission된다 → encoded file read, full `imdecode`, orientation, detection BGR, ROI clone/mask, output encoding이 실제 budget 이상 allocation을 시도한다.
- **기대 동작:** estimate가 hard budget보다 크면 decode 전에 reject/Review Required해야 하며 estimate는 bit depth/channel/effect/encoder peak를 보수적으로 포함해야 한다.
- **실제 동작:** reservation accounting은 budget을 초과한 required amount를 budget과 동일한 것으로 표현해 single oversized item을 허용한다. `imageParallelism`이 estimate가 큰 작업의 동시성을 1로 낮추는 방어는 있지만 단일 item의 peak allocation은 제한하지 않는다.
- **코드 증거:** `std::min(imageMemoryBudget(), pixels*16+fileSize)`가 명시적인 under-reservation이다. 512M-pixel 16-bit RGBA는 decoded image 하나만 약 4GiB이고 detection/clone/mask/encoded buffers가 추가된다. 8GiB machine의 quarter-RAM budget은 약 2GiB지만 이 image가 admission된다. Lines 212–215의 serial fallback은 concurrent peak만 줄인다.
- **동적 검증 결과:** **Not dynamically verified.** 실제 multi-GiB valid compressed image를 decode해 host OOM을 유발하는 검증은 안전상 수행하지 않았다. Arithmetic/control flow는 정적으로 확인했다.
- **사용자 영향:** large scan/batch에서 OS memory pressure, process kill, UI hang, 다른 applications 영향, unfinished temp data가 발생할 수 있다.
- **보안·개인정보 영향:** untrusted local image로 availability를 소진하는 decompression-bomb 성격이며 crash residue를 악화시킨다. Remote upload surface는 없다.
- **기존 테스트가 잡지 못한 이유:** `tests/test_core.cpp:359-382`는 64×64 JPEG file에 padding을 붙여 31MiB로 만든 것뿐이라 decoded-memory pressure를 만들지 않는다.
- **최소 수정 방향:** `requiredEstimate > hardBudget`이면 명시적으로 거부한다. Header bit depth/channel을 가능한 범위에서 반영하고 decode image, oriented copy, detection BGR, largest ROI/mask, encoded buffer의 simultaneous peak를 계산한다. Decoder-level allocation limit도 사용한다.
- **필요한 regression test:** injectable small budget에서 8/16-bit gray/RGB/RGBA와 extreme compression ratio inputs가 decode 전에 reject되는지, near-budget item 하나와 여러 parallel items의 reservation 합이 cap을 넘지 않는지 검사한다.
- **관련 finding:** `CF-013`은 resource pressure가 thread creation failure로 진행됐을 때 abrupt terminate하는 별도 경로다.

### CF-015 — POSIX no-replace fallback이 final filename에 직접 streaming한다

- **분류:** Probable defect
- **심각도:** Medium
- **신뢰도:** High
- **영향받는 플랫폼:** macOS/Linux의 exFAT, FAT, 일부 network/FUSE 또는 exclusive rename/hardlink를 지원하지 않는 destination
- **위치:** `src/ImageIo.cpp:1102-1203` (`publishPosixNoReplace`), `src/ImageIo.cpp:1206-1287` (`writeTemporaryAndPublishAtRoot`), video caller `src/VideoIo.cpp:999-1099`, `src/VideoProcessor.cpp:901-926`; `release-notes/v1.10.0.md:7-9,34-35`
- **위반되는 불변조건:** incomplete/cancelled output은 사용자가 complete로 오인할 final name으로 보여서는 안 되고 publication은 no-replace와 atomic visibility를 동시에 만족해야 한다.
- **사전 조건:** destination filesystem에서 macOS `RENAME_EXCL` 또는 Linux `renameat2(RENAME_NOREPLACE)`가 `ENOTSUP/EINVAL/ENOSYS`이고 hardlink도 `ENOTSUP/EOPNOTSUPP/EPERM/ENOSYS`다.
- **정확한 실행 순서:** private hidden stage의 complete redacted/encoded payload 생성 → publish guard가 한 번 true → exclusive rename 실패 → hardlink 실패 → fallback이 destination final name을 `O_EXCL|O_NOFOLLOW`로 생성 → 64KiB loop로 final file에 직접 copy한다. 이 도중 SIGKILL/crash/power loss가 발생하면 cleanup이 실행되지 않아 truncated final-name prefix가 남는다. Ordinary cancel은 loop에서 poll되지 않아 copy/publication이 끝난 뒤 worker가 cancel을 관측한다.
- **기대 동작:** final name은 complete bytes가 durable해진 뒤 single atomic commit으로만 나타나야 한다. 해당 filesystem에 primitive가 없으면 기능을 명시적으로 거부하거나 crash-recoverable transaction protocol을 사용해야 한다.
- **실제 동작:** crash/power loss에는 final-looking truncated file이 관찰·잔존할 수 있다. Copy 중 ordinary cancel은 즉시 중단되지 않아 완전한 redacted payload가 final name으로 게시될 수 있지만, outer `ProcessorWorker`는 이후 `RunOutcome::Cancelled`와 UI `Cancelled`를 표시한다. 따라서 이 cancel branch는 `Done` 오표시가 아니라 “Cancelled인데 output이 남음”이다.
- **코드 증거:** fallback target open은 line 1151, copy loop는 1164–1197, guard는 caller 1276에서 publish 시작 전에만 호출된다. Handled error의 `unlinkat`는 crash safety를 제공하지 않는다.
- **동적 검증 결과:** **Not dynamically verified.** audit host의 APFS는 exclusive rename을 지원한다. Unsupported syscall shim/mount와 mid-copy kill/power-loss를 실행하지 않았다. Code path와 documented target filesystems로 probable 판정했다.
- **사용자 영향:** destination에 truncated/corrupt image/video가 정상 filename으로 남거나 Cancelled 표시와 함께 complete-looking output이 생길 수 있다. 기존 output은 O_EXCL로 보호된다.
- **보안·개인정보 영향:** fallback source는 이미 redaction/encoding을 마친 hidden-stage payload이므로 “일부 frame만 redacted된 원본”을 final에 copy하는 경로는 아니다. 핵심 영향은 final-name completeness와 cancel/status 무결성이다. Source snapshot 자체를 copy하지도 않는다.
- **기존 테스트가 잡지 못한 이유:** `tests/test_core.cpp:822-953`은 normal local filesystem과 publish 전 false guard만 검사한다. Rename/link unsupported와 final-open 뒤 termination을 만들지 않는다.
- **최소 수정 방향:** atomic no-replace commit을 보장할 수 없는 filesystem에서는 fail closed하는 것이 가장 단순하다. 지원을 유지하려면 manifest/commit marker와 startup recovery, final name과 구별되는 partial name, copy-loop guard polling을 결합하고 API/documentation에서 non-atomic 상태를 숨기지 않는다.
- **필요한 regression test:** syscall shim으로 rename/link를 `ENOTSUP` 처리한 뒤 target-open 직후 kill, mid-copy guard false, ENOSPC, destination race를 주입해 partial final visibility와 cleanup/outcome을 확인한다.
- **관련 finding:** release note의 “every platform atomic” 주장은 이 finding 안에서 증거로 처리했으며 별도 documentation count로 중복하지 않았다. `CF-020`은 다른 crash-residual data다.

## 12. Hardening Opportunities

### CF-016 — Custom ONNX consent가 승인한 file digest에 결합되지 않고 native runtime과 같은 process에서 실행된다

- **분류:** Hardening opportunity
- **심각도:** Medium
- **신뢰도:** High
- **영향받는 플랫폼:** 모든 platform
- **위치:** `src/MainWindow.cpp:1057-1075` (`chooseModel`), `:1153-1202` (`makeDetectorCacheKey`), `:1205-1425` (`startProcessing`), `:1631-1672` (`loadSettings`, 특히 custom path `:1665-1672`), `:1776-1803` (`saveSettings`); `src/ModelDownloader.cpp:185-223` (`customModelFileIsAllowed`, `confirmTrustedCustomModel`); `src/ScrfdFaceDetector.cpp:35-61,124-151` (bytes/hash와 ONNX Runtime session construction)
- **위반되는 불변조건:** 사용자가 승인한 native-runtime input의 identity가 이후 run까지 보존되고, untrusted model parser failure가 main GUI/media process와 격리되는 것이 바람직하다.
- **사전 조건:** 사용자가 한 번 trusted path를 승인한 뒤 같은 path의 bytes가 sync, external editor, removable/network share 또는 다른 process로 교체된다.
- **정확한 실행 순서:** 최초 Browse에서 extension/size check와 trust prompt → path를 settings에 저장 → 다음 launch에서 path를 무경고 복원 → run 시작 시 현재 bytes를 hash → 그 현재 digest를 detector의 expected hash로 넘김 → detector가 같은 current bytes와 digest가 일치함을 확인 → native ONNX Runtime가 same process에서 model을 parse/optimize/execute한다.
- **기대 동작:** consent는 path가 아니라 content digest에 bind되고 content change 시 재승인이 필요하다. 위험한 native parser는 최소 권한 helper process에서 resource/sandbox 제한을 받는 편이 안전하다.
- **실제 동작:** TOCTOU는 current digest 재검사로 방어되지만, “사용자가 승인한 bytes”가 아니라 “현재 bytes가 자기 자신과 일치”하는지만 증명한다. Malformed model crash/취약점은 GUI/media process 전체 권한을 가진다.
- **코드 증거:** settings에는 path만 저장되며 approved digest/version record가 없다. UI/README는 “trusted source only” 경고를 제공하므로 현재 직접적인 보장 위반보다 hardening으로 분류했다.
- **동적 검증 결과:** valid custom model warning/hash paths를 정적으로 확인했다. Malicious ONNX를 실행하지 않았으므로 native-runtime exploit은 **Not dynamically verified**.
- **사용자 영향:** 승인 후 바뀐 model을 알아차리지 못하고 다시 로드할 수 있으며 parser crash는 진행 중 작업을 종료한다.
- **보안·개인정보 영향:** same-process native parser compromise 시 local media 접근이 가능하다. 실제 ORT vulnerability나 exploit은 주장하지 않는다.
- **기존 테스트가 잡지 못한 이유:** tests는 model compatibility/hash mismatch를 보지만 persisted consent와 content replacement/re-prompt, process isolation을 검사하지 않는다.
- **최소 수정 방향:** approved SHA-256과 size를 settings에 저장하고 변경 시 default No 재승인 prompt를 띄운다. Long-term으로 restricted helper process, IPC schema, memory/time limits, crash isolation을 적용한다.
- **필요한 regression test:** 승인 후 same-path byte replacement, symlink target change, helper crash/timeout/OOM이 main process와 output을 안전하게 유지하는지 검사한다.
- **관련 finding:** built-in model의 fixed digest/re-read 방어는 §15 non-issue다. `CF-019`는 built-in cache publication hardening이다.

### CF-017 — Linux non-x86 packaging이 mutable unsigned `continuous` tool을 실행한다

- **분류:** Hardening opportunity
- **심각도:** High
- **신뢰도:** High
- **영향받는 플랫폼:** Linux architecture가 `x86_64`가 아닌 local/future release packaging, 특히 arm64
- **위치:** `scripts/package_linux.sh:42-82` (`verify_sha256`, `fetch`), `:127-136` (downloaded linuxdeploy execution), `cmake/CloakFrameVelopack.cmake:31-39`, `README.md:24,100`
- **위반되는 불변조건:** build/release 중 내려받아 실행하는 third-party tool은 immutable version과 cryptographic digest로 고정되어야 하며 unsupported architecture는 fail closed해야 한다.
- **사전 조건:** developer 또는 미래 CI가 `ARCH=aarch64` 등 non-x86에서 `scripts/package_linux.sh`를 실행한다. 현재 공식 README/CI artifact는 x86_64다.
- **정확한 실행 순서:** architecture-specific expected digest가 빈 문자열로 남는다 → URL은 mutable `releases/download/continuous/...`를 가리킨다 → `verify_sha256`가 “dev build”라며 success → downloaded AppImage에 executable bit를 주고 line 130에서 실행한다 → malicious/upstream-compromised tool은 build user/CI token 권한을 얻는다.
- **기대 동작:** unsupported architecture는 명시적으로 중단하거나 exact immutable asset와 SHA-256/signature를 요구해야 한다.
- **실제 동작:** CMake/Velopack은 arm64 branch를 지원하는 모양이지만 packaging trust policy는 non-x86에서 의도적으로 해제된다.
- **코드 증거:** `expected` empty branch는 verification skip; URL tag가 `continuous`; 결과 binary를 즉시 실행한다.
- **동적 검증 결과:** **Not dynamically verified.** audit host는 macOS arm64이고 Linux non-x86 packaging을 실행하지 않았다. Branch는 정적으로 완결되어 있다.
- **사용자 영향:** 비공식/미래 arm64 artifact와 CI environment가 build-time supply-chain compromise를 당할 수 있다.
- **보안·개인정보 영향:** release artifact 변조, token/secret theft, malicious AppImage 생성 가능성이 있다. 현재 공식 x86_64 path는 pinned digest를 사용하므로 Confirmed current-release defect 대신 hardening으로 판정했다.
- **기존 테스트가 잡지 못한 이유:** CI matrix는 x86_64 Linux 하나뿐이고 script의 trust-policy test가 없다.
- **최소 수정 방향:** `ARCH != x86_64`이면 fail하거나 immutable release/tag assets와 pinned digest를 architecture별로 제공한다. Empty digest를 허용하는 function API를 제거한다.
- **필요한 regression test:** 모든 supported architecture가 nonempty digest와 immutable URL을 갖는지 lint하고 empty/mismatch이면 package job이 실행 전에 실패하는지 검사한다.
- **관련 finding:** `CF-001`은 runtime update, 이 finding은 build-time tool execution이다.

### CF-018 — Sparkle appcast가 EdDSA trust anchor 없이 Apple code signing 하나에만 의존한다

- **분류:** Hardening opportunity
- **심각도:** Medium
- **신뢰도:** High
- **영향받는 플랫폼:** macOS
- **위치:** `assets/MacOSXBundleInfo.plist.in:33-36`, `.github/workflows/release.yml:65-88`, `src/SelfUpdaterSparkle.mm:9-30` (`SparkleSelfUpdater`), `cmake/CloakFrameSparkle.cmake:3-15` (`cloakframe_enable_sparkle`)
- **위반되는 불변조건:** update authenticity는 가능한 경우 package signing identity와 별도의 app-pinned update key로 이중 보호하는 것이 바람직하다.
- **사전 조건:** Apple Developer ID signing identity/CI credential 또는 Apple code-sign verification trust가 compromise되지만 별도 offline Sparkle key는 compromise되지 않았을 상황.
- **정확한 실행 순서:** app은 GitHub `appcast.xml`을 읽는다 → appcast에 Ed signature가 없어도 workflow가 생성/게시한다 → `SUPublicEDKey`가 app plist에 없어 Sparkle EdDSA 검증을 요구할 수 없다 → Apple code signing만 update authenticity를 결정한다.
- **기대 동작:** app에 Sparkle public key를 고정하고 release job은 corresponding offline private key로 모든 update를 서명하며 signature 누락 시 실패해야 한다.
- **실제 동작:** private key가 없으면 workflow가 warning만 출력하고 appcast를 성공 생성한다. Repository secret list에도 `SPARKLE_ED_PRIVATE_KEY`가 없었다.
- **코드 증거:** plist에는 `SUFeedURL`만 있고 `SUPublicEDKey`가 없다. Workflow lines 83–87이 missing-key fallback을 명시한다.
- **동적 검증 결과:** local linked Sparkle 2.9.4와 plist/workflow/secret metadata를 확인했다. 실제 signed DMG/appcast update는 **Not dynamically verified**.
- **사용자 영향:** Apple signing key 하나의 compromise가 update trust 전체를 무너뜨린다.
- **보안·개인정보 영향:** defense-in-depth 부족이다. macOS package code signing/notarization/stapling은 fail closed로 확인되어 현재 unsigned arbitrary update defect로 판정하지 않았다.
- **기존 테스트가 잡지 못한 이유:** appcast signature-required policy test가 없고 missing key가 workflow success path다.
- **최소 수정 방향:** Sparkle keypair를 offline 생성하고 public key를 plist에 넣으며 private key를 protected environment/HSM에 둔다. Missing/invalid signature는 release와 client 양쪽에서 fail해야 한다.
- **필요한 regression test:** signed, unsigned, wrong-key, modified DMG/appcast, expired/replayed feed를 Sparkle integration test로 검증한다.
- **관련 finding:** `CF-001`은 Windows/Linux에 독립 trust anchor 자체가 없어 Critical로 판정했다. `CF-007`은 Apple signing secrets의 workflow exposure다.

### CF-019 — Built-in model의 고정 `.part` path가 symlink를 따라간다

- **분류:** Hardening opportunity
- **심각도:** Low
- **신뢰도:** High
- **영향받는 플랫폼:** 모든 platform; filesystem link semantics에 따라 세부 동작 차이
- **위치:** `src/ModelDownloader.cpp:93-111` (`downloadModelWithProgress`), cache root `src/ModelCatalog.cpp:12-27`
- **위반되는 불변조건:** downloaded model temp/cache publication은 no-follow, exclusive, private, unpredictable이어야 하고 검증된 bytes가 cache 밖 target에 쓰여서는 안 된다.
- **사전 조건:** attacker가 victim의 model cache tree에 `destPath + ".part"` symlink/link를 만들 수 있다. 정상 private home permission에서는 대개 same-user capability이므로 독립 privilege boundary는 약하다.
- **정확한 실행 순서:** network bytes의 fixed SHA-256 검증 성공 → cache directory 생성 → predictable `.part`를 `QFile::WriteOnly`로 열기 → QFile가 link target을 따라가 truncate/write → temp path를 destination으로 rename하기 전 이미 target content가 model bytes로 바뀜.
- **기대 동작:** private random temp를 exclusive/no-follow로 열고 descriptor identity를 유지한 채 fsync + no-replace/atomic replace해야 한다. Parent ownership/mode도 검증해야 한다.
- **실제 동작:** normal QFile open과 fixed name을 사용한다. Destination valid model을 먼저 remove한 뒤 rename하므로 failure resilience도 낮다.
- **코드 증거:** `.part` name, `file.open(QIODevice::WriteOnly)`, `QFile::remove(destPath)`, `QFile::rename` 순서에 link check가 없다.
- **동적 검증 결과:** Qt QFile harness에서 victim `original`과 `model.onnx.part` link를 만든 뒤 같은 open/write를 수행했다. `opened=1 written=14 victim=verified-model`이었다.
- **사용자 영향:** cache permission이 잘못됐거나 shared profile인 환경에서 attacker가 victim-writable file을 verified model bytes로 overwrite하거나 good cached model을 지워 반복 download를 유발할 수 있다.
- **보안·개인정보 영향:** bytes는 attacker-controlled arbitrary content가 아니라 fixed verified model이므로 RCE로 과장하지 않았다. Same-user attacker는 보통 이미 target을 쓸 수 있어 Low hardening이다.
- **기존 테스트가 잡지 못한 이유:** model downloader filesystem adversarial tests와 link-specific temp test가 없다.
- **최소 수정 방향:** rooted private cache directory를 owner/mode 검증하고 random exclusive temp를 `O_NOFOLLOW`/Windows reparse-safe handle로 연다. Good destination은 atomic replacement 가능 시점까지 보존하고 final model을 다시 hash한다.
- **필요한 regression test:** stale file, symlink/hardlink/reparse point, read-only cache, rename failure, concurrent downloader, crash before/after fsync를 검사한다.
- **관련 finding:** model content authenticity 자체는 §15에서 non-issue로 확인했다. Linux updater cache `CF-002`는 권한 경계와 executable package 때문에 훨씬 심각하다.

### CF-020 — 비정상 종료 후 원본 image/video snapshot이 잔존할 수 있다

- **분류:** Hardening opportunity
- **심각도:** Medium
- **신뢰도:** High
- **영향받는 플랫폼:** 모든 image/video-processing platform
- **위치:** image path `src/ProcessorWorker.cpp:704-755` (`processItem` source snapshot), video path `src/VideoProcessor.cpp:280-302` (`RetryingStagingDir`), `src/VideoProcessor.cpp:365-443` (`processVideo` source snapshot)
- **위반되는 불변조건:** temporary files containing full original media는 normal 및 crash recovery에서 private하고 bounded lifetime을 가져야 한다.
- **사전 조건:** full source snapshot 생성 후 destructor cleanup 전에 SIGKILL, power loss, process abort(`CF-013`) 또는 unrecoverable crash가 발생한다.
- **정확한 실행 순서:** image는 default system-temp `QTemporaryDir`에 `source.<ext>`를 copy하고, video는 output root의 `.cloakframe-snapshot-XXXXXX`에 original 전체를 copy한다 → decode/detect/review/encode 수행 → abrupt termination으로 RAII destructor가 실행되지 않음 → 다음 startup은 어느 root에서도 stale snapshot을 scan/delete하지 않음. POSIX의 `QTemporaryDir` mode는 owner-only지만 Windows는 parent DACL을 상속하며 video dot-prefix가 hidden attribute를 뜻하지 않는다.
- **기대 동작:** crash 후 다음 trusted startup이 owner/ACL/mode/name/age가 검증된 stale stages를 안전하게 정리하고 사용자에게 recovery status를 알리는 것이 바람직하다.
- **실제 동작:** normal return/cancel/error에서는 retrying destructor가 정리하지만 crash residue를 회수하는 경로가 없다.
- **코드 증거:** 두 paths 모두 full source copy가 완료된 뒤 그 snapshot을 processing source로 사용한다. Image는 local `QTemporaryDir`, video는 `RetryingStagingDir` object lifetime에만 cleanup을 의존한다.
- **동적 검증 결과:** normal success/failure/cancel에서 stage cleanup을 확인했다. SIGKILL로 residue를 의도적으로 남기는 test는 **Not dynamically verified**; destructor가 실행되지 않는 crash semantics는 명확하다.
- **사용자 영향:** original copy가 system temp 또는 destination storage, backup, sync, quota에 남고 disk space를 소비할 수 있다. Video dot-prefix는 특히 Windows에서 hidden attribute가 아니다.
- **보안·개인정보 영향:** temp/output folder를 다른 계정/서비스와 공유한 경우 원본 media가 기대보다 오래 존재한다. POSIX owner-only mode와 normal RAII cleanup 방어 때문에 defect가 아닌 hardening으로 분류했다. Windows ACL/privacy behavior는 **Not dynamically verified**이며 각 parent permissions에 의존한다.
- **기존 테스트가 잡지 못한 이유:** in-process RAII tests는 SIGKILL/power loss와 next-start scavenging을 실행할 수 없다.
- **최소 수정 방향:** app-specific temp/output manifest를 사용하고 startup에서 exact naming schema, owner/ACL, mode, no-symlink, age를 검증한 stale stage만 descriptor-rooted 방식으로 삭제한다. Active PID/start time을 기록하되 PID reuse를 고려한다.
- **필요한 regression test:** image와 video child process를 snapshot 후 강제 종료하고 next launch가 only-owned stale stages를 지우며 attacker symlink/foreign directory는 건드리지 않는지 검사한다.
- **관련 finding:** `CF-013`의 abrupt terminate가 이 residue를 만들 수 있다. `CF-015`는 final-name partial residue다.

## 13. Documentation and Build Inconsistencies

### CF-021 — 출력 충돌 preflight가 실제 destination filesystem identity와 다르다

- **분류:** Documentation/build inconsistency
- **심각도:** Low
- **신뢰도:** High
- **영향받는 플랫폼:** case-insensitive Linux volume, case-sensitive/insensitive APFS volume, Windows의 non-ASCII case-fold alias와 macOS의 Unicode-equivalent path
- **위치:** `src/OutputPlan.cpp:15-25,39-73` (`destinationKey`, `findOutputConflicts`), caller `src/ProcessorWorker.cpp:434-494`, final sink `src/ImageIo.cpp:1206-1287` (`writeTemporaryAndPublishAtRoot`), `tests/test_core.cpp:199-226`, `README.md:42`
- **위반되는 불변조건:** 서로 다른 input은 같은 실제 output object로 map되지 않아야 하며, collision 판단은 symlink, case folding, Unicode normalization과 destination volume semantics로 우회되지 않아야 한다.
- **사전 조건:** 두 output 철자가 ASCII case 또는 Unicode normalization만 다르고 destination volume의 identity rule이 compile-time OS 가정과 다른 경우다. 예: Linux의 case-insensitive mount 또는 macOS의 case-sensitive APFS.
- **정확한 실행 순서:** scan item의 relative path로 destination 생성 → `lexically_normal()` 적용 → Windows/macOS에서는 locale-dependent byte-wise `tolower`, Linux에서는 변환 없음 → 이 string key들만 preflight 비교 → 실제 volume에서 같은 이름인데도 둘을 허용하거나, 실제로 다른 이름인데 duplicate로 거부한다 → 허용된 collision은 batch 중 첫 publish 뒤 두 번째 no-replace publish에서야 실패한다.
- **기대 동작:** 시작 전에 destination parent와 volume의 실제 name/identity semantics를 기준으로 collision을 판정하고, 두 input이 같은 filesystem object가 될 가능성이 있으면 전체 계획을 fail closed해야 한다.
- **실제 동작:** OS compile-time heuristic이 filesystem identity를 대신한다. README의 “same output path로 map되면 시작하지 않는다”는 보장은 일부 volume에서 성립하지 않는다.
- **코드 증거:** `destinationKey`는 canonicalization, volume case-sensitivity, Unicode normalization 또는 file ID를 사용하지 않는다. Sink의 rooted `O_EXCL`/no-replace publication은 기존 file overwrite를 막지만 preflight의 all-or-nothing 보장은 복구하지 않는다.
- **동적 검증 결과:** 기본 APFS에서 ordinary duplicate/existing-output tests는 통과했다. case-sensitive APFS 별도 volume, case-insensitive Linux mount, NTFS Unicode/reparse 조합은 **Not dynamically verified**.
- **사용자 영향:** batch 일부가 이미 저장된 뒤 나머지가 충돌 실패하거나, case-sensitive APFS에서 합법적인 두 output이 불필요하게 거부될 수 있다.
- **보안·개인정보 영향:** 기존 output/원본 overwrite는 final no-replace 방어로 막힌다. 다만 preflight를 신뢰한 automation의 부분 성공과 상태 혼동을 유발한다.
- **기존 테스트가 잡지 못한 이유:** test는 동일 ASCII relative path만 사용하고 destination volume 특성, Unicode NFC/NFD, case variant를 parameterize하지 않는다.
- **최소 수정 방향:** 실제 destination directory handle을 기준으로 volume semantics를 확인하고 platform-native comparison/name lookup을 사용한다. 완전한 사전 판정이 불가능하면 reservation을 descriptor-rooted 방식으로 먼저 수행하고 batch atomicity limitation을 명시한다.
- **필요한 regression test:** case-sensitive/insensitive volumes, NFC/NFD, non-ASCII case fold, symlink/junction alias, hardlink와 concurrent output creation을 OS matrix에서 검사한다.
- **관련 finding:** final publication 자체의 unsupported-filesystem atomicity는 `CF-015`; ordinary local filesystem의 overwrite 방어는 §15 non-issue다.

### CF-022 — RGBA의 “opaque black” fill이 transparent black을 만든다

- **분류:** Documentation/build inconsistency
- **심각도:** Low
- **신뢰도:** High
- **영향받는 플랫폼:** 모든 platform의 alpha-capable image output
- **위치:** `src/Mosaic.cpp:99-102` (`fillRegion`), `:587-603` (`applyEffect`), `:606-689` (`applyAnonymization`), caller/save path `src/ProcessorWorker.cpp:990-1003` (`processItem`), `src/MainWindow.cpp:657-669`, `translations/cloakframe_ko.ts:410-415`
- **위반되는 불변조건:** UI에 표시되는 anonymization method와 실제 pixel/alpha 결과가 일치해야 하며 “opaque”라고 설명한 output은 불투명해야 한다.
- **사전 조건:** decoded/output image가 4-channel이고 사용자가 Solid fill을 선택한다.
- **정확한 실행 순서:** RGBA/BGRA ROI 선택 → `fillRegion`이 3-component `cv::Scalar(0,0,0)`으로 `setTo` → OpenCV가 누락된 4번째 component를 0으로 채움 → RGB=0, alpha=0인 transparent black 저장 → alpha-aware viewer에서 배경이 비쳐 보인다.
- **기대 동작:** 4-channel output ROI는 color 0과 maximum alpha를 가져 UI가 설명한 irreversible opaque black box가 되어야 한다.
- **실제 동작:** alpha가 0이다. 다만 underlying RGB도 0으로 덮였으므로 이 경로 자체가 원본 pixel을 투명 layer 아래 보존하지는 않는다.
- **코드 증거:** `cv::Scalar(0,0,0)`과 UI의 “Solid fill = opaque black box”가 직접 모순된다. 한국어도 “불투명한 검정 상자”로 번역되어 있다.
- **동적 검증 결과:** 4-channel `setTo` semantics와 production call path를 확인했다. production-path RGBA fixture는 **Not dynamically verified**; existing fill tests는 3-channel image다.
- **사용자 영향:** compositing/viewer 배경에 따라 검정 박스가 아니라 투명 영역이 보여 export appearance가 달라진다.
- **보안·개인정보 영향:** 원본 RGB가 지워져 직접 privacy exposure는 확인되지 않았다. 이후 metadata/alternate representation에 원본을 보존하는 경로도 발견하지 못했다.
- **기존 테스트가 잡지 못한 이유:** alpha custom-overlay test는 존재하지만 Solid fill의 alpha를 assert하지 않는다.
- **최소 수정 방향:** channel-aware fill scalar를 사용해 alpha=max로 설정하고 1/3/4-channel policy를 명시한다.
- **필요한 regression test:** `CV_8UC4`와 16-bit alpha input에서 ROI의 RGB=0, alpha=max이고 ROI 밖 alpha가 보존되는지 encode/decode까지 검사한다.
- **관련 finding:** transparent custom overlay가 먼저 mosaic를 적용하는 방어는 §15에 기록했다.

### CF-023 — 일본어 번역이 96개 source string을 그대로 노출하며 안전 문구도 포함한다

- **분류:** Documentation/build inconsistency
- **심각도:** Low
- **신뢰도:** High
- **영향받는 플랫폼:** 모든 platform의 일본어 locale
- **위치:** `CMakeLists.txt:83-96`, `scripts/check_translations.py:45-54,59-94` (`check`, `main`), `translations/cloakframe_ja.ts:677,953,1078,1141,1299,1357` 일대, `README.md:14`
- **위반되는 불변조건:** 지원한다고 표시한 locale의 개인정보·안전·output 경고는 영어 원문과 같은 의미로 이해 가능하게 전달되어야 한다.
- **사전 조건:** UI language를 일본어로 설정하고 untranslated safety/output path, metadata, model 또는 log warning을 만난다.
- **정확한 실행 순서:** CMake가 세 locale에 normal checker 등록 → 일본어의 source-equal translation은 warning만 생성 → CTest 성공 → package에 `.qm` 포함 → 일본어 UI에서 해당 문구가 영어로 표시된다. Strict source-equality gate는 중국어에만 적용된다.
- **기대 동작:** 지원 locale의 critical safety text는 번역되거나 release gate가 실패하고, 의도적으로 동일한 proper noun만 명시적 allowlist로 제외되어야 한다.
- **실제 동작:** 총 96개 source-equality warning이 success로 취급된다. 그 안에는 output-inside-input, overwrite, metadata, unredacted EXIF/GPS와 filename-in-log 경고가 포함된다.
- **코드 증거:** root CMake가 `translation_quality_zh_CN`에만 `--strict-source-equality`를 넘긴다. README는 Japanese를 지원 언어로 열거한다.
- **동적 검증 결과:** normal checker는 일본어 313 entries를 처리하고 exit 0과 96 warnings를 냈다. 같은 script를 strict mode로 일본어에 실행하면 exit nonzero였다.
- **사용자 영향:** 일본어 사용자에게 mixed-language UI와 중요한 안전 조건의 이해 저하가 생긴다.
- **보안·개인정보 영향:** 사용자가 metadata/log/output 위험을 잘못 이해할 가능성이 있으나 processing code 자체를 바꾸지는 않는다.
- **기존 테스트가 잡지 못한 이유:** CI가 경고를 허용하고 strict gate를 중국어 하나에만 둔다. 번역 의미 동등성이나 critical-message completeness 목록도 없다.
- **최소 수정 방향:** critical safety strings를 우선 번역하고 일본어에도 strict gate를 적용한다. 의도적 source equality는 reviewed allowlist로 관리한다.
- **필요한 regression test:** 각 supported locale에서 safety-message IDs가 finished, nonempty, non-source-equal인지 검사하고 placeholder/newline parity도 강제한다.
- **관련 finding:** 영어 UI 자체와 implementation 차이는 `CF-022`; model/license 조건의 notice는 `CF-025`와 별도다.

### CF-024 — 임의의 `v*` tag가 application version과 달라도 release/update artifact를 게시할 수 있다

- **분류:** Documentation/build inconsistency
- **심각도:** Medium
- **신뢰도:** High
- **영향받는 플랫폼:** 모든 release artifact와 updater feed
- **위치:** `.github/workflows/release.yml:3-8,65-88,172-183,258-267,274-305` (`macos`, `windows`, `linux`, `release` jobs), `CMakeLists.txt:9` (`project` version), `scripts/package_windows.ps1:265-293` (`Version`, `vpk pack`) 및 `scripts/package_linux.sh:33-34,159-174` (`VERSION`, `vpk pack`)
- **위반되는 불변조건:** release tag, binary/package version, release notes와 update feed version은 정확히 일치해야 하며 downgrade/asset confusion을 만들지 않아야 한다.
- **사전 조건:** maintainer 또는 compromised release credential이 `v1.12.0`, `vfoo`, `v1.10.0` 같은 tag를 현재 `CMakeLists.txt` version 1.11.0 commit에 push한다.
- **정확한 실행 순서:** broad `v*` trigger가 모든 builders 실행 → package filenames/version metadata는 CMake 1.11.0에서 파생 → macOS download prefix와 release note lookup/body는 tag 이름에서 파생 → release job은 note file 존재만 검사 → 서로 다른 tag/version identity를 한 GitHub release와 updater feed에 게시한다.
- **기대 동작:** exact semver tag를 parse하고 leading `v`를 제거한 값이 CMake/project, package manifest, release notes 및 appcast/release JSON version과 모두 같지 않으면 secret-using build 전에 실패해야 한다.
- **실제 동작:** equality 검사가 없다. Asset staging 일부는 project version, URL/note/release identity는 tag를 사용한다.
- **코드 증거:** workflow trigger는 `'v*'`; project는 `VERSION 1.11.0`; publish 단계는 `${{ github.ref_name }}`를 그대로 사용한다.
- **동적 검증 결과:** current tag를 새로 발행하는 destructive test는 수행하지 않아 **Not dynamically verified**. Static workflow trace는 완결되어 있다.
- **사용자 영향:** updater가 잘못된 version/asset을 노출하고 release install/delta selection, downgrade 판단, support diagnosis가 깨질 수 있다.
- **보안·개인정보 영향:** 이것만으로 unsigned package가 허용되는 것은 아니지만 `CF-001`의 single-origin update trust와 결합하면 asset confusion 영향이 커진다.
- **기존 테스트가 잡지 못한 이유:** tag-version consistency preflight와 workflow unit test가 없고 package scripts는 CMake version을 정상적으로 읽는지만 확인한다.
- **최소 수정 방향:** 첫 job에서 strict `^v[0-9]+\.[0-9]+\.[0-9]+$`와 exact equality를 검증하고 모든 jobs가 그 output을 사용하게 한다. 이미 출시한 version 이하 tag도 명시적으로 거부한다.
- **필요한 regression test:** matching, malformed, mismatched, prerelease, older tag cases에 대해 reusable validation script가 fail/pass하는지 검사한다.
- **관련 finding:** arbitrary branch가 signing secrets를 사용하는 것은 `CF-007`; updater authenticity는 `CF-001`/`CF-018`이다.

### CF-025 — macOS bundle의 실제 transitive dylib inventory와 고정 notice file이 자동 대조되지 않는다

- **분류:** Documentation/build inconsistency
- **심각도:** Info
- **신뢰도:** Medium
- **영향받는 플랫폼:** macOS package; 같은 방식의 dependency drift가 있는 다른 package도 잠재 영향
- **위치:** `.github/workflows/release.yml:41-42`, `scripts/package_macos.sh:124-152` (`bundle_dependencies_for`와 recursive closure), `THIRD_PARTY_NOTICES.txt:1-35` 및 component sections
- **위반되는 불변조건:** 실제 배포물의 runtime dependency/model과 THIRD_PARTY_NOTICES 및 source/notice obligations가 release마다 일치해야 한다.
- **사전 조건:** unversioned Homebrew dependency graph가 바뀌거나 OpenCV/ONNX Runtime/Qt의 transitive linkage가 새로운 shared library를 끌어온다.
- **정확한 실행 순서:** release job이 그 시점의 latest Homebrew formulae 설치 → package script가 `otool -L` closure를 반복 복사 → repository의 static `THIRD_PARTY_NOTICES.txt`를 그대로 복사 → artifact-derived component/license reconciliation 없이 release한다.
- **기대 동작:** locked dependency manifest/SBOM에서 actual binary closure를 생성하고 모든 shipped component에 license/source/notice disposition이 있음을 release gate로 검사해야 한다.
- **실제 동작:** packaging은 runtime closure와 notice file을 독립적으로 만든다. Local generated bundle에는 `libjasper`, `liblcms2`, `libmng`, `libpugixml`, `libutf8_range`, `libutf8_validity`가 있으나 notice text에는 이 names가 없었다.
- **코드 증거:** workflow의 `brew install`은 version을 pin하지 않는다. script lines 124–148은 dylib closure를 복사하지만 lines 150–152는 고정 notice/license만 복사한다. 이 감사는 법률 결론이 아니라 배포 inventory control 부재를 지적한다.
- **동적 검증 결과:** local `dist/macos/CloakFrame.app/Contents/Frameworks`와 notice text를 대조했다. 현재 official release artifact와 각 transitive library의 exact license/notice obligation은 **Not dynamically verified**.
- **사용자 영향:** release마다 dependency 집합과 재현성이 달라지고 support/SBOM/취약점 대응이 어려워진다.
- **보안·개인정보 영향:** 직접 privacy defect는 아니다. Unpinned dependency drift와 incomplete inventory는 supply-chain incident 식별과 라이선스 준수 증명을 약화한다.
- **기존 테스트가 잡지 못한 이유:** package smoke test는 launch/link success를 보지만 component-to-notice coverage를 assert하지 않는다.
- **최소 수정 방향:** dependency versions를 lock하고 artifact에서 SBOM/license manifest를 생성한다. 각 closure entry를 reviewed component/source/license record와 매핑하고 unmapped library가 있으면 release를 실패시킨다.
- **필요한 regression test:** produced DMG/App bundle의 recursive Mach-O closure와 generated SBOM/notice mapping이 1:1이며 forbidden/debug/unmapped library가 없는지 검사한다.
- **관련 finding:** model/FFmpeg의 주요 license 문구 자체는 §15에서 대조했다. Mutable non-x86 tool은 `CF-017`이다.

### CF-026 — 연속된 hard cuts 중 두 번째를 scene detector가 소비하고도 release note는 “every cut”을 보장한다

- **분류:** Documentation/build inconsistency
- **심각도:** Low
- **신뢰도:** High
- **영향받는 플랫폼:** 모든 video-processing platform
- **위치:** `src/SceneCut.cpp:12-17,112-157` (`SceneCutDetector::push`), `tests/test_tracking.cpp:509-569`, `RELEASE_NOTES.md:143-149`
- **위반되는 불변조건:** scene boundary 뒤 track이 잔존하지 않는다는 문서 보장은 detector가 실제 boundary를 모두 surface한다는 조건과 일치해야 한다.
- **사전 조건:** scene A 뒤 짧은 scene B가 `kConfirmFrames` 확인 구간 안에 있고 곧 scene C로 전환한다.
- **정확한 실행 순서:** A→B diff가 candidate가 됨 → B/C frame을 confirmation branch에서 평가 → candidate를 확정하면서 그 current diff는 `recordDiff`만 됨 → early return 때문에 같은 B→C diff를 새 candidate로 재평가하지 않음 → 두 번째 cut이 `SceneCuts`에 없음 → tracking은 그 boundary를 모른다.
- **기대 동작:** confirmation에 사용한 current frame transition도 새 cut 후보로 평가하거나 algorithm의 resolution limitation을 명시해야 한다.
- **실제 동작:** detector refractory interval 안의 adjacent genuine cut 하나가 유실될 수 있다. Release note는 tracks stop at “every cut”이라고 절대적으로 표현한다.
- **코드 증거:** candidate branch lines 124–140이 항상 return하고, confirm case의 `diff`는 spike test lines 143–152를 통과하지 않는다.
- **동적 검증 결과:** red 10 frames → green 2 frames → blue 10 frames 합성 sequence를 production detector에 넣었을 때 두 명백한 transitions 중 하나만 반환했다.
- **사용자 영향:** 짧은 intermediate shot의 timeline boundary가 표시되지 않고 scene-level review/tracking 설명과 결과가 어긋난다.
- **보안·개인정보 영향:** 기존 IoU, predicted-position, size-ratio gates가 있어 이 fixture에서 실제 unredacted face 또는 cross-scene mask leak까지는 입증하지 못했다. Appearance embedding/re-identification gate는 없다. 따라서 privacy defect로 과장하지 않았다.
- **기존 테스트가 잡지 못한 이유:** tests는 single cut, flash, static sequence와 trailing candidate만 다루며 two close persistent cuts가 없다.
- **최소 수정 방향:** confirmed candidate 이후 current transition을 새 baseline/candidate로 재평가하는 state machine을 만들고 최소-resolvable shot length를 명시한다.
- **필요한 regression test:** 1/2/3-frame intermediate scenes, consecutive cuts, flash-between-cuts에서 exact boundary set과 forward/reverse track separation을 검사한다.
- **관련 finding:** scene cut을 인식했을 때 forward/reverse tracking을 차단하는 로직은 §15 non-issue다. Internal same-scene gap exposure는 `CF-005`다.

## 14. Test Quality and Missing Coverage

### 14.1 현재 test suite가 실제로 증명하는 것

- Core test는 `ImageScanner`, `OutputPlan`, `ProcessorWorker`, `ImageIo`, `Mosaic`, metadata와 rooted no-replace publication의 production code를 직접 호출한다. 단순 UI mock만 통과하는 suite는 아니다.
- Video I/O test는 host FFmpeg/ffprobe를 실제 child process로 실행해 rotation, encode/decode, cancel, existing-output preservation과 corrupt-container rejection을 검사한다.
- Tracking test는 matching, bidirectional merge, interpolation, scene cut, angle wraparound와 bounds를 deterministic synthetic boxes로 검사한다.
- Parallel test는 ordered consumption과 mid-run cancellation을 여러 thread에서 검사한다.
- Release configuration에서도 `tests/CMakeLists.txt:1-7`의 `-UNDEBUG`/`/UNDEBUG` 때문에 543개 raw `assert`가 활성화된다. “Release CI에서 assertions가 모두 제거된다”는 의심은 성립하지 않았다.
- Current-head CI가 세 release platform에서 build와 6개 CTest를 통과했고 pinned detector fixtures도 실행했다. 이는 compile/link/package dependency의 유용한 smoke signal이다.

### 14.2 핵심 test oracle의 공백

가장 큰 문제는 detector/tracker의 중간 객체를 검사하면서 **최종 output pixel coverage와 최종 UI state를 같은 assertion에서 결합하지 않는 것**이다. 이 때문에 “검출이 하나라도 남았는가”가 “모든 알려진 민감 영역을 가렸는가”로 잘못 대체된다.

| Gap | Existing behavior | Missing oracle | Related finding |
|---|---|---|---|
| Crowded detector output | optional real-model test는 nonempty/pose만 확인 | 300/2,000 cap overflow signal, 모든 ROI의 final-pixel masking, non-`Done` outcome | `CF-003` |
| Low-confidence tracks | `tests/test_tracking.cpp:195-239`가 track deletion을 기대값으로 고정 | deleted track의 strong detection frames가 redacted되거나 Review Required인지 | `CF-004` |
| Retained-track internal gap | `tests/test_tracking.cpp:117-129`가 over-limit gap을 빈 상태로 기대하고 short motion/size rejection은 없음 | 길이 또는 guard로 생긴 같은-track coverage hole이 `Done`이 될 수 없는지, timeline hole 표시 | `CF-005` |
| Review timeline | zero-start fixtures만 사용 | preview frame hash와 production reader frame hash의 exact equality | `CF-006` |
| Recoverable decode error | invalid container만 reject | decoder가 exit 0으로 복구하더라도 declared/decoded duration mismatch를 실패시키는지 | `CF-008` |
| Aspect/audio topology | basic single audio/no-audio | SAR/DAR, all audio streams, language/disposition, independent start times | `CF-009`, `CF-010` |
| Hardware fallback | encoder availability preflight | preflight 후 first/mid/final encode failure에서 software retry와 result semantics | `CF-011` |
| Cancellation ordering | mid-run stop만 검사 | cancel-before-entry, close-on-start, cancel immediately followed by new generation | `CF-012` |
| Thread construction | real `std::thread` only | Nth-constructor failure injection and join/cleanup | `CF-013` |
| Memory budget | small image + large padding | decoded bit-depth/channel peak and hard single-item rejection | `CF-014` |
| Filesystem fallback | APFS normal primitives | rename/link `ENOTSUP`, ENOSPC, kill/power-loss after final-name creation | `CF-015` |
| Update trust/cache | no end-to-end updater adversarial test | wrong signer/hash origin, preseeded cache, downgrade, partial package, wrong arch | `CF-001`, `CF-002` |
| Localization | source-equality warning accepted for ja | critical safety message completeness/meaning for every advertised locale | `CF-023` |
| Release identity | package happy path | tag/project/package/feed version equality and stale/downgrade rejection | `CF-024` |

### 14.3 Mocks와 fixtures가 제거하는 위험

- Scripted/synthetic detector는 ONNX parser/provider, tensor shape, NMS overflow와 native-runtime crash boundary를 우회한다. Local machine에는 optional models가 없었고 CI fixture success도 malicious/crowded model behavior를 다루지 않는다.
- Tracking fixtures는 이미 생성된 boxes를 입력하므로 resize/letterbox reverse mapping, detector confidence threshold와 tracker admission의 joint boundary를 검증하지 않는다.
- 모든 ordinary video fixtures는 stream/format start가 사실상 0이고 single-square-pixel, single-audio, 8-bit SDR 위주다. VFR/CFR conversion을 보더라도 display geometry와 timeline origin을 함께 검사하지 않았다.
- Publication tests는 정상 local filesystem을 사용해 kernel primitive failure와 cross-filesystem/network-share semantics를 제거한다.
- GUI tests는 actual user interaction, HiDPI multi-screen scaling, close/re-enter timing과 updater restart/apply를 자동화하지 않는다.
- Packaging tests는 produced app launch 여부 중심이며 actual artifact closure-to-license map, signed update rejection, portable/installer/AppImage feature equivalence를 검사하지 않는다.

### 14.4 필요한 test 전략

1. **Privacy coverage oracle:** 각 known detector candidate/manual ROI마다 final decoded output이 원본과 privacy threshold 이상 달라졌는지, 그리고 uncovered candidate가 있으면 clean `Done`이 아닌지 production worker부터 UI summary까지 검사한다.
2. **Timeline differential oracle:** random rational time base/start/PTS/VFR/SAR/rotation/audio topology를 생성하고 processing reader, preview extractor, encoded output의 source-frame identity를 hash로 대조한다.
3. **Fault injection seams:** filesystem syscalls, `std::thread` factory, FFmpeg process, allocator/memory budget과 network/cache publication을 injectable interface로 만들어 Nth operation failure를 전부 sweep한다.
4. **Platform filesystem matrix:** NTFS/ReFS/UNC/reparse, APFS case-sensitive/insensitive/NFD, ext4와 casefold/FUSE/network share에서 path identity와 no-replace를 실행한다.
5. **Supply-chain integration:** app-pinned test key로 signed/unsigned/wrong-key/replayed/downgraded feed와 package를 실제 updater에 주고 fail-closed 결과를 검사한다.
6. **Fuzz/property tests:** image header/metadata, ffprobe JSON, ONNX graph patcher, detector tensor shapes, review coordinates와 track merge에 size/NaN/overflow corpus를 적용한다.

## 15. Investigated Non-Issues

아래 항목은 처음에는 finding 후보였지만 complete caller-to-sink trace 또는 동적 검증에서 방어가 확인되었다. 같은 오탐을 반복하지 않도록 기록한다.

| Investigated concern | Why it is not a finding at this revision |
|---|---|
| 사용자 media가 network로 upload됨 | `QNetworkAccessManager` 사용처는 fixed model URLs, GitHub release metadata/feed뿐이다. image/video bytes, thumbnails, filenames를 request body/query에 추가하는 sink가 없다. |
| 원본 또는 기존 output overwrite | source는 read-only snapshot으로 처리되고 final publication은 destination-rooted, no-follow/no-replace primitive와 source identity guard를 사용한다. Existing-output/concurrent winner tests가 통과했다. `CF-015`는 기존 file overwrite가 아니라 unsupported primitive에서 partial-new-final visibility다. |
| output symlink가 root 밖 victim으로 escape | `writeTemporaryAndPublishAtRoot`가 parent components를 descriptor/handle 기준으로 열고 symlink/reparse traversal과 existing final을 거부한다. Ordinary local filesystem tests가 pass했다. Hardlink identity와 exotic volume은 §18의 미검증 범위다. |
| transparent custom overlay 아래 원본 pixel 노출 | `src/Mosaic.cpp:628-634`가 custom overlay 전 ROI를 mosaic하고 alpha blend한다. `tests/test_core.cpp:614-644`의 transparent-overlay production test가 fallback을 확인했다. `CF-022`의 fill alpha와 다른 경로다. |
| NaN/negative/out-of-range detector box가 accepted region을 우회 | `FaceDetection.hpp`, detector NMS와 mosaic entry가 finite/positive를 검사하고 padded rectangle을 image bounds에 clip한다. 잘못된 box를 그대로 ROI에 전달하는 path를 찾지 못했다. Silent candidate omission은 `CF-003`–`CF-005`다. |
| `cv::Mat` backing storage가 사라진 뒤 GUI `QImage`가 dangling | `ProcessorWorker.cpp:218-236`의 `matToQImage`는 stride-aware temporary view를 만든 뒤 즉시 `image.copy()`로 deep copy한다. ROI `cv::Mat` views는 owning image의 lexical lifetime 안에서만 사용된다. Exercised ASan path에서도 use-after-free는 없었다. |
| EXIF orientation과 output pixel orientation 불일치 | `ImageIo`가 `IMREAD_UNCHANGED` 뒤 EXIF orientation을 pixels에 적용하고 metadata preserve 시 Orientation=1로 normalize한다. Existing rotation tests가 pass했다. |
| metadata preserve가 thumbnail/GPS를 무고지로 그대로 복사 | 기본은 strip이다. Preserve를 선택해도 embedded thumbnail, MakerNote, IPTC/XMP/comment/ICC는 제거한다. GPS는 사용자가 preserve를 켠 경우에만 allowlist로 의도적으로 복사되고 UI가 location 포함 가능성을 경고한다. 현재 documentation과 path가 일치한다. |
| built-in model hash가 download 때만 검사됨 | Catalog fixed SHA-256을 download에서 검사하고 run start에서 cache file을 hash하며 detector가 bytes를 다시 읽어 expected digest와 맞춘 뒤 session을 만든다. Incomplete/corrupt bytes가 valid session으로 가는 trace는 없다. `CF-019`는 temp filesystem hardening이다. |
| no-detection item이 무조건 `Done` | image no-detection은 저장되더라도 `CompletedWithWarnings`와 `Review required`가 되며 test가 이를 검사한다. 명시적 `Copy Original`도 위험 prompt와 warning outcome을 가진다. Known detections가 조용히 빠지는 별도 paths는 `CF-003`–`CF-005`다. |
| output extension과 actual format/codec 불일치 | Video `outputRelativePath`는 extension을 `.mp4`로 바꾸고 writer는 `-f mp4`와 H.264/HEVC encoder를 사용한다. Images는 supported original extension을 유지하고 그 extension으로 `imencode`한다. Reachable extension/format mismatch를 찾지 못했다. |
| 10-bit/HDR가 8-bit raw path로 조용히 내려감 | Probe가 supported H.264/HEVC, 8-bit pixel format와 SDR color constraints를 completion 전 검사하고 unsupported input을 거부한다. Codec/profile/pixel-format 조합 전체의 platform FFmpeg dynamic matrix는 §18에 남겼다. |
| FFmpeg path/filename shell injection | probe/decode/preview/encode 모두 `QProcess::start(program, QStringList)` 또는 set arguments를 사용한다. Shell string을 만들거나 `sh -c`/`cmd /c`로 넘기지 않는다. 공백/Unicode synthetic path가 성공했다. 모든 OS의 newline/reserved-name semantics는 미검증이다. |
| FFmpeg stdout/stderr pipe deadlock | raw stdout은 bounded frame reads이고 writer는 backpressure/timeout/cancel kill을 가진다. `QProcess`가 child channels를 내부 capture하므로 application이 OS pipe를 방치하는 명백한 deadlock은 찾지 못했다. Application은 failure detail이 필요할 때만 stderr를 읽으므로 exit-0 error 누락은 `CF-008`이고, 악의적인 대량 stderr의 QProcess memory 증폭은 **Not dynamically verified**다. |
| detection pass와 redaction pass가 서로 다른 source/timeline을 읽음 | 둘 다 같은 source snapshot과 동일 fps/timeline filter를 사용하고 actual frame counts가 다르면 실패한다. Analysis pass만 resolution scale을 추가하고 redaction pass는 native size다. `CF-006`은 별도 preview extractor와 origin contract의 mismatch다. |
| fractional FPS와 normal VFR가 무조건 drift | 30000/1001 round trip은 60→60 frames/2.002s와 rational rate를 보존했다. 합성 VFR은 documented CFR normalization으로 70 packets→67 frames/2.999s가 되었고 endpoint rounding 외 독립 defect를 확인하지 못했다. |
| rotation metadata가 encoder에서 무시됨 | reader filter가 rotation을 pixels에 bake하고 output rotation을 0으로 만든다. Existing real-FFmpeg test가 pass했다. SAR는 같은 geometry family지만 `CF-009`로 재현됐다. |
| scene cut을 찾은 뒤 reverse track이 넘어감 | `SceneCuts::reversed`와 forward/reverse tracker 양쪽에서 cut span을 차단하며 single-cut tests가 pass한다. `CF-026`은 detector가 cut 자체를 놓치는 state다. |
| 179°와 -179° angle interpolation이 0°가 아닌 긴 방향으로 회전 | production pose validation은 roll을 ±60°로 제한하므로 ±179° input은 accepted track box에 도달하지 않는다. 유효 범위 계산도 `std::remainder`와 sin/cos smoothing을 사용한다. 직접적인 ±179° regression test는 없다. |
| manual keyframe의 NaN, duplicate, invalid range | review conversion은 finite/positive/in-range 검사, sorting, duplicate collapse, inclusive range clipping과 total-box cap을 적용한다. Preview-origin mismatch `CF-006`은 validation 뒤 의미가 달라지는 문제다. |
| stale video preview callback이 새 frame/dialog에 적용 | `VideoReviewDialog`는 generation token, current process identity, disconnect/kill와 parent ownership을 사용한다. completion lambdas가 mismatched generation을 discard한다. |
| QObject review return이 lock 없이 cross-thread read되는 TSan race | code는 `Qt::BlockingQueuedConnection`과 `Q_RETURN_ARG`를 사용하므로 caller는 receiver invocation 완료 전 return하지 않는다. Focused TSan은 Qt binary 내부 semaphore를 instrument하지 못해 stack storage write/read synchronization을 보지 못했다. Qt contract와 control flow상 false positive로 판정했으며 §19에 원문 요약을 남겼다. |
| worker completion 뒤 GUI object use-after-free | MainWindow shutdown은 `shuttingDown_`, cancel, thread quit/wait, `QPointer`와 receiver destruction auto-disconnect를 결합한다. 두 sweep에서 reachable dangling callback을 찾지 못했다. `CF-012`는 shutdown 전 cancel flag loss다. |
| 이전 run callback이 새 run state를 갱신하거나 signal이 중복 연결됨 | `processing_`, `worker_`, `workerThread_` guards가 이전 run 종료 전 start를 거부하고 connections는 매 run의 새 worker/thread에 한 번만 생성된다. `onWorkerFinished`가 thread quit/wait와 pointer reset 뒤 UI를 해제한다. Generation 없는 pre-entry cancel만 `CF-012`로 확인됐다. |
| 매 item마다 GPU/ORT session을 새로 만들어 resource가 누적됨 | Detector cache key에 canonical path, size, mtime, SHA-256, model kind/provider policy가 포함되고 successful run 뒤 session을 cache로 되돌린다. Detector call은 worker orchestration에서 serialize되며 ordinary run에서 unbounded session accumulation path를 찾지 못했다. Provider-specific leak/OOM은 동적으로 미검증이다. |
| release tests에서 raw `assert`가 사라짐 | test target이 Release에서도 `-UNDEBUG`/`/UNDEBUG`를 강제한다. Current Release CTest가 실제 assertions와 함께 pass했다. |
| third-party GitHub Actions가 moving tag 사용 | checkout, install-qt, code-sign import, artifact upload/download, release action 모두 full commit SHA로 pin되어 있다. Top-level permissions는 read이고 publish job만 contents write다. Secret exposure scope는 별도 `CF-007`이다. |
| 모든 release dependency/model archive가 무검증임 | Windows/Linux direct archives와 platform FFmpeg/model assets는 pinned SHA-256과 fail-on-mismatch를 사용한다. macOS OpenCV/ONNX Runtime 등은 unversioned Homebrew closure이므로 이 방어의 범위가 아니며 drift는 `CF-025`; non-x86 mutable linuxdeploy는 `CF-017`이다. |
| macOS notarization/signing failure가 무시됨 | package/notarize scripts는 Developer ID, hardened runtime, entitlements, notary acceptance와 stapling verification failure에서 exit nonzero한다. Sparkle EdDSA absence `CF-018`은 별도 update trust hardening이다. |
| Windows Exiv2가 packaging script의 warning 때문에 실제 누락 | `CMAKE_PREFIX_PATH` environment는 CMake search에 추가되며 `find_package(spdlog REQUIRED)`/Exiv2 discovery가 작동한다. v1.10.2 Windows job log는 “metadata preservation enabled”와 6/6 tests를 보였다. Script line 68의 warning은 misleading하지만 material package defect 증거는 아니다. |
| runtime model download가 license obligation을 없앤다고 문서화됨 | notices는 models가 bundled되지 않고 runtime download된다는 사실과 별개로 YOLO/WIDER FACE, YuNet, custom-model terms를 명시한다. README도 commercial-use caveat를 표시한다. 법률적 충분성은 이 감사 범위 밖이다. |

## 16. Platform-Specific Risks

| Platform | Confirmed/current risk | Defenses observed | Not dynamically verified |
|---|---|---|---|
| Windows x64 | `CF-001` update authenticity, `CF-003`–`CF-014`의 cross-platform privacy/state defects, `CF-007` secret exposure | Velopack package/hash plumbing, rooted `CreateFileW`/reparse checks, `CREATE_NEW`, DirectML fallback configuration, current CI pass | 실제 Velopack install/update/downgrade, NTFS hardlinks/reparse/junction/UNC/long paths/trailing dot-space/reserved names, DirectML/GPU failure, Windows hardware encoder |
| macOS arm64 | `CF-003`–`CF-015`, `CF-018`, `CF-021`–`CF-026`; workflow branch가 Apple secrets 사용 (`CF-007`) | Developer ID/notary/staple fail closed, Sparkle/Apple validation, APFS no-replace primitive, local sanitizer builds | signed DMG/appcast update, case-sensitive APFS and NFD aliases, alias files, network/exFAT publish fallback, official release artifact closure; LSan unavailable |
| Linux x86_64 | `CF-001`, shared `/var/tmp` updater cache `CF-002`, cross-platform defects | official dependency/model/FFmpeg hashes, AppImage/Velopack current CI build/test | two-user cache preseed/apply, AppImage sandbox/mount behavior, casefold/FUSE/NFS paths, system FFmpeg variations, hardware VAAPI/NVENC path |
| Linux non-x86 | no current official release; `CF-017` if packaging branch is used | README/CI currently x86_64로 한정 | entire build/package/update branch; architecture-specific asset selection and ABI closure |

### 16.1 Windows-specific notes

- `ImageIo`는 directory handles와 reparse-point checks를 사용하지만 Windows path parser의 reserved device names, trailing spaces/dots, UNC/network shares, long-path policy와 case-preserving aliases를 실제 host에서 실행하지 못했다.
- DirectML/ONNX Runtime versions는 release workflow에서 별도로 pin되어 있다. Provider creation 실패 시 CPU fallback code는 있지만 실제 DirectML device removal/driver reset/large tensor behavior는 검증하지 못했다.
- Portable ZIP와 Setup EXE가 같은 update/cache/state semantics를 갖는지 설치 환경에서 비교하지 못했다.

### 16.2 macOS-specific notes

- Local Homebrew dependency graph는 release workflow의 advertised versions(Qt 6.10.3/OpenCV 4.13.0/ORT 1.27.1)과 달랐다. 정상적으로 build/test되었지만 official DMG의 exact closure와 minimum macOS 15 compatibility를 대신 증명하지 않는다.
- APFS normal path에서 no-replace가 작동했다. exFAT/network destination은 `CF-015`의 fallback을 실제 mount로 재현하지 못했다.
- Apple code signing은 update의 현 trust anchor이므로 signing secret scope `CF-007`과 missing independent Sparkle key `CF-018`을 함께 해결해야 한다.

### 16.3 Linux-specific notes

- `/var/tmp`는 OS-wide shared namespace라 Velopack 1.2.0 cache design `CF-002`의 precondition이 현실적이다. Sticky bit는 다른 사용자의 file 삭제는 막지만 attacker가 먼저 만든 자신의 directory/file을 victim process가 신뢰하는 문제를 막지 않는다.
- Official x86_64 build는 SHA-pinned linuxdeploy/FFmpeg/ORT/OpenCV를 사용한다. 그 밖의 architecture는 empty-digest `continuous` branch이므로 지원 전에 fail closed해야 한다.
- Bundled FFmpeg가 없을 때 PATH의 system FFmpeg를 사용한다. 이는 shell injection은 아니지만 codec/provider behavior가 distro별로 달라지므로 compatibility matrix가 필요하다.

## 17. Prioritized Remediation Plan

> 감사 시점의 계획이다. 현재 상태만 요약하면 P0 2·3항은 완료, P0 1항은
> `CF-001`이 닫히고 `CF-002`와 `CF-007`의 secret scope가 남았다. P0 4항의
> regression gate는 절반이다 — coverage, timeline, update signature 쪽 테스트는
> 생겼지만 two-user cache preseed와 실제 updater를 태우는 end-to-end 테스트는
> 없다. P1은 `CF-021`, `CF-026`과 8항의 fault-injection suite를 빼고 완료되었다.
> 항목별 현재 상태는 §0.1, 감사 밖 남은 작업은 §0.2를 본다.

### P0: 배포 전에 반드시 수정

1. **Update chain을 재설계한다 (`CF-001`, `CF-002`, `CF-007`).** Windows/Linux client에 offline-held key의 public trust anchor를 pin하고 manifest와 package를 서명한다. Linux cache는 per-user secure root, owner/mode verification, random exclusive temp, every-use digest/signature recheck로 바꾼다. Secret-using jobs는 protected tag/environment와 reviewed default branch commit만 허용한다.
2. **Privacy coverage를 fail closed한다 (`CF-003`, `CF-004`, `CF-005`).** Detector cap/truncation을 explicit unsafe result로 전파하고, 삭제되는 candidate track과 interpolation coverage hole을 기본 redaction 또는 Review Required로 처리한다. UI/summary는 known-but-uncovered region count가 0일 때만 `Done`을 허용해야 한다.
3. **하나의 video timeline contract를 사용한다 (`CF-006`).** stream/format start, time base, CFR mapping과 source-frame identity를 `VideoInfo`에 모델링하고 reader/preview/audio mux가 동일 mapping을 사용하게 한다. Manual review result를 source PTS 또는 검증된 common frame index에 bind한다.
4. **P0 regression gate를 추가한다.** 301+ detections, mixed-confidence track, 21-frame 및 short guard-rejected gap, differential-start preview hash, signed/wrong-key update와 two-user cache preseed가 세 platform release job에서 fail-closed인지 검사한다.

### P1: 다음 릴리스에서 수정

1. Recoverable decode error를 strict failure/Review Required로 만들고 declared/decoded duration을 대조한다 (`CF-008`).
2. SAR/DAR과 모든 supported audio streams/start/disposition을 보존하거나 UI에서 제한을 명시하고 사전 거부한다 (`CF-009`, `CF-010`).
3. Hardware encoder가 실제 encode 중 실패하면 private stage를 폐기하고 software encoder로 처음부터 재시도한다 (`CF-011`).
4. Generation-aware cancellation을 도입하고 thread construction을 RAII-safe하게 만든다 (`CF-012`, `CF-013`).
5. Single-image hard budget과 decoder allocation limits를 enforce하고 unsupported POSIX publication fallback은 fail closed한다 (`CF-014`, `CF-015`).
6. Sparkle EdDSA를 mandatory로 만들고 tag/project/package/feed exact-version gate를 추가한다 (`CF-018`, `CF-024`).
7. Destination volume-aware collision 검사, RGBA opaque fill, 일본어 critical translations와 consecutive-cut state를 수정한다 (`CF-021`–`CF-023`, `CF-026`).
8. Fault-injection, privacy-pixel, timeline differential tests를 release blocking suite로 만든다.

### P2: 장기적인 안정성 및 방어 강화

1. Custom ONNX consent를 content digest에 bind하고 restricted helper process로 parser/inference를 격리한다 (`CF-016`).
2. Non-x86 Linux는 immutable tool/digest가 준비되기 전 명시적으로 unsupported 처리한다 (`CF-017`).
3. Built-in model download를 random exclusive no-follow temp + atomic durable replace로 바꾼다 (`CF-019`).
4. Owner/mode/manifest를 검증하는 crash-recovery scavenger를 도입한다 (`CF-020`).
5. Locked dependency graph, generated SBOM, artifact-derived notice/source inventory를 release gate로 추가한다 (`CF-025`).
6. Windows/macOS/Linux filesystem laboratory와 fuzz corpus를 유지하고 sanitizer/static-analysis jobs를 compiler/platform별로 분리한다.

## 18. Residual Risk and Unverified Areas

다음은 결함이 없다는 뜻이 아니라 현재 증거로 동적 판정을 확정하지 못한 영역이다.

- **Windows/Linux runtime:** audit host가 macOS arm64이므로 Velopack apply/restart, DirectML, NTFS/reparse/UNC, AppImage, `/var/tmp` two-user exploit와 Linux filesystem branches는 정적 trace 또는 current CI 결과만 있다.
- **Actual release artifacts:** current official DMG/Setup/Portable/AppImage의 signature, entitlement, SBOM, exact runtime closure, updater feed와 clean-machine launch/install을 직접 검사하지 않았다. Local `dist/macos` artifact는 개발 환경 산출물이다.
- **Detector model adversarial behavior:** local external model files가 없어 301+ real/model-generated faces, edge/crossing faces, malicious ONNX와 provider-specific failure를 실행하지 않았다. Current CI의 pinned model tests는 normal fixture만 다룬다.
- **Memory/resource extremes:** multi-GiB 16-bit/RGBA decode, 4K/8K long video, 매우 큰 folder batch, GPU OOM, thread exhaustion, file-descriptor/process limit은 host 안정성을 위해 실제 소진하지 않았다.
- **Crash/storage faults:** disk-full, read-only/network share, cross-filesystem fallback, power loss, SIGKILL mid-publication, filesystem rename/link unsupported와 startup recovery는 fault-injection layer가 없어 완전 재현하지 않았다.
- **Timestamp/codec matrix:** negative PTS, B-frames/reordered timestamps, complex edit lists, HEVC Main10/HDR rejection variants, anamorphic rotation 결합, subtitles/data streams와 codec profiles를 exhaustive하게 실행하지 않았다.
- **GUI race/HiDPI:** 실제 multi-screen fractional DPI, accessibility scaling, dialog close/re-entry, application shutdown during native file dialogs/update restart를 automated GUI test로 실행하지 않았다.
- **Sanitizers:** LSan은 host runtime이 지원하지 않았다. TSan은 Qt/OpenCV/Homebrew libraries가 instrument되지 않았고 external symbolizer도 없어 third-party race와 project race의 완전 분리는 불가능했다. 확인 가능한 project report는 반대 trace에서 Qt blocking-call contract로 기각했다.
- **Static analyzers:** `clang-tidy`, `cppcheck`, `scan-build`, Infer, IWYU가 설치되지 않아 실행하지 않았다.
- **Licenses:** repository evidence와 local dependency names만 대조했다. 배포 지역별 법률 의견, upstream model training-data 권리, 각 transitive component의 최종 obligation 충족 여부는 법률/릴리스 담당자의 별도 검토가 필요하다.
- **Remote state:** current HEAD CI와 secret/environment metadata는 2026-08-06 관찰값이다. 이후 repository settings, release assets와 upstream URLs 변경은 이 보고서에 반영되지 않는다.

배포 시점의 잔여 risk는 P0 세 축—independent update signing, known-region coverage fail-close, unified video timeline—을 해결하고 그 regression tests가 실제 release artifact에 통과하기 전까지 높다.

## 19. Appendix: Sanitizer and Static Analysis Output

### 19.1 ASan + UBSan

별도 Debug build에 `-fsanitize=address,undefined -fno-omit-frame-pointer -fno-sanitize-recover=all`을 compile/link 모두 적용했다. `ASAN_OPTIONS=detect_leaks=0 UBSAN_OPTIONS=print_stacktrace=1`로 CTest 6/6이 통과했고 project test에서 heap/stack out-of-bounds, use-after-free, double-free 또는 UB diagnostic이 없었다.

이 결과는 exercised inputs에 한정된다. Optional detector tests가 local model 부재로 skip되었고 GUI/updater/package 및 extreme resource branches는 이 binary로 실행되지 않았다.

### 19.2 LeakSanitizer

동일 ASan binary에 `ASAN_OPTIONS=detect_leaks=1`을 주자 test 본문 전 runtime이 다음과 같이 종료했다.

```text
AddressSanitizer: detect_leaks is not supported on this platform.
```

따라서 leak 검증은 **Not dynamically verified**다. 정상 tests와 code review에서 반복 session/process/temp handle의 명백한 ordinary-path leak은 찾지 못했지만 LSan pass로 표현하지 않는다.

### 19.3 ThreadSanitizer

별도 Debug TSan build 자체는 성공했다. Full CTest는 translation 2, tracking, parallel, video I/O 5개가 통과하고 core test 하나가 TSan report로 abort했다. OpenCV/TBB worker noise를 줄이기 위해 `OPENCV_FOR_THREADS_NUM=1`로 focused rerun했다.

핵심 report는 worker stack의 `ReviewResult reviewResult` storage를 GUI receiver가 `QMetaObject::invokeMethod` return argument로 쓰는 동안 worker가 나중에 읽는 것으로 표시했다. Relevant source는 `src/ProcessorWorker.cpp:888-906` 및 video equivalent `:1240-1247`이다. 두 access 사이에는 project mutex가 없지만 connection은 `Qt::BlockingQueuedConnection`; Qt contract상 receiver invocation이 반환될 때까지 caller가 block된다. Synchronization은 uninstrumented Qt binary 내부 semaphore에서 수행되므로 TSan이 happens-before를 관찰하지 못한다. 반대 trace와 Qt API contract를 재검토해 **project data race가 아닌 external synchronization false positive**로 판정했다.

외부 `llvm-symbolizer`가 없어 system/Qt frames 일부는 address-only였고, 모든 third-party library가 TSan으로 rebuild된 것도 아니다. 그러므로 “TSan clean”으로 요약하지 않고 “suite 5/6, one investigated false positive, residual unverified”로 기록한다.

### 19.4 Compiler warnings

- Project의 기본 warning set을 `CMAKE_COMPILE_WARNING_AS_ERROR=ON`으로 빌드: success.
- 추가 `-Wpedantic -Wconversion -Wsign-conversion -Wshadow`: build success with diagnostics.
- Diagnostics는 주로 Qt `qsizetype`/OpenCV `int`/`size_t` 사이의 명시적 또는 implicit narrowing/sign conversion에 집중되었다. 관련 arithmetic과 input caps를 추적했지만 독립적인 reachable overflow/corruption 증거는 찾지 못했다. 실제 memory admission under-accounting은 warning과 무관하게 `CF-014`로 분리했다.

### 19.5 Unavailable static analysis

```text
clang-tidy: not found
cppcheck: not found
scan-build: not found
infer: not found
include-what-you-use: not found
valgrind: not found
llvm-symbolizer: not found
```

설치되지 않은 도구는 실행한 것처럼 간주하지 않았다. Network로 도구를 설치해 audit environment를 바꾸지 않았고, available compiler/sanitizer와 manual data-flow/ownership sweep으로 대체했다.

### 19.6 Dynamic reproduction digest

| Reproduction | Observed result |
|---|---|
| Pre-entry cancel | `outcome=1`, `output_exists=1` |
| Model `.part` symlink write | `opened=1`, `written=14`, victim=`verified-model` |
| Hardware preflight then real encode exit 42 | hardware selected; software command succeeds independently; production outcome Failed, no retry |
| Nonzero-start video | production frame 10 MD5 `12e50944df614adfc0e8cdeecebefbbb`; preview frame 10 `961b13242a282c5c1ed7b5c8c4598e27`; preview 10 equals production 30 |
| Differential-start A/V | output video 60 frames/6s; first audio about 4.010s |
| Recoverable H.264 corruption | ffprobe 120 frames/12s; production publishes 100 frames/10s as Completed |
| Mixed-confidence track | Completed, one retained track; second ROI unchanged even on its score-0.9 frames |
| 21-frame internal gap | Completed, same track at both ends; middle ROI unchanged |
| SAR 16:15 input | output SAR/DAR absent |
| Two AAC streams | output contains one audio stream |
| Rapid two scene cuts | detector returns one cut |
| 30000/1001 Unicode/space path | 60→60 frames, 2.002s, rational rate preserved |

모든 reproduction media는 lavfi/color 등으로 합성했으며 실제 개인 media는 사용하지 않았다. Temporary sources, wrapper와 harness는 repository 밖 `/private/tmp`에만 두었다.

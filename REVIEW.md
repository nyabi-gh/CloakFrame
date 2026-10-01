# CloakFrame 코드·개인정보·보안 리뷰

- 기준: `main` @ `80374a4` (v1.11.3), 2026-10-01
- 범위: 저장소 전체 (src, include, tests, tools, cmake, .github, scripts, 문서·번역)
- 방식: 핵심 경로(입력 → 디코딩 → 검출 → 추적 → 마스킹 → 인코딩/게시, 네트워크, 로그, 임시 파일)는 코드를 직접 따라 읽었다. 공급망·CI, UI/번역·문서 대조, 빌드·테스트 실행은 보조 검토를 병행했고, 그 결과 중 보고서에 넣은 항목은 해당 코드 줄·저장소 설정을 다시 확인했다. 배포 바이너리를 열어 본 결과처럼 직접 재확인하지 못한 것은 "보조 검토"로 표시했다.
- 코드는 수정하지 않았다. 이 파일만 추가했다.
- 표기
  - **확인됨**: 코드(또는 실행 결과)로 직접 확인. **추정**: 코드 경로는 확인했으나 실제 영향이 환경·데이터에 좌우되거나, 정황상 의심.
  - 심각도: Critical / High / Medium / Low / 제안. 결과물이나 디스크·로그에 개인정보가 남는 문제는 High 이상에서 출발해 조건을 따져 조정했다.
  - 이미 `docs/open-work.md`, `docs/PROJECT_REVIEW.ko.md`가 추적 중인 항목은 "기존 추적"으로 표시하고 HEAD에서 여전히 유효한지만 적었다.

---

## 1. 요약

CloakFrame은 개인정보 도구로서 기본기가 매우 단단하다. 이미지 출력은 픽셀에서 다시 인코딩하므로 EXIF·GPS·썸네일·XMP·ICC·MPF 보조 이미지가 기본적으로 따라가지 않고, 영상은 `-map`을 명시해 자막·데이터(GPS 텔레메트리)·첨부·커버아트 스트림을 버리며, 게시는 전 플랫폼에서 no-replace 원자 연산으로 처리한다. 원본 변경 감지, 모델 해시 고정, 업데이트 서명, fail-closed 판정 체계도 의도가 분명하고 테스트가 받쳐 준다.

다만 "실패 = 유출"이라는 기준으로 보면, **처리 도중 원본 영상의 전체 사본이 사용자가 공유하려는 출력 폴더 안에 놓이는 설계**와, **'완료'가 실제보다 낙관적으로 나오는 경로**(검토를 끈 영상의 저신뢰 트랙 가지치기, 장면 전환 경계의 추적 끊김, Enter 한 번으로 확정되는 검토 화면)가 남아 있다. 기본 디스크 로그는 깨끗하지만 활동 로그 전체(전체 경로 포함)가 항상 표준 출력으로 나간다. 업데이트 서명 체계는 코드상 잘 짜여 있으나, **서명 개인키가 저장소 범위 시크릿**이고 서명 거부가 미서명 안내로 우회되는 등 운영 쪽이 그 전제를 받쳐 주지 못한다. 또 **Linux에 번들한 FFmpeg는 재배포가 금지된 nonfree 빌드**다.

빌드·테스트: Windows 11에서 Debug/Release 모두 `/W4 /WX` 경고 0으로 빌드되고 ctest 17/17이 통과했다. clang-tidy는 CI가 분석하지 않는 업데이터·Windows 전용 코드에서 실패한다(D-4).

### 가장 시급한 문제 Top 5

| # | 항목 | 심각도 | 위치 |
|---|---|---|---|
| 1 | 영상 처리 중 원본 영상 전체 사본이 출력 폴더(`.cloakframe-stage-*/source.*`)에 존재 — 동기화 폴더면 업로드될 수 있고 크래시 시 다음 실행까지 남음 (A-1) | High | `VideoProcessor.cpp:410-431`, `ProcessorWorker.cpp:1342` |
| 2 | 검토를 끈 영상에서 저신뢰 트랙의 약한 검출 프레임을 **보고 없이** 지우고 "완료"로 끝낼 수 있음 (A-2) | High | `Tracking.cpp:761-784`, `VideoProcessor.cpp:655` |
| 3 | 검토 화면이 실수로 확정됨 — 영상 검토에서 목록·타임라인에 포커스가 있을 때 Enter = 인코딩, 이미지 검토에서 선택 없이 Return = 저장, 추적 공백 체크는 구간을 보지 않아도 가능 (E-1, E-2, A-9) | High | `VideoReviewDialog.cpp:839-841, 629-630`, `ReviewDialog.cpp:387-394, 722` |
| 4 | 업데이트 서명 개인키 2개와 Apple 서명 자격 6개가 모두 **저장소 범위** 시크릿(`release` 환경은 0개), 서명 거부 시 미서명 "새 버전" 안내로 우회 (B-1, B-2) | High | GitHub 저장소 설정, `MainWindow.cpp:2176-2185` |
| 5 | Linux AppImage에 번들한 FFmpeg가 `--enable-nonfree` 빌드라 재배포 불가 (F-0) | High (라이선스) | `release.yml:33-35, 652-663` |

그다음으로 시급한 것: A-3(장면 전환 경계 끊김 미보고), A-4(활동 로그 stdout), B-3(서명이 버전에 묶이지 않아 롤백 가능), F-1(libsodium 고지 누락).

---

## 2. 구조 개요

### 2.1 모듈 구성

| 계층 | 파일 | 책임 |
|---|---|---|
| 진입점 | `src/main.cpp` | Redactly 설정 이전, 로깅 구성, 이전 실행 스테이지 정리(백그라운드), 테마, `MainWindow` |
| UI | `MainWindow`, `ReviewDialog`, `VideoReviewDialog`, `ResultsDialog`, `SettingsDialog`, `Theme`, `ThumbnailLoader`, `ModelDownloader` | 입력·설정, 모델 준비(다운로드·해시 확인·동의), 검토 대화상자, 결과·설정 |
| 작업 조율 | `ProcessorWorker` (QThread), `OrderedParallel`, `MemoryBudget` | 스캔 → 충돌 검사 → 이미지 병렬/영상 순차 처리 → 판정 집계 |
| 입력 | `ImageScanner`, `OutputPlan`, `PathSafety` | 폴더 순회, 출력 경로 계산, 경로 탈출 방지 |
| 이미지 I/O | `ImageIo` | 스냅샷 복사, 디코딩, 방향 보정, 재인코딩, (선택) Exiv2 메타데이터 이전, no-replace 원자 게시 |
| 검출 | `Detector`, `Yolo5FaceDetector`, `YuNetFaceDetector`, `ScrfdFaceDetector`(+`OnnxGraphPatch`), `PlateDetector`, `OrtAcceleration` | ONNX Runtime 세션, 텐서 형태 검증, NMS, 가속기 선택 |
| 영상 | `VideoIo`(ffmpeg/ffprobe 프로세스), `VideoProcessor`(2패스), `Tracking`(ByteTrack 양방향), `SceneCut`, `VideoReviewTypes` | 디코드 파이프, 추적·보간·공백 산출, 인코딩·게시 |
| 마스킹 | `Mosaic` | 모자이크/블러/채움/사용자 이미지, 타원·소프트 에지 |
| 네트워크 | `ModelDownload`, `UpdateChecker`, `SelfUpdaterVelopack`/`SelfUpdaterSparkle`/`SelfUpdaterNull`, `UpdateSignature`, `UpdateCache` | 모델 다운로드, 새 버전 확인, 승인 후 업데이트 다운로드·서명 검증 |
| 보조 | `Logging`, `StageCleanup`, `ModelStore`, `ModelCatalog`, `CustomModelConsent`, `ReleaseNotes` | 로그 정책, 스테이지 정리, 모델 캐시, 커스텀 모델 동의 |

### 2.2 처리 파이프라인

```
[입력: 파일/폴더 드롭]
  └─ ImageScanner::scanMedia  ─ 미지원 확장자는 조용히 제외, 읽기 실패는 ScanIssue
       └─ OutputPlan::findOutputConflicts ─ 중복/기존 출력 있으면 전체 거부
            │
            ├─ 이미지 (검토 OFF: processOrdered 병렬 / 검토 ON: 순차)
            │    copyFileNoReplaceAtRoot → %TEMP%/.cloakframe-stage-*/source.ext  (원본 스냅샷)
            │    imageFrameCount>1 → 건너뜀 │ 크기·메모리 예산 검사
            │    imreadUnicode(IMREAD_UNCHANGED) → readExifOrientation → applyOrientation
            │    toDetectionBgr → Detector::detect (+PlateDetector) ─ omitted 집계
            │    [검토] ReviewDialog (BlockingQueuedConnection, 1600px 미리보기)
            │    applyAnonymization (Mosaic.cpp)
            │    cv::imencode → (선택) Exiv2 copyMetadata(스테이징) → writeTemporaryAndPublishAtRoot
            │         출력폴더/.cloakframe-<난수>.tmp/payload → no-replace rename
            │
            └─ 영상 (항상 순차)
                 locateFfmpegTools → probeVideo(ffprobe JSON) → videoUnsupportedReason(10bit/HDR/코덱/한도)
                 copyFileNoReplaceAtRoot → 출력폴더/.cloakframe-stage-*/source.ext   ← A-1
                 Pass 1: ffmpeg(-xerror, setpts,fps, scale≤analysisLongEdge) → bgr24 파이프
                         → SceneCutDetector + detect(frame) → frameDetections
                 buildBidirectionalTracks → postProcessTracks(가지치기·보간·평활·끝 연장)
                         → uncoveredSpans / droppedTracks                       ← A-2, A-3
                 [검토] VideoReviewDialog (트랙 포함/제외, 수동 트랙, 공백 확인)
                 Pass 2: 같은 디코드 → 프레임별 마스킹(워커 풀) → ffmpeg 인코더 stdin
                         -map 0:v:0 -map 1:a? -map_metadata -1 -map_chapters -1
                         HW 인코더 실패 시 SW로 전체 재시도
                 출력폴더/.cloakframe-stage-*/video.mp4 → moveFileNoReplaceAtRoot
            │
       └─ applyOutcome → FileResult/RunSummary → Completed | CompletedWithWarnings | Cancelled | Failed
```

네트워크 요청은 `ModelDownload::downloadModelData`(모델), `UpdateChecker::check`/Velopack 확인/Sparkle appcast(새 버전), Velopack `downloadUpdate`(승인 후)뿐이며 모두 HTTPS + `NoLessSafeRedirectPolicy`다.

---

## 3. README / SECURITY.md 약속 vs 실제 동작

| 약속 (출처) | 판정 | 근거 |
|---|---|---|
| 파일은 내 컴퓨터 안에서만 처리, 업로드 없음 (README) | **부분 불일치** | 앱이 미디어를 전송하는 코드는 없음(확인됨). 그러나 영상 원본 사본을 출력 폴더에 두므로, 출력 폴더가 OneDrive/Dropbox/iCloud/NAS 동기화 대상이면 처리 중 원본이 외부로 나갈 수 있음 → A-1 |
| 네트워크 요청은 모델 다운로드·시작 시 버전 확인·승인 후 다운로드 3종 (README) | 대체로 일치 | `ModelDownload.cpp:47-50`, `UpdateChecker.cpp:47-56`, `MainWindow.cpp:2186-2196`. 단 macOS Sparkle은 세션 중 설정을 꺼도 예약 확인이 계속됨 → B-6 |
| 업데이트 확인을 끌 수 있음 / 승인 전 다운로드 없음 (README) | 일치(macOS 예외 위와 같음) | `MainWindow.cpp:2160-2163`, `2192-2195`, `2231-2250` |
| 원본 파일은 수정하지 않음 (README, CONTRIBUTING) | **일치** | 입력은 읽기 전용 핸들로만 열고 스냅샷에서 처리(`ImageIo.cpp:605-636`, `ProcessorWorker.cpp:900-953`). 쓰기·이동은 스테이징 파일에만 적용 |
| 같은 이름 결과·공유 출력 경로면 시작하지 않음 (README) | 일치(기존 추적 예외) | `OutputPlan.cpp:41-78`, 게시도 no-replace. 대소문자 판정의 OS 가정은 open-work §1-3(CF-021) |
| 기본 디스크 로그에는 파일명 없는 진단만 (README) | **문구상 일치, 실질 부분 불일치** | 로그 파일은 `logDiagnostic`만 기록(`Logging.cpp:63-70`). 그러나 활동 로그 전체가 stdout으로 나가고(A-4), 출력 폴더 경로 32개가 `stage-roots.txt`에 영구 저장됨(A-5) |
| 상세 로그는 별도로 켜야 하며 즉시 적용, 열기·삭제 가능 (README) | 일치 | `Logging.cpp:28-32, 46-60`, `SettingsDialog.cpp:97` |
| 썸네일은 백그라운드에서 최대 2개 (README) | 일치 | `ThumbnailLoader.cpp:72, 89`. 디스크 캐시 없음(메모리 전용) |
| 회전 정보는 픽셀에 반영, 컨테이너 메타데이터 제거 (README) | 일치 | 디코드 자동 회전 + `VideoIo.cpp:1227-1228`. 전역 메타데이터 제거는 테스트됨(`test_video_io.cpp:868-870`), 스트림 단위 태그·부가 스트림은 테스트 없음(D-1) |
| 오디오는 MP4 호환 시 유지, 필요 시 AAC (README) | 일치 | `VideoIo.cpp:1146, 1206-1220`. 음성 자체는 익명화 대상이 아님 → A-8 |
| 10비트/HDR은 거부 (README) | 일치 | `VideoIo.cpp:669-679` |
| GPU 실패 시 CPU로 자동 전환 (README) | 부분 일치 | 모델 로드·워밍업 시 `Ort::Exception`만 폴백(`ProcessorWorker.cpp:458-469, 1360-1372`). 처리 중 GPU 오류는 그 파일 실패로 집계(조용히 실패하지는 않음) → C-2 |
| 실패·건너뜀·검출 없는 저장이 하나라도 있으면 "검토 필요" (README) | 일치 | `ProcessorWorker.cpp:808-818` |
| "완료"는 보고된 미확인 경고가 없다는 뜻 (README) / "증명하지 못하면 미가림으로 보고하고 깨끗이 끝내지 않는다" (SECURITY.md) | **불일치** | 가지치기된 약한 프레임(A-2)과 장면 전환 경계 끊김(A-3)은 어떤 카운터에도 들어가지 않음. `Tracking.hpp:142-146`의 계약 주석과도 어긋남 |
| 완료된 실행 뒤 임시·스테이징 파일에 개인정보가 남지 않음 (SECURITY.md) | 정상 종료 시 일치 | 스테이지는 소멸자에서 제거(`StageCleanup.cpp:239-251`). 크래시 시 다음 실행 때 스윕(`main.cpp:84-88`). 출력 폴더의 `.cloakframe-*.tmp`·`.cloakframe-partial`은 스윕 대상 밖(A-12) |
| 이전 이름 Redactly 설정·모델 자동 이어 사용 (README) | 일치 | `main.cpp:38-66`, `ModelCatalog.cpp:18-21, 34`. 이어받은 모델도 실행 전 고정 해시로 재검증(`MainWindow.cpp:1637-1678`) |
| 커스텀 모델은 내용 해시에 동의를 묶음 (SECURITY.md) | 대체로 일치 | `CustomModelConsent.cpp:36-61`. 승인 확인과 로드 해시 사이 재대조 없음 → B-7 |
| 업데이터는 빌드에 고정한 공개키로 서명된 패키지만 수락 (SECURITY.md) | 코드는 일치, 운영은 약함 | Ed25519 검증과 빌드 시 키 고정(`UpdateSignature.cpp`, `SelfUpdaterVelopack.cpp:192-215`), v1.11.3 실제 서명 확인(보조 검토). 그러나 개인키가 저장소 시크릿(B-1), 거부 시 미서명 안내로 우회(B-2), 버전 미결합으로 롤백 가능(B-3) |
| 미디어·모델·업데이트를 여는 것이 코드 실행으로 이어지지 않음 (SECURITY.md) | 부분 | 커스텀 ONNX 파서 격리 부재(open-work §2, 기존 추적), macOS 번들의 넓은 이미지 디코더 면적(B-11) |
| 의존성과 라이선스를 NOTICES에 기록 (CONTRIBUTING) | **불일치** | libsodium 누락(F-1), Linux FFmpeg nonfree(F-0), FFmpeg 라이선스 표기 오류(F-2) |

---

## 4. 범주별 상세 지적

### A. 마스킹 결과물과 기기 내 흔적의 개인정보 (최우선)

#### A-1. 영상 원본 전체 사본이 처리 내내 출력 폴더에 있다 — High · 확인됨 (외부 유출은 추정)

- **근거**
  - `ProcessorWorker.cpp:1342` — `options.outputRootPath = pathToQString(safeRoot);`
  - `VideoProcessor.cpp:410-431` — `stagingBase = options.outputRootPath` 로 `StageDirectory`를 만들고, 원본을 `<출력>/.cloakframe-stage-XXXXXX/source<확장자>`로 통째로 복사.
  - 이 사본은 Pass 1, **사용자 검토(대기 시간 무제한)**, Pass 2가 끝날 때까지 유지(`VideoProcessor.cpp:504-1021`).
  - `StageCleanup.hpp:13-17` 주석이 "크래시 시 출력 옆에 전체 사본이 남는다"는 점을 인정하고, `main.cpp:84-88`에서 **다음 실행 때** 스윕한다.
  - 도입 커밋 `08550a8`의 근거는 "대상 볼륨의 여유 공간만 필요, 같은 파일시스템에서 원자 rename, tmpfs `/tmp` 회피"다. rename이 필요한 것은 인코딩 결과뿐이고 **소스 스냅샷에는 같은 볼륨일 이유가 없다.**
- **영향**
  - 출력 폴더는 정의상 "공유하려고 고른 곳"이다. Windows 11은 문서·사진·바탕화면이 기본으로 OneDrive 백업 대상인 경우가 많고, Dropbox·Google Drive·NAS 동기화 폴더도 흔하다. 동기화 클라이언트는 점(.)으로 시작하는 폴더도 올린다(Windows에서 `.`은 숨김 속성도 아니다). 결과적으로 **가리기 전 원본이 클라우드로 업로드될 수 있다.** (추정: 클라이언트·제외 규칙에 좌우)
  - 앱이 강제 종료·크래시되면 다음 실행 전까지 원본이 출력 폴더에 남는다. 사용자가 그 사이 출력 폴더를 통째로 공유하면 그대로 유출된다.
  - Windows 탐색기 썸네일 캐시에도 원본 프레임이 남는다.
- **재현**: 출력 폴더를 OneDrive 폴더로 지정 → 얼굴이 있는 영상 1개, 검토 ON으로 시작 → 영상 검토 창이 뜬 상태에서 출력 폴더를 열면 `.cloakframe-stage-xxxxxx\source.mp4`가 보이고 SHA-256이 원본과 같다. OneDrive 활동 센터에 업로드가 표시된다.
- **개선안**
  1. 소스 스냅샷만 사용자 전용 위치로 옮긴다. 인코딩 스테이징(`VideoIo.cpp:1133-1144`)은 원자 게시를 위해 출력 루트에 둬도 된다(마스킹된 데이터).
     ```cpp
     // VideoProcessor.cpp: 출력 루트 대신 사용자 전용 캐시(Linux ~/.cache: tmpfs 아님)
     QString privateStagingRoot()
     {
         const QString base = QStandardPaths::writableLocation(QStandardPaths::CacheLocation);
         const QString root = base + QStringLiteral("/staging");
         QDir().mkpath(root);              // POSIX에서는 0700으로 chmod, Windows는 사용자 프로필 ACL 상속
         return root;
     }
     StageDirectory sourceStaging(privateStagingRoot());
     ```
     공간 부족이면 "원본 사본을 만들 공간이 없습니다(필요: N GB, 위치: …)"로 실패시키거나, 사용자가 명시적으로 스테이징 위치를 고르게 한다.
  2. 대안: 사본을 만들지 않고 원본을 **쓰기 금지 공유 모드 핸들**로 잡아 둔 채 처리하고(Windows `FILE_SHARE_READ`만 허용), POSIX는 지금처럼 identity 검사로 변경을 감지해 fail-closed. 디스크 사용량도 절반이 된다.
  3. 최소한 출력 루트에 둘 수밖에 없다면 Windows에서 `FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_NOT_CONTENT_INDEXED`를 설정하고 README에 명시한다(동기화 문제는 해결되지 않음).
- **테스트 아이디어**: `test_video_io`에 있는 "staging lives beside the destination" 단정을 뒤집는다. 검토 콜백 안에서 출력 루트를 재귀 탐색해 **원본과 같은 SHA-256을 가진 파일이 없음**을 단정하고, 캐시 위치의 스테이지가 완료 후 사라지는지도 확인한다.

#### A-2. 검토를 끈 영상에서 약한 검출 프레임을 보고 없이 지운다 — High · 확인됨

- **근거**
  - `VideoProcessor.cpp:655` — `postProcess.retainLowConfidenceTracks = static_cast<bool>(review);` (검토 OFF면 false)
  - `Tracking.cpp:727-753` — 강한 검출(기본 임계 0.5 → 영상 강한 임계 0.4, `VideoProcessor.cpp:329-336`)이 3개 미만이고 강한 비율이 50% 미만이면 저신뢰 트랙.
  - `Tracking.cpp:761-784` — 저신뢰 트랙에서 **약한 박스와 보간 박스를 모두 지우고**, 트랙이 완전히 빈 경우에만 `droppedTracks`에 센다.
  - `Tracking.hpp:142-146`의 계약("세 종류 중 하나라도 0이 아니면 깨끗한 결과를 보고하면 안 된다")에 **부분 가지치기는 포함되지 않는다.** 따라서 `ProcessorWorker.cpp:657-663, 808-818`에서 Completed가 된다.
  - 기존 테스트 `tests/test_tracking.cpp:228-241`이 바로 이 상황(20프레임 중 약한 검출 18개, 강한 검출 2개)을 만들지만, 남은 프레임만 확인하고 사라진 프레임과 보고 부재는 검사하지 않는다.
- **영향**: 약한 박스는 IoU로 강한 검출 트랙에 이어 붙은 것이라 같은 얼굴일 가능성이 높다(측면·원거리·모션 블러). 위 시퀀스에서 결과는 프레임 0–7만 가려지고 8–19는 **노출된 채 "완료"** 다.
- **재현(단위)**: `movingObjectSequence(20, …, score 0.2)` + 프레임 3·4만 0.9 → `buildBidirectionalTracks` → `postProcessTracks(tracks, {}, 20)` → `report.droppedTracks == 0 && report.uncoveredFrames == 0`인데 `boxAtFrame(10) == nullptr`.
- **개선안**: 개인정보 측면에서 안전한 쪽은 "약한 박스를 유지해 과잉 마스킹"이다. 오탐 억제가 꼭 필요하면 최소한 보고한다.
  ```cpp
  // Tracking.hpp
  struct TrackCoverageReport {
      int uncoveredFrames = 0;
      int droppedTracks = 0;
      int prunedWeakFrames = 0;                 // 새 항목: 약한 검출이 있었으나 지운 프레임
      std::vector<UncoveredSpan> uncoveredSpans;
      std::vector<UncoveredSpan> prunedSpans;   // 검토 화면·파일별 결과로 전달
  };
  // Tracking.cpp: erase_if 전에 지울 프레임을 span으로 모아 report에 누적
  ```
  `FileIssueKind::PrunedLowConfidence`를 추가해 `NeedsReview`로 연결하고, 완료 대화상자 수치에도 넣는다.
- **테스트 아이디어**: 위 시퀀스로 `prunedWeakFrames > 0`을 단정하고, `ProcessorWorker` 수준에서 검토 OFF 실행이 `CompletedWithWarnings`로 끝나는지 확인한다.

#### A-3. 장면 전환을 가로지르는 추적 끊김은 보고되지 않는다 — Medium · 경로 확인됨 / 빈도 추정

- **근거**
  - `Tracking.cpp:266-273` — 컷 프레임에서 활성 트랙을 모두 종료.
  - `Tracking.cpp:553-558` — 두 박스 사이 공백이 컷을 가로지르면 **보간도 보고도 하지 않고** `continue`("A cut means the subject legitimately left").
  - `Tracking.cpp:685-687, 704-706` — 끝 연장도 컷을 넘지 않음.
  - `SceneCut.cpp:12-22, 149-158` — 48×27 회색조 전역 평균 차이가 22 이상이고 최근 중앙값의 2.5배면 후보, 2프레임 안에 돌아오지 않으면 컷. 플래시는 걸러지지만 **조명이 켜짐, 빠른 휩팬, 카메라 앞을 지나는 물체(2프레임 초과)** 는 컷으로 판정될 수 있다.
- **영향**: 가짜 컷 직후 검출기가 같은 얼굴을 몇 프레임 놓치면(모션 블러가 흔함) 그 프레임은 마스크도 경고도 없다. 컷이 없었다면 같은 상황이 `uncoveredSpans`로 보고됐을 것이다. "검출기가 못 찾은 것"(SECURITY.md 범위 밖)이 아니라 **추적기가 끊김을 알고도 분류상 숨기는** 경우다.
- **개선안**: 컷 앞 트랙의 마지막 박스와 컷 뒤 K프레임(예: 0.5초) 안에서 시작하는 트랙의 첫 박스가 공간적으로 겹치면(IoU>0 또는 중심 거리 < 박스 크기) 그 사이를 "컷 경계 공백"으로 보고한다. 컷 뒤에 같은 위치의 트랙이 아예 없으면 컷 직후 몇 프레임을 "장면 전환 직후 미확인"으로 표시하는 것도 방법이다.
- **테스트 아이디어**: 프레임 0–9 얼굴, 10에서 전역 밝기 +60(컷 판정), 10–13 검출 없음, 14부터 같은 위치 얼굴 → `uncoveredSpans`에 10–13이 포함되는지.

#### A-4. 활동 로그 전체가 항상 stdout으로 나간다 — Medium · 확인됨 (영구 저장은 환경 의존 추정)

- **근거**
  - `MainWindow.cpp:2787-2792` — `appendLog`가 모든 활동 메시지를 `spdlog::info`로 보낸다.
  - `Logging.cpp:113-115` — 콘솔 sink는 상세 로그 설정과 무관하게 항상 붙는다. 파일 sink만 `detailed_`로 막힌다(`Logging.cpp:63-70`).
  - 활동 메시지에는 전체 경로가 들어간다: `ProcessorWorker.cpp:525-533`(읽지 못한 입력 전체 경로), `873, 1017`(이미지 전체 경로), `1180, 1228`(출력 전체 경로), 출력 충돌 메시지(`588-595`). `VideoProcessor.cpp:407`, `StageCleanup.cpp:209`도 경로를 spdlog로 직접 쓴다.
- **영향**: Windows(WIN32 서브시스템)·macOS(Finder 실행)에서는 stdout이 버려진다. 그러나 GNOME/KDE는 앱을 systemd 사용자 scope로 실행하고 stdout을 **journald에 영구 저장**한다(`journalctl --user`). 터미널 실행·리다이렉션도 같다. README의 "기본 디스크 로그에는 파일명이 없다"는 CloakFrame 로그 파일에 대해서만 참이다.
- **개선안**: 콘솔 sink를 상세 로그와 같은 스위치로 묶거나 릴리스 빌드에서 붙이지 않는다. `appendLog`는 UI 위젯에만 쓰고, 디스크 기록은 상세 로그 ON일 때만 파일 sink로 직접 보낸다.
  ```cpp
  void configureLogging(const QString &directory, bool detailed) {
      localSink = std::make_shared<PrivateLogSink>(directory, detailed);
      std::vector<spdlog::sink_ptr> sinks{localSink};
  #ifndef NDEBUG
      sinks.push_back(std::make_shared<spdlog::sinks::stdout_color_sink_mt>());
  #endif
      ...
  }
  ```
- **테스트 아이디어**: `test_logging`에서 `detailed=false`로 구성한 뒤 경로가 든 메시지를 기록하고, 기본 로거의 sink 목록에 콘솔 sink가 없음(또는 stdout 캡처가 비어 있음)을 단정.

#### A-5. 출력 폴더 경로가 `stage-roots.txt`에 영구 기록된다 — Low · 확인됨

- **근거**: `StageCleanup.cpp:45-58`(`<GenericData>/CloakFrame/stage-roots.txt`), `84-138`(실행마다 맨 앞에 추가, 최대 32개). 정상 완료 후에도 지우지 않고, 설정의 "로그 삭제"(`Logging.cpp:46-60`)도 이 파일은 건드리지 않는다.
- **영향**: 폴더 이름 자체가 민감할 수 있다(예: `…/이혼소송_증거/`). 상세 로그를 끈 사용자에게도 남는다.
- **개선안**: 스테이지가 정상 제거되면 그 루트를 목록에서 뺀다(남은 스테이지가 있는 루트만 유지). A-1을 고쳐 소스 스냅샷이 출력 루트를 떠나면, 이 목록에는 인코딩 스테이징만 남으므로 더 줄어든다. "로그 삭제"에 포함하거나 README에 고지한다.

#### A-6. 메타데이터 보존을 켜면 GPS·일련번호·소유자명 등 식별 EXIF가 남는다 — Medium · 확인됨 (opt-in)

- **근거**
  - `ImageIo.cpp:2013-2015` — `exif.gpsinfo.`가 허용 그룹에 포함.
  - `ImageIo.cpp:2016-2047` — 블록리스트 방식. `BodySerialNumber`, `LensSerialNumber`, `CameraOwnerName`, `Artist`, `HostComputer`, `ImageUniqueID`, `DocumentName`, `ImageDescription`, `XPAuthor`/`XPComment`(unsignedByte라 `safeType` 통과) 등은 128바이트 이하면 그대로 복사된다.
  - `tests/test_core.cpp:2032-2034` — `Exif.Image.Artist`가 **유지되는 것을 기대값**으로 단정.
  - 체크박스 라벨 `MainWindow.cpp:684`은 "Preserve selected EXIF metadata"이고, 위치가 함께 남는다는 사실은 툴팁(`692-697`)에만 있다.
- **영향**: 기본값은 OFF(`MainWindow.cpp:686, 2000`)라 기본 출력은 깨끗하다(아래 "잘된 점" 참고). 하지만 익명화 앱에서 "보존"을 켠 사용자가 위치·카메라 일련번호까지 남길 의도인 경우는 드물다.
- **개선안**: 허용 목록으로 바꾸고 GPS는 별도 선택지로 분리한다.
  ```cpp
  constexpr std::array<std::string_view, 12> kAllowed = {
      "Exif.Image.Make", "Exif.Image.Model", "Exif.Image.Orientation",
      "Exif.Photo.DateTimeOriginal", "Exif.Photo.ExposureTime", "Exif.Photo.FNumber",
      "Exif.Photo.ISOSpeedRatings", "Exif.Photo.FocalLength", "Exif.Photo.LensModel",
      "Exif.Photo.ExposureBiasValue", "Exif.Photo.Flash", "Exif.Photo.WhiteBalance"};
  // GPS는 "위치 정보도 유지(권장하지 않음)" 체크가 켜진 경우에만 Exif.GPSInfo.* 추가
  ```
- **테스트 아이디어**: GPS·`BodySerialNumber`·`CameraOwnerName`·`HostComputer`·`XPAuthor`를 넣은 원본으로 `copyMetadata` → 모두 부재 단정. GPS 옵션 ON일 때만 GPS 존재.

#### A-7. 모자이크·블러 강도 — Medium · 추정 (공격 모델에 좌우)

- **근거**
  - `Mosaic.cpp:51-63` — 셀 수를 **변당 최대 12개**로 고정(해상도와 무관하게 비례: 좋은 설계). 다운샘플은 `INTER_LINEAR`(블록 평균이 아니라 점 샘플에 가까움).
  - `Mosaic.cpp:71-98` — 블러 σ = 짧은 변/6, 커널 크기 ≈ 3σ(±1.5σ에서 잘림)로 명목 σ보다 약하다.
  - 기본 방식은 모자이크(`ProcessorWorker.hpp:53`, `MainWindow.cpp:2009`), 기본 패딩 0.18(`MainWindow.cpp:89`).
- **영향**
  - 패딩을 포함한 12셀이면 얼굴 폭은 약 8–9셀이다. 학습된 인식기가 모자이크·블러 얼굴을 **후보 집합 안에서** 상당한 정확도로 식별한 연구가 있다(McPherson 외, 2016 "Defeating Image Obfuscation with Deep Learning"). "아는 사람 중 누구인지" 맞히는 공격에는 충분히 강하지 않을 수 있다.
  - 영상은 ROI가 프레임마다 조금씩 움직여 셀 격자가 얼굴의 다른 위치를 샘플링하므로 다중 프레임 초해상 복원 여지가 생긴다.
- **개선안**: 셀 상한을 6–8로 낮춘 "강함" 기본값, 다운샘플을 `INTER_AREA`로, 영상은 트랙 단위로 격자를 고정(트랙 첫 박스 기준 정렬·크기 양자화), 블러 커널을 `6σ+1`로. UI·README에 "모자이크/블러는 아는 사람의 재식별을 완전히 막지 못한다, 위험이 크면 채움"을 명시.
- **테스트 아이디어**: 임의 크기 ROI에서 셀 수 ≤ 상한 단정, 한 트랙 내 연속 프레임에서 셀 경계 좌표가 박스 기준으로 고정되는지 단정.

#### A-8. 영상의 음성은 익명화되지 않으며 판정에도 드러나지 않는다 — Medium · 확인됨

- **근거**: `VideoIo.cpp:1177-1183`(모든 오디오 스트림 매핑), `1206-1220`(복사 또는 AAC). 제거 옵션 없음. README는 "오디오 유지"를 적고 있지만, 완료 대화상자(`MainWindow.cpp:1866-1891`)에는 오디오 언급이 없다.
- **영향**: 목소리, 대화 속 이름·전화번호·차량번호는 그대로 남는다. "완료"가 영상 전체가 안전하다는 인상을 줄 수 있다.
- **개선안**: "오디오 제거" 옵션(`-an`), 오디오가 포함된 출력이 있으면 결과 화면에 "음성은 가려지지 않았습니다" 고지.
- **테스트 아이디어**: 옵션 ON 시 출력의 오디오 스트림 0개, OFF 시 결과 메시지에 고지 포함.

#### A-9. 추적 공백 확인을 구간을 보지 않고 켤 수 있다 — Medium · 확인됨

- **근거**
  - `VideoReviewDialog.cpp:629-630` — 모든 공백 항목이 처음부터 체크 가능.
  - `VideoReviewDialog.cpp:891-899` → `ProcessorWorker.cpp:1495-1498, 1654-1666` — 체크된 공백은 `pendingTrackingGapFrames`에서 빠지고, 남은 경고가 없으면 Completed.
  - `VideoReviewDialog.cpp:742-777` — "Include all/Exclude all"은 `QSignalBlocker`로 `setTrackIncluded`(`1068-1070`, 체크 초기화 포함)를 우회해 **체크를 초기화하지 않는다.** 대화상자 안내 문구("Editing masks resets these checks", `632-634`)와 README와 어긋난다(제외 트랙은 별도 경고가 남으므로 영향은 작다).
- **영향**: 공백 목록을 위에서부터 클릭(또는 Space)만 해도 "완료"가 된다. README는 "구간을 방문하는 것만으로는 해제되지 않는다"고 말하지만, 방문하지 않고도 해제된다. 같은 성격의 "실수로 확정" 경로로 E-1(Enter → 인코딩)과 E-2(Return → 저장)가 있다.
- **개선안**: 해당 공백의 모든 프레임(최소한 시작·중간·끝)을 표시한 뒤에만 체크를 활성화(`Qt::ItemIsUserCheckable`을 그때 부여), 일괄 포함/제외에서도 `invalidateGapAcknowledgements()` 호출.
- **테스트 아이디어**: `test_video_review`에서 미방문 공백 체크 시도 → 체크 불가 단정, "Exclude all" 후 `acknowledgedGapIndices` 비어 있음 단정.

#### A-10. 폴더 안의 미지원 형식은 조용히 빠진다 — Low · 확인됨

- **근거**: `ImageScanner.cpp:46-49` — 확장자가 지원 목록에 없으면 기록 없이 반환. 로그는 "지원 파일 N개"만 말한다(`ProcessorWorker.cpp:516`).
- **영향**: iPhone 기본 HEIC/HEIF, AVIF, GIF, MKV/AVI가 섞인 폴더도 "완료"가 된다. 출력 폴더에는 그 파일들이 없으므로 직접 유출은 아니지만, "폴더 전체를 처리했다"는 오해로 원본 폴더를 공유하는 실수 여지가 있다.
- **개선안**: 제외된 파일 수와 확장자 분포를 결과에 표시("처리하지 않은 파일 12개: .heic 11, .gif 1").

#### A-11. 크래시 덤프에 원본 픽셀이 남을 수 있다 — Low · 추정

- **근거**: 덤프 억제 코드가 없다(`PR_SET_DUMPABLE`, `RLIMIT_CORE`, `WerAddExcludedApplication` 등 검색 결과 없음).
- **영향**: Linux `systemd-coredump`는 힙 전체를 `/var/lib/systemd/coredump`에 저장한다. Windows WER은 설정에 따라 로컬 덤프·업로드를 한다.
- **개선안**: Linux 시작 시 `prctl(PR_SET_DUMPABLE, 0)`(디버거 부착도 막히므로 릴리스 빌드에서만), Windows는 `WerAddExcludedApplication` 또는 문서 고지.

#### A-12. 출력 폴더의 게시용 임시 디렉터리는 크래시 후 정리되지 않는다 — Low · 확인됨

- **근거**: 이미지 게시는 출력 폴더에 `.cloakframe-<난수>-<순번>.tmp/payload`를 만든다(`ImageIo.cpp:384-391, 870-899, 1280-1307`). FAT 계열 폴백은 `<이름>.cloakframe-partial`(`ImageIo.cpp:1121`). 스윕은 `.cloakframe-stage-??????`만 본다(`StageCleanup.cpp:27`).
- **영향**: 내용은 대부분 마스킹된 결과다. 예외는 "원본 복사 + 메타데이터 보존"(`ProcessorWorker.cpp:1154-1158`)으로, 이 경우 원본 바이트다(사용자가 원본 복사를 고른 경우라 Low).
- **개선안**: 같은 스윕에 두 패턴을 포함한다(락이 없으므로 수정 시각 기준 유예).

> **잘 막혀 있는 것(확인됨)**: 기본 경로 이미지 출력에는 메타데이터가 없다(`ImageIo.cpp:1624-1628, 1674-1681`: `cv::imencode` 결과만 기록). EXIF 내장 썸네일, XMP(Google 카메라의 `GImage:Data` 원본 포함), IPTC, ICC, JPEG MPF 보조 이미지(게인맵·깊이맵), TIFF SubIFD 프리뷰, PNG 텍스트 청크가 모두 따라가지 않는다. 다중 페이지 TIFF·APNG·애니메이션 WebP는 건너뛰고 경고(`ProcessorWorker.cpp:956-964`). `IMREAD_UNCHANGED`는 OpenCV의 자동 EXIF 회전을 끄므로 방향은 앱이 한 번만 적용한다(`ProcessorWorker.cpp:1008-1033`). 채움은 알파를 불투명으로 설정한다(`Mosaic.cpp:100-117`). 영상은 `-map 0:v:0 -map 1:a?`로 자막·데이터(GoPro/DJI GPS 텔레메트리)·첨부·커버아트·타임코드 트랙을 버린다(`VideoIo.cpp:1177-1183`).

### B. 일반 보안

> 기존 추적 항목의 HEAD 상태(보조 검토가 저장소 설정 읽기 API와 v1.11.3 배포 산출물로 확인): open-work §1-1 `CF-001`은 v1.11.3의 `.nupkg.sig`가 고정 키로 검증되고 appcast에 `sparkle:edSignature`, DMG에 `SUPublicEDKey`가 있어 **닫아도 된다.** §1-2는 여전히 유효하며 기록보다 넓다(B-1). §1-11(brew 미고정·SBOM 없음), §1-12(앱 번들 미스테이플), §3(Authenticode 없음), §5 `CF-002`(Linux `/var/tmp` 캐시, 감수)는 그대로다.

#### B-1. 업데이트 서명 개인키가 저장소 범위 시크릿이다 — High · 확인됨

- **근거**
  - `gh secret list` → 저장소 범위 8개: `CLOAKFRAME_UPDATE_PRIVATE_KEY`, `SPARKLE_ED_PRIVATE_KEY`, `MACOS_CERTIFICATE_P12`, `MACOS_CERTIFICATE_PASSWORD`, `APPLE_DEVELOPER_ID`, `MACOS_NOTARY_ISSUER_ID`, `MACOS_NOTARY_KEY`, `MACOS_NOTARY_KEY_ID`. `gh secret list --env release` → **0개.**
  - 코드와 문서는 반대 전제를 깔고 있다. `release.yml:43-46`("A repository-scoped secret would be readable from any branch … so the key belongs to this environment"), `114-116`. `UpdateSignature.hpp:12-16`("A signature made with a key held offline is the only part of an update that domain cannot forge"). 그런데 `BUILDING.md:182-186, 205-208`은 두 개인키를 **repository secret**으로 저장하라고 안내한다.
  - open-work §1-2는 "Apple 서명 secret 7개"만 적었지만 실제로는 업데이트 서명 키 2개가 함께 저장소 범위에 있다.
- **영향**: `environment: release`의 태그 제한·리뷰어 승인은 **환경 시크릿에만** 적용된다. 저장소 시크릿은 쓰기+workflow 권한을 가진 주체(탈취된 개인 토큰 포함)가 임의 브랜치에 워크플로를 올려 승인 없이 읽을 수 있다. 업데이트 서명 키는 이 앱의 업데이트 신뢰의 유일한 근거이므로, 유출되면 모든 Windows/Linux/macOS 사용자에게 임의 코드가 든 "정상 서명" 업데이트를 보낼 수 있다.
- **개선안**
  1. 즉시: 8개를 `release` 환경으로 옮기고(`gh secret set NAME --env release`) 저장소 사본을 삭제, BUILDING.md 수정. 환경에는 이미 태그 정책과 필수 리뷰어가 있다.
  2. 구조: 업데이트 패키지 서명을 CI 밖으로. CI는 초안 릴리스에 다이제스트를 올리고, 유지보수자가 하드웨어 기반 키로 로컬 서명해 `.sig`를 올린 뒤 공개한다. 단일 유지보수자 체계에서는 환경 시크릿도 같은 토큰으로 승인될 수 있으므로 이것이 근본 해결이다.
  3. 키가 이미 저장소 범위에 있었으므로 교체를 검토한다. 클라이언트가 키를 고정하므로 새 키를 함께 신뢰하는 과도기 릴리스가 필요하다.
- **검증**: 임시 브랜치 워크플로에서 `${{ secrets.CLOAKFRAME_UPDATE_PRIVATE_KEY != '' }}`가 `false`인지.

#### B-2. 서명 거부가 미서명 "새 버전" 안내로 바뀐다 — Medium · 확인됨

- **근거**: 서명 검증 실패 시 `SelfUpdaterVelopack.cpp:74-78`이 `checkFailed(rejection)`을 낸다. `MainWindow.cpp:2176-2185`는 이를 info 로그로만 남기고 `startLegacyUpdateCheck()`를 호출한다. 레거시 확인기는 같은 GitHub "latest" 릴리스를 "CloakFrame X is available"로 안내하고, 업데이트를 누르면 릴리스 페이지를 연다(`2200-2215`).
- **영향**: 릴리스는 올릴 수 있지만 서명은 못 하는 공격자(B-1의 위협 모델)가 막혔던 지점을, 앱이 사용자를 그 릴리스의 미서명 설치본으로 안내해 되돌린다. 사용자의 다운로드·실행이 필요하므로 Medium.
- **개선안**: 거부는 별도 시그널(`updateRejected`)로 분리해 "서명이 맞지 않는 업데이트를 거부했습니다" 경고를 보이고, 그 버전에 대해서는 레거시 안내를 띄우지 않는다. 레거시 폴백은 Velopack 자체를 쓸 수 없을 때만.
- **테스트 아이디어**: 거부를 내는 가짜 업데이터로 `MainWindow::checkForUpdates`를 돌려 `UpdateChecker`가 생성되지 않음을 단정.

#### B-3. 서명이 버전에 묶이지 않아 롤백이 가능하다 — Medium · 확인됨

- **근거**: 서명 대상은 패키지 SHA-256의 hex 문자열뿐이다(`sign_update_packages.sh:47-48`, `UpdateSignature.hpp:18`). `.sig` URL은 피드가 준 `Version`/`FileName`으로 조립되고(`SelfUpdaterVelopack.cpp:167-169`) 피드 버전과 서명 내용의 버전을 대조하지 않는다.
- **영향**: 서명할 수 없는 공격자도 과거에 정상 서명된 nupkg와 공개된 `.sig`를 새 버전 번호(예: `99.0.0`)로 다시 게시하면 클라이언트가 받아들여, 이미 고친 취약점이 있는 구버전이 설치된다. 업데이트를 숨기는 freeze 공격도 대응이 없다.
- **개선안**: `cloakframe-update-v1\n<packageId>\n<channel>\n<version>\n<fileName>\n<sha256>` 같은 정규 문자열에 서명하고, 클라이언트에서 `version == TargetFullRelease.Version > 현재 버전`, 채널·파일명 일치를 요구한다. `.sig` 형식이 바뀌므로 서버·클라이언트를 함께 전환. macOS Sparkle은 피드 서명 지원 여부를 확인한다.
- **테스트 아이디어**: 구버전 다이제스트 서명 + 새 버전 라벨 피드 → `Rejected`.

#### B-4. 서명 키를 쓰는 잡이 고정되지 않은 서드파티 코드를 먼저 실행한다 — Medium · 확인됨(구조)

- **근거**: macOS 잡은 버전 미고정 `brew install …`(`release.yml:130`) 뒤 같은 잡에서 p12·공증 키·`SPARKLE_ED_PRIVATE_KEY`를 쓴다. Windows 잡은 러너 이미지의 vcpkg 커밋에 따라 바뀌는 classic 모드 설치와 `dotnet tool install vpk`(`443-446`) 뒤 서명한다(`571-576`). Linux도 같은 구조다.
- **영향**: 앞 단계에서 실행된 침해된 패키지가 작업 공간의 서명 스크립트를 바꾸거나 `PATH`에 `openssl`/`codesign`을 끼워 키를 빼낼 수 있다.
- **개선안**: 빌드 잡은 서명 없는 산출물만 만들고 `environment`를 두지 않는다. `environment: release`의 별도 sign 잡은 checkout·download-artifact·검증·서명만 한다. vcpkg는 `vcpkg.json` + `builtin-baseline`으로 고정.

#### B-5. Linux AppImage의 라이브러리 해석 — Medium · 코드 확인 + 배포 바이너리 검사(보조 검토) / 실행 영향 추정

- **근거**: Linux 실행 파일에 `INSTALL_RPATH`가 설정되지 않는다(`src/CMakeLists.txt:201-204`, 저장소 전체에 해당 설정 없음). 릴리스 스모크 테스트는 `--appimage-version`만 실행해(`release.yml:721`) 앱 바이너리를 로드하지 않는다. 보조 검토가 v1.11.3 AppImage를 열어 본 결과: 실행 파일에 RUNPATH 없음, libsodium·exiv2·spdlog·fmt·libjpeg/png/tiff/webp 미번들(호스트 의존), 번들 OpenCV 8개에 CI 경로 `RUNPATH=/home/runner/opencv/lib`.
- **영향(추정)**: 빌드 환경과 다른 배포판에서는 실행되지 않거나 호스트의 Qt·OpenCV·ORT를 대신 쓴다. `/home/runner`를 만들 수 있는 주체가 라이브러리를 먼저 로드시킬 여지도 있다.
- **개선안**: `set_target_properties(CloakFrame PROPERTIES INSTALL_RPATH "$ORIGIN/../lib/x86_64-linux-gnu")`, 번들 라이브러리 RUNPATH를 `$ORIGIN`으로(patchelf), 의존성 번들 또는 정적 링크. 다른 배포판 컨테이너에서 `QT_QPA_PLATFORM=offscreen LD_DEBUG=libs`로 앱을 실제 기동하는 스모크 테스트와 RUNPATH 검사 단계.

#### B-6. macOS: 세션 중 업데이트 확인을 꺼도 Sparkle 예약 확인이 계속된다 — Low · 확인됨

- **근거**: `SelfUpdaterSparkle.mm:14-19`(`initWithStartingUpdater:YES`, `automaticallyChecksForUpdates = YES`), `MacOSXBundleInfo.plist.in:33-36`(`SUEnableAutomaticChecks`), 설정 해제는 다음 실행에서만 컨트롤러 생성을 막는다(`MainWindow.cpp:2160-2175`). 컨트롤러는 삭제되지 않는다.
- **영향**: 앱을 하루 이상 켜 두면 사용자가 끈 뒤에도 appcast 요청이 나간다. 또 Sparkle은 `automaticallyChecksForUpdates`를 사용자 기본값에 저장하므로 앱 설정과 별개의 상태가 생긴다.
- **개선안**: 설정 변경 시 `controller_.updater.automaticallyChecksForUpdates = NO`로 동기화하고, 시작 시 앱 설정 값을 Sparkle에 그대로 반영한다. `SUEnableAutomaticChecks`는 plist에서 빼고 코드에서만 제어.

#### B-7. 커스텀 모델: 동의 확인 해시와 실제 로드 해시 사이 재대조가 없다 — Low · 확인됨

- **근거**: 동의 확인(`MainWindow.cpp:1536-1538`, 파일을 해시해 승인 다이제스트와 비교) → 그 뒤 `makeDetectorCacheKey`가 파일을 **다시** 해시(`1630-1633`, `1446-1474`) → 워커는 이 두 번째 해시로 바이트를 검증한다(`1685-1688`, `ScrfdFaceDetector.cpp:50-57`). 두 해시가 같은지는 비교하지 않는다.
- **영향**: 두 시점 사이에 파일이 바뀌면 동의하지 않은 모델이 로드된다. 같은 사용자 권한의 쓰기가 필요하므로 SECURITY.md의 범위 밖에 가깝지만, 한 줄로 닫을 수 있다.
- **개선안**: `if (isCustom && runState.faceKey.modelSha256.toHex() != customModelApproval_.digest.toLatin1()) { 거부 }`.

#### B-8. 커스텀 ONNX 파싱의 메모리 증폭 — Low · 추정 (격리 부재는 기존 추적)

- **근거**: `OnnxGraphPatch.cpp:46-106` — 경계 검사는 정확하다(`length > size - pos`). 다만 모든 필드의 바이트를 `std::vector`로 복사하고(`85, 96`), 중첩 메시지를 다시 파싱·복사한다. 512 MB 상한(`kMaxCustomModelBytes`)의 모델을 작은 필드 수백만 개로 채우면 수 GB를 할당할 수 있다.
- **영향**: 동의한 파일에 한정된 DoS. ORT 파서 자체의 프로세스 격리 부재는 open-work §2(기존 추적, 가장 큰 남은 보안 격차라는 판단에 동의).
- **개선안**: 필드 수 상한(예: 1,000,000)과 `std::span` 기반 비복사 파싱. 퍼저(libFuzzer)로 `patchScrfdInputSize` 진입점을 돌린다.

#### B-9. 외부 프로세스 호출 — 대체로 안전, 방어적 보강 제안 · 확인됨

- **확인된 사항**: 모든 ffmpeg/ffprobe 호출은 인자 목록으로 전달(쉘 없음). 입력 경로는 항상 절대 경로이므로 `-`로 시작해도 `-i` 다음 인자로만 해석된다. 출력 경로는 앱이 만든 이름이다. 공식 배포본은 번들 바이너리를 먼저 찾고(`VideoIo.cpp:120-139`) `.sha256` 매니페스트가 있으면 검증한다(`60-112`). ffprobe JSON은 `QJsonDocument`로 파싱하고 수치는 모두 범위 검사한다(`VideoIo.cpp:631-661`).
- **제안**
  - 입력에 `file:` 접두사를 붙여 프로토콜 해석 여지를 원천 차단(`VideoIo.cpp:747-748`, `484-485`, `1177`; `ThumbnailLoader.cpp:32-33`; `VideoReviewTypes.cpp:182`). POSIX 절대 경로는 `/`로 시작해 지금도 안전하지만, 향후 상대 경로가 들어오는 경로가 생기면 `concat:`·`subfile,` 같은 접두사가 의미를 갖는다.
  - 소스 빌드는 `PATH`와 `/usr/local/bin`에서 ffmpeg를 찾는다(`VideoIo.cpp:141-156`). 번들이 아닐 때 UI에 실제 경로·버전을 표시하면 하이재킹을 알아채기 쉽다.
  - 매니페스트가 없으면 검증을 건너뛴다(`VideoIo.cpp:63-66`). 번들 빌드에서는 매니페스트 부재를 실패로 처리하는 편이 의도에 맞다.

#### B-10. 파일 시스템 — 강점이 크고, 남은 것은 사용성 · 확인됨/추정

- **확인됨(강점)**: 출력 경로는 Windows에서 재분석 지점을 거부하는 디렉터리 핸들 체인으로 고정하고(`ImageIo.cpp:756-835`), POSIX에서 `openat`/`O_NOFOLLOW`/`renameat2(RENAME_NOREPLACE)`·`renameatx_np(RENAME_EXCL)`·`linkat` 순으로 게시한다(`1064-1266`). 사전 충돌 검사와 실제 쓰기 사이의 TOCTOU는 "게시 자체가 no-replace"로 닫혀 있다.
- **Low · 확인됨**: Windows 게시 파일은 보호된 개인 DACL(`D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FA;;;OW)`)로 생성되고(`ImageIo.cpp:837-849, 893-899`) rename 후에도 유지된다(영상은 `1808-1832`에서 같은 DACL 적용). 상속이 끊기므로 공유 폴더·팀 드라이브에 저장한 결과를 다른 계정이 열지 못할 수 있다. 개인정보에는 유리하지만 "공유용 결과물"과는 긴장 관계다. 게시 직후 부모 ACL 상속을 복원하는 옵션(`SetSecurityInfo(..., UNPROTECTED_DACL_SECURITY_INFORMATION, ...)`)을 검토.
- **Low · 추정**: 재귀 스캔은 심볼릭 링크 디렉터리를 건너뛰지만(`ImageScanner.cpp:146-150`), MSVC STL에서 디렉터리 **정션**은 `is_symlink()`가 false라 따라간다. 정션 순환이 있으면 파일은 canonical 경로로 중복 제거되더라도 순회가 긴 경로 한도(장경로 매니페스트로 32K자)까지 이어질 수 있다. 디렉터리도 canonical 키로 방문 기록을 남기면 닫힌다.

#### B-11. 기타 공급망·하드닝 — Low · 확인됨(별도 표시는 추정)

- **액션 고정 관리**: 모든 `uses:`가 40자 SHA로 고정되어 있다. 다만 `download-badge.yml:31`의 `actions/github-script` SHA는 커밋이 아니라 `v9` **태그 객체**이고 주석은 `# v8`이다(내용은 공식 v9.0.0, 보조 검토). 저장소 설정은 `sha_pinning_required: false`, `allowed_actions: all`이고 Dependabot 설정이 없다 → 커밋 SHA로 재고정, 설정 강화, `github-actions` 생태계 Dependabot.
- **릴리스 게시 순서**: 초안 없이 바로 게시한다(`release.yml:777-780`). 업로드 중 피드가 `.sig`보다 먼저 보이면 B-2와 겹쳐 레거시 안내가 뜰 수 있다 → `draft: true`로 올린 뒤 공개. `persist-credentials: false` 미설정, `v*` 태그 규칙·`main` 보호 없음, 태그 커밋이 `main`의 조상인지 검사 없음. `release.yml:81-84`는 비어 있는지 보려고 개인키 전체를 env에 올린다(`secrets.X != ''` 식이면 충분).
- **Homebrew**: `HOMEBREW_NO_REQUIRE_TAP_TRUST: 1`(`ci.yml:68, 97`, `release.yml:124`)의 이유가 기록돼 있지 않다.
- **FFmpeg 네트워크 프로토콜**: 번들 FFmpeg는 TLS·SRT·SSH·ZMQ 등 네트워크 프로토콜을 포함한다(보조 검토, 구성 문자열). 모든 `-i` 앞에 `-protocol_whitelist file,pipe`를 넣으면 "미디어가 기기 밖으로 나감"을 FFmpeg 쪽에서도 원천 차단한다(B-9의 `file:` 접두사와 함께).
- **첫 설치 진위 확인**: AppImage 미서명, `SHA256SUMS` 미서명(`release.yml:765-768`), 빌드 출처 증명(attestation) 없음 → `actions/attest-build-provenance`, `SHA256SUMS` 서명.
- **바이너리 하드닝**: `CloakFrame.exe`에 `/CETCOMPAT` 없음(`cmake/CloakFrameHarden.cmake:12-17`), Linux `-D_FORTIFY_SOURCE=2`(`:24`)는 최근 배포판 기본(3)보다 낮다. `-fstack-clash-protection -fcf-protection=full -D_GLIBCXX_ASSERTIONS`, Windows `SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_DEFAULT_DIRS)` 권장. macOS dylib 13개에 Homebrew 절대 `LC_RPATH`가 남아 있다(보조 검토, hardened runtime 라이브러리 검증으로 완화됨).
- **macOS 디코더 면적(추정)**: macOS 번들은 jp2(JasPer)·mng·pdf·tga·heif 등 Qt 이미지 플러그인을 더 싣는다(보조 검토). `QImageReader`를 형식 지정 없이 쓰므로(`ImageIo.cpp:145, 1563`, `ProcessorWorker.cpp:199`) `.jpg` 이름의 파일도 내용 스니핑으로 이 파서들에 도달할 수 있다 → 불필요 플러그인 제외, `setDecideFormatFromContent(false)`.
- **업데이트 입력 검증(추정)**: 피드의 `FileName`을 검증 없이 URL·로컬 경로에 쓴다(`SelfUpdaterVelopack.cpp:167-169, 259-263`; 실제 악용은 재현하지 못함). `UpdateChecker.cpp:68`의 `readAll()`에 크기 상한이 없다.
- **버전 드리프트(추정, 보안 공지 대조 필요)**: Windows ORT-DirectML 1.24.4는 다른 플랫폼의 1.29.0보다 뒤처진다(커스텀 모델 파싱 면적). Windows/Linux는 Qt 6.10.3, macOS는 Homebrew Qt 6.11.2로 플랫폼마다 다르다(§1-11 미고정의 결과가 배포본에 나타남).

### C. 정확성과 견고성

#### C-1. "완료 / 검토 필요" 집계 — 구조는 정확, 예외는 A-2·A-3·A-9 · 확인됨

- `ProcessorWorker.cpp:625-693`(파일별 상태)과 `808-818`(전체 판정)이 실패·건너뜀·검출 없는 저장·원본 복사·메타데이터 경고·생략 영역·버린 트랙·제외 트랙·미확인 공백·스캔 실패를 모두 반영한다. 예외 경로는 파일 단위 `catch`(`1264-1276`)와 실행 단위 `catch`(`820-831`)가 모두 Failed로 떨어진다. 검토를 약속했는데 검토 창을 띄울 수 없으면 저장하지 않는다(`435-441, 1101-1108`).
- 남은 낙관 경로는 A-2(가지치기), A-3(컷 경계), A-9(무방문 체크)다.
- 참고: 영상 검토 창에서 **수동 트랙 한도 초과·잘못된 키프레임**은 사용자 취소와 같은 `cancelled_ = true`로 처리된다(`ProcessorWorker.cpp:1514-1556`). 결과는 Cancelled라 낙관적이지는 않지만 원인이 사라진다(기존 PROJECT_REVIEW에도 언급).

#### C-2. GPU → CPU 폴백과 결과 동등성 — Medium · 추정

- **근거**: 폴백은 모델 생성·워밍업에서 `Ort::Exception`만 잡는다(`ProcessorWorker.cpp:458-469, 493-501, 1360-1372`). 처리 중 GPU 오류(DirectML 장치 제거 등)는 파일 실패로 집계되고, 같은 세션을 재사용하므로 이후 파일도 연달아 실패한다(조용한 실패는 아님). macOS CoreML은 `MLComputeUnits=ALL`(`OrtAcceleration.cpp:77-80`)이라 Neural Engine의 FP16 경로를 탈 수 있다.
- **영향**: 임계값 근처의 작은·측면 얼굴은 FP16에서 점수가 달라져 검출이 빠질 수 있다. CPU와 가속기 결과가 같다는 검증이 없다.
- **개선안**: 처리 중 `Ort::Exception`이 나면 CPU 세션으로 교체하고 **그 파일을 다시 처리**. CI(macOS 러너)에서 `astronaut.png` 등으로 CPU와 CoreML 결과를 박스 IoU·점수 허용오차로 비교하는 패리티 테스트를 추가.

#### C-3. 썸네일 작업의 `QPersistentModelIndex`가 작업 스레드에서 파괴될 수 있다 — Low · 추정

- **근거**: `ThumbnailLoader.cpp:95-111` — GUI 스레드에서 만든 `QPersistentModelIndex`를 풀 작업 람다가 값으로 캡처하고, 다시 큐 람다로 복사한다. 큐 람다가 GUI에서 먼저 실행·파괴되면 마지막 참조가 풀 스레드의 람다 소멸 시 해제되어 모델 내부의 영속 인덱스 테이블을 다른 스레드에서 수정한다(Qt는 모델 인덱스를 모델 스레드에서만 쓰도록 요구).
- **영향**: 드물게 크래시·메모리 손상. 경쟁 창은 짧다.
- **개선안**: 작업에는 정수 키(항목별 일련번호)만 넘기고, GUI 스레드에서 키 → 항목을 조회한다.

#### C-4. 큰 사진의 작은 얼굴 — 제안 · 확인됨(구조)

- `Yolo5FaceDetector.cpp:192-212` — 한 번의 640×640 레터박스 추론. 6000×4000 사진은 약 0.107배로 줄어, 원본에서 폭 ~100 px 미만인 얼굴은 입력에서 ~10 px 이하가 된다. 단체 사진의 뒷줄 얼굴은 체계적으로 놓친다. "검출기가 못 찾은 것"이라 SECURITY.md 범위 밖이지만, 긴 변이 일정 이상이면 타일 검출(겹침 20%)을 추가하는 것이 가장 큰 커버리지 개선이다. 기존 PROJECT_REVIEW의 "분석 해상도·타일 검출" 제안과 같은 방향.

#### C-5. 10비트/HDR 판정과 오디오 분기 — 이상 없음 · 확인됨

- `VideoIo.cpp:669-674`의 `(9|10|12|14|16)(le|be)?$`는 허용 코덱(H.264/HEVC/VP8/VP9)이 낼 수 있는 고비트 `pix_fmt`(`yuv420p10le`, `p010le`, `yuv444p12le` …)를 모두 잡는다. HDR 전달 특성(PQ `smpte2084`, HLG `arib-std-b67`)도 거부한다(`675-679`).
- 오디오는 `aac/mp3/ac3/eac3/alac`만 복사, 나머지는 AAC 192k(`VideoIo.cpp:1146, 1212-1220`). PCM(카메라 MOV)·Opus·FLAC은 재인코딩된다. 보수적이고 올바르다.

#### C-6. 스레딩·취소·메모리 — 대체로 견고 · 확인됨

- 작업 스레드 ↔ UI는 시그널(큐)과 검토용 `BlockingQueuedConnection`만 쓰고, 종료 시 소멸자가 이벤트를 돌리며 기다려 교착을 피한다(`MainWindow.cpp:1206-1227`). 취소는 메모리 예약 대기·디코드·마스킹·인코딩·게시 가드까지 전파된다.
- 이미지 메모리 예약(`ProcessorWorker.cpp:142-195, 984-1006`), 영상 검출·트랙 데이터 한도(`VideoProcessor.cpp:560-594`, `Tracking.cpp:14-22`), 마스킹 배치 계획(`VideoProcessor.cpp:338-376`)으로 큰 입력에도 상한이 있다.
- 남은 성능 항목(소프트 에지 전역 mutex, Pass 1 직렬)은 open-work §1-4·6·7(기존 추적).

### D. 코드 품질과 구조

#### D-1. 개인정보 회귀 테스트의 빈칸 — Medium · 확인됨

현재 테스트는 경로 안전·원자 게시·원본 변경·추적·서명·FFmpeg 왕복에 강하다(CI가 실제 모델과 얼굴 샘플을 내려받아 양성 검출까지 확인: `.github/scripts/fetch_test_models.sh`, `test_core.cpp:2451-2499`). 그러나 A 항목을 직접 지키는 테스트가 비어 있다.

| 빈칸 | 지금 | 추가할 테스트 |
|---|---|---|
| 기본 경로(보존 OFF) 이미지에 메타데이터 없음 | 없음. `testMetadataFailurePublishesCleanImage`(`test_core.cpp:1873-1887`)는 읽을 수 있는지만 봄 | GPS·썸네일·XMP·ICC·PNG `tEXt/eXIf`·WebP `EXIF/XMP ` 청크가 있는 원본 → 워커 실행 → 출력 바이트에서 `Exif\0\0`, `http://ns.adobe.com/xap`, `ICC_PROFILE`, `tEXt`, `eXIf` 부재 |
| 영상 부가 스트림·스트림 태그 제거 | 전역 태그만(`test_video_io.cpp:43-46, 868-870`) | 자막(mov_text)·데이터(`-f data`)·첨부·커버아트·챕터·오디오 스트림 `title`/`handler_name` 태그를 넣은 샘플 → 출력 스트림이 v1+aN뿐, 태그 없음 |
| 검토 OFF 가지치기(A-2), 컷 경계(A-3) | 없음 | A-2·A-3의 테스트 아이디어 |
| 소스 스냅샷 위치(A-1) | 출력 루트에 있음을 **기대**하는 테스트 | 처리 중 출력 루트에 원본 해시 파일 없음 |
| 기본 로그에 경로 없음 | 파일 sink만(`test_logging.cpp`) | 콘솔 sink 포함 전체 sink 검사(A-4) |
| 마스크 강도 | 영역 변화만 확인(`test_core.cpp:1281-1305`) | 셀 수 상한·블러 후 원본과의 상관계수 상한 |

#### D-2. 정적·동적 분석 — 제안 · 확인됨

- 경고: MSVC `/W4 /sdl /guard:cf`, GCC/Clang `-Wall -Wextra`, 하드닝 플래그(`cmake/CloakFrameHarden.cmake`). `-Wconversion`, `-Wshadow`, `-Wold-style-cast`는 없다. 이 코드는 정수 폭 변환이 많아(영상 프레임·바이트 수) `-Wconversion`의 가치가 크다.
- clang-tidy는 `clang-analyzer-*, bugprone-*, performance-*, portability-*` 중심(`.clang-tidy`). `cppcoreguidelines-pro-bounds-*`, `cert-*`(특히 `cert-err33-c`, `cert-flp30-c`)를 단계적으로 추가할 만하다.
- **sanitizer·퍼저가 CI에 없다.** 손으로 쓴 바이너리 파서가 여럿(`OnnxGraphPatch`, TIFF/PNG/WebP 프레임 수 추정 `ImageIo.cpp:196-382`, ffprobe JSON 소비)이므로, Linux CI에 ASan+UBSan 잡 하나와 libFuzzer 타깃 2개(ONNX 패치, 프레임 수 추정)를 권한다.

#### D-3. 구조 — 제안

- `MainWindow.cpp`(2,873줄)가 UI 구성·설정 영속화·모델 준비·업데이트·작업 수명을 모두 가진다. 기존 PROJECT_REVIEW의 분리 제안(작업 제어, 설정 매핑)에 동의한다. 우선순위는 개인정보 판정과 맞닿은 **실행 시작 검증(모델 해시·동의·입출력 겹침, `1479-1700`)** 을 화면 없는 함수로 빼서 테스트하는 것이다(B-7 같은 결함이 그 자리에서 잡힌다).
- `FaceDetection`에 대상 종류(얼굴/번호판)가 없다(`FaceDetection.hpp:10-16`). 번호판에는 타원 마스크가 특히 맞지 않는데(문자가 상자 끝까지 차므로) 종류별 정책을 둘 수 없다. 기존 PROJECT_REVIEW 지적과 같다.
- RAII·소유권은 일관되다(`FileDescriptor`, `WindowsHandle`, `PosixDescriptor`, `StageDirectory`, `ImageMemoryReservation`). 예외 정책도 "파일 단위 catch → Failed"로 통일되어 있다.

#### D-4. 빌드·테스트·정적 분석 실행 결과 (Windows 11) — 확인됨

- **환경**: VS Build Tools 2026(MSVC 19.51), CMake 4.4.3, Ninja 1.13.2, Qt 6.11.2(CI는 6.10.3), OpenCV 5.0.0·ORT-DirectML 1.24.4(CI와 같은 SHA-256 고정본), vcpkg로 spdlog 1.17.0·libsodium 1.0.22·exiv2 0.28.9, FFmpeg 9.0.2(gyan.dev), LLVM 22.1.3(CI와 같은 메이저). 의존성·빌드 트리·로그는 모두 저장소 밖 세션 스크래치에 두었고 시스템에는 아무것도 설치하지 않았다.
- **빌드**: Debug(`BUILD_TESTING=ON`, `CLOAKFRAME_WARNINGS_AS_ERRORS=ON`, `CLOAKFRAME_SELF_UPDATE=ON`)와 CI를 따른 Release 모두 `/W4 /WX`에서 경고 0·오류 0. `cmake --install` 결과물도 CI의 배포 검사 조건을 만족했다.
- **ctest**: Debug·Release 모두 **17/17 통과.** 실제 YOLO5Face·YuNet·번호판 모델 추론과 FFmpeg 왕복(60프레임, HEVC, 회전, 영상 처리기)을 포함한다. `cloakframe_tests` 내부 건너뜀 4건: POSIX 권한 픽스처(Windows 미지원), 메모리 예산 초과 이미지(호스트 메모리가 커서 조건 불성립), SCRFD 그래프 패치 2건(모델 파일 없음, CI도 받지 않음).
- **clang-format**: 추적 중인 `*.cpp/*.hpp/*.mm` 100개 통과. `node --test` 4/4, 번역 검사 3개 언어 각 407개 통과.
- **clang-tidy: 실패** — 47개 위치(진단 55개), `.clang-tidy`의 `WarningsAsErrors: '*'`.

  | 묶음 | 내용 | 위치 |
  |---|---|---|
  | Windows 전용 코드 | `performance-no-int-to-ptr`(HANDLE 캐스트), `bugprone-misplaced-widening-cast` | `ImageIo.cpp:562, 912, 938, 1795`, `ModelStore.cpp:220, 263` |
  | 업데이터 | `bugprone-unchecked-optional-access`, `bugprone-branch-clone`(의도적) | `SelfUpdaterVelopack.cpp:80, 81, 107, 125, 209` |
  | 테스트 27곳 | `unchecked-optional-access` — UCRT의 `_wassert`가 noreturn이 아니라 `assert`가 가드로 인식되지 않음 | `test_custom_model_consent.cpp`, `test_tracking.cpp`, `test_video_io.cpp` |
  | 기타 | MSVC STL 헤더의 `EnumCastOutOfRange` 7곳(`std::filesystem` 경유), `bugprone-throwing-static-initialization` | `Mosaic.cpp:452-453` 등 |

  핵심은 결함 자체보다 **업데이터(보안 경로)와 Windows 전용 파일 게시 코드가 정적 분석의 사각지대**라는 점이다. CI tidy 잡은 macOS에서 `CLOAKFRAME_SELF_UPDATE=OFF`로만 돈다(`ci.yml:61-89`). 또 `.clang-tidy`의 `HeaderFilterRegex`(`^.*/(include|src|tests|tools)/`)는 `/`만 맞으므로 Windows에서는 프로젝트 헤더가 빠지고(구분자 무관 패턴으로 다시 돌려도 추가 발견은 없었다), `cloakframe_tidy`의 의존성 목록(`cmake/CloakFrameTools.cmake:134-147`)에서 테스트 타깃 5개(`custom_model_consent`, `model_store`, `stage_cleanup`, `update_cache`, `update_signature`)가 빠져 있다.
- **개선안**: Linux에서 `SELF_UPDATE=ON` tidy 잡 추가(또는 Windows tidy 잡), `HeaderFilterRegex`를 `[/\\]` 구분자로, 누락된 테스트 타깃을 의존성에 추가, 테스트의 optional 접근은 `assert` 대신 값 꺼내기 헬퍼로.

### E. UI/UX

> 키 처리(Enter/Esc/기본 버튼)는 Qt 6.11.2로 각 대화상자의 위젯·버튼 구성을 그대로 옮긴 임시 프로브를 offscreen으로 실행해 확인했다(저장소 밖 스크래치 디렉터리, 커밋하지 않음). 실제 앱 창을 띄운 검증은 아니다.

#### E-1. 영상 검토에서 목록·타임라인에 포커스가 있을 때 Enter를 누르면 바로 인코딩된다 — High · 확인됨

- **근거**
  - `VideoReviewDialog.cpp:839-841` — "Encode video"를 `QDialogButtonBox`의 `AcceptRole`로 추가하고 `setAutoDefault(false)`를 하지 않는다. `QDialogButtonBox`는 표시될 때 첫 Accept 버튼을 기본 버튼으로 만든다. 다른 버튼은 모두 `setAutoDefault(false)`(`656-657, 804`).
  - 트랙 목록·공백 목록(`QAbstractItemView`)과 타임라인(`QSlider`)은 Enter를 처리한 뒤 이벤트를 무시하므로 대화상자가 기본 버튼을 누른다. 트랙 목록은 Enter로 "해당 트랙으로 이동"하도록 연결돼 있어(`566-591`) 사용자가 Enter를 누를 이유가 충분하다.
  - 프로브: 트랙 목록·공백 목록·타임라인에서 Return → `encodeClicked=1`, 대화상자 닫힘, `result=1`.
- **영향**: 검토 도중 되돌릴 수 없이 인코딩된다. 공백이 없는 영상이면, 사용자가 놓친 얼굴에 수동 트랙을 추가하려던 참이어도 **"완료"** 로 끝날 수 있다. 사용자가 질문한 "실수로 확인 완료 처리될 위험"의 가장 직접적인 경로다.
- **개선안**: `encode->setAutoDefault(false); encode->setDefault(false);`로 기본 버튼을 없애고, 인코딩은 명시적 단축키(Ctrl+Enter)와 클릭으로만. 또는 `keyPressEvent`에서 포커스가 `QPushButton`이 아니면 Return/Enter를 소비.
- **테스트 아이디어**: `QTest::keyClick(trackList, Qt::Key_Return)` 후 `dialog.isVisible()`이고 `result()==0`, 타임라인이 트랙 첫 프레임으로 이동했는지. 공백 목록·타임라인도 같은 방식.

#### E-2. 이미지 검토에서 선택된 상자가 없을 때 Return을 누르면 "저장 후 다음"이 실행된다 — Medium · 확인됨

- **근거**: `ReviewDialog.cpp:387-394` — `focusedIndex_ >= 0`일 때만 Return을 처리하고 아니면 `break`로 빠져 이벤트가 무시된다. 대화상자의 기본 버튼은 `save->setDefault(true)`(`722`). 초깃값 `focusedIndex_ = -1`(`636`). 안내 문구(`686`, ko "클릭 또는 Return으로 상자 전환")는 Return이 상자를 토글한다고 말한다.
- **영향**: "이전 이미지"가 없으므로 그 실행에서는 다시 검토할 수 없다. 검출이 하나 이상 있으면 "Redacted"로 집계되어 경고도 남지 않는다.
- **개선안**: 캔버스가 Return/Enter를 항상 소비(선택이 없으면 첫 상자 선택), 저장은 명시적 단축키로 옮기고 안내에 표기.
- **테스트 아이디어**: 선택 없는 상태에서 `keyClick(canvas, Key_Return)` → 대화상자가 열려 있는지.

#### E-3. 추적 공백 확인과 일괄 포함/제외 — A-9 참고 (Medium · 확인됨)

#### E-4. 영상 검토의 Esc·닫기·"Cancel all"이 배치 전체를 버린다 — Medium · 기존 추적(open-work §1-9), 세부 신규

- HEAD에서도 `VideoReviewDialog.cpp:902-906`의 `reject()`가 `CancelAll`이다. 새로 확인한 점: "Cancel all" 버튼(`840-842`)은 이미지 검토(`ReviewDialog.cpp:733-753`)와 달리 **확인을 묻지 않는다.** "Add missed track"으로 그리는 중 Esc는 그리기 취소가 아니라 배치 취소다(`keyPressEvent` 재정의 없음). "Remove manual track"(`1258-1284`)은 확인·실행 취소가 없다.
- 개선: Esc는 먼저 그리기 모드 해제, 그 외에는 확인 대화상자. Cancel all·창 닫기에도 확인.

#### E-5. "완료" 표현과 경고 표시 — Medium/Low · 확인됨

- **좋은 점**: 어디에도 "모든 얼굴을 가렸습니다" 같은 과장 문구가 없다. 경고는 ⚠ 기호, 굵은 빨간 글씨(`Theme.cpp:348-351`), 모달 대화상자로 색에만 의존하지 않는다.

  | 위치 | 영어 원문 | 한국어 |
  |---|---|---|
  | 상태 표시 `MainWindow.cpp:1855` | Done | 완료 |
  | 상태 표시 `:1863-1864` | ⚠ Review required | ⚠ 검토 필요 |
  | 활동 로그 `:1859` | Completed with warnings — review the results before sharing. | 주의 사항과 함께 완료됨 — 공유하기 전에 결과를 검토하세요. |

- **Medium**: "완료"에 단서가 없다. `onWorkerFinished`(`1851-1857`)는 검토를 껐는지 보지 않으며, README의 "모든 개인정보가 검출·마스킹되었다는 보증은 아닙니다"(README.md:78)에 해당하는 문구가 앱 안에 없다. 영상이 포함된 실행이면 음성이 그대로라는 사실(A-8)도 빠진다. 제안: "완료 — 보고된 경고 없음", 검토 OFF였다면 "(검토하지 않음)", 오디오 포함 시 "음성은 가려지지 않음".
- **Low**: 경고 대화상자(`1866-1891`)는 0인 항목까지 14개 수치를 나열하고 "파일별 결과 열기" 버튼이 없다. 실패(출력 충돌 포함)는 상태 문구만 바뀌고 대화상자가 없다(`1898-1904`).
- **Low**: ko/ja가 "Dropped tracks: %10"을 "제외된 트랙"/"除外されたトラック"으로 옮겨(ko.ts:671, ja.ts:671), 같은 대화상자의 "%14 검토 중 제외한 트랙"과 구분이 안 된다. 결과 필터는 "자동 제외 트랙"(ko.ts:1473)으로 올바르다.

#### E-6. 파일별 결과의 "재검토/재처리"가 숨은 부작용을 가진다 — Medium · 확인됨

- **근거**: `MainWindow.cpp:1066-1084` — 확인 없이 `inputList_->clear()`로 사용자의 입력 목록 전체를 지우고, 출력 폴더를 `…/review-<시각>-<uuid>`로 덮어쓴 뒤 즉시 `startProcessing()`. 출력 폴더 값은 종료 시 `saveSettings()`(`2130`)로 저장되어 다음 실행에도 남고, 반복하면 `review-*/review-*`로 중첩된다.
- **개선안**: 부작용을 나열한 확인 대화상자, 재처리 후 원래 입력·출력 폴더 복원(또는 메인 창 상태를 건드리지 않는 별도 실행).

#### E-7. 첫 실행과 오프라인 — Medium · 확인됨 (일부 기존 추적 R8)

- **잘된 점**: 모델이 없으면 처리를 시작할 수 없고, 다운로드는 동의 → 진행률 → 해시 확인 순서이며 취소가 25 ms마다 반영된다(`MainWindow.cpp:1518-1534`, `ModelDownloader.cpp:27-58`, `ModelDownload.cpp:71-82`).
- **빠진 것**: 실패 원인이 사용자에게 가지 않는다. `ModelDownloadResult`에는 오류 문자열·HTTP 상태가 없고, 오프라인(호스트 없음) 같은 비일시적 오류는 1회 시도 후 "N회 시도 후 실패했습니다. 연결을 확인하세요"(`ModelDownloader.cpp:73-77`)로만 끝난다. DNS·TLS·프록시·403이 모두 같아 보인다. 수동 설치 경로(캐시 폴더·URL·SHA-256 안내)가 없는데, `firstExistingModelPath`(`ModelCatalog.cpp:29-50`)는 이미 손으로 넣은 파일을 받아들인다. 모델이 번들되지 않으므로 오프라인 첫 실행은 아무것도 처리할 수 없다.
- **Low**: 다운로드 동의 대화상자의 기본 버튼이 Yes(`ModelDownloader.cpp:155-159`). 모델 다운로드 질문이 입력·출력 검증보다 먼저 나온다(`MainWindow.cpp:1510-1571`).

#### E-8. GUI 스레드 해싱이 기록보다 넓다 — Medium · 기존 추적(open-work §1-8) 확장

- open-work은 `MainWindow.cpp:1454`만 적었지만, 커스텀 모델은 `CustomModelConsent.cpp:21, 53`의 `sha256HexOfFile`로도 GUI 스레드에서 해싱된다(`MainWindow.cpp:1336` 선택 시, `2712-2735` 시작할 때마다). 512 MB 모델이면 시작할 때마다 GUI 스레드에서 두 번 전체를 읽는다. 한 번만 작업 스레드에서 해싱해 동의 확인과 캐시 키에 같이 쓰면 B-7도 함께 닫힌다.

#### E-9. 기타 — Low · 확인됨

- 처리 중 창을 닫으면 묻지 않고 종료한다(`MainWindow.cpp:1935-1939`; 소멸자가 안전하게 취소하므로 데이터 문제는 없음).
- ffmpeg가 없으면 시작 시점이 아니라 영상마다 "FFmpeg was not found"로 실패한다(`ProcessorWorker.cpp:1297-1303`). 해결 방법 안내가 없다. 안전 한도 초과 메시지는 한도 값을 말하지 않는다(`VideoIo.cpp:640-660`).
- 진행 막대는 파일 단위라 긴 영상 하나를 처리하는 동안 0%에 머문다(퍼센트는 상태 문구에만, `ProcessorWorker.cpp:1407-1434`).
- 이미지 검토에서 Undo/Redo 버튼을 누르면 포커스가 버튼에 남아 Space(미리보기)가 다시 Undo를 실행한다(Fusion 스타일, `main.cpp:90`).
- 결과 창 표에서 Enter는 자동 기본 버튼 "입력 파일 열기"를 눌러 **가리지 않은 원본**을 연다.
- 취소 반응성은 좋다. 영상 루프 곳곳에서 취소를 확인하고(`VideoProcessor.cpp:551, 822, 864, 911, 1001`), 블로킹 `QProcess` 대기는 모두 작업 스레드에 있다.

#### E-10. 접근성 — Medium · 확인됨 (방향은 기존 추적)

- **키보드로 영역을 추가할 방법이 없다.** 이미지 검토의 추가는 마우스 드래그뿐이고(`ReviewDialog.cpp:273-361`) 확대·이동은 휠·오른쪽 드래그뿐이다(`233-271`). 영상 캔버스는 포커스 정책과 키 처리가 없다(`VideoReviewDialog.cpp:75-362`). 보조기술에는 `setAccessibleName("Review image")` 하나만 노출되고, 영상 캔버스·타임라인에는 접근성 이름이 없다(`497, 787-788`).
- **대비(WCAG, `Theme.cpp` 색상값으로 계산)**: 밝은 테마 placeholder `#9CA3AF` on `#FFFFFF` = **2.54:1**(입력 목록 빈 상태 안내 `MainWindow.cpp:316`, 검토 화면의 "i / N"), 어두운 테마 `#6E7681` on `#0D1117` = **4.12:1**, 타임라인 표식(호박색 2.02, 하늘색 1.70, 회색 2.39 : 비텍스트 3:1 기준 미달, `VideoReviewDialog.cpp:413, 427`). 본문(16.7:1)·안내(4.83:1)·경고(4.83:1)는 통과.
- **색으로만 구분**: 타임라인에 범례가 없고 포함/제외 트랙이 호박색/회색으로만 다르다. 이미지 검토의 자동(호박)/수동(파랑) 상자도 색으로만 구분된다.
- **포커스 순서**: `recursiveCheck_`가 `reviewCheck_`보다 먼저 생성되지만 배치는 뒤라 Tab이 오른쪽→왼쪽으로 간다(`MainWindow.cpp:600, 607, 632-633`).

#### E-11. 다국어 — Medium/Low · 확인됨

- `python scripts/check_translations.py --strict-source-equality` → ko/ja/zh_CN 각 407개 OK, unfinished 0, 빈 번역 0, 자리표시자·복수형 불일치 0. 소스와 카탈로그 동기화 상태 양호.
- **Medium — 업데이트 거부 메시지 4개가 ko/ja/zh에서 영어로 나간다.** `SelfUpdaterVelopack.cpp:197, 216, 237, 285`의 문자열이 `.ts`에서 `type="vanished"`(ko.ts:1880-1898)다. `qt_add_translations`가 `SOURCE_TARGETS`만 스캔하는데(`src/CMakeLists.txt:171-179`) Velopack 소스는 조건부로만 대상에 들어가므로(`126-127`) macOS에서 lupdate를 돌리면 사라짐으로 표시되고, lrelease는 vanished를 버린다. 이 중 둘은 Linux에서 "Update Failed" 대화상자에 그대로 뜬다. 검사 스크립트는 vanished를 보지 않아 놓친다. → updater 소스를 `qt_add_translations`의 `SOURCES`에 명시하고, 검사기가 "소스에 존재하는 vanished"를 실패로 처리.
- **Medium — 모델 로더 예외 문구가 영어로 활동 로그에 들어간다**: `tr("Error: %1").arg(e.what())`(`ProcessorWorker.cpp:822`)로 `Yolo5FaceDetector.cpp`, `YuNetFaceDetector.cpp`, `PlateDetector.cpp`, `ScrfdFaceDetector.cpp`의 영어 메시지가 그대로 노출된다.
- **Low**: 경과 시간 `"%1m %2s"` 미번역·연결 조립(`MainWindow.cpp:1843-1845, 1855`), "%1 attempt(s)"는 `%n` 복수형이 아님(`ModelDownloader.cpp:74-75`), 일·중 문장 연결에 ASCII 공백(`VideoReviewDialog.cpp:479-491`), `"no update is pending"` 미번역(`SelfUpdaterVelopack.cpp:88`). 고정 폭 위젯에 의한 잘림은 찾지 못했다.

### F. 문서와 라이선스

> 라이선스 항목은 엔지니어링 관점의 지적이며 법률 자문이 아니다.

#### F-0. Linux에 번들한 FFmpeg가 재배포 불가(nonfree) 빌드다 — High(라이선스) · 확인됨

- **근거**: `release.yml:33-35, 652-663`이 martin-riedl.de의 `1783011670_8.1.2` 빌드를 고정 해시로 내려받아 AppImage에 넣는다. 고정 해시(`LINUX_FFPROBE_SHA256 = c6f2d36e…`)와 일치하는 `ffprobe.zip`을 직접 받아 바이너리의 구성 문자열을 확인했다: `--enable-gpl --enable-version3 --enable-nonfree --enable-decklink`. 같은 빌드의 `ffmpeg`도 동일하다는 것은 보조 검토가 배포 AppImage에서 확인했다. 반면 `THIRD_PARTY_NOTICES.txt:19-36`은 GPL-2.0-or-later 빌드라고 적는다.
- **영향**: FFmpeg는 `--enable-nonfree`로 만든 바이너리를 재배포할 수 없다고 명시한다. 현재 Linux AppImage 배포 자체가 라이선스 위반 소지가 있다.
- **개선안**: nonfree 없는 GPL 빌드(예: BtbN의 `gpl` 변형, 또는 필요한 디코더·libx264/libx265만 넣은 최소 자체 빌드)로 교체하고, CI에서 번들 FFmpeg의 구성 문자열에 `--enable-nonfree`가 있으면 실패시킨다(세 플랫폼 공통 검사).

#### F-1. libsodium 고지 누락 — Medium · 확인됨

- `THIRD_PARTY_NOTICES.txt`에 "sodium"이 한 번도 나오지 않고 `docs/MODELS.md:47-52`의 의존성 목록에도 없다. 그러나 libsodium은 필수 의존성이고(`cmake/CloakFrameSodium.cmake`, 없으면 FATAL_ERROR) 업데이트 서명 검증에 링크되며, Windows 릴리스는 vcpkg `x64-windows-static-md`로 **정적 링크**한다. ISC 라이선스는 모든 사본에 저작권·허가 고지를 요구한다. `CONTRIBUTING.md:73-76`의 규칙과도 어긋나고, `SECURITY.md:54`는 libsodium을 언급해 문서끼리도 맞지 않는다.
- 개선: NOTICES에 libsodium(ISC) 절 추가. CI에서 링크된 라이브러리 목록과 NOTICES 제목을 대조하는 검사.

#### F-2. 고지의 정확성과 대응 소스 — Medium · 확인됨(일부 보조 검토)

- **FFmpeg 라이선스 표기**: 세 플랫폼의 번들 FFmpeg는 `--enable-version3`로 빌드되어 **GPLv3**다(Linux는 직접 확인, Windows·macOS는 보조 검토가 배포본에서 확인). NOTICES(`:19, :26-28`)는 GPL-2.0-or-later로, `docs/MODELS.md:50`은 "LGPL… 선택적 GPL"로 적어 서로도 다르고 실제와도 다르다.
- **대응 소스**: NOTICES는 ffmpeg.org 다운로드 페이지를 가리킬 뿐, 서드파티 정적 빌드에 들어간 x264·x265·aom·svt-av1·SRT(MPL-2.0)·gnutls 등의 정확한 버전과 소스를 제공하지 않고 서면 제공 약속도 없다. Exiv2도 업스트림 저장소만 가리킨다. 정확한 소스 아카이브와 빌드 스크립트를 각 릴리스 자산으로 함께 올리거나 3년 서면 제공을 추가한다.
- **빠진 고지(보조 검토, 배포본 내용 기준)**: libsodium(F-1) 외에 macOS 번들의 OpenVINO(Apache-2.0, NOTICE 동반)·JasPer·lcms2·pugixml·libmng, **GPL-3.0 전용인 Qt Virtual Keyboard 플러그인**; Windows 번들의 Mesa `opengl32sw.dll`, `dxcompiler.dll`/`dxil.dll`, `d3dcompiler_47.dll`. Windows 배포본에는 재배포 대상이 아닌 OS 스텁 `icuuc.dll`도 들어 있다.
- **기타**: NOTICES는 Exiv2를 "동적 링크"라고 적지만(`THIRD_PARTY_NOTICES.txt:5636-5637`) Windows는 정적 triplet이다. NOTICES 머리말(`:9-16`)은 번호판 모델 다운로드를 빠뜨렸다.
- **개선안**: 플랫폼별로 실제 번들 내용에서 고지를 생성하고, CI에서 "번들된 모든 라이브러리 ↔ NOTICES 항목" 대응을 검사한다. Windows 배포에서 `icuuc.dll`을 제외한다. NOTICES·LICENSE가 세 플랫폼 산출물 모두에 들어가는 것은 이미 확인됐다(`cmake/CloakFramePackaging.cmake:32-40`).

#### F-3. 앱 안의 모델 라이선스 고지 — Low~Medium · 확인됨

- **좋은 점**: YOLO5Face의 비상업 조건은 다운로드 동의 대화상자에 나온다(`ModelDownloader.cpp:126-133`, ko "비상업적 용도로만").
- **빈칸**: 파일이 없을 때 한 번만 보인다. 기본 선택이 YOLO5Face(`MainWindow.cpp:2627, 2632`)인데 콤보 라벨 "Accurate · YOLO5Face-n"에는 단서가 없다. Redactly 폴더·앱 폴더에서 발견된 모델(`ModelCatalog.cpp:32-39`)로 시작한 사용자는 고지를 한 번도 보지 못한다. 앱 안에 정보/라이선스 대화상자가 없다.
- LGPL-3 §4(c)는 앱이 실행 중 저작권 고지를 보여 줄 때만 적용되므로 현재 위반은 아니다. LICENSE·NOTICES는 모든 패키지에 설치된다(`cmake/CloakFramePackaging.cmake:32-40`). 그래도 "정보" 대화상자(GPL 고지, NOTICES, 모델 라이선스 표)와 모델 콤보 툴팁을 권한다.

#### F-4. SECURITY.md와 실제 운영 — 일치 · 확인됨

- 비공개 취약점 제보 링크(`nyabi-gh/CloakFrame/security/advisories/new`)는 원격 저장소와 일치하고, GitHub API에서 private vulnerability reporting이 `enabled: true`로 확인됐다. `CONTRIBUTING.md#safety-and-compatibility` 앵커가 존재하고(`CONTRIBUTING.md:48`), 지원 버전 정책(최신만)과 exiv2/libsodium/Sparkle/Velopack 언급도 빌드 구성과 맞다.
- 다만 "범위 안" 목록과 현재 코드의 차이는 3절 대조표 참고(A-1, A-2, A-3). "signed release"(`SECURITY.md:63`)는 Windows Authenticode로 읽힐 수 있으나 설치본에는 Authenticode가 없다(open-work §3, 기존 추적).

#### F-5. README 4개 언어판 불일치 — Medium/Low · 확인됨

- 요구 사항, 자산 이름, 지원 형식·코덱, GPU 표, 네트워크 요청 목록, 라이선스 사실, 링크는 네 언어 모두 일치한다(모두 `9ebd0ed`에서 마지막 수정).
- **Medium**: README.zh.md(21, 59, 61, 126행)는 기능 이름을 `保存前审阅`라 부르지만 실제 UI 라벨은 `保存前检查`(zh_CN.ts:318)다. 사용자가 설정을 찾지 못한다.
- **Low**: zh가 "gap"을 `空白`과 `空缺`으로 섞어 씀(zh.md:68-69 vs 78-80). ja는 로그 문장에서 "진단 정보만"을 뺐고(ja.md:90-91) ja/zh는 "로그 설정" 없이 "즉시 적용"만 적음(ja.md:91, zh.md:84). en/ja는 라이선스 절에서 "처음 사용할 때 다운로드"를 뺌(en.md:158, ja.md:154). 저작권 줄 표기가 언어마다 다름. 업데이트 확인 대상이 GitHub라는 점은 README.en.md:130에만 있음. 한국어 "추적 공백 전체를 검토한 뒤"(README.md:84)는 "각 공백"이 아니라 "모든 공백"으로 읽힐 수 있음.

---

## 5. 잘된 점

- **기본 출력의 메타데이터 위생**: 이미지를 픽셀에서 재인코딩해 EXIF/GPS/썸네일/XMP/IPTC/ICC/MPF가 기본적으로 따라가지 않는다. 보존 옵션도 썸네일·프리뷰·MakerNote·SubIFD·XMP·IPTC·ICC를 제거하고 방향을 1로 정규화한다(`ImageIo.cpp:1998-2075`).
- **영상 스트림 화이트리스트**: `-map`을 명시해 영상 1개 + 오디오만 내보내고 전역 메타데이터·챕터를 지운다. `-xerror`로 손상 스트림이 짧은 결과로 조용히 게시되는 것을 막는다(`VideoIo.cpp:743-746`).
- **원자적·비덮어쓰기 게시**: 플랫폼별 no-replace 원시 연산과 디렉터리 핸들 고정, FAT 폴백에서도 완성처럼 보이는 부분 파일을 남기지 않는 `.cloakframe-partial` 설계.
- **원본 변경 감지**: 스냅샷 + 파일 identity(장치·inode·크기·mtime·ctime) 비교를 단계마다 수행하고 게시 직전에도 확인한다.
- **fail-closed 집계**: `omitted`(안전 상한 초과 검출), 버린 트랙, 미확인 공백, 스캔 실패가 모두 판정에 들어가며, 검토를 약속했는데 못 띄우면 저장하지 않는다.
- **보수적 보간**: 움직임·크기 점프가 큰 공백은 선형 보간 대신 양 끝 박스의 합집합으로 덮고, 평활화는 원 박스를 줄이지 않는다(`Tracking.cpp:565-584, 650-665`).
- **모델 무결성**: 고정 SHA-256, 크기 상한, 메모리로 읽은 바이트를 해시하고 그 바이트로 세션 생성(TOCTOU 없음), 실행 전마다 재검증.
- **네트워크 위생**: 요청 3종 모두 HTTPS + `NoLessSafeRedirectPolicy`, 릴리스 노트는 `PlainText`, 업데이트 URL은 `https://github.com`만 신뢰(`UpdateChecker.cpp:93-100`).
- **공급망 기본기**: 모든 GitHub Action을 커밋 SHA로 고정했고 ffmpeg·OpenCV·ORT·DirectML 다운로드를 SHA-256으로 검증한다.
- **문서 문화**: `open-work.md`/`PROJECT_REVIEW.ko.md`가 근거의 "읽는 쪽까지 확인" 규칙(§6)을 스스로 두고, 틀렸던 판정을 기록한다. 이 보고서도 그 규칙을 따랐다.
- **업데이트 서명 코드**: libsodium Ed25519, 잘못된 입력은 fail-closed, 다운로드 **전에** 서명 확인, 태그 릴리스는 키 없이 나갈 수 없음(`release.yml:103-109`), 서명 스크립트가 키 쌍 일치와 서명 결과를 되읽어 검증(`sign_update_packages.sh:30-36, 51-52`). v1.11.3의 실제 서명도 검증된다(보조 검토).
- **워크플로 권한**: 최상위 토큰 `contents: read`, 쓰기는 게시 잡에만, `pull_request_target`·신뢰할 수 없는 값의 `run:` 보간 없음, 릴리스 잡은 캐시를 쓰지 않음.
- **배포 바이너리 하드닝**: Windows ASLR·NX·CFG·/GS, Linux PIE·full RELRO·NX 스택·SSP, macOS 전체 Mach-O hardened runtime과 최소 entitlements(`allow-jit`, `user-selected.read-write`), 서명·공증·스테이플된 DMG(보조 검토).
- **빌드 위생**: Windows에서 `/W4 /WX` 경고 0, 테스트 17/17, clang-format 무결(D-4). CI가 실제 모델과 얼굴 샘플로 양성 검출까지 확인한다.
- **번역 품질**: 407개 메시지 × 3개 언어에 미번역·자리표시자 불일치가 없고, 원문과 같은 번역을 금지하는 검사가 CTest에 들어 있다.

---

## 6. 개선 로드맵

난이도: ★ 작음(하루 이내) · ★★ 중간(수일) · ★★★ 큼(설계 변경)

### 지금 당장 (다음 패치)

| 항목 | 난이도 | 비고 |
|---|---|---|
| B-1 서명·공증 시크릿 8개를 `release` 환경으로 이동, BUILDING.md 수정 | ★ | 저장소 설정 작업, 코드 변경 없음 |
| F-0 Linux FFmpeg를 nonfree 없는 빌드로 교체 + 구성 문자열 CI 검사 | ★ | |
| A-1 영상 소스 스냅샷을 사용자 전용 캐시로 이동 (+ 출력 루트에 원본 없음 테스트) | ★ | 인코딩 스테이징은 그대로 |
| E-1·E-2 검토 대화상자의 Enter/Return 기본 버튼 제거 | ★ | 키 입력 회귀 테스트 동반 |
| A-2 가지치기 프레임 보고(또는 약한 박스 유지) + 테스트 | ★★ | `TrackCoverageReport` 확장, 결과 화면 연결 |
| B-2 서명 거부 시 레거시 안내 폴백 중단 | ★ | |
| A-4 콘솔 sink를 상세 로그와 연동 | ★ | |
| A-9 일괄 포함/제외에서 체크 초기화 | ★ | 무방문 체크 차단은 다음 릴리스 |
| B-7 커스텀 모델 승인 해시와 로드 해시 대조 | ★ | 한 줄 |
| F-1 libsodium 고지, E-11 vanished 업데이트 문자열 복구 | ★ | |
| D-1 기본 경로 메타데이터 부재 회귀 테스트 | ★ | |

### 다음 릴리스

| 항목 | 난이도 | 비고 |
|---|---|---|
| A-3 컷 경계 공백 보고 | ★★ | 공간 연속성 기준 |
| A-6 메타데이터 허용 목록 + GPS 분리 | ★ | 라벨·툴팁·번역 동반 |
| A-8 오디오 제거 옵션과 결과 고지 | ★ | |
| A-9 공백 방문 후에만 체크 가능 | ★★ | |
| A-5, A-10, A-12 흔적·안내 정리 | ★ | |
| B-6 Sparkle 자동 확인을 앱 설정과 동기화 | ★ | |
| C-2 실행 중 GPU 오류 시 CPU 재시도 + 가속기 패리티 테스트 | ★★ | |
| C-3 썸네일 인덱스 처리 | ★ | |
| D-1 영상 부가 스트림·태그 제거 테스트 | ★ | |
| D-2 ASan/UBSan CI 잡 | ★★ | |
| B-3 버전·채널·파일명을 서명 대상에 포함 | ★★ | 서버·클라이언트 동시 전환 |
| B-4 서명 전용 잡 분리, vcpkg 기준선 고정 | ★★ | |
| B-5 AppImage RUNPATH·번들 정리와 실제 기동 스모크 테스트 | ★★ | |
| D-4 업데이터·Windows 코드를 tidy 대상에 포함 | ★ | |
| E-4 영상 검토 Esc·Cancel all 확인, E-6 재처리 부작용 정리 | ★ | |
| E-5 "완료" 문구에 단서(검토 OFF, 음성 미처리) | ★ | 번역 동반 |
| E-7 다운로드 실패 원인 표시와 수동 설치 안내 | ★ | |
| E-8 모델 해싱을 작업 스레드로(B-7과 함께) | ★ | |
| F-2 플랫폼별 고지 자동 생성, FFmpeg 대응 소스 첨부 | ★★ | |
| F-5 README.zh 기능명 등 언어판 정리 | ★ | |
| B-11 Low 항목(액션 재고정, draft 게시, `-protocol_whitelist`, `/CETCOMPAT` 등) | ★ | |

### 장기

| 항목 | 난이도 | 비고 |
|---|---|---|
| A-7 마스킹 강도 재설계(트랙 고정 격자, 강함 기본값, 방식별 한계 고지) | ★★ | 측정 기반 |
| C-4 큰 사진 타일 검출 | ★★ | 정확도·속도 평가 동반 |
| 커스텀 ONNX 파서 프로세스 격리 (open-work §2) | ★★★ | 기존 추적 |
| D-2 퍼저 + `-Wconversion` 단계 도입 | ★★ | |
| D-3 실행 시작 검증 로직 분리 | ★★ | |
| B-1 업데이트 서명을 CI 밖(하드웨어 키)으로, 키 교체 과도기 릴리스 | ★★★ | |
| E-10 키보드만으로 영역 추가·편집, 캔버스 접근성 | ★★★ | |

---

## 7. 검토하지 못한 부분과 이유

| 항목 | 이유 |
|---|---|
| macOS·Linux에서의 빌드·테스트·clang-tidy | 이 리뷰 환경이 Windows 11뿐이다. macOS tidy 잡에서 D-4의 테스트·Mosaic 진단이 재현되는지는 모른다. Qt도 CI(6.10.3)와 다른 6.11.2를 썼다 |
| 실제 GUI 조작·렌더링·스크린리더 | 앱 창을 손으로 띄워 조작하지 않았다. E-1·E-2 등 키 처리는 같은 위젯 구성을 옮긴 Qt 프로브로만 확인했다 |
| GPU 추론(DirectML·CoreML·CUDA) 결과 | 테스트가 어떤 실행 공급자를 썼는지, CPU와 결과가 같은지 측정하지 않았다(C-2) |
| 검출 정확도·마스킹의 실제 재식별 내성 | 측정 데이터셋과 공격 실험이 없다. A-7은 문헌에 근거한 추정이다 |
| 동기화 클라이언트 업로드(A-1)·journald 기록(A-4) | 사용자 환경에 좌우되며 재현하지 않았다. 코드 경로만 확인했다 |
| Velopack·Sparkle 업데이트 실제 적용 흐름 | 로컬 빌드에 공개키를 넣지 않아 실행하지 않았다. v1.11.3 서명의 유효성은 보조 검토가 배포 산출물로 확인했다 |
| 배포 바이너리 내부(B-5 RUNPATH, B-11의 macOS 플러그인·dylib rpath, F-2의 번들 구성요소, Windows·macOS FFmpeg 구성) | 보조 검토가 v1.11.3 산출물을 열어 확인한 결과다. 직접 다시 확인한 것은 Linux `ffprobe`의 구성 문자열(F-0)뿐이다 |
| 번들 구성요소의 알려진 취약점(CVE) 상태 | FFmpeg 8.1.2, ORT 1.24.4/1.29.0, Qt 6.10/6.11, exiv2 0.28.9, libsodium 1.0.22를 보안 공지 데이터베이스와 대조하지 않았다 |
| SCRFD 그래프 패치 테스트 2건, POSIX 권한 테스트 | 모델 파일이 없고(CI도 받지 않음), POSIX 권한 픽스처는 Windows에서 건너뛴다 |
| `tools/inspect_model.cpp` | 배포물에 들어가지 않는 개발 도구라 깊게 보지 않았다 |
| 라이선스의 법적 판단 | F 절은 엔지니어링 관점의 지적이며 법률 자문이 아니다 |

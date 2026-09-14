# 남은 작업 — 감사 계열

마지막 확인 revision: `9ebd0ed` (`main`), 2026-09-14.

**이 문서는 남은 작업 목록의 절반이다.** 감사(`CF-xxx`) 계열 미결 항목, 릴리스
엔지니어링, 성능을 다룬다. 나머지 절반인 UX와 결과 신뢰성은
[PROJECT_REVIEW.ko.md](PROJECT_REVIEW.ko.md)의 `R1`~`R8` 트랙과 개선 진행 기록이
관리한다. 한 항목을 두 문서에 같이 적지 않는다.

상태가 바뀌면 여기서 갱신한다. 고치지 않기로 한 항목은 표에서 지우지 말고 §5에
이유와 함께 남긴다.

근거를 적는 규칙은 §6에 있다. 요약하면, 자료구조나 필드가 생겼다는 사실은 근거가
아니다. 그 값을 읽어 사용자에게 도달시키는 caller까지 확인하고 적는다.

## 0. 이 목록의 유래

원본은 `docs/cloakframe-audit-2026-08-06.md` §0이었고, 감사 finding
`CF-001`~`CF-026`의 상태를 관리했다. 그 문서는 `ef49104`(2026-08-20)에서 세션
기록과 함께 삭제되었다. 감사 본문(재현 절차, 기각한 접근, 측정값)은 히스토리에
그대로 있으므로 필요하면
`git show ef49104^:docs/cloakframe-audit-2026-08-06.md`로 읽는다.

여기 실린 항목은 삭제 시점의 목록을 옮긴 것이 아니라 `9ebd0ed` 트리에서 하나씩
다시 확인한 결과다. 줄 번호도 그 기준이다. 그 사이에 닫힌 것은 §7에 적었다.

## 1. 우선순위

| 순위 | 항목 | 위치 | 비용 |
|---:|---|---|---|
| 1 | `CF-001` 서명이 실제 릴리스에서 동작했는지 확인 | 릴리스 로그와 asset | 거의 없음 |
| 2 | Apple 서명 secret 7개를 `release` 환경으로 | 저장소 설정 | 작음 |
| 3 | `CF-021` 출력 충돌 판정과 입력 dedupe의 OS 가정 | `OutputPlan.cpp:18`, `ImageScanner.cpp:96` | 작음 |
| 4 | soft-mask 전역 직렬화 | `Mosaic.cpp:451` | 작음 |
| 5 | 나머지 16개 테스트에 timeout 없음 | `tests/CMakeLists.txt` | 한 줄 |
| — | *커스텀 ONNX parser의 process 격리* | §2 | 큼 |
| 6 | pass 1 scene-cut 워커 분리 | `VideoProcessor.cpp:557` | 중간 |
| 7 | pass 1 검출 병렬화 | `VideoProcessor.cpp:566` | 중간 |
| 8 | 모델 SHA-256이 GUI 스레드에서 계산된다 | `MainWindow.cpp:1454` | 작음 |
| 9 | 영상 검토의 Esc가 배치 전체를 버린다 | `VideoReviewDialog.cpp:902` | 중간 |
| 10 | YOLO5Face 점수가 objectness만 쓴다 | `Yolo5FaceDetector.cpp:268` | 작음 |
| 11 | `CF-025` release 의존성 미고정, SBOM 없음 | `release.yml:130` | 중간 |
| 12 | app 번들이 staple되지 않는다 | `release.yml:325` | 작음 |
| 13 | 기타 Low | §4 | 작음 |

### 1. `CF-001` 서명 경로의 실제 동작 확인

감사 문서가 이 finding을 닫으면서 남긴 조건이다. 서명 경로는 local round trip과
CI build까지 확인했지만, **실제 release에서 동작하는 것은 다음 `v*` tag가
처음**이라고 적혀 있었다. 그 뒤로 `v1.11.0`과 `v1.11.1`이 나갔으므로 지금 확인할
수 있다.

release job 로그에 `signed <package> <digest>`가 찍혔는지, release asset에
`.nupkg.sig`가 올라왔는지, macOS appcast에 `edSignature=`가 실렸는지 셋을 본다.
비용이 거의 없는데 확인하기 전까지는 업데이트 신뢰 체계 전체가 미검증이므로 가장
먼저 둔다.

### 2. Apple 서명 secret을 `release` 환경으로

`CF-007`의 남은 절반이다. 코드 쪽은 `9699d83`이 서명 키를 읽는 네 job에
`environment: release`를 붙여 끝냈다(`release.yml:47,117,384,588`). 남은 것은
Apple 서명 secret 7개가 아직 repository scope라는 점이다. 그대로면 어느
workflow의 어느 job이든 읽을 수 있다.

`gh secret list --env release`가 비어 있는지로 확인한다. 저장소 설정 작업이므로
코드 변경은 없다.

### 3. `CF-021` — 출력 충돌 판정이 OS 가정에 묶여 있다

`OutputPlan.cpp:18`의 `destinationKey`가 `#if defined(_WIN32) || defined(__APPLE__)`
로 대소문자를 접을지 정한다. 이것은 컴파일 대상 OS이지 실제 파일시스템 특성이
아니다. `ImageScanner.cpp:96`의 `markVisited`도 같은 문제의 다른 절반으로,
canonical 경로 바이트를 그대로 키로 쓰므로 대소문자를 구분하지 않는 볼륨에서 같은
입력을 두 번 센다. 근본 원인이 하나이므로 함께 고친다.

`R1`이 고친 것은 권한 거부 경로의 보고이지 이 키 계산이 아니다. 두 파일 모두
2026-09-05 이후 이 부분은 바뀌지 않았다.

빗나가는 방향이 둘이다. 대소문자를 구분하는 macOS 볼륨에서는 서로 다른 두 출력을
충돌로 보고 정당한 실행을 거부한다. Linux에 마운트한 exFAT/NTFS/CIFS에서는 같은
파일이 될 두 출력을 충돌로 보지 않는다.

**심각도를 정확히 적는다.** 두 번째 경우에도 조용한 덮어쓰기는 일어나지 않는다.
게시 경로가 전부 no-replace이기 때문이다(`imwriteUnicodeNoReplaceAtRoot`,
`copyFileNoReplaceAtRoot`, `moveFileNoReplaceAtRoot`). 실제로 깨지는 것은 README와
`findOutputConflicts`가 약속한 **"시작 전에 막는다"**는 보장이다. 실행이 시작되고
파일 몇 개를 처리한 뒤 중간에 실패한다. fail-closed는 유지되므로 Critical이 아니라
Medium이고, 그래서 §2의 큰 건보다 앞에 두되 1·2번 뒤에 둔다.

### 4. soft-mask 계산의 전역 직렬화

`Mosaic.cpp:451`의 `g_maskComputationMutex`가 마스크 계산을 전 스레드에 걸쳐
직렬화한다(`:485`에서 획득). `softEdges`가 켜진 실행에만 영향을 준다(`:692`).

이 항목의 우선순위가 올라간 이유는 결함 자체가 아니라 2026-08-24 작업이다. 이미지
배치가 `processOrdered`로 병렬 처리되고 검출기가 동시 호출을 견디게 된
(`27b6b29`) 지금, 이 mutex는 넣어 둔 병렬화를 soft edge 실행에서 되돌린다. 성능
항목이 아니라 회귀로 다룬다.

캐시 조회(`g_maskCacheMutex`)와 계산을 한 lock으로 묶지 말고, 계산을 lock 밖에서
하고 삽입만 잠그거나 항목별로 잠그는 쪽이 맞다.

### 5. 나머지 16개 테스트에 timeout이 없다

`tests/CMakeLists.txt`에 `add_cloakframe_test`가 17개 있고 `TIMEOUT`은 한
곳(`cloakframe_worker_video_review_tests`, `TIMEOUT 90`)에만 붙어 있다. 나머지
16개는 멈추면 CI job 전체 한도까지 간다. 실제 FFmpeg 프로세스를 띄우는
`cloakframe_video_io_tests`가 특히 그렇다.

`add_cloakframe_test` 함수 안에서 기본 `TIMEOUT`을 주고 필요한 곳만 덮어쓰면
한 번에 끝난다.

### 6. pass 1 scene-cut 워커 분리

`VideoProcessor.cpp:557`의 `cutDetector.push(frame)`가 `:566`의 `detect(frame)`
앞에서 직렬로 돈다. 감사 시점 측정으로 110초 실행 중 8.6초, 약 7.8%다.

**옛 메모의 제약 조건은 더 이상 맞지 않는다.** 삭제된 문서는 "reader가 `cv::Mat`
하나를 재사용하므로 매 submit을 다음 `readFrame` 전에 기다려야 한다"고 적었다.
`bb2cf21` 이후 pass 1은 `PrefetchingVideoFrameReader`를 쓰고, 프레임 소유권이
`recycle()`(`:597`)까지 호출자에게 있다. 워커가 프레임을 들고 있어도 되며,
`recycle()`을 워커 완료 뒤로 옮기기만 하면 된다. 착수 비용이 기록된 것보다 낮다.

### 7. pass 1 검출 병렬화

같은 루프의 `detect(frame)`가 프레임당 한 번, 한 스레드에서 돈다. 감사 시점
측정으로 preprocess 14.3초 + inference 38.9초 + postprocess 2.4초.

**여기서도 접근이 바뀌었다.** 삭제된 문서는 `Detector`를 prepare/infer 두 단계로
쪼개야 한다고 적었다. `27b6b29`가 `detect()`를 동시 호출 가능하게 만들면서
(`Detector.hpp`: 준비와 디코딩은 호출자 스레드, 추론 상태 접근은 구현 내부에서
동기화) 인터페이스를 쪼개지 않고도 여러 스레드에서 부르면 같은 겹침이 나온다.
프레임 순서는 `OrderedParallel.hpp`의 `processOrdered`가 이미 보장하며, 이미지
배치가 그렇게 쓰고 있다.

즉 남은 일은 pass 1 루프를 `processOrdered`로 바꾸는 것이고, 검출 메모리 예산
검사(`:572`~`:594`)와 progress 보고가 consume 쪽으로 가야 한다.

frame striding은 기각된 상태를 유지한다. 어떤 영역이 가려지는지를 바꾸기 때문에
(stride보다 짧게 등장하는 얼굴을 통째로 놓칠 수 있다) 기본값이 될 수 없고, 명시적
opt-in 뒤에만 둘 수 있다.

### 8. 모델 SHA-256이 GUI 스레드에서 계산된다

`MainWindow.cpp:1454`가 `QCryptographicHash`로 모델 파일을 GUI 스레드에서 해싱한다.

원래 이 항목은 썸네일 추출의 동기 `waitForStarted`/`waitForFinished`와 한 쌍이었다.
그쪽 절반은 `R5`가 `ThumbnailLoader`로 닫았고, `MainWindow.cpp`에 blocking wait는
남아 있지 않다. 해싱은 `R5`의 범위가 아니었으므로 여기 남긴다. 커스텀 모델은
`kMaxCustomModelBytes`가 512 MB까지 허용하므로 무시할 수 있는 크기가 아니다.

### 9. 영상 검토의 Esc가 배치 전체를 버린다

`VideoReviewDialog.cpp:902`의 `reject()`가 `VideoReviewDecision::CancelAll`을
설정한다. 이미지 검토의 Esc는 한 장만 건너뛰므로 동작이 비대칭이고, Esc 한 번이
배치 전체를 버린다. undo도 없다.

이 화면은 2026-09-05 이후 공백 탐색과 프레임 이동이 들어오면서 크게 바뀌었고
앞으로도 `R` 트랙이 계속 건드린다. 고칠 때
[PROJECT_REVIEW.ko.md](PROJECT_REVIEW.ko.md)의 다음 작업과 순서를 맞춘다.

### 10. YOLO5Face 점수가 objectness만 쓴다

`Yolo5FaceDetector.cpp:268`이 `row[4]`를 그대로 점수로 쓰고 class score와 곱하지
않는다. 단일 클래스 모델이라 실제 차이는 작지만, 사용자가 정한 신뢰도 임계값의
의미가 의도와 달라지고 `videoStrongScoreThreshold`와 tracker의
`highScoreThreshold`가 그 눈금 위에 서 있다. 고칠 때 기본 임계값을 다시 봐야 하므로
비용이 코드 한 줄보다 크다.

### 11~13

- `CF-025` — `release.yml:130`의 `brew install`이 버전을 고정하지 않는다. macOS
  릴리스가 그날의 Homebrew에 의존한다. SBOM도 없다.
- `release.yml:325`가 DMG만 staple한다. 사용자가 app 번들을 DMG에서 꺼내 옮기면
  Gatekeeper가 온라인 확인에 기댄다.
- 기타 Low는 §4.

## 2. 규모가 큰 단일 항목 — 커스텀 ONNX parser의 process 격리

`CF-016`의 남은 절반이다. 동의는 경로가 아니라 내용에 묶였지만(`341cc4a`,
`CustomModelConsent.cpp`), 승인된 바이트가 여전히 GUI/미디어 프로세스와 같은
권한으로 파싱되고 실행된다(`ScrfdFaceDetector.cpp:138`의 `Ort::Session`).
`SECURITY.md`가 **"모델 파일을 여는 것이 코드 실행으로 이어진다"**를 명시적으로
범위 안에 두고 있으므로, 이것이 지금 남은 것 중 가장 큰 실제 보안 격차다.

우선순위 표에 번호를 주지 않은 이유는 중요도가 낮아서가 아니라 한 세션 분량이
아니기 때문이다. 별도 실행 파일, tensor IPC(영상은 프레임마다이므로 공유 메모리),
메모리·시간 제한, 플랫폼별 sandbox가 필요하고 검출 성능 작업과 얽힌다. 착수 여부가
결정 사항이라 §1의 흐름과 분리해 둔다.

`OnnxGraphPatch` 경로는 `ScrfdFaceDetector`에서만 쓰이므로, SCRFD를 걷어내는
선택지를 함께 검토하면 이 항목의 범위가 줄어든다.

## 3. 저장소 밖에 막힌 항목

| 항목 | 막힌 이유 |
|---|---|
| Windows Qt 6.11.1 핀 | Qt가 6.11부터 Windows 저장소를 아키텍처별로 쪼갰고 aqtinstall이 아직 평면 경로를 요구한다. issues #959, #1000은 master에만 반영되었고 최신 릴리스는 v3.3.0(2025-06-02). `install-qt-action`의 `aqtsource` 입력으로 강제할 수는 있다 |
| Windows 설치본 Authenticode 서명 | 인증서 비용 문제다. `CF-001`은 앱에 고정한 키로 닫혔고 Authenticode는 그것과 별개로, 없으면 SmartScreen 경고가 난다 |

## 4. Low

- `Mosaic.cpp:74` — 짧은 변이 2픽셀 미만인 영역은 blur가 그대로 반환한다. 1픽셀
  영역을 흐릴 수 없는 것은 맞지만, 가려지지 않은 채 보고되지도 않는다.
- 타원 마스크가 사각형 모서리를 덮지 않는다. 사용자가 고른 모양이므로 의도대로지만
  얼굴 모서리가 비어져 나올 수 있다.

## 5. 고치지 않고 감수하기로 한 것

**`CF-002` — Linux 공유 `/var/tmp` update cache (2026-08-10 결정).** 같은 기계에
적대적인 로컬 계정이 있는 상황을 위협 모델 밖에 두기로 했다. 상류 Velopack에
`package_dir_override`의 C API 노출을 요청하지 않고, cache 위치를 바꾸려는 시도도
하지 않는다.

정확히 적어 둔다. `/var/tmp`가 모든 계정에 열려 있는 것은 Linux의 기본값이지 잘못
설정된 기계가 아니고, 검증 없이 그 위를 믿기로 한 것은 번들한 Velopack 1.2.0의
cache 설계다. 즉 "사용자 환경 문제"라서 감수하는 것이 아니라, 그 위협 모델을 지원
범위에서 뺐기 때문에 감수하는 것이다.

감수하는 것이 finding 전체가 아니라 잔여 위험 하나라는 점도 같이 남긴다.
`inspectCacheDirectory`와 `fileMatchesDigest`가 `SelfUpdaterVelopack.cpp:234,261,283`
에 들어가 있어서 공격은 "임의 AppImage 실행"에서 "마지막 digest 확인과 Velopack이
파일을 여는 사이의 경합 창"으로 줄어 있다. 그 검사들은 제거하지 않는다.
`SECURITY.md`의 "Out of scope"에도 같은 결정이 실려 있다.

다시 열어야 하는 조건: Velopack이 cache 위치나 existing-package 검증을 바꾸는 경우,
또는 다중 사용자 Linux 데스크톱이 지원 대상에 들어오는 경우.

## 6. 이 표가 한 번 틀렸던 곳

2026-08-09 판은 `CF-004`를 "`TrackCoverageReport::droppedTracks`가 삭제된 track을
결과로 올린다"는 근거로 Fixed로 적었다. 그 시점(`b3beb25`)에 `droppedTracks`는
**쓰이기만 하고 읽는 곳이 없었다.** 값이 사용자에게 도달한 것은 `f1f52a1`부터다.

2026-09-14에 이 문서를 복원할 때도 같은 종류의 오판이 두 번 있었다. 테스트
`TIMEOUT`이 파일에 나타났다는 이유로 닫힌 것으로 적었으나 17개 중 1개뿐이었고,
GUI 스레드 블로킹을 `R5` 때문에 통째로 닫힌 것으로 적었으나 모델 해싱은 그
범위가 아니었다.

근거는 항상 그 값을 읽는 caller까지, 그리고 **적용 범위가 전부인지까지** 확인해서
적는다.

## 7. 2026-08-10 이후 닫힌 항목

| 항목 | 닫은 커밋 |
|---|---|
| pass 1 검출 성능의 큰 절반 | `bb2cf21` 분석 프레임 프리페치, `27b6b29` 검출기 전/후처리 동시 실행, `c911af2` 인코드 패스 프리페치, `168d26b` CPU ONNX 최적화 상향, `15f8e95` CoreML 컴파일 모델 캐시 |
| 스캐너 에러 처리 비대칭 | `30ea773` — `ScanIssue`가 읽지 못한 입력을 올리고 실행이 **검토 필요**로 끝난다. `R1`(`840596d`)이 폴더별 순회로 확장했다 |
| `OnnxGraphPatch`의 Resize scale 하드코딩 | `cc45d06` — 원본 `sizes` initializer를 읽어 다시 쓰고, 유도할 수 없으면 패치를 포기한다 |
| 리뷰를 약속하고 건너뛰던 경로 | `c15906f`, `515f166` — 리뷰 창을 띄울 수 없으면 저장하지 않고, 모든 검출을 제외하면 한 번 더 묻는다 |
| `CF-007`의 코드 절반 | `9699d83` — 서명 키를 읽는 job에 `environment: release` |
| 키 없이 릴리스가 나가던 경로 | `c1265d9` — tag 릴리스가 update key pair를 요구한다 |
| 썸네일 생성의 GUI 스레드 블로킹 | `R5`(`9ebd0ed`) — `ThumbnailLoader`. 같은 항목의 모델 해싱 절반은 §1.8로 남았다 |

`R1`~`R8` 트랙이 닫은 항목의 전체 목록은 [PROJECT_REVIEW.ko.md](PROJECT_REVIEW.ko.md)의
개선 진행 기록에 있다. 여기에는 이 문서의 항목과 겹치는 것만 적는다.

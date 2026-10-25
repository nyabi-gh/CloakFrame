# M6 단계 계획: desktop 앱

작성 2026-10-25. [Rust 포팅 계획](RUST_PORT_PLAN.md) 8장 M6(6장 UX 전체, 사진·영상 검토 화면, 설정, 3개 언어·테마, 정보 창)을 단계로 나누기 전에 정할 것과 근거다. 시작점은 main `a8b13ad`(M5 완료와 [M0–M5 재검토](M0_M5_REVIEW.md) 수정, PR #55까지)다.

## 결정(2026-10-25, 메인테이너가 추천대로)

- **단계:** 아래 0–5단계. 단계마다 main에서 브랜치·PR 하나. 범위 조정 순서의 ① 영상 미리보기 연속 재생과 ② 앱 재시작을 넘는 검토 저장·재개는 미루지 않는다(M5 끝 누적 9일, 남은 M6–M8 13주가 예산에 들어감). [M0–M5 재검토](M0_M5_REVIEW.md)에서 남긴 것 중 9번(결정의 입력 키)은 2단계, 11번의 미리보기 panic(`VideoEdits::check` 먼저)은 3단계에 넣는다. 7번(영상 타일의 성긴 보기 확인)과 10번(움직이는 대상의 다시 잇기)은 감지·추적 동작이라 M8 품질 비교 전에 별도 브랜치로 본다. 11번의 나머지는 닿는 단계에서 함께 본다.
- **egui 판:** eframe·egui·egui_kittest를 0.37.x로 올린다. 0단계 첫 커밋에서 M0 egui spike의 자동 시험(IME 조합 순서, AccessKit 트리, 1080p 업로드·4K 확대 측정)을 새 판으로 다시 돌려 [M0 기록](M0_EGUI.md)과 비교한다. muda·rfd도 그때의 최신 판으로 정한다. 처음에는 공개 2주 규칙 때문에 0.37.0(2026-10-15)이 2주가 되는 10월 29일 뒤에 0단계를 시작하기로 했으나, 같은 날 메인테이너가 규칙을 없앴다(계획 3장 "crate 판").
- **기술 선택:**
  - 글꼴: 일본어 부분 집합을 JIS X 0213 전체로, 한국어 부분 집합에 KS X 1001 한자 4,888자를 더해 번들한다(M0의 이름 이체자 공백, 약 +1.5 MB 예상). OS 글꼴 런타임 fallback은 OS마다 글꼴을 찾는 코드가 들어 쓰지 않는다.
  - 메뉴: muda는 macOS·Windows에서 M0의 "custom" 방식(편집 항목을 앱이 egui 이벤트로 넣고 단축키 표시는 유지), Linux는 egui 메뉴 막대.
  - 번역: 영어 원문 `.ftl`을 기준으로 두고, 1.12.1 `.ts`(언어마다 427개, 미완 0개)는 스크립트로 쓰는 문장만 옮긴다. 새 문장은 한국어는 직접, 일본어는 기계 초안에 "검토 필요" 표시를 단다.
  - 설정: 앱 데이터 폴더(`app_dirs`)의 `settings.toml`. 형식 버전을 두고, 모르는 키는 버리지 않고 경고한다. 1.12.1 QSettings 이전은 M7.
  - 렌더러: wgpu 기본, 시작하지 못하면 glow로 다시 시작.
- **실제 OS 점검:** macOS는 5단계에서 메인테이너가 [M0 egui 절차](M0_EGUI.md#사람이-할-확인-macos)(2벌식·일본어 IME, VoiceOver, Finder 드롭, 메뉴 단축키, 배율·테마)로 확인한다. Windows·Linux는 CI runner에서 돌릴 수 있는 것(앱 실행과 `egui_kittest`, GPU 없는 환경의 렌더러 대체 경로, 창 스크린샷)을 자동으로 하고, Narrator·실제 IME·원격 데스크톱은 M7의 Windows 10 VM과 M8 실기기 점검으로 옮긴다. preview 릴리스는 macOS 확인 뒤.

## 단계

| # | 브랜치 | 내용 |
| --- | --- | --- |
| 0 | `m6-app` | 새 crate `cloakframe-app`(eframe). egui 0.37 재측정, 글꼴, fluent(영·한·일), 테마(시스템·밝음·어두움), `settings.toml`, 메뉴(muda/egui), 정보 창(Apache-2.0, 서드파티 고지·Microsoft 약관, 모델 라이선스), 창 위치 기억, 시작 모달 없음. 메인 화면: 드롭 영역과 썸네일 그리드(형식별 개수, 미지원 파일은 이유와 함께), 무엇을·어떻게 한 줄, 저장 위치와 시작. engine 호출은 작업 스레드에서 하고 `RunHandle` 이벤트를 UI로 넘겨 다시 그린다. `egui_kittest` 틀 |
| 1 | `m6-run` | 처리 화면(파일별 상태, 작업량 기준 진행률과 예상 시간, 취소, 상세 로그 패널), 결과 문장 요약과 항목별 바로가기, 검토 세션을 앱 재시작 뒤에도 다시 엶(②, engine `SessionStore`) |
| 2 | `m6-photo-review` | 검토 큐(미해결 → 감지 0건 → 저신뢰 순), 편집기(원본 해상도까지 확대·필요한 타일만 디코드한 타일 텍스처, 클릭 토글·드래그 추가·키보드 이동과 크기, Space 마스킹 미리보기, undo/redo, `?` 단축키 안내), 해결 표시, 경고 포함·익명화 없이 내보내기(확인). 앱의 결정은 `ReviewKey`까지 묶어 적용(재검토 9번) |
| 3 | `m6-video-review` | 타임라인(track 레인·gap), 프레임 단위 이동과 `VideoProcessor::preview`, 저해상도 연속 재생 + 마스킹 미리보기(①), keyframe 편집, gap마다 덮음/오탐. 미리보기 전에 `VideoEdits::check`(재검토 11번) |
| 4 | `m6-settings` | 고급 설정(정확/빠름, 민감도, 타일, 메타데이터·GPS, 오디오 제거, 영상 품질·코덱, GPU), 샘플 사진 미리보기, 하드웨어 인코드 실패 이유 표시(M5에서 넘어옴), 번역 이전 완료, 접근성 이름·역할·포커스 순서(목록은 표 대신 역할을 붙인 리스트, egui #8700) |
| 5 | `m6-verify` | 모든 흐름의 `egui_kittest` 시험 표, macOS 실제 OS 점검, Windows·Linux CI 실행과 스크린샷, M0 공개 이슈 표의 재확인, 범위 조정 점검, preview 릴리스 준비 |

## 0단계(2026-10-25)

- **egui 0.37 재확인:** [M0 기록](M0_EGUI.md#0370-재확인-2026-10-25). API 변경 두 곳과, `egui_kittest` 0.37이 그릴 수 없는 글자에 panic하는 것(첫 프레임 전에 글꼴을 넣어야 함)을 찾았다. 앱도 창을 만들 때(`AppState::install_fonts`) 첫 프레임 전에 넣는다.
- **글꼴:** `tools/fonts/subset.sh`가 만든 부분 집합을 저장소에 넣는다(`include_bytes!`로 빌드에 들어가므로). 일본어는 JIS X 0213에 cp932의 NEC·IBM 확장을 더했다: 髙는 JIS X 0213에 없고 IBM 확장에만 있다. 결과는 한국어 3.4 MB(17,725자), 일본어 4.1 MB(14,538자)로 합계 7.5 MB, M0의 4.1 MB보다 3.4 MB 크다(계획의 예상 +1.5 MB보다 큼). 이름 이체자 髙·﨑·德·瀨·栁·濵·彅·塚은 모두 일본어 글꼴에 있다. 한국어 글꼴에도 한자가 들어가므로, 앞에 둔 글꼴의 자형이 쓰인다: 화면 언어가 한국어면 한국어 글꼴, 그 밖에는 일본어 글꼴을 앞에 둔다.
- **결정(2026-10-25, 메인테이너가 추천대로):**
  - egui 내장 글꼴(`epaint_default_fonts`, OFL-1.1 + Ubuntu Font Licence)은 라이선스 허용 목록 밖이라 eframe의 `default_fonts`를 끄고 영문까지 Noto 부분 집합으로 그린다. 허용 목록은 그대로다. 시험이 빠진 글자에서 실패하므로 빠진 글자는 부분 집합 스크립트에 더한다.
  - zbus 5의 unmaintained 권고 세 건(RUSTSEC-2026-0370–0372, zbus 6에 합쳐진 crate)은 `deny.toml`에서 이유와 함께 무시한다. Linux 접근성(AccessKit AT-SPI)과 파일 대화상자(rfd xdg-portal)가 아직 zbus 5라 빼면 Linux 스크린리더나 대화상자를 잃는다. 둘이 zbus 6으로 옮기면 해제한다.
- **구현(2026-10-25, 브랜치 `m6-app`):**
  - engine: CLI 실행 파일에만 있던 감지기 시작(ORT 로드, CoreML 캐시 정리, 1.12.1 모델 가져오기)을 `startup`으로, 오래된 임시 출력 정리와 실행 뒤 검토 세션 저장을 `batch::remove_stale_outputs`·`keep_for_review`로 옮겼다. 알림과 오류는 값으로 돌려주고 각 front end가 문장을 정한다(CLI 출력은 같음, CLI 시험 32개). `RunHandle::finish`는 이벤트를 직접 읽은 호출자도 실행 스레드를 기다리게 한다. `photo::decode::thumbnail`은 실행과 같은 검사·메모리 예산으로 디코드해 줄인다.
  - 앱 `cloakframe-app`(실행 파일 `cloakframe`): `i18n`(fluent, 영어 원문, 번역에 없는 문장은 영어), `fonts`, `settings`, `menu`, `about`, `app`(메인 화면), `run`(작업 스레드에서 세션 잠금 → job → `startup` → `run::start_under` → 이벤트를 메시지로 → `keep_for_review`), `thumbnails`(작업 스레드 하나, 화면에 보이는 칸만, 최근 200개; 사진은 `thumbnail`, 동영상은 `source::open` 뒤 `source_frame`으로 프레임 0), `models`(모델마다 크기·라이선스·조건·상태, 상태는 파일을 해시하므로 스레드에서, 엔진의 `download`로 받기·진행·취소, YOLO5Face-n은 조건에 동의한 뒤에만), `tools`(ORT·FFmpeg는 실행 파일 옆, debug 빌드는 `CLOAKFRAME_*` 환경 변수도; 배치는 M7). FFmpeg가 없으면 동영상은 스캔에서 빼고 이유를 보인다. 검토 모드는 "저장하기 전에 검토"(기본 켬) 하나로 둔다.
  - 시험: 앱 단위 10개(번역 id 일치·인자·복수형, 설정 파일 왕복·모르는 키 보존·읽을 수 없거나 새 판이면 쓰지 않음, 모델 조건의 영어 문장이 engine과 같음, 썸네일 사진·동영상·없는 도구), `egui_kittest` 8개(세 언어 메인 화면과 정보 창의 모든 글자, 드롭·스캔·처리할 수 없는 형식과 FFmpeg 없음, 시작 조건, 언어 메뉴, 실제 모델로 실행해 저장하고 요약, 모델 창의 조건 동의와 받기·다른 파일). 받기 시험은 `Fetch`를 시험 자산을 내주는 것으로 바꿔 network 없이 해시 검사까지 돈다. mutation: 조건 동의 없이 받게 하면 시험이 잡는다. engine 단위 1개(썸네일 회전·크기). 로컬 전체 517개 통과(M0–M5 재검토 수정 뒤 498개), fmt·clippy 두 조합·cargo-deny 통과.
  - 찾은 것: 설정 저장이 선택 키를 먼저 지워 그 아래의 모르는 키(`window.monitor`)가 사라졌다(시험이 잡아 고침). 첫 CI에서 macOS·Windows runner의 UI 시험이 스캔 spinner가 도는 동안 `Harness::run`으로 기다려 실패했다(이 Mac에서는 스캔이 먼저 끝남). 한 프레임 진행 → 스캔 기다림 → 정착 순서로 고치고 스캔을 300 ms 늦춰 확인했다. 두 번째 CI(workflow_dispatch)에서 3개 OS 통과: Windows·Linux의 앱 첫 빌드·시험이다.
  - 실제 창: 이 Mac에서 시스템 언어(한국어)와 네이티브 메뉴(파일·편집)로 열린다. 사람이 하는 확인은 5단계.
  - 남긴 것: 동영상 썸네일은 프레임 0만(장면 대표 프레임은 고르지 않음). 처리 화면의 예상 시간·로그 패널·결과 바로가기, 앱 재시작 뒤 세션 다시 열기는 1단계. 고급 설정(정확/빠름 모델, 민감도, 타일, 메타데이터, 오디오, 영상 품질, GPU)과 샘플 미리보기는 4단계. 패키지 안 ORT·FFmpeg 위치는 M7.
- **함께 정한 것(질문하지 않음):** eframe `clipboard`를 켠다(입력란 복사·붙여넣기). eframe `persistence`는 `directories`(option-ext, MPL-2.0)를 끌어와 쓰지 않고 창 위치를 `settings.toml`에 둔다. 테마·언어는 OS마다 같은 창 안 "보기" 메뉴에 두고, 네이티브 메뉴에는 정보·끝내기·파일·편집을 둔다. 메뉴 편집 항목은 `raw_input_hook`에서 egui 입력 이벤트로 넣는다. 설정 저장은 알 수 없는 키(중첩 포함)를 남기고, 읽을 수 없거나 새 판이 쓴 파일은 덮어쓰지 않는다.

## 근거

- **engine 쪽은 준비됨:** `run::start`·`RunHandle`(이벤트, 진행, 항목 취소), `SessionStore`(저장·잠금·정리), `VideoProcessor::preview`(프레임 k의 pixel·영역·마스크 미리보기), core `mask::render`, `app_dirs`(앱 데이터·모델 폴더)가 있다. M6은 주로 앱 crate다.
- **egui:** M0에서 0.36.2로 IME·AccessKit·텍스처 업로드를 확인했다(egui 유지 확정 2026-10-17). 0.37.0은 2026-10-15 공개다. 앱 코드가 없을 때 올리면 옮길 것이 없다.
- **번역:** v1.12.1 `translations/cloakframe_{ko,ja}.ts`는 각각 `<message>` 427개, `unfinished` 0개다. 새 UX(6장)라 그대로 쓰이는 문장은 일부다.
- **장치:** 메인테이너는 Windows 10·Intel/AMD·Linux 실기기를 쓸 수 없다. Windows 10 VM은 M7 완료 기준에 이미 있다.

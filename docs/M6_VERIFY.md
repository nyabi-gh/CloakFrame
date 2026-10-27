# M6 5단계: 완료 확인

작성 2026-10-26, 브랜치 `m6-verify`(main `4f5f848` 위). [M6 단계 계획](M6_PLAN.md)의 "5단계로 넘어온 것"과 [계획](RUST_PORT_PLAN.md) 8장 M6 완료 기준을 하나씩 확인한다. macOS 사람 점검은 메인테이너가 아래 목록으로 하고, 그 결과를 이 문서에 적은 뒤 M6을 완료로 본다.

## 결정(2026-10-26, 메인테이너가 추천대로)

- **렌더러 표시:** 정보 창에 "그래픽: wgpu, metal, Apple M4"처럼 렌더러·백엔드·어댑터를 보이고(소프트웨어 어댑터면 "(software)"), 시작할 때 stderr에 같은 내용을 `graphics: …` 한 줄로 쓴다. 버그 보고에 쓰고, CI가 대체 경로를 이 줄로 확인한다.
- **두 렌더러 모두 실패:** wgpu와 OpenGL 둘 다 시작하지 못하면 네이티브 오류 창(rfd)으로 "CloakFrame을 시작할 수 없습니다"와 두 오류를 보이고 끝난다. 설정의 언어, 없으면 시스템 언어로 번역한다. 전에는 창 없이 끝나 Windows release(콘솔 없음)에서는 아무 표시가 없었다. Linux의 rfd 메시지 창은 zenity가 있어야 뜨고, 없으면 stderr만 남는다.
- **preview 릴리스:** macOS arm64 zip. `preview.yml`이 release 빌드에 ORT·FFmpeg를 넣은 ad-hoc 서명 앱(공증 없음)을 zip으로 만들어 GitHub의 초안 prerelease에 올린다. 게시는 메인테이너가 macOS 점검 뒤에 한다. Windows·Linux 배포본과 DMG·서명·공증은 M7.

함께 정한 것(질문하지 않음):

- 렌더러 문장은 기술 정보라 번역하지 않는다(라벨 "그래픽"만 번역).
- preview 앱 이름은 "CloakFrame Preview", 식별자는 `com.cloakframe.app.preview`다. 1.12.1(`com.cloakframe.app`)과 응용 프로그램 폴더에 함께 둘 수 있게 했다. 최소 macOS 15.0(계획 2장 지원 OS).

## 흐름별 시험 표

앱 UI 시험(`egui_kittest`) 17개(`tests/main_screen.rs` 15, `tests/models_window.rs` 2)와 앱 단위 시험 36개. 이번 단계에 UI 시험 2개(테마, 취소)와 단위 시험 1개(렌더러 문장)를 더했다.

| 흐름 | UI 시험 | 단위 시험 | 사람·CI 점검 |
| --- | --- | --- | --- |
| 드롭·스캔(형식별 개수, 미지원 파일과 이유, FFmpeg 없으면 영상 뺌) | `dropped_files_are_scanned_and_counted` | `thumbnails::photos_get_thumbnails_and_unreadable_ones_none`, `videos_get_their_first_frame` | Finder 드롭, 파일·폴더 대화상자(rfd는 시험 밖) |
| 시작 조건(파일·저장 위치·모델·가릴 대상) | `start_waits_for_models_files_and_a_folder` | | |
| 모델 받기(동의 뒤 받기, 다른 파일 표시) | `models_window.rs` 2개 | `models::the_english_terms_are_the_engines` | |
| 실행·결과(상태, 요약, 바로가기 단추, 로그, 새 실행) | `start_runs_the_engine_and_shows_the_result` | `progress` 4개, `summary` 3개, `shell::file_uris_escape_everything_but_unreserved_bytes` | Finder가 실제로 파일을 선택하는지 |
| 취소(항목 하나, 실행 전체) | `cancel_stops_an_item_or_the_whole_run`(새) | `run::an_item_is_cancelled_alone_until_the_engine_stops_it` | |
| 실행 중 닫기(묻기, 계속, 취소하고 종료) | `closing_during_a_run_asks_and_waits_for_the_run_to_stop` | | Windows CI가 실제 창 닫기와 창 위치 저장 |
| 세션 다시 열기·버리기, 검토 큐 순서 | `reviews_kept_by_earlier_runs_are_listed_opened_and_discarded` | `queue::unresolved_then_nothing_detected_then_least_confident` | |
| 사진 검토(토글·추가·undo/redo·내보내기·렌더) | `the_editor_decides_what_is_masked_and_saves_it` | `draft` 5개, `tiles` 3개 | 트랙패드 핀치, VoiceOver 캔버스 |
| 영상 검토(track·keyframe·gap·미리보기·렌더) | `the_video_editor_edits_tracks_covers_a_gap_and_saves_it` | `draft` 영상 2개, `lanes::spans_that_do_not_overlap_share_a_row` | 연속 재생 실제 속도 |
| 고급 설정·샘플 | `advanced_options_are_saved_and_run_with` | `settings` 6개, `run::every_advanced_setting_changes_what_the_engine_processes_with`, `sessions_keep_the_advanced_settings`, `sample::what_and_how_change_only_the_face` | 민감도 슬라이더 읽기 |
| 언어 | `the_language_menu_changes_the_text`, `every_language_draws_the_main_screen`, `every_message_of_every_language_can_be_drawn` | `i18n` 3개, `wording::every_engine_value_is_worded_in_every_language` | |
| 테마 | `the_theme_menu_changes_the_colours`(새) | | 시스템 밝게·어둡게를 따라오는지 |
| 정보 창(렌더러 포함) | `the_about_window_draws_in_every_language` | `graphics::the_backend_adapter_and_a_software_adapter_are_named`(새) | |
| 접근성(목록 역할, Tab 순서) | `tab_follows_the_screen_from_top_to_bottom`, 목록 역할은 위 시험들 | | VoiceOver |
| 렌더러 대체, 창 실행 | | | `tools/ci/app_window.sh`·`.ps1`(CI) |
| 창 위치 기억 | | `settings` 왕복 | Windows CI가 닫은 뒤 `[window]` 확인 |

- 새 취소 시험은 처음에 사진 8장으로 썼는데 항목 취소가 맞지 않았다. 클릭은 전달되어 대기 중 항목의 토큰이 취소됐지만, 화면이 그 상태를 받기 전에 engine이 8장을 모두 분석해 검토 대기로 넘긴 뒤였다(분석이 끝난 항목은 취소 단추 대상이 아님). 화면이 한 프레임 늦은 데서 오는 경쟁이라 제품 결함이 아니고, 시험이 끝나지 않은 1분짜리 합성 영상을 취소하게 바꿔 결정적으로 만들었다. 같은 가정이던 닫기 시험도 이 준비를 함께 쓴다. 실행이 항목을 알리기 전에 누르지 않도록 `AppState::wait_for_run_items`를 더했다.
- 작업 스레드 9곳(스캔, 모델 둘, 썸네일, 세션, 실행, 검토, 타일, 영상)을 300 ms 늦춘 복사본에서도 앱 시험이 모두 통과했다.
- 시험으로 덮지 않는 것: 네이티브 파일 대화상자(rfd), muda 메뉴의 실제 단축키, OS 파일 관리자 선택, 실제 IME·스크린리더. 아래 사람 점검이 맡는다.

## Windows·Linux(CI)

`ci.yml`의 시험 job이 시험 뒤 앱을 실제로 띄운다. 스크린샷과 로그는 실행의 artifact(`app-window-<OS>-<run id>`)로 남는다.

- **Linux(`tools/ci/app_window.sh`):** Xvfb(GPU 없음)에서 (1) 기본 실행, (2) Vulkan 로더가 드라이버를 못 찾게 하고 `WGPU_BACKEND=vulkan`으로 wgpu가 어댑터를 못 찾게 한 실행. 로그의 `graphics:` 줄을 기다리고, 창이 보인 뒤 화면이 한 색이 아니게 될 때까지 찍는다. (1)은 `graphics: wgpu, `, (2)는 "wgpu could not start"와 `graphics: OpenGL, `을 요구한다. 창 관리자가 없어 닫기는 시험하지 않는다.
- **Windows(`tools/ci/app_window.ps1`):** runner 데스크톱에서 (1) 기본 실행을 띄우고 찍은 뒤 `CloseMainWindow`(WM_CLOSE)로 닫아 종료 코드 0과 `settings.toml`의 `[window]`를 확인한다. (2) `WGPU_BACKEND=vulkan`. runner에 Vulkan 드라이버가 있으면 wgpu로 뜨고, 없으면 OpenGL로 대체하거나, OpenGL도 1.1뿐이면 오류 창이 뜬다. 셋 중 무엇이 되는지가 결과이고 첫 CI 결과를 아래에 적는다.
- **사전 실행(2026-10-26, 이 Mac의 Docker arm64 Ubuntu 24.04, Mesa 25.2.8):** (1) `graphics: wgpu, vulkan, llvmpipe (LLVM 20.1.2, 128 bits) (software)`, (2) `wgpu could not start (Failed to create surface for any enabled backend: {}); trying OpenGL` → `graphics: OpenGL, llvmpipe (LLVM 20.1.2, 128 bits), 4.5 (Core Profile) Mesa 25.2.8`. 둘 다 메인 화면이 그려졌다. 처음에는 `libXcursor.so.1`이 없어 winit이 창을 못 열었으므로 workflow가 winit이 여는 X 라이브러리를 명시해 설치한다.
- **macOS 로컬:** 기본 `graphics: wgpu, metal, Apple M4`, `WGPU_BACKEND=vulkan`이면 OpenGL로 대체해 `graphics: OpenGL, Apple M4, 4.1 Metal - 91.7`.
- **첫 CI(PR #62, 2026-10-26):** macOS 시험 통과. Windows 기본 실행은 `graphics: wgpu, dx12, Microsoft Basic Render Driver (software)`로 떴지만, 스크립트가 제목 없는 다른 최상위 창을 앱 창으로 잡았고 화면 전체를 찍어 runner 데스크톱의 색만으로 "그려짐"이 참이 되어(앱 창은 runner 터미널 뒤) 실패했다. 제목이 맞는 창을 기다리고 그 창만 `PrintWindow`로 찍게 고쳤다. Linux는 창 실행 전에 기존 세션 시험이 실패했다(아래 결함).
- **찾은 결함(세션 목록):** 검토를 닫으면(뒤로) 세션 목록을 스레드에서 다시 읽는다. 그 읽기가 끝나기 전에 사용자가 세션을 버리면, 버리기 전에 읽은 목록이 나중에 도착해 버린 세션이 목록에 다시 보였다(다음 읽기까지). Linux runner에서 `reviews_kept_by_earlier_runs_are_listed_opened_and_discarded`가 이 순서로 실패했고, 읽은 뒤 결과를 보내기 전에 300 ms를 늦춘 복사본에서 같은 실패를 재현했다(시작 전에 늦추면 버린 뒤의 상태를 읽어 재현되지 않음). 버리면 진행 중인 읽기 결과를 버리고 지운 뒤의 상태로 다시 읽게 고쳤고, 같은 복사본에서 통과한다.
- **CI 결과(PR #62 run 40280951135, 2026-10-26, artifact `app-window-*`):** 4개 job 통과.
  - Linux 기본 `graphics: wgpu, vulkan, llvmpipe (LLVM 20.1.2, 256 bits) (software)`. Vulkan을 막으면 "wgpu could not start"를 거쳐 `graphics: OpenGL, llvmpipe (LLVM 20.1.2, 256 bits), 4.5 (Core Profile) Mesa 25.2.8`. 사전 실행과 같고 x86 runner라 llvmpipe 폭만 256 bits다.
  - Windows 기본 `graphics: wgpu, dx12, Microsoft Basic Render Driver (software)`. WM_CLOSE로 닫혀 종료 코드 0, `settings.toml`에 `[window]`가 저장됐다.
  - Windows `WGPU_BACKEND=vulkan`: runner에 Vulkan 드라이버가 없어 wgpu가 실패하고, OpenGL은 1.1뿐이라 glow가 "egui_glow requires opengl 2.0+"로 실패해 "CloakFrame cannot start" 오류 창이 떴다(셋 중 세 번째).
- **오류 창 문구의 겹친 접두:** 창에 `OpenGL: egui_glow: OpenGL: egui_glow requires opengl 2.0+.`가 보였다. 앱이 붙인 `OpenGL: `, eframe `Error::OpenGL`의 Display가 붙이는 `egui_glow: `, egui_glow `PainterError`의 Display가 붙이는 `OpenGL: `, 메시지 본문(`"egui_glow requires opengl 2.0+. "`)이 겹친 것이다. egui master도 같다. 결정(추천대로): 앱의 렌더러 라벨을 없애고 두 오류를 eframe의 Display 그대로 줄마다 보인다(`WGPU error: …` / `egui_glow: OpenGL: …`). 남는 겹침은 upstream 몫이다. stderr 줄(`wgpu could not start (…)`)은 CI 스크립트가 찾으므로 그대로 둔다.
- **main push CI(d10636a, run 40291125802)의 Windows 실패:** `start_runs_the_engine_and_shows_the_result`의 마지막 `harness.run()`이 "exceeded max_steps (4) … would have settled after 6", 원인 `sessions.rs:90`(세션 목록 읽기 스레드의 `request_repaint`)으로 실패했다. 실행이 끝나면 다음 frame이 세션 목록을 스레드에서 다시 읽는데, 시험이 이 읽기를 기다리지 않았다. 다른 스레드의 repaint 요청은 pass 두 번을 부르므로, 결과가 원래 4단계인 마지막 `run()`의 3–4단계 도중에 오면 한도를 넘는다. 복사본에서 읽기 스레드가 파일 신호를 기다리게 하고 시험이 마지막 `run()` 직전에 신호를 주자, 6.0–7.8 ms 늦출 때 같은 오류(`steps_to_settle: Some(6)`)가 재현됐다. 300 ms 늦춘 확인에서 못 잡은 것은 결과가 시험이 끝난 뒤에 와서다. 이 시험은 `SavedSessions`에 데이터 폴더를 주지 않아 읽을 것도 없었다. 고친 것: 세션 저장소가 없으면 스레드 없이 빈 목록으로 두고, `wait`는 결과를 받은 뒤 스레드를 join해 repaint 요청까지 끝난 뒤 돌아온다. 데이터 폴더가 있는 시험은 읽기가 시작되는 곳(파일 놓기 뒤, 실행 끝, 검토에서 뒤로, 버리기)마다 기다린 뒤 정착한다. 확인: 복사본에서 UI frame의 `take`가 세션 결과를 받으면 panic하게 하고 앱 시험 전체를 그대로, 보내기 전 300 ms, 보낸 뒤 repaint 전 300 ms 늦춰 돌려 모두 통과했다(고치기 전에는 편집기 시험의 마지막 "Back"에서 잡힘). 이 수정으로 b128154(버리기 전에 시작한 읽기)의 경합은 시험이 더 이상 우연히 밟지 않는다. 그 수정은 코드에 남아 있다.

## macOS 사람 점검(메인테이너)

preview zip(아래) 또는 개발 빌드(`cargo run -p cloakframe-app`, CONTRIBUTING의 환경 변수)로 한다. 항목마다 "정상 / 이상(증상)"을 적는다. [M0 절차](M0_EGUI.md#사람이-할-확인-macos)를 앱에 맞게 고쳤다. 앱에는 글자 입력 칸이 없으므로(저장 위치는 대화상자로 고름) IME는 단축키와 겹치는지만 본다. preview zip에는 FFmpeg가 없으므로(아래 preview 릴리스 준비) 영상이 걸린 항목(2의 영상, 7)은 개발 빌드로 한다.

1. **열기:** zip을 풀어 "CloakFrame Preview"를 연다. 공증이 없으므로 처음에는 막히고, 시스템 설정 > 개인정보 보호 및 보안의 "그래도 열기"로 연다. 정보 창(앱 메뉴 > CloakFrame 정보)의 그래픽 줄을 적는다.
2. **Finder 드롭:** 파일 하나, 여러 개, 폴더, 이름에 한글·일본어(髙橋 등)·공백·이모지가 있는 파일, 외장 드라이브·iCloud Drive의 파일. 놓기 전 "놓으면 N개 추가", 놓은 뒤 형식별 개수와 썸네일. 사진 앱에서 끌어 놓으면 무엇이 들어오는지.
3. **대화상자·메뉴:** 파일 > 파일 추가…(⌘O)·폴더 추가…, 저장 위치 "선택…", 메뉴 단축키, CloakFrame > 종료(⌘Q). 실행 중 ⌘Q가 확인을 묻는지.
4. **결과 바로가기:** 실행이 끝난 뒤 "폴더에서 보기"가 Finder에서 그 파일을 선택하는지, "열기"가 기본 앱으로 여는지.
5. **사진 편집기:** 트랙패드 핀치 확대·두 손가락 이동, 클릭 토글, 끌어 추가, 키보드(화살표·Shift), Space 미리보기, ⌘Z·⇧⌘Z, `?` 안내.
6. **입력 소스와 단축키:** 한국어 2벌식과 일본어 로마자 입력으로 바꾼 상태에서 편집기의 Space·M·⌘Z·화살표가 그대로 동작하는지(winit #4508 계열). 입력 소스를 바꾼 직후 첫 키(winit #3095)도 본다.
7. **영상 편집기:** 1080p·4K 영상의 연속 재생(Space)이 끊기지 않는지, 체감 속도, M을 누르는 동안 가린 프레임. ←/→·Shift·Home/End·[ ].
8. **VoiceOver(⌘F5):** 메인 화면을 VO+→로 읽어 순서를 적는다. 목록(찾은 파일, 처리 결과, 검토 큐, 문제 목록)이 목록과 항목으로 읽히는지, 썸네일 이름("사진 이름.jpg"), 민감도 슬라이더의 이름과 단계 이름, 샘플 이미지 대체 문장. 편집기 캔버스에서 무엇이 읽히는지(box는 접근성 노드가 없음, 2·4단계). Tab·Shift+Tab 순서가 화면 순서와 같은지.
9. **배율·테마:** 외장 모니터(1배)와 내장 화면(2배) 사이로 창을 옮겨 글자·썸네일이 흐려지지 않는지. 보기 > 테마 "시스템"에서 시스템 밝게·어둡게를 바꾸면 바로 따라오는지.
10. **창 위치:** 창을 옮기고 크기를 바꾼 뒤 끝내고 다시 열어 같은 자리인지.

**결과(2026-10-27, 메인테이너):** 10항목 모두 동작은 정상. 다만 화면이 투박하고 무엇이 무엇인지 구분되지 않아 쓰기 어렵다고 봤다(아래 "UX 문제"). 사진 항목은 main `71b81c1`로 이 Mac에서 만든 preview zip(`2.0.0-preview.1`, 28 MB), 영상 항목은 개발 빌드로 했다. 그래픽 줄은 `wgpu, metal, Apple M4`. preview에서 "앱 옆에서 FFmpeg를 찾지 못해 동영상은 뺐습니다."가 보이는 것은 사진만 내기로 한 결과다. 점검 중 알게 된 것: 앱은 환경 변수의 도구 경로를 debug 빌드에서만 읽으므로(`tools.rs`, 패키지와 같게) 재생 속도를 release로 보려면 ORT·FFmpeg·ffprobe를 `target/release/cloakframe` 옆에 두어야 한다. 로컬에서 만든 zip에는 격리 속성이 없어 1번의 Gatekeeper 차단은 이 zip으로 나타나지 않으므로, 게시 전에 `preview.yml`이 만든 zip을 받아 한 번 더 본다.

### UX 문제(2026-10-27)

결정(메인테이너가 추천대로): M6에 6단계 UX 다듬기를 더하고, 화면 설계안(HTML)을 먼저 확인받은 뒤 egui로 옮긴다. M6 완료 표시와 preview 게시는 그 뒤로 미룬다. 데이터 폴더에 세션 둘이 남은 메인 화면(`target/release/cloakframe`, 1100×792)에서 본 것:

- 위계가 없다. 제목 하나를 빼면 글자 크기·굵기가 모두 같고, 단계(파일 → 가릴 대상·방법 → 저장 위치·시작)가 눈에 보이지 않는다.
- 맨 위의 "검토를 기다리는 작업"이 주 작업(파일 놓기)보다 먼저 보인다. 줄이 "오늘 /Users/…/Downloads에 저장할 항목 1개"로 똑같아 서로 구분되지 않고 전체 경로를 그대로 쓴다.
- 드롭 영역의 "파일 추가…"·"폴더 추가…"가 안내 문장과 떨어져 왼쪽 가운데에 붙어 있다.
- 가리는 방법 줄에서 슬라이더·숫자 칸 뒤에 "강도" 라벨이 오고, "미리보기" 라벨과 샘플 사진이 옆 열에 떠 있어 무엇의 미리보기인지 알기 어렵다.
- 아래 막대의 "보기" 단추는 테마·언어 메뉴인데 이름으로는 알 수 없고 시작 줄에 있을 이유가 없다. 저장 위치의 "선택…"이 "감지 모델…" 뒤에 떨어져 있다. "시작"은 오른쪽 끝의 흐린 작은 단추이고 왜 누를 수 없는지 말하지 않는다.
- 창의 아래 절반이 비어 있다. egui 기본 위젯 모양 그대로라 앱으로서의 완성감이 없다.

## M0 공개 이슈 표 재확인(2026-10-26)

쓰는 판은 egui·eframe 0.37.0, winit 0.30.13이고 둘 다 지금 최신 판이다. M0 표의 이슈는 모두 열려 있다.

| 이슈 | 상태 | 이 앱에서 |
| --- | --- | --- |
| winit [#3095](https://github.com/rust-windowing/winit/issues/3095) 입력 소스 전환 직후 첫 자모 분리 | 열림 | 글자 입력 칸이 없어 영향 없음. 단축키 첫 키만 사람 점검 6 |
| egui [#7974](https://github.com/emilk/egui/issues/7974) / winit [#4524](https://github.com/rust-windowing/winit/issues/4524) 2벌식 한자 변환 창 | 열림 | 입력 칸이 없어 영향 없음 |
| egui [#7975](https://github.com/emilk/egui/issues/7975) X11 + Fcitx5 조합 표시 없음 | 열림 | 입력 칸이 없어 영향 없음 |
| winit [#4508](https://github.com/rust-windowing/winit/issues/4508) Windows 조합 중 `KeyboardInput` | 열림 | 편집 칸이 없으면 IME가 허용되지 않아 단축키와 겹칠 일이 없다고 본다. Windows 실제 IME는 M7 VM |
| egui [#8671](https://github.com/emilk/egui/issues/8671) 가려진·최소화된 창은 AccessKit에 답하지 않음 | 열림 | 추적만 |
| egui [#8700](https://github.com/emilk/egui/issues/8700) Table에 표 역할 없음 | 열림 | 4단계에서 목록 역할을 직접 붙여 피함 |

## 범위 조정 점검

M6은 2026-10-25에 시작해 지금까지 2일(예산 7주)이다. M1–M5 누적 12일과 합쳐 14일로, 32주 예산 가운데 남은 M7·M8(6주)이 넉넉히 들어간다. 범위 조정 순서의 ①(영상 연속 재생)과 ②(재시작 뒤 검토 재개)는 모두 M6에 들어갔고 미루는 항목은 없다. 계획 8장 표의 M6 "실제" 칸은 macOS 점검까지 끝난 날로 적는다. macOS 점검은 2026-10-27에 끝났지만 UX 단계(6단계)를 더해 M6은 이어진다.

## preview 릴리스 준비

- `tools/package/macos_preview.sh`: release 실행 파일과 ORT를 `Contents/MacOS`에(앱은 실행 파일 옆에서 찾음, `tools.rs`), 아이콘과 `LICENSE`·`THIRD_PARTY_NOTICES.txt`를 `Contents/Resources`에 넣고, 안쪽부터 ad-hoc 서명한 뒤 `ditto`로 zip을 만든다. 처음에는 FFmpeg·ffprobe도 넣었다(zip 84 MB, 앱이 Metal로 뜨고 FFmpeg를 찾음). 사진만으로 바꾼 뒤(아래) 이 Mac에서 만든 zip은 28 MB이고, 풀어 연 앱이 `graphics: wgpu, metal, Apple M4`로 뜨고 "앱 옆에서 FFmpeg를 찾지 못해 동영상은 뺐습니다."를 보였다.
- `.github/workflows/preview.yml`(수동 실행, 입력 `tag`): macOS 15 runner에서 위 스크립트로 zip을 만들어 artifact로 남기고 같은 zip으로 초안 prerelease를 만든다. 설명은 `docs/PREVIEW_NOTES.md`.
- **FFmpeg(2026-10-26 결정):** 번들 FFmpeg는 GPL-3.0-or-later 빌드인데 THIRD_PARTY_NOTICES에 고지와 대응 소스가 없었다. 처음에는 macOS 고지를 앞당기기로 했다. 그런데 Martin Riedl 9.0.2 빌드는 x264를 빌드할 때마다 `master` tarball로 받고(`script/build-x264.sh`), 공개 빌드 스크립트는 9.0.1 커밋까지만 있으며, 버전 목록에도 x264가 "0.165.x"로만 적혀 있다. 이 바이너리의 대응 소스(GPLv3 6조)는 낼 수 없다. 결정(추천대로): preview는 사진만 낸다. FFmpeg는 M7에서 고정한 소스로 직접 빌드하고, 그 소스와 스크립트를 대응 소스로 낸다(계획 10.1).

## 남긴 것

- 6단계 UX 다듬기(위 "UX 문제"), 그 뒤 M6 완료 표시.
- UX 단계 뒤 preview 게시(메인테이너, 사진만). 그 전에 workflow가 만든 zip을 받아 Gatekeeper 차단과 "그래도 열기"를 본다.
- 일본어 "검토 필요" 문장(새 2개 포함)은 일본어 검토자가 생길 때까지 남는다.

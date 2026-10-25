# M0 spike: egui/eframe (macOS)

실행일: 2026-10-16. 장치: Apple M4(MacBook Air, Mac16,13), macOS 27.0.1, Retina 2배 배율. 코드: [`spikes/egui`](../spikes/egui). [Rust 포팅 계획](RUST_PORT_PLAN.md) M0의 egui spike 항목 중 macOS에서 할 수 있는 것과, 사람이 해야 하는 확인의 절차다. Windows·Linux(Narrator, GPU 없는 환경, 원격 데스크톱)는 [실행 절차](M0_PLATFORM_RUNBOOK.md)에 있다.

버전: eframe·egui·egui_kittest **0.36.2**(계획의 0.36.x. 0.37.0이 10월 15일에 나왔지만 하루 된 버전이라 쓰지 않음), muda 0.20.0, 렌더러 wgpu(Metal)와 glow(OpenGL) 둘 다.

## 결론

| 항목 | 결과 | 근거 |
| --- | --- | --- |
| 한글·일본어 표시 | 통과 | Noto Sans KR/JP 부분 집합을 기본 글꼴 뒤 fallback으로 넣어 실제 창과 시험 렌더러 모두에서 정상 표시 |
| 텍스트 입력 IME(자동) | 통과 | `egui_kittest`로 macOS 한국어 2벌식이 winit을 거쳐 내는 순서(`Preedit` ㅎ→하→한, `Commit` 한, …)를 넣으면 조합 중 표시와 최종 값 "한글"(2자)이 맞음. 일본어 변환(かな preedit → 日本 commit)과 조합 취소(빈 preedit)도 통과 |
| 텍스트 입력 IME(실제 입력기) | **사람 확인 필요** | 아래 절차 1 |
| AccessKit | 트리 통과, VoiceOver는 **사람 확인 필요** | eframe 기본 `accesskit` 기능으로 입력란이 `TextInput`·`MultilineTextInput` 역할과 "이름"·"메모" 이름을 갖고, 버튼·4K 미리보기도 이름으로 찾힘. 절차 2 |
| OS 파일 드롭 | 이벤트 처리 통과, Finder 드롭은 **사람 확인 필요** | `RawInput.dropped_files`/`hovered_files`로 경로를 받음(한글 경로 포함). 절차 3 |
| 네이티브 메뉴(muda) | macOS·Windows 연동 가능, **Linux 불가** | macOS는 `Menu::init_for_nsapp`으로 앱 메뉴 막대에 붙고 창이 정상 동작. Windows는 `init_for_hwnd`와 winit `with_msg_hook`의 `TranslateAcceleratorW`로 가속키까지 연결(교차 `cargo check` 통과, 실행은 Windows 절차). Linux의 muda는 GTK 창에만 붙으므로 winit 창에는 쓸 수 없다. 단축키가 텍스트 입력과 충돌하는지는 절차 4 |
| 4K 텍스처 확대·이동 | 통과 | 3840×2160 텍스처를 UV 자르기로 1–16배 확대하며 이동해도 vsync 60 fps 유지(아래 표) |
| 1080p 30 fps 업로드 | 통과(여유 2배 이상) | 매 프레임 1920×1080 RGB → `ColorImage` → `TextureHandle::set`. vsync에서 60 fps 유지, 프레임 CPU p50 5.3 ms |

필수 항목(IME·접근성)은 자동 시험 범위에서 실패가 없고, 공개 이슈 조사(아래)에서도 대안 재평가가 필요한 결함이 없다. **egui 유지로 확정한다(2026-10-17).** 아래 사람 확인 절차는 M0에서 돌리지 않고 M6의 실제 OS 점검에서 쓴다.

## 0.37.0 재확인 (2026-10-25)

M6 0단계 첫 일로 spike를 eframe·egui_kittest **0.37.0**, muda **0.21.2**로 올려 같은 장치에서 자동 시험과 측정을 다시 돌렸다([M6 계획](M6_PLAN.md)).

- **API 변경 두 곳:** `egui::WidgetType`이 AccessKit의 `egui::Role`로 바뀌어 `Other`가 없다(4K 미리보기는 `Role::Image`). 창 캡처는 `Event::Screenshot` 입력 이벤트 대신 `Context::request_screenshot`의 callback으로 받는다.
- **자동 시험 8개 통과**(IME 조합·취소·일본어 변환, AccessKit 트리, 파일 드롭, 한글·가나 글리프, CJK 렌더, 이름 이체자). 처음에는 8개 모두 "No glyph for '사'"로 실패했다. `egui_kittest` 0.37은 글리프가 없는 글자에 panic하는데(`MissingGlyphPolicy::Panic`, 앱의 기본값은 tofu), harness가 `build_ui_state` 안에서 첫 프레임을 그린 뒤에 시험이 글꼴을 넣고 있었다. 0.36에서는 첫 프레임이 소리 없이 tofu로 그려졌던 순서 오류다. `HarnessBuilder::with_context_setup`으로 첫 프레임 전에 글꼴을 넣었다. 앱 시험도 같은 방식으로 한다.
- **창 캡처:** 3200×1902 px를 저장했다(빈 화면 아님).
- **측정**(M0과 같은 창·프레임 수):

| 렌더러 | 시험 | vsync | fps | 프레임 간격 p50 / p95 | 프레임 CPU p50 / p95 | RGB→ColorImage p50 | 최대 텍스처 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| wgpu(Metal) | 1080p 업로드 | 켬 | 60.1 | 16.6 / 17.0 ms | 5.2 / 5.7 ms | 1.20 ms | 8192 |
| wgpu(Metal) | 1080p 업로드 | 끔 | 154.9 | 3.6 / 17.5 ms | 3.5 / 3.8 ms | 0.82 ms | 8192 |
| wgpu(Metal) | 4K 확대·이동 | 켬 | 60.0 | 16.7 / 17.0 ms | 1.0 / 1.2 ms | — | 8192 |
| wgpu(Metal) | 4K 확대·이동 | 끔 | 137.4 | 4.1 / 18.9 ms | 0.7 / 1.0 ms | — | 8192 |
| glow(OpenGL) | 1080p 업로드 | 켬 | 120.2 | 6.6 / 12.7 ms | 3.8 / 4.0 ms | 0.82 ms | 16384 |
| glow(OpenGL) | 1080p 업로드 | 끔 | 226.2 | 4.0 / 6.9 ms | 3.7 / 4.1 ms | 0.82 ms | 16384 |
| glow(OpenGL) | 4K 확대·이동 | 켬 | 88.6 | 11.7 / 17.3 ms | 0.2 / 1.3 ms | — | 16384 |
| glow(OpenGL) | 4K 확대·이동 | 끔 | 804.2 | 1.2 / 3.4 ms | 0.1 / 0.4 ms | — | 16384 |

0.36.2(위 [측정](#측정))와 비교하면 vsync 켬의 프레임 간격과 프레임 CPU는 같은 범위이고(1080p 업로드 wgpu 5.3 → 5.2 ms), vsync 끔 fps는 흔들림 안이다. wgpu 최대 텍스처는 그대로 8192라 사진 검토의 타일 텍스처 결정은 그대로다. 사람이 하는 확인(아래 절차)은 M6 5단계에서 0.37로 한다.

## 공개 이슈 조사 (2026-10-17)

실제 입력기·스크린리더 확인 대신 egui·winit 이슈 트래커를 한국어·일본어 IME와 AccessKit 기준으로 조사했다. CJK IME는 0.34–0.35에서 크게 고쳐졌다(조합 확정 Enter의 줄바꿈 [#7876](https://github.com/emilk/egui/issues/7876), 조합 중 Backspace가 확정 글자를 지움 [#7908](https://github.com/emilk/egui/issues/7908), 일본어 확정 뒤 포커스 해제 [#7809](https://github.com/emilk/egui/issues/7809), 합자 조합 중복 [#8087](https://github.com/emilk/egui/issues/8087)이 모두 닫힘). 0.36.2에 남은 것은 아래와 같고, 모두 우회 가능하거나 이 앱의 입력 범위(설정·짧은 이름 칸)에서 영향이 작다.

| 이슈 | 영향 | M6에서 할 일 |
| --- | --- | --- |
| winit [#3095](https://github.com/rust-windowing/winit/issues/3095): macOS에서 입력 소스를 한국어로 바꾼 직후 첫 자모가 조합 없이 확정됨 | 바꾼 직후 첫 글자만 "ㅇ안" 식으로 갈라짐 | 실제 2벌식으로 재현 확인, winit 갱신 추적 |
| egui [#7974](https://github.com/emilk/egui/issues/7974) / winit [#4524](https://github.com/rust-windowing/winit/issues/4524): macOS 2벌식 Option+Return 한자 변환 창이 안 열림 | 한자 입력 불가(이 앱에서는 드묾) | 추적만 |
| egui [#7975](https://github.com/emilk/egui/issues/7975): Linux X11 + Fcitx5에서 조합 중 글자가 입력란에 보이지 않음 | 확정은 되지만 조합 표시 없음. Wayland + IBus는 정상 | X11·Wayland 각각 확인, 알려진 제한으로 문서화 |
| winit [#4508](https://github.com/rust-windowing/winit/issues/4508): Windows에서 조합 중에도 `KeyboardInput`이 옴 | 단축키가 조합 중 키와 겹칠 수 있음 | 앱 단축키를 조합 중에는 무시하는지 시험 |
| egui [#8671](https://github.com/emilk/egui/issues/8671): 가려지거나 최소화된 창은 AccessKit 요청에 답하지 않음 | 창을 보이기 전까지 스크린리더가 트리를 못 받음 | 추적만 |
| egui [#8700](https://github.com/emilk/egui/issues/8700): `egui_extras::Table`에 표·행·셀 역할 없음 | 결과 목록을 표로 만들면 스크린리더가 구조를 못 읽음 | 목록은 역할을 직접 붙이거나 Table 대신 리스트로 |

## 측정

창 1600×1000 pt(3200×2000 px), 300 프레임 중 처음 10 프레임(텍스처 생성·셰이더 준비)은 뺐다.

| 렌더러 | 시험 | vsync | fps | 프레임 간격 p50 / p95 | 프레임 CPU p50 / p95 | RGB→ColorImage p50 |
| --- | --- | --- | --- | --- | --- | --- |
| wgpu(Metal) | 1080p 업로드 | 켬 | 60.3 | 16.7 / 17.0 ms | 5.3 / 6.0 ms | 1.24 ms |
| wgpu(Metal) | 1080p 업로드 | 끔 | 120.3 | 4.8 / 18.2 ms | 3.8 / 4.5 ms | 0.85 ms |
| wgpu(Metal) | 4K 확대·이동 | 켬 | 60.0 | 16.7 / 17.3 ms | 1.0 / 1.3 ms | — |
| wgpu(Metal) | 4K 확대·이동 | 끔 | 157.5 | 4.7 / 17.2 ms | 0.7 / 1.2 ms | — |
| glow(OpenGL) | 1080p 업로드 | 켬 | 120.4 | 7.8 / 12.7 ms | 3.9 / 4.4 ms | 0.82 ms |
| glow(OpenGL) | 1080p 업로드 | 끔 | 209.1 | 4.1 / 8.4 ms | 3.8 / 4.8 ms | 0.82 ms |
| glow(OpenGL) | 4K 확대·이동 | 켬 | 92.9 | 11.5 / 17.4 ms | 0.2 / 1.2 ms | — |
| glow(OpenGL) | 4K 확대·이동 | 끔 | 753.5 | 1.0 / 4.4 ms | 0.1 / 0.3 ms | — |

- 프레임 CPU에는 RGB→RGBA 변환, egui 레이아웃·tessellation, 텍스처 업로드 명령이 들어 있다. 30 fps(33 ms)에 비해 1080p 업로드는 6 ms 이하라 미리보기 재생의 병목은 디코드 쪽이다(4.4·FFmpeg 측정 참고).
- glow의 vsync 켬 수치가 60을 넘는 것은 이 장치의 macOS OpenGL 경로가 swap interval을 지키지 않기 때문으로 보인다. 렌더러 선택에는 영향이 없다.
- 4K 테스트 카드 생성과 업로드 요청은 8 ms였다.
- **wgpu(Metal)의 기본 최대 텍스처 크기는 8192**, glow는 16384다. 8192를 넘는 사진(예: 1억 화소급 11648×8736)은 원본 해상도로 한 장에 올릴 수 없으므로, 계획 6장의 "필요한 타일만 디코드"대로 타일 텍스처로 그린다. 어댑터가 더 큰 한도를 지원하면 장치 생성 시 요청할 수 있지만 GPU마다 다르므로 타일을 기본으로 한다.

## 글꼴

| 파일 | 내용 | 크기 |
| --- | --- | --- |
| NotoSansKR-Subset.otf | ASCII·Latin-1·문장 부호·CJK 기호·전각 + 한글 자모(U+1100–11FF, U+3130–318F) + 현대 한글 음절 11,172자 | 1.97 MB(12,260 code point) |
| NotoSansJP-Subset.otf | 같은 공통 범위 + 히라가나·가타카나 + JIS X 0208 한자(1·2수준 6,355자) | 2.13 MB(7,297 code point) |

원본은 `notofonts/noto-cjk` 태그 `Sans2.004`의 SubsetOTF(Regular), SIL OFL 1.1이다(`spikes/egui/fetch_fonts.sh`가 해시로 받아 `pyftsubset`으로 자름). 한국어 글꼴에는 한자를 넣지 않아 한자는 항상 일본어 자형으로 나온다.

- **이름 이체자가 빠진다.** 髙·﨑·德·瀨·栁·濵·彅(IBM 확장·JIS X 0213)는 JIS X 0208 밖이라 글리프가 없다(塚은 있음). 일본어 사용자의 이름·파일 이름에 흔하므로 M6에서 JIS X 0213 전체로 넓히거나(약 +1 MB 예상), OS 글꼴(macOS 히라기노, Windows Yu Gothic·Meiryo, Linux Noto CJK)을 런타임 fallback으로 더한다. 한국어 한자(KS X 1001 4,888자)도 같은 결정 대상이다.
- 굵은 글꼴은 넣지 않았다. 제목·강조에 굵기를 쓰면 같은 크기의 파일이 하나씩 더 필요하다.

## 네이티브 메뉴 설계

`--menu predefined|custom|plain` 세 가지로 만들었다. macOS에서 메뉴 항목에 단축키(key equivalent)를 걸면 키 입력이 winit 창에 오기 전에 메뉴가 가로채므로, egui 텍스트 입력의 ⌘C·⌘V·⌘Z가 계속 동작하는지가 쟁점이다.

| 방식 | 편집 메뉴 | 예상 |
| --- | --- | --- |
| predefined | muda가 AppKit 표준 항목(`copy:` 등 responder 메시지) | winit 뷰가 `copy:`를 구현하지 않으면 ⌘C가 egui에 오지 않을 수 있음 |
| custom | 자체 항목 + ⌘ 단축키. 선택되면 앱이 `Event::Copy`·`Cut`·`Paste`(클립보드는 `arboard`)·⌘Z·⌘A를 egui 입력에 넣음 | 단축키가 egui에 직접 가지 않아도 같은 결과 |
| plain | 자체 항목, 단축키 없음 | 키는 항상 egui로 감. 메뉴에 단축키 표시가 없음 |

결과에 따라 M6에서 한 방식을 고른다(절차 4).

## 사람이 할 확인 (macOS)

준비:

```bash
cd spikes/egui
./fetch_fonts.sh
cargo build --release
```

### 1. 한글·일본어 IME

```bash
./target/release/egui-spike
```

1. 시스템 설정 > 키보드 > 입력 소스에 "한국어 - 2벌식"과 "일본어 - 로마자 입력"이 있는지 확인한다.
2. "이름" 칸을 클릭하고 2벌식으로 `gksrmf`(한글)을 친다. 조합 중 글자가 칸 안 커서 위치에 보이는지, 다 친 뒤 "이름 글자 수: 2"인지 본다.
3. 이어서 확인할 것: 조합 중 Backspace(`한`에서 Backspace → `하`), 조합 중 Enter(마지막 글자가 두 번 들어가지 않는지), 조합 중 마우스로 다른 칸 클릭(반쯤 된 글자가 남거나 사라지는지 기록), 조합 중 ←/→, `ㅇ` 하나만 치고 Space.
4. "메모" 칸(여러 줄)에서 2와 3을 반복하고, 줄바꿈 직후 첫 글자 조합을 본다.
5. 일본어 입력으로 바꿔 `nihongo` → Space(변환) → Enter. 후보 창이 커서 근처에 뜨는지, 결과가 `日本語`인지 본다. 변환 중 Space를 여러 번 눌러 후보를 바꿔 본다.
6. 한자 변환: 한국어 입력에서 `한` 입력 후 Option+Enter(한자 변환)로 후보 창이 뜨고 `韓`이 들어가는지.
7. 결과를 항목별로 "정상 / 이상(증상)"으로 적는다. 이상이 하나라도 있으면 egui·winit 버전과 함께 기록한다(계획상 필수 항목).

### 2. VoiceOver

1. ⌘F5로 VoiceOver를 켠다. 앱 창을 앞에 둔다.
2. VO(Control+Option)+→로 차례로 이동하며 읽히는 내용을 적는다. 기대: 제목, 세 문장, "이름, 텍스트 필드", "메모, 텍스트 영역", "파일을 여기로 끌어 놓으세요, 버튼", "확대 초기화, 버튼", "4K 미리보기".
3. Tab/Shift+Tab 순서가 화면 순서와 같은지 본다.
4. "이름" 칸에서 VO+Space로 편집에 들어가 한글을 입력하고, 입력한 글자를 VoiceOver가 읽는지 본다.
5. "확대 초기화"에서 VO+Space로 눌러지는지 본다.
6. 한국어 문장이 한국어 음성으로, 일본어 문장이 일본어 음성으로 읽히는지(언어 태그가 없으면 시스템 언어 음성으로 읽힘) 기록한다.

### 3. Finder 파일 드롭

1. Finder에서 파일 하나를 창 위로 끌고 온다. 놓기 전에 버튼 문구가 "놓으면 추가: 1개"로 바뀌는지 본다.
2. 놓은 뒤 "받은 파일 1개"와 경로가 보이는지 본다.
3. 파일 여러 개, 폴더 하나, 이름에 한글·공백·이모지가 있는 파일, 외장 드라이브나 iCloud Drive의 파일로 반복한다. 폴더는 경로 하나로 들어오는지 기록한다.
4. 사진 앱에서 사진을 끌어 놓았을 때 무엇이 들어오는지(파일 경로, 아무것도 없음) 기록한다.

### 4. 네이티브 메뉴

세 방식 각각:

```bash
./target/release/egui-spike --menu predefined
./target/release/egui-spike --menu custom
./target/release/egui-spike --menu plain
```

1. 메뉴 막대에 "CloakFrame", "파일", "편집"이 보이고, CloakFrame > CloakFrame 종료(⌘Q)로 끝나는지.
2. 파일 > 파일 추가…(⌘O)를 메뉴로 한 번, 단축키로 한 번 → 왼쪽 "메뉴 이벤트" 수가 늘고 `open`이 찍히는지.
3. "이름" 칸에 글자를 쓰고 ⌘A, ⌘C, 다른 칸에서 ⌘V, ⌘Z가 동작하는지. 같은 동작을 편집 메뉴 클릭으로도 한다.
4. 한글 조합 중에 ⌘C·⌘V를 눌렀을 때 조합 글자가 어떻게 되는지.
5. 방식별로 3과 4의 결과를 표로 적는다.

### 5. 화면 배율·테마

1. 외부 모니터(1배)와 내장 화면(2배) 사이로 창을 옮겨 글자가 흐려지지 않는지.
2. 시스템 설정에서 밝게/어둡게를 바꿔 창이 즉시 따라오는지.

## 다시 돌리기

```bash
cd spikes/egui
./fetch_fonts.sh && cargo build --release && cargo test --release
./target/release/egui-spike --bench upload            # 1080p 업로드, JSON 출력
./target/release/egui-spike --bench zoom --renderer glow --no-vsync
```

## 계획에 반영할 것

- 3장 UI: egui/eframe 0.36.2로 고정. muda는 macOS·Windows만, Linux는 egui 자체 메뉴 막대. Windows는 winit `with_msg_hook`에서 `TranslateAcceleratorW`를 불러야 가속키가 동작한다.
- 6장 사진 검토: 원본 해상도 확대는 타일 텍스처(wgpu 기본 한도 8192).
- 글꼴: 부분 집합 기준(한글 전체·JIS X 0208)과 이름 이체자 처리 방식을 M6에서 정한다. 번들 크기는 현재 Regular 두 개 4.1 MB.
- 렌더러: wgpu를 기본으로 두고 glow를 대체 경로로 유지. GPU 없는 환경·원격 데스크톱에서 어느 쪽이 뜨는지는 Windows·Linux 절차로 확인한다.
- 11장 위험: egui 유지 확정. 공개 이슈 표의 항목은 M6 실제 OS 점검 목록으로 옮긴다.

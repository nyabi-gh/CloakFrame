# M0–M6 재검토

실행일: 2026-10-29. 대상: `m6-complete` `fa3f922`(main `87202a3` + 병합 기록, PR #72까지). 장치: Apple M4, macOS, 고정 FFmpeg 9.0.2·ORT 1.30.0. M6 완료 표시와 사진 전용 preview 게시 전에, 지금까지의 코드·문서·결정을 다시 보고 결함·위험·문서 오류를 찾은 기록이다. 코드는 고치지 않았다. 고칠 순서는 메인테이너가 정한다. 줄 번호는 `fa3f922` 기준이다.

## 방법

- 읽은 것: [계획](RUST_PORT_PLAN.md) 전체, [CHANGES](CHANGES_FROM_1.12.md), [M6_PLAN](M6_PLAN.md)·[M6_UX](M6_UX.md)·[M6_VERIFY](M6_VERIFY.md)·[M6_COREML_SPEED](M6_COREML_SPEED.md)·[PREVIEW_NOTES](PREVIEW_NOTES.md), [M0–M5 재검토](M0_M5_REVIEW.md), CI workflow, `manifests/`, `THIRD_PARTY_NOTICES.txt`, `tools/package/macos_preview.sh`.
- 코드는 영역 여섯(core, engine 사진·게시·세션·모델 파일, engine 미디어·영상, engine 실행·검토·CLI·감지, 앱 셸·실행·설정, 앱 편집기)과 문서 대조 하나로 나눠 하위 검토에 맡겼다. 결정(데이터 폴더, 고지, preview 판)은 직접 봤다. 영향이 큰 것은 다시 재현하거나 코드로 확인했고, 확인 방법을 항목마다 적었다.
  - **재현**: 직접 실행해 확인
  - **코드**: 직접 읽어 확인
  - **하위 검토**: 하위 검토의 추적만 있음
- 재현은 작업 트리에 추적하지 않는 임시 시험 파일을 넣고 같은 `target`에서 돌린 뒤 지웠다. 입력은 스크래치 폴더에 만들었고 저장소에는 넣지 않았다.
- 회귀 확인: `cargo fmt --check`, clippy 두 조합(기본·`--all-features`), `cargo test --workspace --locked --all-features`(569개 통과, 무시 4개), `cargo deny check` 모두 통과했다.

## 요약

| # | 심각도 | 영역 | 내용 | 확인 |
| --- | --- | --- | --- | --- |
| 1 | 결함 | 앱 편집기 | 내보내기 확인 창이 떠 있는 동안 뒤의 캔버스가 키를 받는다. Enter가 고른 얼굴을 "가리지 않음"으로 바꾸고, PageDown이 확인 창을 연 채 다음 항목으로 넘어간다 | 재현 |
| 2 | 결함 | 사진 | EXIF의 다른 항목(IFD1·Interop 포인터 등) 하나가 깨지면 Orientation도 버려, 옆으로 누운 사진을 그대로 감지한다 | 재현 |
| 3 | 결함 | 영상 미리보기 | HEVC open GOP의 leading picture는 정지 미리보기가 실패하고, 거기서 시작한 재생은 프레임 번호가 밀려 마스크가 다른 그림에 그려진다(P-3) | 재현 |
| 4 | 결정 불일치 | 앱 | 앱의 기본 가리기 방식은 모자이크인데 계획 5장·CHANGES·core·CLI는 단색 채우기다. 바꾼 결정 기록이 없다 | 코드 |
| 5 | 결함 | 앱 실행 화면 | 200개 넘는 실행에서 모든 행의 썸네일을 매 frame 요청해, 캐시에서 밀려난 것을 끝없이 다시 만든다 | 코드 |
| 6 | 결함 | 앱 실행 화면 | engine 진행률 갱신이 다시 그리기를 부르지 않아, 긴 항목 동안 진행률·남은 시간이 입력이 있을 때까지 멈춘다 | 코드 |
| 7 | 결함 | 앱 메인 | 다시 스캔하는 동안에도 시작할 수 있어, 실행이 이전 스캔 결과를 쓴다 | 코드 |
| 8 | 미룬 일 | Windows | `CREATE_NO_WINDOW`가 없어 GUI에서 FFmpeg·ffprobe를 부를 때마다 콘솔 창이 뜬다. M4에서 "M6 GUI 전"으로 미뤘다 | 코드 |
| 9 | 위험 | core 마스크 | 이미지 가장자리에 잘린 얼굴은 보이는 부분으로 칸 수·sigma를 정해, 고른 강도보다 약한 모자이크·블러를 받는다(D3) | 코드 |
| 10 | 결정 | Windows 배포 | 앱 데이터 폴더 `%LocalAppData%\CloakFrame`이 1.12.1 Velopack 설치 폴더와 같다. 1.12.1을 지우면 2.0의 세션·설정·모델도 지워진다 | 코드·문서 |
| 11 | 결정 | preview | 번들하는 ONNX Runtime(MIT)과 Rust crate의 고지가 THIRD_PARTY_NOTICES에 없다. FFmpeg와 같은 이유로 게시 전에 정해야 한다 | 코드 |
| 12 | 낮음 | 여러 곳 | 드문 경우의 결함·성능·표시 문제(아래 목록) | 항목별 |
| 13 | 문서 | — | 문서가 코드나 결정과 어긋난 곳(아래 목록) | — |

## 결함

### 1. 확인 창 뒤의 편집기가 키를 받는다

- **위치:** `editor.rs:150-156`이 캔버스를 먼저 그리고 `confirm_dialog`를 나중에 그린다. 캔버스는 포커스가 없으면 `request_focus()`를 다시 부른다(`editor.rs:1461-1465`). egui 0.37의 modal은 포커스를 가져가지 않고 Enter도 받지 않으므로, 확인 창이 열린 동안 캔버스가 키를 처리한다(`editor.rs:1626-1635`, 영상은 `video_editor.rs:230-234`·`537-546`).
- **재현:** 기존 편집기 시험과 같은 준비(lena 두 장, 검토 켬)로 Tab으로 얼굴을 고르고 "More → Export Without Anonymization…"을 연 뒤 Enter를 눌렀다. 확인 창이 열린 채 `is_excluded(0)`이 `true`가 됐다. 이어 PageDown을 누르자 확인 창이 열린 채 편집기가 `ItemId(1)`로 넘어갔다.
- **영향:** "경고 포함 내보내기" 확인 창에서 습관처럼 Enter를 누르면 고른 얼굴이 가리지 않음으로 바뀌고, 그대로 내보내면 그 얼굴이 드러난 채 저장된다. Delete는 그린 영역을 지운다. Tab이 캔버스에 묶여 키보드로 확인 창 단추에 갈 수 없다.
- **시험:** 지금 시험은 확인 창 단추만 누른다.

### 2. 깨진 EXIF 항목 하나가 방향을 버린다

- **위치:** `photo/decode.rs:314-325`의 `exif_orientation`과 `photo/metadata.rs:160`이 `exif::Reader::new()`를 쓰고, kamadak-exif 0.6.1은 기본으로 IFD 하나의 오류에 전체 해석을 버린다.
- **재현:** lena.jpg에 IFD0 Orientation=6만 있는 EXIF를 넣은 파일과, 같은 EXIF에서 IFD0의 다음 IFD 포인터만 파일 밖(0xFFF0)으로 바꾼 파일을 만들었다. exiftool은 둘 다 "Rotate 90 CW"를 읽고 뒤 파일에 "Bad IFD1 directory" 경고만 낸다. `exif_orientation`은 앞 파일 6, 뒤 파일 1을 돌려준다.
- **영향:** 사진을 돌리지 않은 채 감지해 누운 얼굴을 놓칠 수 있고, 놓친 얼굴은 근거 없이 게시된다. 출력에는 EXIF가 없어 원본과 다르게 누워 보인다. `--keep-metadata`의 태그도 조용히 사라진다(안전한 쪽).
- **제안:** 방향은 `continue_on_error(true)`와 부분 결과로 IFD0만 읽는다. 보존 태그(`keep`)는 엄격하게 둬도 된다.
- **시험:** 방향 시험은 모두 온전한 EXIF만 쓴다.

### 3. HEVC open GOP 중간의 미리보기

- **위치:** `media/decode.rs:270`의 `seek_to`(`-ss`로 keyframe에 seek), `source_frame`(`decode.rs:277-311`, `select=eq(pts,P)`), `FrameReader::start_at`(`decode.rs:141-184`, `select=gte(pts,P)`), `video/preview.rs:321-352`의 `Playback::next_frame`(번호를 스스로 셈).
- **재현:** `testsrc2` 4초를 libx264(`open-gop=1:bframes=3`)와 libx265(`open-gop=1:bframes=4`, x265 기본값)로 만들고, 모든 k에서 `source_frame(k)`와 `start_at(k)`의 첫 프레임을 전체 디코드와 비교했다. H.264는 120개 모두 같았다. HEVC는 28·29·58·59·87·88·89의 정지 미리보기가 "the requested video frame could not be decoded"로 실패했고, 그 프레임에서 시작한 재생의 첫 그림은 30·60·90번이었다.
- **영향:** 검토자가 이 프레임들을 볼 수 없고, 거기서 재생하면 끝까지 프레임 번호가 밀린 채 마스크가 다른 그림에 그려진다. 프레임 수 검사는 재생 끝에서야 실패한다. 내보내기는 0번부터 디코드하므로 출력은 맞다.
- **제안:** keyframe 하나 앞에서 seek하거나, 재생 프레임 번호를 디코드한 pts로 정하고 어긋나면 거절한다.
- **시험:** 지금 open GOP 시험은 잘라 낸 파일의 0번과 마지막 프레임만 본다.

### 4. 앱의 기본 가리기 방식

- 앱 `settings.rs:22-29`의 `Method` 기본값이 `Mosaic`이다(앱 첫 커밋 `ba7239a`부터). core `mask.rs:109-118`(`MaskStyle::default`, 주석 "plan 5")와 CLI는 `Fill`이고, 계획 5장 "익명화 기본값: 단색 채우기"와 CHANGES "기본 가리기 방식: 모자이크 → 단색 채우기"도 같다. M6 문서에 바꾼 결정이 없다.
- 영상 모자이크는 트랙별 고정 격자로 줄였지만 움직이는 얼굴은 막지 못한다고 기록했다([M5_RENDER](M5_RENDER.md#모자이크-격자)). 계획이 채우기를 기본으로 둔 이유와 맞물리므로, 앱 기본값을 채우기로 고칠지 계획을 바꿀지 정해야 한다.

### 5. 실행 화면의 썸네일 반복

- `app.rs:1553-1561`이 `ScrollArea::show` 안에서 보이는지와 상관없이 모든 행의 `thumbnails.get`을 부른다. 캐시는 200개(`thumbnails.rs:19`)이고, 밀려날 때 `asked`도 지워(`thumbnails.rs:96-101`) 다음 frame에 다시 요청한다.
- 300장을 실행하면 썸네일 하나가 생길 때마다 가장 오래된 것이 밀려나고, 그 repaint가 그것을 다시 요청한다. 실행 내내, 결과 화면에서도 썸네일 스레드가 사진을 디코드하고 영상은 FFmpeg를 띄우며 engine과 CPU를 다툰다. 파일 격자도 보이는 칸이 200개를 넘는 큰 화면에서 같은 일이 생긴다(칸 최소 120 pt).
- [M6_PLAN](M6_PLAN.md) 0단계의 "보이는 칸만 썸네일"과 다르다.

### 6. 진행률이 다시 그려지지 않는다

- engine `ProgressReporter::set`(`events.rs:49-56`)은 보드만 바꾸고, 앱은 메시지가 올 때만 repaint를 요청한다(`run.rs:142-146`). 앱에 `request_repaint_after`가 없다.
- 긴 영상 하나를 분석하는 동안 상태 메시지가 없으므로, 도구 막대 백분율·행 막대·남은 시간("계산 중")이 마우스를 움직일 때까지 멈춰 보일 것이다. 앱에서 손을 떼고 보는 확인이 필요하다.

### 7. 다시 스캔하는 동안 시작

- `start_blocker`(`app.rs:1370-1386`)가 `scanning`을 보지 않고, `rescan`(`app.rs:199-221`)은 이전 `found`를 남긴다. `start_run`은 `found`를 쓴다.
- 폴더를 더 넣고 바로 시작하면 처음 폴더만 처리하고 알리지 않는다. 처음 실행에서 `~/Photos`를 넣은 뒤 저장 위치로 `~/Photos/blurred`를 고르고 다시 스캔이 끝나기 전에 시작하면, 이전 결과(`blurred/*`)를 입력으로 다시 처리한다.

### 8. Windows 콘솔 창

- `media/process.rs:438-440`이 `CREATE_NEW_PROCESS_GROUP`만 준다. 앱은 `windows_subsystem = "windows"`이므로, 영상 열기·패킷 목록·미리보기 프레임마다·재생·인코드 시험마다 콘솔 창이 깜빡인다. [M4_VERIFY](M4_VERIFY.md)의 남은 것 "Windows 콘솔 창(`CREATE_NO_WINDOW`)과 Job 시점: M6 GUI 전"이 아직 남아 있다. Windows CI는 GUI 실행이 아니라 잡지 못한다.

### 9. 가장자리에 잘린 얼굴의 강도

- `padded_region`(`mask.rs:400-418`)이 이미지 안으로 자른 사각형으로 모자이크 칸 수(`mask.rs:592-595`), 트랙 격자 칸(`MosaicGrid::cell_for`, `mask.rs:177-186`), 블러 sigma(`mask.rs:693`)를 정한다.
- 1920×1080 사진에서 왼쪽으로 반 넘게 잘린 300×360 얼굴(보이는 폭 120 px, padding 0.18, 강도 8)은 칸이 약 51 px 대신 21 px이다. 얼굴 전체 기준으로 약 14칸이라 가장 약한 허용 강도 12보다 촘촘하다. 블러 sigma는 68 대신 29다. 프레임 가장자리에 머무는 영상 track도 같다.
- 가리는 pixel은 맞다. 강도는 잘리기 전 상자에서 정하고 쓰는 pixel만 자르면 된다.

## 결정

### 10. Windows 앱 데이터 폴더와 1.12.1 설치 폴더

- 1.12.1의 Velopack `packId`는 `CloakFrame`이다(`v1.12.1:.github/workflows/release.yml:635`). Velopack은 `%LocalAppData%\{packId}`에 설치하고, 제거할 때 그 폴더 전체를 지운다([Velopack](https://docs.velopack.io/integrating/uninstalling)).
- 2.0의 데이터 폴더(`app_dirs.rs:85-93`)가 같은 `%LocalAppData%\CloakFrame`이고, 세션·`settings.toml`·모델·캐시(`Cache`)가 모두 그 아래다.
- 그래서 1.x 사용자가 2.0을 새로 설치한 뒤 1.12.1을 지우면(새로 설치하라는 안내대로라면 자연스러운 순서) 2.0의 검토 대기 세션과 받은 모델이 지워진다. 2.0이 같은 `packId`를 쓰면 2.0을 설치할 때 1.12.1 설치를 덮고, 2.0 제거가 사용자 데이터도 지운다.
- M7 완료 기준 "1.12.1이 설치된 컴퓨터에 새로 설치해도 1.12.1의 설정·모델이 그대로"도 이 배치에 달려 있다. M7 전에 Windows 데이터 폴더 이름(또는 `packId`)을 정해야 한다. macOS·Linux는 해당 없다.

### 11. preview의 서드파티 고지

- preview zip은 `libonnxruntime.dylib`를 번들한다(`macos_preview.sh:29`). THIRD_PARTY_NOTICES에는 ONNX Runtime(MIT)과 그 안의 서드파티 고지, 링크한 MIT·Apache crate(eframe, egui, wgpu, jiff 등)의 고지가 없다.
- [M0–M2 재검토](M0_M2_REVIEW.md)는 이를 "M7에서 생성할지 정함"으로 남겼지만, preview는 M7 전에 배포된다. FFmpeg를 빼기로 한 결정(10.1)과 같은 판단이 필요하다. 게시 전에 ORT 고지를 넣고 crate 고지를 생성(예: cargo-about)하는 쪽을 추천한다.

### 그 밖에 정할 것

- **preview 판:** Cargo 판은 `2.0.0-alpha.0`이고 `preview.yml`이 권하는 tag는 `v2.0.0-preview.1`이다. 이대로면 `CloakFrame-Preview-2.0.0-alpha.0-…zip`이 `CloakFrame 2.0.0-preview.1` 릴리스에 올라가고 정보 창은 alpha.0을 보인다. `CFBundleVersion`도 `2.0.0-alpha.0`이다(Apple은 숫자 세 개를 기대한다).
- **preview가 고정하는 형식:** 세션 형식 2와 `settings.toml`은 그동안 "공개된 적 없음"을 이유로 판을 올리지 않고 바꿨다(M6 1·4단계). preview를 게시하면 사용자 데이터 폴더에 이 형식이 남으므로, 그 뒤의 변경은 판을 올리거나 거절 경로를 둬야 한다.
- **1.12.1 모델 복사:** 계획 0.1·CHANGES와 `60205f2`는 "M3에서 구현한 모델 폴더 복사는 그대로"인데, 2026-10-28 결정 메모에는 "모델 폴더 이전도 뺀다"로 남아 있다. 어느 쪽이 결정인지 확인이 필요하다. 코드는 앱의 모델 확인과 실행마다 복사한다(앱 `models.rs:76-80`, engine `startup.rs`).

## 낮음

- **앱**
  - 영상에서 M을 누른 채 편집하면 이전 편집으로 만든 가린 그림이 보인다. 응답을 프레임 번호와 가림 여부로만 맞춘다(`video.rs:164-167`·`219-234`·`284`). 하위 검토
  - 한 변이 8192 px를 넘는 영상(probe는 16384까지 받음)은 일시 정지 프레임을 원본 크기 텍스처로 올려 wgpu 한도를 넘는다(`video.rs:176-188`·`286-290`). release에서 panic. 하위 검토
  - 사진 타일 캐시가 넣은 순서로 밀어내(LRU 아님, `tiles.rs:21`·`266-273`) 보이는 타일이 96개를 넘으면 계속 다시 자른다. 하위 검토
  - 실행·검토 화면에서는 `notices`를 그리지 않는다(`app.rs:747-749`). 실행 스레드가 `Finished`를 보낸 뒤에야 세션 잠금을 풀므로(`run.rs:352`·`453`) 끝나자마자 "검토"를 누르면 `InUse`가 보이지 않는 알림으로 간다. 코드
  - 실행 스레드가 panic하면 보드가 끝나지 않아 "취소하고 종료"가 영원히 기다린다(`run.rs:125-127`·`157-163`). 하위 검토
  - macOS Dock의 "종료"·로그아웃은 winit이 `applicationShouldTerminate:`를 다루지 않아 확인·취소·세션 저장 없이 끝난다. Cmd+Q는 맞다. 하위 검토
  - 렌더 중에 검토를 떠나면(사이드바 "새 작업", 다른 세션 열기) 결과를 받지 않고 잠금을 푼다(`review.rs:222-232`·`321-325`). 하위 검토
  - 큰 실행에서 frame마다 행마다 `found.files`를 찾고(`app.rs:1555-1558`) 평균을 다시 계산한다(`progress.rs:40-60`). 1만 장이면 frame당 약 10^8번. 하위 검토
  - 디코드에 실패한 프레임을 frame마다 다시 요청한다(`video.rs:191-216`). 확대한 영상 프레임이 재생 줄·타임라인 위에 그려진다(`video_editor.rs:414`). 저장된 항목에서도 undo가 화면을 바꾼다. 끌기 도중 화살표 키를 누르면 undo 순서가 섞인다(`draft.rs:125-160`). Delete·Enter 자동 반복이 다음 항목에 이어진다. Windows "열기"가 쉼표 있는 이름을 나눈다(`shell.rs:52-57`). 쓰이지 않는 번역 키 5개(`found-summary`, `no-output-folder`, `close`, `session-open`, `sample-title`). 하위 검토
- **engine**
  - 세션 폴더 fsync가 rename 뒤에 실패하면 `save`가 오류를 내고 `keep_for_review`가 방금 쓴 세션을 지운다(`session.rs:173-181`, `batch.rs:89-93`). 게시는 같은 경우를 경고로 둔다. 코드
  - WebP ICCP chunk는 선언한 크기만큼 먼저 할당한다(image-webp 0.2.4). 100바이트 파일로 4 GiB를 요청할 수 있고 Windows에서는 commit 한도로 프로세스가 죽을 수 있다. PNG iCCP와 같은 종류(M0–M2 11번). 하위 검토
  - 원본 바이트(최대 2 GiB)를 메모리 permit 전에 읽고, permit을 기다리는 동안 들고 있다(`process.rs:205-209`, `decode.rs:92-112`). 앱은 실행·썸네일·타일에 예산을 셋 만들고 각각 RAM의 4분의 1을 가정한다. 하위 검토
  - 90°로 회전하는 4:2:2 JPEG는 출력도 원본 축의 4:2:2라 회전 뒤 축이 바뀐다(D6 품질, `encode.rs:150-185`). 하위 검토
  - 첫 취소나 항목 실패 뒤 CLI가 정상 반환할 때 추론 스레드가 남은 요청을 아직 돌린다(`detect/mod.rs:131-166`). M4 재검토 9번의 SIGABRT와 같은 위험. 하위 검토
  - 마지막 취소 확인 뒤 `sync_all` 중의 취소는 그대로 게시한다. CLI 안내 "더 게시하지 않음"과 다르다(`run.rs:656-660`, `publish.rs:76`). 하위 검토
  - `process_ordered`의 진행 중 상한이 앞 항목에 묶여, 긴 영상 뒤의 사진들이 일꾼을 놀린다(`ordered.rs:72-74`). 처리량만. 하위 검토
  - CLI가 조용히 무시하는 플래그: `review`의 `--output`·`--review`·입력, `run`의 `--session`·`--decisions`, 영상의 `--keep-metadata`·`--keep-gps`. 하위 검토
  - CoreML 캐시 폴더를 만들지 못하면 CoreML 세션 전체가 CPU로 떨어진다(`runtime.rs:219-222`). 하위 검토
  - 길이 정보 없는 한 프레임 WebM은 출력 프레임 0개로 일반 인코드 오류가 된다(`timeline.rs:444-482`). 하위 검토
- **core**
  - 사용자 이미지 방식은 영역 pixel마다 16바이트 사본을 만들어 사진 메모리 추정을 넘을 수 있다(`mask.rs:887-996`). 앱에는 아직 없다. 하위 검토
  - `max_interpolation_gap: 0`이면 구멍을 채우지도 보고하지도 않는다(`tracking/post.rs:89`). engine은 1 이상으로 강제한다. 하위 검토
  - 이미지 밖의 수동 영역은 가장자리 1 px 줄을 가린다(`review.rs:355-363`). 하위 검토

## 문서

- **1.x 그대로인 문서:** README(SCRFD 사용자 모델 `:86-87`, Redactly 설정이 넘어옴 `:89-90`, `PATH`의 ffmpeg `:83-84`, 검토를 켜라 `:42`(기본 켬), Windows 10 이상 `:25`(1903+), 업데이트 확인 `:31`·`:59-61`, 계획 10장이 요구하는 "새로 설치" 안내 없음), [ROADMAP](ROADMAP.md) 전체(Qt·clang-tidy·`OnnxGraphPatch`, 이미 끝난 모자이크·타일·검토 항목), [RELEASING](RELEASING.md)(`:3-6` "update feeds … carry over", 지운 `CloakFrame-updates` 저장소와 토큰 `:32-36`·`:126-129`, 죽은 링크 `:32`), [SECURITY](../SECURITY.md)(Qt·OpenCV·exiv2·libsodium `:54-57`, staging `:34`, 사용자 모델 `:39-40`, 소유권 검사 `:52-53`). 계획 5장은 위협 모델이 "SECURITY.md와 같음"이라고 한다. CONTRIBUTING `:200-203`은 ROADMAP을 현재 할 일로 안내한다.
- **서명 키:** RELEASING은 키를 이어 쓴다고 하고 ROADMAP은 교체에 두 키를 믿는 중간 릴리스가 필요하다고 한다. 1.x 업데이트 경로가 없으므로 2.0이 키를 바꿀 자연스러운 시점이다.
- **M6_VERIFY `:9`:** preview가 "ORT·FFmpeg"를 넣는다. 사진 전용 결정(`:109`) 뒤로 ORT만 넣는다.
- **계획 3장:** 글꼴 "Regular 2개 4.1 MB, 이름 이체자 처리는 M6"(지금은 Regular·Medium 넷, 약 15 MB). 쓰지 않는 crate(Rayon, `tracing`, `thiserror`, `fast_image_resize`)와 `windows` crate(실제는 `windows-sys`). "eframe MSRV(0.36.x 기준)"와 참고 자료의 eframe 0.36.2.
- **manifest:** `native.toml`에 Medium 글꼴의 URL·해시가 없다(`tools/fonts/subset.sh:20-21`에만). `rust.toml`에 `jiff =0.2.38`이 없고 `egui_kittest`의 `eframe` 기능이 빠졌다.
- **계획 4.1·7장:** crate 이름 `cloakframe-desktop`(실제 `cloakframe-app`), 저장소 배치 `assets/{icons,fonts,locales}`·`tests/{golden,integration}`.
- **계획 6장:** 가리는 방법에 "이미지"(M6에서 뺌, [M6_UX](M6_UX.md) 1단계 결정).
- **설정 이전 흔적:** [M6_PLAN](M6_PLAN.md) `:13` "1.12.1 QSettings 이전은 M7", 계획 M1 줄 "이월: 1.12 설정 이전(M7)". 2026-10-28 결정 뒤로 남았다.
- **감지 모델 창:** PREVIEW_NOTES `:5`와 CONTRIBUTING `:100`의 "Detection Models window"는 고급 옵션 시트의 절로 바뀌었다. PREVIEW_NOTES는 릴리스 설명이 된다. CHANGES `:31` "(앱 화면은 M6)"도 낡았다.
- **CHANGELOG `:3`:** 지금 트리에 없는 release workflow를 말한다.
- **수치:** CoreML batch 고정 효과가 계획(1패스 25–28%), CHANGES(영상 분석 28%), M6_COREML_SPEED(벤치마크 1패스 26%, 클립 전체 28%, 클립 1패스 39%, 구현 절 25%)마다 다르다. M5 끝 누적 일수가 M6_PLAN(9일)과 M6_VERIFY(12일)에서 다르다.
- **참조:** M6_VERIFY `:14` "계획 2장 지원 OS"(1.1절). 계획 머리말의 M6 기록 목록에 M6_UX·M6_COREML_SPEED가 없다.
- 그 밖의 상대 링크와 앵커는 모두 맞다.

## 확인했고 문제없는 것

- **게시 계약:** 취소·실패·미해결 근거가 있는 항목에 `PublishPermit`이 나가거나 게시되는 경로가 없다. 원본 identity는 패스 사이와 게시 직전에 비교한다. no-replace rename과 임시 파일 정리, 세션 원자 저장, 모델 해시·크기·호스트 검사가 맞다.
- **메타데이터:** EXIF는 allowlist로 새로 쓰고 IFD1·썸네일·MakerNote는 버린다. GPS는 동의할 때만. 영상은 metadata·chapter를 모두 버린다.
- **마스크:** 모든 방식·모양·soft edge·padding 조합에서 감지 상자 안 pixel은 가중치 1이다. P-6(상자가 줄지 않음)도 맞다.
- **시간축:** 출력 프레임 수, NTSC 짝, VFR, 시작 상한, open GOP 시작 처리가 맞다. 분석·내보내기·P-4는 0번부터 디코드하고 프레임 수를 비교한다.
- **검토 결정:** `ForeignKey` → 수정 번호 → 설정 → 원본 순으로 거절하고, 해결 표시는 결정마다 다시 정한다. 앱은 세션의 현재 수정 번호와 키로 보낸다. gap은 덮거나 오탐으로 고를 때만 해결된다.
- **CoreML batch 고정(#72):** `-batch1`은 YOLO5Face-n에만 붙고, 정리는 이 빌드가 쓰는 폴더를 지우지 않으며, override는 CoreML 세션에만 건다. initializer를 바꾼 번호판 세션은 캐시하지 않는다.
- **앱 설정:** 깨진 `settings.toml`은 기본값으로 열고 덮지 않는다. 설정이 engine 실행 설정과 fingerprint까지 이어진다. 저장 위치를 고르기 전에는 시작할 수 없다. 창 닫기·Cmd+Q는 확인 → 취소 → 세션 저장 → 닫기 순이다. 번역 세 언어의 자리표시자가 같다.
- **FFmpeg 인자:** `-protocol_whitelist file`, `file:` 입력, 숫자만 넣는 필터 인자, ISO 639 언어 코드 검사로 파일 이름이 인자가 되지 않는다.

## 추천 순서

1. M6 완료·preview 전: 1(확인 창 키) → 2(EXIF 방향) → 3(HEVC 미리보기) → 4(기본 방식 결정) → 5·6·7(실행 화면) → 9(가장자리 강도) → 11(고지)과 preview 판·PREVIEW_NOTES 문구
2. Windows 배포(M7) 전: 8(콘솔 창), 10(데이터 폴더)
3. 문서: 1.x 문서(README·ROADMAP·RELEASING·SECURITY)는 preview 게시 전에, 나머지는 다음 문서 커밋에
4. 낮음 목록은 M8 품질 비교 전에 다시 본다

## 처리(2026-10-29, `m6-review`)

`m6-complete` `6ff88d1`에서 브랜치를 만들어 항목마다 실패하는 시험을 먼저 넣고 고쳤다. 결정 항목은 시작할 때 한 번에 물었고 메인테이너가 모두 추천대로 답했다.

| # | 커밋 | 처리 |
| --- | --- | --- |
| 1 | `5bd030d` | 원인은 egui 0.37 modal이 뒤 레이어의 `request_focus`를 막지 않는 것(Tab 이동만 막음). modal 뒤의 위젯(사진·영상 캔버스, 영상 타임라인)은 포커스를 내놓고 다시 요청하지 않는다(`editor::behind_modal`). 편집기 시험에 확인 창이 열린 동안 Enter·PageDown·Tab을 더함 |
| 2 | `b1d1d6d` | 방향만 `continue_on_error(true)`의 부분 결과로 IFD0에서 읽는다. 보존 태그(`metadata::keep`)는 엄격하게 둠. IFD1 포인터가 파일 밖인 EXIF 시험 |
| 3 | `075b279` | 원인은 seek: `-ss`가 DTS 기준으로 CRA에 내려앉아, CRA 뒤에 디코드하지만 앞 GOP를 참조하는 RASL(28·29번 등)을 만들 수 없음. 패킷 목록의 keyframe을 `SourceFrames::with_keys`로 넘기고, 미리보기·재생은 프레임 pts 이하의 마지막 keyframe으로 seek한다(`seek_frame`). H.264·HEVC open GOP 4초의 모든 프레임에서 정지 미리보기와 재생 첫 프레임이 전체 디코드와 같음 |
| 4 | `61065c5` | 결정(추천대로): 앱 기본값을 채우기로, 계획·CHANGES·core·CLI와 같게. 저장된 설정·세션은 고른 값을 그대로 씀. 기본 설정의 `RunOptions::style()`이 core `MaskStyle::default()`와 같은지 시험 |
| 5 | `d6c75c0` | 실행 목록은 보이는 행만 썸네일을 요청하고(행 찾기도 그때만), 캐시는 직전 pass에 보인 것을 밀어내지 않고 가장 오래 안 보인 것부터 밀어낸다. 300행 실행에서 요청 수, 250칸이 보이는 캐시 시험 |
| 6 | `963d360` | 실행이 끝나기 전에는 실행 화면이 250 ms마다 다시 그리기를 예약한다(`request_repaint_after`). 엔진 보드는 그대로(UI가 보드를 읽는 쪽이 다시 그리기를 맡음). 실행 화면의 repaint 지연 시험 |
| 7 | `a60fab9` | 스캔 중에는 시작을 막고 "사진과 동영상을 다 찾을 때까지 기다리세요"를 보인다(세 언어, 일본어는 검토 필요). 시작 조건 시험에 다시 스캔을 더함 |
| 9 | `1979c83` | 강도(모자이크 칸, 트랙 격자 칸, 블러 sigma)를 이미지로 자르기 전의 padding 상자(`Grown`)에서 정하고, 쓰는 pixel만 자른다. 칸 경계는 자르기 전 상자에 놓고 이미지 안에 걸치는 칸만 계산한다. 이미지보다 큰 상자의 블러는 이미지 긴 변 크기로 셈(box pass 첫 합이 kernel 크기에 비례). 엔진 시험 전체가 그대로 통과 |
| 11 | `ad95dd2` | 결정(추천대로): THIRD_PARTY_NOTICES에 ONNX Runtime(MIT) 절, 패키지에 ORT의 LICENSE·ThirdPartyNotices를 `ONNXRUNTIME_*.txt`로, crate 고지는 cargo-about 0.9.2(`tools/about`, deny.toml 허용 목록, 앱 자체 crate 제외)로 `RUST_CRATE_NOTICES.txt`를 만든다. `preview.yml`이 cargo-about을 고정 판으로 설치. 로컬에서 zip을 만들어 파일 다섯 개를 확인 |
| preview 판 | `fb4816d` | 결정(추천대로): workspace 판 `2.0.0-preview.1`, `CFBundleVersion`·`CFBundleShortVersionString`은 `2.0.0` |
| 모델 복사 | — | 결정(추천대로): 계획 0.1·CHANGES대로 M3의 모델 폴더 복사를 유지한다. 2026-10-28 메모가 틀렸고 코드·문서는 그대로 |
| 13 | 문서 커밋 | PREVIEW_NOTES·CONTRIBUTING·CHANGES의 감지 모델 창, M6_VERIFY의 ORT·FFmpeg와 1.1절, M6_PLAN의 설정 이전·누적 일수, 계획 3장(글꼴, 쓰지 않는 crate, `windows-sys`, MSRV, 참고 자료)·4.1·7장(crate 이름, 배치)·6장(이미지)·머리말 목록, CoreML 수치(측정 이름을 붙임), manifest(Medium 글꼴 둘, jiff, kittest `eframe`), CHANGELOG `:3` |

남은 것:

- **8·10번:** Windows 배포(M7) 전.
- **1.x 문서:** README·ROADMAP·RELEASING·SECURITY는 고치지 않았다. README를 2.0 기준으로 쓸지(최신 릴리스는 아직 1.12.1), 서명 키를 2.0에서 바꿀지 정해야 한다. preview 게시 전에 한다. 아래 "1.x 문서" 절에서 처리.
- **12번(낮음):** M8 품질 비교 전.
- 검사(로컬 macOS, 문서 커밋 전): `cargo fmt --check`, clippy 두 조합(기본·`--all-features`, `-D warnings`), `cargo test --workspace --locked --all-features`(577개 통과, 새 시험 8개, 무시 4개), `cargo deny check` 모두 통과. Windows·Linux CI는 아직 돌지 않았다.
- **병합(2026-10-29):** PR #73으로 올려 CI 13개(형식·린트·라이선스, 3개 OS 시험, fuzz, ORT 3개, 탐지·영상 벤치마크, 1패스 처리량 3개)가 통과했고 main `78bff36`으로 fast-forward했다. 탐지 벤치마크 기본 설정 얼굴 재현율 87.9%, 영상 추적 벤치마크 노출 프레임 타일 끔 5,322·켬 912로 기록과 같다(9번은 가리기 강도만 바꿈).

## 1.x 문서(2026-10-29, `preview-docs`)

main `78bff36`에서 브랜치를 만들었다. 결정은 시작할 때 한 번에 물었고 메인테이너가 모두 추천대로 답했다.

- **README:** 다운로드 링크와 배지가 가리키는 판은 preview를 게시해도 1.12.1이다(prerelease는 latest가 되지 않음). 그래서 사용법·지원 형식·FAQ는 1.12.1 설명으로 두고, "CloakFrame 2 preview" 절을 더했다(main은 2.0, macOS 사진 전용 preview, 새로 설치, 1.12.1과 함께 둠, 설정은 옮기지 않고 받은 모델은 같이 씀). 직접 빌드할 때의 FFmpeg 안내는 CONTRIBUTING으로 보낸다. 2.0 전체로 다시 쓰는 것은 M8.
- **새로 설치 안내:** README 2.0 절과 PREVIEW_NOTES "Coming from 1.x". 계획 10장에 적음.
- **서명 키:** 2.0은 1.x 업데이트 키를 믿지 않는다. M7에서 Sparkle·Velopack 키를 새로 만들어 `release` environment 비밀로만 둔다. 저장소 범위 비밀 8개와 공개 키 변수 2개는 메인테이너가 지운다(Apple 인증서·공증 키는 M7에서 다시 넣음). RELEASING "Signing keys", 계획 10장.
- **RELEASING:** 1.x 절차(CMake, `release.yml`, `CloakFrame-updates`)를 빼고 `v1.12.1` 태그와 `78bff36`의 파일 이력으로 안내. preview 절차(판, PREVIEW_NOTES, `preview.yml`, 초안 확인 뒤 게시)와 서명 키 원칙만 둔다.
- **SECURITY:** 지원 판은 2.0(1.12.1은 고치지 않음, 2026-10-16 재작성만 결정). staging 파일, 사용자 모델, 소유권 검사, Qt·OpenCV·exiv2·libsodium을 빼고 계획 5장 위협 모델(같은 사용자의 다른 프로세스와 다른 계정은 범위 밖)과 맞춤.
- **ROADMAP:** 2.0의 남은 일(M6 앱 점검, M7 설치본·업데이트·8·10번·FFmpeg 대응 소스·약관, M8 비교·실제 OS·일본어 검토·낮음 목록·릴리스 문서, M9, 메인테이너 할 일, upstream turbojpeg-sys, 받아들인 위험)로 다시 씀. 1.x 항목은 `78bff36`의 파일 이력으로 안내. CONTRIBUTING의 ROADMAP 안내 문장도 맞춤.
- **병합(2026-10-29):** PR #74로 올려 CI 4개(형식·린트·라이선스, 3개 OS 시험)가 통과했고 main `44039fb`로 fast-forward했다.

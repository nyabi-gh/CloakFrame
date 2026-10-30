# M7: Windows·Linux 업데이트

작성 2026-10-30, 브랜치 `m7-updates`(main `d12d2f6` 위). [ROADMAP](ROADMAP.md) M7 "Updates"의 Velopack 부분과 결함 P-7([계획](RUST_PORT_PLAN.md) 3.2)이다. macOS Sparkle, 실제 키 생성, 릴리스 workflow는 남았다.

## 동작

- **확인:** 앱이 시작할 때(설정 `check_updates`, 기본 켬) 또는 도움말 메뉴의 "업데이트 확인…"으로 feed를 읽는다. 시작할 때의 확인은 새 버전이 있거나 업데이트를 거부했을 때만 알린다. 끄는 메뉴 항목 "시작 시 업데이트 확인"은 진행 중인 시작 시 확인의 답도 버린다(계획 10장). macOS에는 메뉴 항목이 없다(Sparkle은 남은 일).
- **알림:** 파일 목록 위의 비모달 줄(계획 10장 "시작 시 모달 금지")에 "업데이트"·"나중에"를 둔다. 받는 동안 진행 막대, 받은 뒤 "지금 다시 시작"·"나중에". 작업(실행, 검토의 렌더)이 진행 중이면 다시 시작 버튼을 막는다. 문구는 1.12.1의 번역을 가져왔고 새 문구의 일본어는 `## needs review`에 둔다.
- **feed:** `https://github.com/nyabi-gh/CloakFrame/releases/download/updates/`의 `releases.<channel>.json`, 패키지, `<패키지>.sig`. 앱의 HTTPS 클라이언트(engine `model_files::download::Https`, OS 인증서 저장소, GitHub 호스트 allowlist)로 읽는다. Velopack의 `HttpSource`는 feed 요청마다 설치된 버전과 무작위 사용자 ID를 붙이므로 쓰지 않는다. feed 4 MiB, 서명 1 KiB를 넘으면 읽지 않고, 파일 이름은 영숫자와 `.-_`만 받는다(feed가 packages 폴더 밖을 가리키지 못함).
- **서명:** 1.12.1과 같은 v2 텍스트(`CloakFrame update v2`, channel, version, file, sha256)에 대한 Ed25519 서명을 build가 고정한 공개 키로 확인한다(`cloakframe-engine/src/update.rs`). 1.12.1 시험 기록(`tests/fixtures/legacy/update_signature_tests`)의 호출 30개를 그대로 재생해 같은 답을 내고, 1.12.1이 통과시키던 키 없는 빌드(`unpinned`)는 거부한다.
- **설치 전 확인:**
  - 받기 전에 같은 이름의 파일이 서명된 SHA-256과 다르면 지운다. Velopack은 그 이름의 파일이 있으면 해시 없이 받기를 건너뛴다.
  - 받은 뒤와 적용 직전에 파일의 SHA-256을 서명된 값과 다시 비교한다.
  - delta 패키지는 쓰지 않는다. 앱이 base release 없는 `UpdateInfo`를 만들어 넘기고, vpk는 `--delta None`으로 만든다. Velopack은 delta로 다시 만든 패키지를 해시하지 않는다.
  - Velopack의 시작 시 자동 적용(`set_auto_apply_on_startup`, 기본 켬)을 끈다. 켜 두면 packages 폴더에 있는 더 높은 버전의 패키지를 서명 확인 없이 설치한다.
- **Linux packages 폴더:** AppImage용 Velopack 기본값 `/var/tmp/velopack/<id>/packages`는 모든 계정이 쓸 수 있어, 사용자의 캐시 폴더(`$XDG_CACHE_HOME/cloakframe/updates`)로 바꾼다. 1.12.1은 같은 문제를 폴더 소유권 검사로 막았다.
- **P-7:** Windows·Linux release 빌드는 `CLOAKFRAME2_UPDATE_PUBLIC_KEY`가 없거나 32바이트 키의 base64 모양이 아니면 `build.rs`에서 실패한다. 키 없는 debug 빌드는 업데이트를 확인하지 않고, 사용자가 확인하면 이유를 보인다.
- **시험용 feed:** `CLOAKFRAME_UPDATE_FEED`에 폴더를 주면 그 폴더에서 feed와 패키지를 읽는다. 서명 확인은 같으므로 고정 키가 서명한 것만 설치된다.
- **서명 도구:** `tools/package/sign_updates.py <폴더> <channel>`이 feed에 적힌 full 패키지마다 해시를 feed와 비교하고 서명해 `.sig`를 쓴다. 개인 키의 공개 반쪽이 `CLOAKFRAME2_UPDATE_PUBLIC_KEY`와 다르면 서명하지 않는다. OpenSSL 3을 쓴다.

## 계획과 다르게 한 것

- **downgrade 차단:** 계획 10장은 "최고 설치 버전을 기록"한다. 기록은 두지 않았다. 서명이 버전을 덮어 서명된 패키지를 다른 버전으로 내놓을 수 없고, 앱은 설치된 버전보다 높은 버전만 받는다(`AllowVersionDowngrade: false`, 앱에서 semver로 다시 비교). 기록이 더 막는 것은 사용자가 직접 낮은 버전을 설치한 뒤의 업데이트뿐인데, 그때 최신으로 가는 것은 막을 이유가 없다.
- **Ed25519 구현:** 계획 표는 `ed25519-dalek`이지만 rustls가 이미 쓰는 `ring` 0.17.14로 확인한다. base64 0.23.1도 ureq가 이미 쓰는 판이라 새로 들어온 crate가 없다(`Cargo.lock` 변화는 의존성 줄 3개).
- **키 이름:** 1.x의 `CLOAKFRAME_UPDATE_PUBLIC_KEY`·`_PRIVATE_KEY`와 겹치지 않게([RELEASING](RELEASING.md#signing-keys), 1.x 키는 2026-10-30에 지움) 새 키는 `CLOAKFRAME2_UPDATE_PUBLIC_KEY`·`CLOAKFRAME2_UPDATE_PRIVATE_KEY`다.
- **velopack `public-utils` feature:** `UpdateSource::get_release_feed`가 받는 `bundle::Manifest`가 이 feature 없이는 crate 밖에서 이름을 쓸 수 없다. 의존성은 늘지 않는다.

## 시험

- 단위·UI: engine `update::tests` 4개(1.12.1 기록 재생 포함), app `tests/updates.rs` 7개(stand-in backend로 시작 시 확인·나중에·끈 확인·수동 확인·거부·업데이트 불가 빌드·진행 중 확인 버리기). 앱 테스트 전체 통과.
- 빌드: 키 없이 `cargo build --release -p cloakframe-app`, 그리고 `CLOAKFRAME2_UPDATE_PUBLIC_KEY=abc`는 둘 다 `build.rs`에서 실패.
- 이 PC(Windows 11 Pro 26300)에서 시험용 키 쌍으로 `windows.ps1`이 만든 2.0.0-preview.1(A)과 워크스페이스 버전만 올린 2.0.0-preview.2(B)를 썼다. A는 `Setup.exe --silent`로 설치하고 앱은 `CLOAKFRAME_UPDATE_FEED`로 시험 feed를 읽혔다. 버튼은 UI Automation, 메뉴는 `GetMenu`로 찾은 항목에 `WM_COMMAND`를 보내 눌렀다.

| feed / 상황 | 결과 |
| --- | --- |
| B, 시험 키로 서명 | 시작 시 "CloakFrame 2.0.0-preview.2 버전을 사용할 수 있습니다." → 업데이트 → 다시 시작. 새 프로세스로 다시 떴고 `current\sq.version`이 preview.2 |
| B 설치 상태에서 A(서명됨) | 시작 시 아무것도 없음. 수동 확인은 "최신 버전을 쓰고 있습니다" |
| 다른 키로 서명 | 시작 시 거부 줄. 받지 않음 |
| `.sig` 없음 | 거부 |
| feed의 Version만 preview.9로 바꿈 | 거부 |
| 패키지 1바이트 바꿈(feed·서명 그대로) | 업데이트를 누르면 거부. packages에 남은 것 없음 |
| packages에 바뀐 패키지를 같은 이름으로 넣고 B로 업데이트 | 지우고 다시 받아 해시가 feed와 같아짐, preview.2로 업데이트 |
| packages에 바뀐 preview.2 패키지를 두고 A 시작(빈 feed) | 설치하지 않고 preview.1로 실행 |
| 메뉴 "시작 시 업데이트 확인" | `settings.toml`에 `check_updates = false` |
| 서명 도구에 다른 개인 키 | "every client would refuse these updates"로 멈춤 |

시험 뒤 설치를 제거하고 `CloakFrameData`를 지웠다. 시험 키는 scratchpad에만 있었다.

Linux 경로(packages 폴더 바꾸기)는 컴파일만 CI에서 확인하고 실행해 보지 않았다. Linux Velopack 설치본이 아직 없다(ROADMAP "Installers").

## 남은 것

- 키 쌍은 2026-10-30에 만들어 등록했고, 서명과 feed 게시는 릴리스 workflow가 한다([M7_RELEASE](M7_RELEASE.md)). 첫 릴리스에서 그 job들이 실제로 돌아야 한다.
- macOS Sparkle(새 EdDSA 키, `SURequireSignedFeed`, `SUVerifyUpdateBeforeExtraction`).
- Windows 10 1903 VM과 Linux 설치본에서 업데이트.

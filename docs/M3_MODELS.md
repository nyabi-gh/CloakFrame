# M3 모델 파일: 고정 목록·모델 폴더·다운로드·1.12.1에서 넘어오기

실행일: 2026-10-19. 장치: Apple M4, macOS 27.0. [Rust 포팅 계획](RUST_PORT_PLAN.md) 8장 M3 4단계와 4.6의 조사·결정·구현 기록이다. 코드는 `crates/cloakframe-engine/src/model_files/`(`mod.rs`, `download.rs`), `legacy.rs`, `app_dirs.rs`, CLI(`src/bin/cloakframe-cli.rs`)에 있다.

## 결론

- **고정 목록:** `model_files::MODEL_FILES`가 모델마다 이름·URL·SHA-256·크기·이용 조건을 담는다. 이름과 해시는 감지기 상수(`detect::yolo5face::MODEL_SHA256` 등)를 그대로 쓰고, URL·크기·license·source가 `manifests/native.toml`과 같은지 시험한다.
- **모델 폴더(2026-10-19 결정):** `app_dirs::model_dir()`, 곧 앱 데이터 폴더의 `models`다. 이용 조건에 동의하고 받은 파일이고 정리 도구는 캐시 폴더를 비우므로 캐시 폴더에 두지 않는다. CLI `--models`의 기본값이다.

  | OS | 모델 폴더 |
  | --- | --- |
  | macOS | `~/Library/Application Support/CloakFrame/models` |
  | Windows | `%LOCALAPPDATA%\CloakFrame\models` |
  | Linux | `${XDG_DATA_HOME:-~/.local/share}/cloakframe/models` |

- **1.12.1 모델(2026-10-19 결정: 복사):** 1.12.1은 Qt `GenericDataLocation` 아래 `CloakFrame/models`에 받았고, 옛 이름 `Redactly/models`도 읽었다(`ModelCatalog.cpp`). macOS·Windows에서는 이 폴더가 새 모델 폴더와 **같은 경로**라 그대로 쓴다(로드할 때 해시로 확인). Linux(`~/.local/share/CloakFrame/models`, 새 폴더는 소문자)와 세 OS의 `Redactly/models`에 있는 모델은 앱 폴더에 그 모델이 없을 때 해시를 확인하며 복사하고 원본은 둔다(`ModelStore::import`). CLI가 앱 폴더를 쓸 때(`--models`를 주지 않았을 때) `run`(그 실행이 감지할 모델만), `models`, `models fetch`가 복사한다. `--models DIR`로 준 폴더에는 복사하지 않는다. 1.12.1이 찾던 나머지 위치(앱 옆·번들 안·작업 폴더의 `models`)는 보지 않는다.
- **설치:** 모델 폴더 안에 `.cloakframe-tmp-<random>.onnx`를 만들어 64 KiB씩 쓰며 SHA-256을 계산한다. 고정 크기를 넘는 순간 멈추고, 끝나면 크기와 해시를 확인한 뒤 fsync하고 모델 이름으로 rename한다(같은 이름의 다른 파일은 바꾼다). 실패·취소하면 임시 파일을 지우고, 강제 종료로 남은 하루 넘은 임시 파일은 다음 설치가 지운다. 폴더는 기본 권한으로 만들고 권한은 보지 않는다(D5).
- **다운로드(2026-10-19 결정: ureq):** HTTPS만, 허용 호스트는 `github.com`과 GitHub가 다운로드를 넘기는 `release-assets.githubusercontent.com`·`objects.githubusercontent.com`(예전 릴리스 자산 호스트)·`media.githubusercontent.com`(Git LFS, YuNet). redirect는 자동으로 따라가지 않고 한 단계마다 allowlist로 검사한다(최대 5번, 사용자 정보·포트·IP 주소 거부). 인증서는 OS 저장소로 확인하고(`rustls-platform-verifier`), 암호 구현은 ring이다. 시간 한도는 연결·응답 30초, 본문 전체 15분(1.12.1의 전체 한도). proxy는 `HTTPS_PROXY`·`NO_PROXY` 등 환경 변수를 따른다. 자동 재시도는 하지 않는다.
- **이용 조건(2026-10-19 결정: CLI 명령 + 동의 플래그):** `model_files::Terms`(license, source, 비상업 전용 여부, 한 문장 요약)가 표시 자료다. `cloakframe-cli models`가 모델마다 상태·크기·license·요약·source를 보여 주고, `models fetch NAME...`은 받기 전에 요약을 출력한다. 비상업 전용인 YOLO5Face-n은 `--accept-terms`가 없으면 요약을 보여 주고 받지 않는다(종료 코드 2). `run`은 내려받지 않고, 모델이 없으면 받을 명령을 알려 준다. 앱(M6)은 같은 자료를 번역해 다운로드 전에 보여 준다.
- **D5:** 모델 폴더 권한 검사가 없다. umask 002(폴더 0775)에서도 저장되고, 바뀐 파일은 로드할 때 해시로 거부된다(이미 M2부터 감지기가 세션을 만들기 전에 확인).
- **D7:** CLI는 스캔에서 처리할 파일이 나온 뒤에만 런타임과 모델을 불러오고(M2부터), 그 실행이 감지할 모델만 읽는다. 세 모델 모두 입력 크기가 고정이라 (모델, 입력 크기, 실행 위치)의 세션은 모델과 실행 위치마다 하나다. 실행이 만든 세션 수를 `fallback::Backends::sessions_made`로 세어 시험한다. GPU 실패로 CPU로 옮기면 그 모델의 CPU 세션이 하나 더 생긴다(실행 위치가 다름). 영상은 M5에서 같은 실행의 감지 스레드를 쓴다.
- **CoreML 캐시 정리(2026-10-19 결정: 바로 지움 + 1.12.1 것도):** 실행이 CoreML 캐시를 설정할 때 `<캐시 폴더>/coreml` 아래에서 이 빌드의 ORT 폴더가 아닌 것과, 그 폴더 안의 고정 모델 해시가 아닌 것을 지운다(`OrtRuntime::remove_unused_coreml_caches`). 1.12.1의 `~/Library/Caches/CloakFrame/CloakFrame/coreml-models`(Qt `CacheLocation` + `coreml-models`)도 지운다(`legacy::remove_coreml_cache`). 두 판을 동시에 돌리면 다른 판의 컴파일 결과가 지워져 그 판이 다시 컴파일하거나, 컴파일 중이었다면 CPU로 시작한다. 지우지 못하면 CLI가 stderr에 알리고 실행은 계속한다.
- **미러(4.6):** 자체 GitHub 릴리스를 만드는 일이라 M7로 미뤘다. 지금은 upstream URL을 쓴다.

## 다운로드 경로

2026-10-19에 `curl -L`로 세 URL의 redirect를 확인했다.

| 모델 | 첫 응답 | 받는 곳 |
| --- | --- | --- |
| YOLO5Face-n | `github.com` 302 | `release-assets.githubusercontent.com`(서명된 query) 200, 11,021,477 B |
| YuNet | `github.com` 302 | `media.githubusercontent.com/media/opencv/opencv_zoo/...`(Git LFS) 200, 232,589 B |
| 번호판 | `github.com` 302 | `release-assets.githubusercontent.com` 200, 7,799,480 B |

## HTTP crate

계획 3장은 `reqwest` + rustls와 HTTP용 작은 Tokio runtime(M3)을 적었다. 다운로드는 감지·인코딩처럼 전용 스레드에서 돌고 청크마다 취소를 확인하면 되므로 async가 필요 없다. 두 후보를 스크래치 프로젝트에서 비교했다.

| | ureq 3.4.2 (선택) | reqwest 0.13.5 blocking |
| --- | --- | --- |
| 정상 의존성(macOS / 모든 대상) | 33 / 73 | 114 / 149 |
| async runtime | 없음 | tokio·hyper·h2(blocking도 내부에 runtime 스레드) |
| 암호 구현 | ring(engine이 provider를 직접 넘김) | aws-lc-rs(`aws-lc-sys`, C 빌드) |
| cargo-deny 라이선스 | `webpki-root-certs`(CDLA-Permissive-2.0) 하나 거부 | 같음 |

`webpki-root-certs`는 `rustls-platform-verifier`가 wasm32에서만 쓴다. `deny.toml`의 `[graph]`에 배포하는 세 대상(`x86_64-pc-windows-msvc`, `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`)을 적어 그래프에서 뺐다. ureq의 `rustls` feature는 Mozilla 루트 목록(`webpki-roots`, CDLA-Permissive-2.0)을 넣으므로 `rustls-no-provider` + `platform-verifier`를 쓰고, rustls를 직접 의존해 ring provider를 넘긴다.

**버전:** rustls는 0.23.46(2026-10-14)이다. 0.23.45에는 보안 권고 네 건(RUSTSEC-2026-0346–0349: 세션 값 `Debug`의 비밀 노출, ECH 거부 뒤 데이터 전송, Brotli 인증서 압축 해제 메모리, ServerHello downgrade)이 있어, 나온 지 며칠 안 된 판을 피하는 규칙보다 보안 수정을 앞세웠다. 공개 2주가 안 된 판인 ureq-proto 0.6.5, schannel 0.1.30, combine 4.6.9, rustls-native-certs 0.8.5, zeroize 1.9.1은 각각 0.6.4, 0.1.29, 4.6.8, 0.8.4, 1.9.0으로 고정했다. 새로 들어온 crate의 라이선스는 모두 기존 허용 목록 안이다.

## 시험

| 시험 | 확인하는 것 |
| --- | --- |
| `model_files` 단위 7건 | 고정 목록이 manifest와 같음, 확인이 끝나야 모델 이름이 생김(읽는 동안 폴더에는 임시 이름만), 다른·긴·짧은·끊긴·취소된 내용은 아무것도 남기지 않음, 바뀐 파일을 교체하고 하루 넘은 임시 파일을 지움, 상태, 1.12.1 폴더에서 첫 번째로 맞는 파일을 복사하고 원본을 둠(있으면 복사하지 않음) |
| `model_files::download` 단위 6건 | allowlist(고정 URL과 redirect 호스트 통과, http·비슷한 이름·다른 GitHub 호스트·사용자 정보·포트·IP 거부), 상대 redirect, 로컬 서버로 redirect를 따라 본문까지, allowlist 밖 redirect는 요청하지 않음, 404와 끝없는 redirect, 너무 크다고 알린 본문은 읽지 않음 |
| `legacy` 단위 2건 | 1.12.1 폴더 위치(macOS·Windows는 새 폴더와 같고 Linux는 다름), 1.12.1 CoreML 캐시 삭제 |
| `detect::fallback` 단위 | 세션 생성 수(같은 위치에서 여러 장 = 1, CPU로 옮기면 +1, 시작 실패는 세지 않음) |
| `d5_model_folder.rs` | umask 022·002에서 저장되고 폴더 0755·0775, 파일 0644·0664. 0777 폴더에도 저장. Windows는 Users에 수정 권한을 준 폴더에 저장 |
| `d7_sessions.rs` 2건 | 사진 4장·작업자 2에서 YOLO5Face-n만 1개, YuNet + 번호판 2개 세션(GPU 켬·끔 모두). 번호판을 끄면 그 파일을 읽지 않고, 켜면 없는 파일을 `LoadError::Missing`으로 |
| `cli_models.rs` 4건 | D7 첫 경우(미디어 없는 폴더 + 없는 런타임·모델 경로 → 완료), 번호판이 꺼져 있으면 번호판 파일이 없어도 되고 켜면 받을 명령을 안내, **실제 GitHub에서** YuNet·번호판 다운로드와 목록·이용 조건·`--accept-terms` 거부·잘못된 이름, 앱 폴더로 1.12.1 모델 복사와 `--models` 폴더에는 복사하지 않음, macOS CoreML 캐시 정리(Windows는 known folder를 시험 홈으로 옮길 수 없어 제외) |
| `detect_fixtures.rs` `coreml_caches_compiled_models_per_runtime_build_and_model` | 다른 빌드·고정되지 않은 모델·떨어진 파일 삭제, 이 빌드의 컴파일 결과 유지 |
| `cli_photo.rs` `only_the_pinned_models_run` | 바뀐 모델 파일은 로드할 때 거부(D5의 해시 확인) |

변형 확인(작업 트리 사본에서 한 곳씩 바꿔 시험): 이미지마다 세션을 새로 만들기 → `d7_sessions`와 fallback 단위, 첫 URL만 allowlist 검사 → redirect 시험, 스캔 전에 모델 로드 → `cli_models` D7 첫 경우, 고정되지 않은 CoreML 폴더 유지 → CoreML 캐시 시험, `--models` 폴더에도 1.12.1 모델 복사 → `cli_models` 앱 폴더 시험, 이용 조건 확인 생략 → `cli_models` 다운로드 시험이 실패했다.

## 확인하지 못한 것

- Windows·Linux의 다운로드(OS 인증서 저장소 확인)는 CI의 `cli_models.rs`가 처음 돌린다. AppImage(M7)에서 Linux 인증서 위치를 찾는지는 패키지 시험에서 확인한다.
- 데이터가 오지 않고 연결만 남은 다운로드는 취소가 다음 데이터나 본문 한도(15분)까지 늦는다. ureq에는 읽기마다 다시 시작하는 시간 한도가 없다(1.12.1은 30초). CLI는 Ctrl+C로 끝나므로 지금은 문제가 없고, 앱(M6)은 취소 응답 목표(9.3)를 위해 다운로드 스레드를 기다리지 않고 취소를 끝내는 방식으로 붙인다.
- YOLO5Face-n(11 MB)은 `--accept-terms` 경로를 실제로 받아 보지 않았다. 다운로드 코드는 다른 두 모델과 같다.
- 1.12.1이 남긴 다른 파일: 데이터 폴더의 `stage-roots.txt`(출력 루트 기억 파일, 계획 1.2에서 제거)와 캐시 폴더의 `staging`(D12의 원본 사본 stage, 1.12.1이 정리함). 이 단계는 모델과 CoreML 캐시만 다뤘다. 나머지를 M7 이전에서 지울지는 그때 정한다.

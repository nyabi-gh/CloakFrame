# 버전·네이티브 manifest

Rust 재작성이 쓰는 외부 구성 요소의 정확한 버전과 출처다. [계획](../docs/RUST_PORT_PLAN.md) 3장·10장의 "임의 최신 판을 받지 않는다"를 지키는 기준이다.

| 파일 | 내용 | 읽는 쪽 |
| --- | --- | --- |
| [rust.toml](rust.toml) | Rust 툴체인, M0에서 정하고 이후 단계에서 들인 crate의 버전과 feature | workspace의 `rust-toolchain.toml`·`Cargo.toml` |
| [native.toml](native.toml) | ONNX Runtime, Windows ML, FFmpeg, Sparkle, Velopack, 글꼴, 감지 모델의 플랫폼별 버전·URL·SHA-256·라이선스·소스 위치 | 빌드·패키징(M7), 모델 다운로드(engine `model_files`, 같은 값인지 시험), THIRD_PARTY_NOTICES |

- `status = "m0"`은 M0 spike가 그 파일로 결과를 확인한 항목, `"m4"`처럼 다른 단계 이름이면 그 단계에서 다시 정한 항목, `"carried"`는 1.12.1 릴리스 고정값을 이어받아 `stage` 단계에서 다시 정할 항목이다.
- 값을 바꿀 때는 근거 기록(`evidence`)을 함께 갱신한다. ORT·Windows ML을 바꾸면 세 OS의 `api-N`, 감지 fixture 비교, 텔레메트리 감시를 다시 돌린다(계획 3장).
- `use = "build"` 항목(libjpeg-turbo, libwebp)은 crate가 담은 C 소스를 빌드해 정적 링크하므로 따로 내려받지 않는다. URL·해시는 그 crate 파일이다.
- [`tools/fetch_native.py`](../tools/fetch_native.py) `<id> <platform> <dest>`가 항목을 해시로 받아 풀고 `library` 경로를 출력한다. CI의 engine 시험(`ci.yml`)과 ORT workflow가 쓴다.
- 2026-10-17에 `native.toml`의 모든 URL을, 2026-10-18에 추가한 두 crate 파일을 내려받아 SHA-256이 적힌 값과 같음을 확인했다.

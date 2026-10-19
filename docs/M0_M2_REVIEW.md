# M0–M2 재검토

실행일: 2026-10-19. 대상: `m2-verify` `adc5678`(main `02dc34f` 위에 M2 7단계 커밋 4개). 장치: Apple M4, macOS 27.0.1, 고정 ORT 1.30.0. [계획](RUST_PORT_PLAN.md) M2를 마친 뒤 M0–M2의 작업과 계획을 다시 보고 회귀·버그·잘못된 판단을 찾은 기록이다. 결과만 적었고 코드는 고치지 않았다. 고칠 순서는 메인테이너가 정한다. 줄 번호는 `adc5678` 기준이다. 같은 날 처리한 내용은 끝의 [처리](#처리-2026-10-19) 절에 있다.

## 방법

- 읽은 것: 계획 전체, [CHANGES](CHANGES_FROM_1.12.md), M0·M2 기록, [fixture README](../tests/fixtures/README.md)·`legacy/index.json`·`defects/*/expected.json`, core·engine 소스와 시험, CI·manifest·`deny.toml`·THIRD_PARTY_NOTICES.
- 로컬에서 `cargo test --workspace --locked --all-features` 194개가 통과함을 다시 확인했다.
- 재현 실험과 mutation 시험은 저장소를 `git clone --local`로 스크래치 폴더에 복사해 별도 `CARGO_TARGET_DIR`로 했다. 실험용 시험 파일과 입력은 저장소에 넣지 않았다. mutation은 한 번에 하나만 넣고 `git checkout`으로 되돌렸다.
- Windows·Linux에서는 실행하지 못했고, m2-verify의 3개 OS CI도 아직 돌지 않았다. 그 OS에서만 드러나는 항목은 코드로만 판단했고 "확인하지 못함"으로 적었다.

## 요약

| # | 심각도 | 내용 | 걸리는 계획 항목 |
| --- | --- | --- | --- |
| 1 | 결함 | NMS 뒤 상한(300)을 넘은 감지를 버리고, 그 상태로 경고 포함 내보내기·검토 승인 게시 | P-1(사진), D11 |
| 2 | 결함 | 검토 결정 JSON의 모르는 key를 무시(`added` 오타면 수동 마스크 없이 게시) | M2 검토 결정 적용 |
| 3 | 결함 | 한 항목의 panic이 실행 전체를 멈추고 보고서·세션을 잃음 | 4.7, 9.1 실패 주입 |
| 4 | 결함 | JPEG 압축 TIFF: 정상 파일은 거부, 잘못 표기된 파일은 zune-jpeg로 잘못 디코드 | 1.1 사진 입력, M2 지원 형식 |
| 5 | 결함 | 대소문자 판정이 출력 폴더가 아니라 그 이름이 놓인 부모 폴더 기준 | D10 |
| 6 | 결함 | CMYK JPEG는 거부, CMYK TIFF는 단순 변환으로 처리 | M2 결정, CHANGES |
| 7 | 결함 | 회색 사진의 회색 ICC를 "쓸 수 없음"으로 버림 | D2 |
| 8 | 결함 | 비표준 크로마 서브샘플링 JPEG가 인코딩에서 실패 | D6, 1.1 |
| 9 | 결함 | macOS에서 NFC·NFD로만 다른 이름을 다른 이름으로 계획 | D10 |
| 10 | 결함 | CLI: 검토 대기 항목이 세션에서 조용히 빠짐, 런타임 오류를 사용법 오류로 보고 | M1 CLI |
| 11 | 위험 | PNG iCCP 압축 폭탄이 메모리 예산 밖에서 풀림 | 4.7, 5장 위협 모델 |
| 12 | 위험 | "수동 마스크로 덮음" 해결이 아무 위치의 1 px 영역으로 통과 | 4.2, P-1 |
| 13 | 위험 | 모자이크 강도 상한 16이 1.12.1의 12셀보다 약함 | D3, 5장 |
| 14 | 위험 | 감지 0건을 표시 없이 게시, 범위 조정 ③ 타일 모드와 겹침 | 4.2, 8장 |
| 15 | 위험 | 게시 rename 뒤 폴더 fsync 실패를 게시 실패로 보고 | 4.5 |
| 16 | 위험 | Windows: 긴 경로의 `MoveFileExW`, FAT·exFAT의 파일 ID | 4.3, 4.5 |
| 17 | 의심 | 시험이 요구를 확인하지 못하는 곳(mutation 5개 생존 등) | D2, D6, D12, P-1 |
| 18 | 의심 | Detection 저장 형식 변경을 시험 수정으로 덮음(형식 버전 그대로) | 세션 형식 |
| 19 | 의심 | 검토 키의 입력·타임라인 해시를 계산만 하고 비교하지 않음 | 4.2 |
| 20 | 의심 | 기타: 공개된 PublishPermit 발급, 재명명 확장자, 메모리 예산, 사용자 이미지 색, 알파, Windows 임시 파일 | 4.2, 4.3, 4.5 |
| 21 | 의심 | 시험 helper 중복 | — |
| 22 | 문서 | 문서끼리 어긋난 곳 | 7장 |

M2 범위에 걸리는 항목은 1·2·4·6·7·8·12·17번, M1 범위는 5·9·10·15·16·18·19번이다. 13번은 계획 D3 자체(강도 4–16)에 대한 판단이다.

## 결함

### 1. 상한을 넘은 감지를 버린 채 게시할 수 있음

- 위치: [yolo5face.rs:192](../crates/cloakframe-engine/src/detect/yolo5face.rs#L192)가 NMS 뒤 `MAX_DETECTIONS`(300)를 넘는 감지를 잘라 개수(`over_limit`)만 돌려준다. [process.rs:222](../crates/cloakframe-engine/src/photo/process.rs#L222)는 그 개수만 `CappedCandidates`로 남긴다.
- 재현: `p1_photo.rs`와 같은 설정으로 crowd-280을 상한 없이 분석해 box 280개를 얻고, 상한 100으로 분석해 `Decision::ExportWithWarnings`로 렌더했다. 280개 box의 중심 pixel 중 180개가 원본 그대로였다. [p1_photo.rs:126](../crates/cloakframe-engine/tests/p1_photo.rs#L126)은 남긴 100개만 확인한다.
- 영향: 감지기가 찾은 얼굴이 가려지지 않은 출력이 게시된다. 정규 상한에서도 300명이 넘는 사진(경기장, 단체 사진)이면 같다. 버린 box는 저장되지 않아 검토에서 볼 수 없고, `dismissed`로 승인하면 정상 완료("published (reviewed)")로 센다.
- 선택지: (a) 사진은 NMS를 통과한 감지를 모두 가린다. 상한이 영상 추적 비용 때문이라면 영상에만 둔다. (b) 상한을 넘은 box를 분석 결과에 남겨 내보내기에서도 가리고 검토에 보인다. (c) 유지하고 문서화한다.
- 추천: (a). 사진의 마스크 비용은 가리는 면적을 따르므로 box 수 상한이 필요 없고, YOLO5Face-n은 출력 25,200행이 이미 상한이다. D11 expected와 CHANGES를 함께 고친다.

### 2. 검토 결정 JSON의 모르는 key를 무시함

- 위치: [review.rs:24](../crates/cloakframe-engine/src/review.rs#L24) `ItemDecision`이 `#[serde(flatten)]`으로 편집을 받고 모르는 key를 거부하지 않는다.
- 재현: astronaut.png를 `--review required`로 run한 뒤 `{"items":[{"item":0,"revision":0,"action":"approve","add":[{"x":400,"y":400,"width":40,"height":40}]}]}`로 review했다. 종료 코드 0, "published (reviewed)"였고 (420,420)은 원본과 같았다.
- 영향: 검토자가 가렸다고 믿는 영역이 가려지지 않은 채 정상 완료로 게시된다. `excluded` 오타는 더 가리는 쪽이라 안전하지만 `added`·`resolutions` 오타는 그렇지 않다.
- 선택지: (a) 받은 JSON의 key를 알려진 목록과 대조한 뒤 변환한다(형식 유지). (b) 편집을 하위 객체로 옮기고 `deny_unknown_fields`를 쓴다(serde는 flatten과 함께 쓰지 못함).
- 추천: 형식을 유지하려면 (a). 데스크톱(M6) 전에 형식을 정리할 생각이면 (b).

### 3. 한 항목의 panic이 실행 전체를 멈춤

- 위치: [ordered.rs:125](../crates/cloakframe-engine/src/ordered.rs#L125)가 producer의 panic을 실행 스레드에서 다시 던지고, [run.rs:242](../crates/cloakframe-engine/src/run.rs#L242)가 호출자에게 다시 던진다.
- 재현: 복사본의 합성 처리기 `analyze`에 "파일 이름에 panic이 있으면 `panic!`"을 넣고 `a-ok`, `b-gap`, `c-panic`, `d-ok`를 `--workers 1`로 run했다. 종료 코드 101, 보고서 출력과 세션 저장이 없었고, `a-ok`만 게시되고 `d-ok`는 처리되지 않았다.
- 영향: 신뢰하지 않는 입력을 읽는 서드파티 디코더·parser(image, png, tiff, moxcms, kamadak-exif)에서 panic 하나가 나면 배치 전체가 멈추고, 이미 끝난 항목의 검토 대기 세션까지 잃는다.
- 추천: runner에서 항목 단위로 `catch_unwind`해 그 항목만 실패("internal error")로 끝내고 나머지를 계속한다. `process_ordered`의 의미는 그대로 둔다.

### 4. JPEG 압축 TIFF

- 위치: `image`가 `tiff`를 기본 feature로 쓰면서 `tiff`의 `jpeg` feature가 켜져, zune-jpeg 0.5.15가 빌드에 남아 있다(`cargo tree -p cloakframe-engine -e features`). 2026-10-18에 JPEG 파일에서 이 디코더를 뺀 이유([M2 spike](M2_ENCODE_SPIKE.md))가 TIFF 경로에는 남는다.
- 재현: `tests/fixtures/decode`의 JPEG 두 개를 각각 strip 하나로 담은 96×64 TIFF(Compression 7, SamplesPerPixel 3)를 만들어 엔진 `decode`에 넣었다.
  - Photometric 6(YCbCr, 정상 파일): "does not support the color type `Unknown(24)`"로 실패했다. Pillow(libtiff)는 같은 파일을 djpeg 대비 PSNR 138 dB로 읽는다.
  - Photometric 2(RGB로 잘못 표기, libtiff는 거부): 오류 없이 디코드됐고 djpeg 대비 PSNR은 5.42 dB(non-interleaved)·8.55 dB(interleaved)였다.
- 영향: 스캐너·Photoshop이 만드는 JPEG 압축 TIFF를 1.12.1과 달리 처리하지 못한다. 잘못 표기된 파일은 엉뚱한 pixel로 감지·마스크·게시된다.
- 추천: 헤더에서 TIFF Compression 6·7을 판정해 명확한 이유로 거부하고 CHANGES에 적는다. 지원은 수요를 본 뒤 strip을 libjpeg-turbo로 디코드하는 방식으로 정한다.

### 5. D10: 대소문자 판정의 기준 폴더

- 위치: [fs_case.rs:19](../crates/cloakframe-engine/src/fs_case.rs#L19) `NameCase::probe`는 폴더 자신의 이름을 대소문자만 바꿔 부모 폴더에서 찾는다. 그래서 그 폴더 안의 규칙이 아니라 부모 폴더의 규칙을 잰다.
- 재현: 대소문자 구분 APFS 디스크 이미지를 마운트하고, 그 안에 대소문자를 무시하는 폴더(시스템 볼륨)를 가리키는 링크 `link`를 뒀다. 입력 두 곳의 `in1/Same.jpg`, `in2/same.jpg`를 `--synthetic --output <이미지>/link/out --review off`로 run하자 두 번째가 "the output name was taken during the run"으로 실패했다. 실제 경로로 주면 "needs review"와 `same (2).jpg` 제안이 나온다.
- 영향: 출력 이름 충돌을 검토로 보내지 못하고 게시 단계에서 실패한다. 덮어쓰기는 no-replace rename이 막는다. 같은 구조로 Linux `/media/<user>/<USB>`(vfat, 부모는 ext4)와 NTFS 폴더별 대소문자 설정의 경계에서도 틀린다(확인하지 못함).
- 추천: 경로를 `canonicalize`한 뒤 대상 폴더 안의 기존 항목, 없으면 임시 probe 파일로 판정한다.

### 6. CMYK 처리 불일치

- 위치: [decode.rs:187](../crates/cloakframe-engine/src/photo/decode.rs#L187)은 CMYK·YCCK JPEG를 거부하지만, `image`의 TIFF 디코더는 CMYK를 단순 공식(`cmyk_to_rgb`)으로 RGB로 바꾼다.
- 재현: Pillow로 만든 64×48 CMYK JPEG는 "a CMYK JPEG is not supported"로 실패했다. 같은 그림의 CMYK TIFF는 표시 없이 게시됐고, Generic CMYK Profile을 넣은 TIFF는 "unusable color profile dropped"로 게시됐다.
- 영향: CHANGES의 거부 이유("잘못된 색으로 처리하지 않는다")가 TIFF에는 적용되지 않는다.
- 추천: 일관성을 위해 CMYK TIFF도 거부하거나, 둘 다 내장 profile로 sRGB 변환한다(moxcms의 CMYK 변환은 확인하지 못함). 정한 뒤 CHANGES를 확정한다.

### 7. 회색 ICC를 버림(D2)

- 위치: [color.rs:322](../crates/cloakframe-engine/src/photo/color.rs#L322)는 profile이 RGB가 아니면 `Ignored(NotForTheseSamples)`로 버린다.
- 재현: macOS의 `Generic Gray Gamma 2.2 Profile`을 넣은 회색 JPEG·PNG가 "unusable color profile dropped"로 게시됐다.
- 영향: D2 요구("그 밖의 profile은 sRGB로 변환")와 D2 expected의 기준("pixel과 맞지 않는 profile")에 맞지 않는다. 회색 profile은 회색 pixel에 맞는 profile이다. 흑백 사진의 계조가 달라지고(dot gain profile이면 더 큼) 표시 문구가 틀리다.
- 추천: 회색에서 sRGB 회색으로 변환하고 `Converted`로 표시한다. 시험을 추가한다.

### 8. 비표준 서브샘플링 JPEG 인코딩 실패

- 위치: [encode.rs:144](../crates/cloakframe-engine/src/photo/encode.rs#L144)가 원본의 서브샘플링을 그대로 쓰는데, libjpeg-turbo는 표준 조합이 아니면 `Subsamp::Unknown`을 돌려준다.
- 재현: `cjpeg -quality 92 -sample 2x1,1x2,1x1`로 만든 JPEG는 디코드되지만 "could not encode the photo: TurboJPEG error: tj3Set(): Parameter value out of range"로 실패했다. `-sample 4x1,1x1,1x1`은 통과했다.
- 영향: 1.12.1(항상 4:4:4)이 처리하던 파일이 실패한다(fail closed).
- 추천: `Unknown`이면 4:4:4로 쓴다. 시험을 추가한다.

### 9. NFC·NFD로만 다른 이름(macOS)

- 위치: [fs_case.rs:44](../crates/cloakframe-engine/src/fs_case.rs#L44) `NameCase::key`는 `to_lowercase`만 한다. APFS·HFS+는 정규화 형태를 무시하고 이름을 비교한다.
- 재현: `nfc/사진.jpg`(NFC)와 `nfd/사진.jpg`(NFD)를 합성 처리기로 같은 출력 폴더에 run하자 두 번째가 "the output name was taken during the run"으로 실패했다.
- 영향: Windows·NAS에서 온 NFC 이름과 macOS에서 만든 NFD 이름이 섞이면 충돌을 검토로 보내지 못한다. 덮어쓰기는 없다.
- 추천: 정규화를 무시하는 볼륨에서는 key에 NFC 정규화를 더한다(`unicode-normalization`, MIT/Apache-2.0).

### 10. CLI 보고

- [batch.rs:72](../crates/cloakframe-engine/src/batch.rs#L72)는 분석 뒤 원본이 바뀌었거나 SHA-256 계산에 실패한 검토 대기 항목을 세션에서 조용히 뺀다. 보고서는 "needs review"를 출력하지만 그 항목은 검토할 수 없다(코드로 확인, 재현은 하지 않음).
- [cloakframe-cli.rs:439](../crates/cloakframe-engine/src/bin/cloakframe-cli.rs#L439)는 모델 해시 불일치, 세션 저장 실패 같은 런타임 오류도 종료 코드 2(사용법 오류)와 USAGE 출력으로 끝낸다. `cli_photo.rs` `only_the_pinned_model_runs`가 이 동작을 고정한다. 세션 저장에 실패하면 검토 대기 항목도 잃는다.
- 추천: 세션에서 뺀 항목은 이유와 함께 출력하고, 런타임 오류는 종료 코드 1과 오류 문구만 낸다.

## 위험

### 11. PNG iCCP 압축 폭탄

- 위치: [decode.rs:118](../crates/cloakframe-engine/src/photo/decode.rs#L118)의 `no_limits()`, [color.rs:147](../crates/cloakframe-engine/src/photo/color.rs#L147)의 `read_icc`. 메모리 예산은 pixel 수와 파일 크기로만 계산한다.
- 재현: 64×64 PNG에 zlib으로 압축한 1 GiB(0 바이트)를 iCCP로 넣었다(파일 1,043,911 B). `/usr/bin/time -l`로 12.87초, 최대 RSS 2,827,042,816 B였고 결과는 "unusable color profile dropped"로 게시됐다. 어느 호출이 풀었는지는 나누어 재지 않았다.
- 영향: 작업자 수만큼 병렬로 처리하면 배로 늘어나고, 파일 상한 2 GB 안에서 풀린 크기는 훨씬 커질 수 있다. 계획 5장의 위협 모델은 ICC를 신뢰하지 않는 입력으로 본다.
- 추천: ICC 크기 상한(예: 16 MB, JPEG APP2 조각 255개와 비슷한 크기)을 두고, 디코더 limit을 메모리 예산에서 계산해 준다.

### 12. "수동 마스크로 덮음"의 검사가 형식뿐

- 위치: [review.rs:209](../crates/cloakframe-engine/src/review.rs#L209)는 추가 영역이 하나라도 있으면 어느 evidence든 "덮음"으로 받는다.
- 재현: 합성 처리기의 `crowd-capped.jpg`(100/280)에 `approve` + `covered_by_manual_mask` + (0,0)의 1×1 영역을 주자 "published (reviewed)", 종료 코드 0이었다.
- 영향: 위치가 없는 CappedCandidates를 "덮음"으로 해결해 정상 완료로 센다. `dismissed`는 원래 검사 없이 쓸 수 있으므로 새 우회는 아니지만, 결과 기록의 의미가 틀어진다.
- 추천: 위치가 있는 evidence(약한 후보 box, 영상 gap의 track box)는 추가 영역이 덮는지 확인하고, CappedCandidates에는 "덮음"을 허용하지 않는다. 1번을 (a)로 고치면 사진에서는 이 evidence 자체가 사라진다.

### 13. 모자이크 강도 상한 16

- 위치: [mask.rs:55](../crates/cloakframe-core/src/mask.rs#L55), 계획 D3("강도 4–16").
- 근거: 1.12.1은 셀 12개가 상한이었고, [분석 보고서](REWRITE_ASSESSMENT.ko.md)(20·287행)와 [ROADMAP](ROADMAP.md) A-7은 그 12×12도 약하다고 봤다. 13–16은 1.12.1에서도 불가능했던 더 약한 모자이크다.
- 추천: 상한을 12 이하로 줄이거나, 16을 허용하는 이유를 계획에 적는다. 기본값 8은 유지한다.

### 14. 감지 0건 게시와 타일 모드

- 감지 0건을 정상 게시하는 결정(M2 6단계)은 타당하지만, 보고서와 CLI에 0건이라는 표시가 없다(1.12.1은 경고했음). 범위 조정 순서 ③에서 타일 모드가 미뤄지면, 고해상도 사진의 작은 얼굴을 놓친 결과가 조용히 "자동 처리·미검토"로 게시된다.
- 추천: 게시를 막지 않는 note("감지 0건")를 결과에 넣고, 타일 모드를 범위 조정 목록에서 빼는 것을 검토한다.

### 15. 게시 뒤 폴더 fsync 실패

- 위치: [publish.rs:86](../crates/cloakframe-engine/src/publish.rs#L86). rename이 끝난 뒤 폴더 fsync가 실패하면 `Err`를 돌려줘, 출력이 이미 보이는데도 항목은 실패가 된다. hard link 대체 경로에서 임시 이름 삭제가 실패할 때도 같다.
- 영향: 폴더 fsync를 지원하지 않는 네트워크·FUSE 볼륨에서는 모든 항목이 실패로 보이고, 다음 실행은 이름 충돌로 검토에 간다(확인하지 못함).
- 추천: rename 이후의 실패는 게시 성공으로 보고 경고만 남긴다.

### 16. Windows(확인하지 못함)

- [publish.rs:150](../crates/cloakframe-engine/src/publish.rs#L150)은 `MoveFileExW`에 경로를 그대로 넘긴다. std 파일 함수는 긴 경로에 `\\?\`를 붙이지만 이 호출은 그렇지 않고, CLI에는 longPathAware manifest도 없다. MAX_PATH를 넘는 출력 경로에서는 임시 파일만 생기고 게시가 실패할 수 있다.
- [identity.rs:48](../crates/cloakframe-engine/src/identity.rs#L48)은 `FileIdInfo`만 쓴다. FAT32·exFAT·일부 SMB 볼륨이 이를 지원하지 않으면 스캔이 모든 파일을 읽지 못한 것으로 보고한다(카메라 SD 카드).
- 추천: 경로를 verbatim으로 바꾸거나 핸들 기반 rename(`SetFileInformationByHandle`, 대체 금지)을 쓰고, `FileIdInfo`가 실패하면 `GetFileInformationByHandle`의 파일 인덱스로 대체한다. Windows CI에 FAT32 VHD와 깊은 경로 시험을 넣는다.

## 의심

### 17. 시험이 요구를 확인하지 못하는 곳

아래 mutation 중 위 다섯 개는 관련 시험이 하나도 실패하지 않았다. 아래 셋은 대조군이다.

| 바꾼 것 | 위치 | 돌린 시험 | 결과 |
| --- | --- | --- | --- |
| 손실 WebP 품질 90 → 50 | `photo/encode.rs:25` | lib, encode_fixtures, photo_pipeline | 118개 통과 |
| 버린 profile의 `ColorProfileIgnored` 표시 제거 | `photo/process.rs:267` | lib, photo_pipeline, cli_photo | 118개 통과 |
| 게시 허가 직전의 취소 확인 제거 | `run.rs:472` | lib, cli, photo_pipeline | 120개 통과 |
| `read_source`가 원본 사본을 TMPDIR에 씀 | `photo/decode.rs:74` | d12_no_source_copy, photo_pipeline | 13개 통과, TMPDIR에 사본이 생김 |
| Adobe RGB 감지 사본을 sRGB로 바꾸지 않음 | `photo/color.rs:403` | lib, detect_fixtures, photo_pipeline | 113개 통과 |
| (대조) GPS 동의 없이 GPS 보존 | `photo/metadata.rs:171` | lib, encode_fixtures, photo_pipeline | 4개 실패 |
| (대조) NoManualMask 검사 제거 | `review.rs:209` | lib | 1개 실패 |
| (대조) BMP LINK profile을 untagged로 | `photo/color.rs:236` | lib, photo_pipeline | 1개 실패 |

- [mask_fixtures.rs:63](../crates/cloakframe-engine/tests/mask_fixtures.rs#L63)은 해석하지 못한 기록 호출을 버린다. 필드 이름을 바꿔 모든 호출을 해석하지 못하게 하자 `fill_is_opaque_on_alpha_images`가 0건으로 통과했다(나머지는 `[0]` 접근 때문에 실패).
- D12 시험은 게시 모듈만 쓴다. 실제 파이프라인으로는 TMPDIR·TMP·TEMP를 빈 폴더로 두고 astronaut.png, lena.jpg, car_image.webp, D6 `lossless.tif`, D2 `chart-wide-gamma18.jpg`를 `--keep-metadata`로 처리했을 때 그 폴더가 비어 있었다. 동작은 맞고 시험만 비어 있다.
- core의 "감지 box는 모든 조합에서 가림" property 시험([mask.rs:991](../crates/cloakframe-core/src/mask.rs#L991))은 채우기만 쓴다. 가중치 경로는 방식과 무관하지만 계획의 "모든 조합"보다 좁다.
- 1번(상한을 넘은 감지)을 잡는 시험이 없다.
- 추천: WebP 품질(같은 입력의 `cwebp -q 90`과 크기·PSNR 비교), 결과 note, 취소 시점, 실제 파이프라인의 D12, Adobe RGB 감지 사본, 빈 기록 호출 거부를 시험에 더한다.

### 18. Detection 저장 형식 변경

- M2 3단계(`65289ac`)에서 Detection 저장 형식의 `kind`가 필수 필드가 됐다([detection.rs:71](../crates/cloakframe-core/src/detection.rs#L71)). 그 형식을 지키는 시험([serde_forms.rs:26](../crates/cloakframe-core/tests/serde_forms.rs#L26))은 입력에 `kind`를 넣어 통과시켰고, `FORMAT_VERSION`([session.rs:18](../crates/cloakframe-engine/src/session.rs#L18))은 1 그대로다.
- 영향: 실제 영향은 없다(M1 세션에는 감지가 없었고 릴리스 전이다). 다만 그 시험 머리말의 "세션 형식 버전과 함께만 바뀐다"는 규칙과 어긋난다.
- 추천: `kind`가 없으면 모델에서 유도하거나, 첫 stable 전에는 세션 형식을 버전 없이 바꿀 수 있다고 명시한다.

### 19. 검토 키의 입력·타임라인 해시

- [batch.rs:81](../crates/cloakframe-engine/src/batch.rs#L81)이 검토 대기 항목마다 원본 전체의 SHA-256을 계산하지만, [review.rs:191](../crates/cloakframe-engine/src/review.rs#L191)은 모델·설정만 비교한다. 원본 확인은 실제로 SourceStamp(identity, 크기, 수정 시각)가 한다.
- 영향: 계획 4.2의 "입력·모델·설정·타임라인 해시에 묶인 수정 번호"가 코드와 다르다. 파일을 한 번 더 읽는 비용과 10번의 조용한 누락만 생긴다.
- 추천: 해시를 없애고 계획 문구를 SourceStamp 기준으로 고치거나, 렌더할 때 다시 읽은 바이트의 해시를 비교한다.

### 20. 기타

- `PublishPermit::issue`가 core의 공개 함수라([item.rs:245](../crates/cloakframe-core/src/item.rs#L245)) 계획 4.2의 "engine 내부에서만 생성"이 타입으로 보장되지 않는다.
- `rename_to`([review.rs:251](../crates/cloakframe-engine/src/review.rs#L251))는 확장자를 제한하지 않고, 이미지 확장자가 아니면 원본 형식으로 쓴다([process.rs:184](../crates/cloakframe-engine/src/photo/process.rs#L184)). `astro.txt`로 바꾸자 PNG 데이터가 `astro.txt`로 게시됐다. `p3.bmp`로 바꾸면 sRGB 변환과 표시가 맞게 적용됐다. Windows의 `:`(대체 데이터 스트림)·예약 이름은 확인하지 못했다.
- 메모리 예산(pixel당 16 B, [budget.rs:10](../crates/cloakframe-engine/src/photo/budget.rs#L10))은 16-bit RGBA 사진에 색 변환이 붙어 버퍼 전체를 한 번 더 복사할 때(약 22 B/px)보다 작다.
- 넓은 색역을 유지하는 출력에서 사용자 이미지(sRGB)를 변환 없이 그린다([mask.rs:749](../crates/cloakframe-core/src/mask.rs#L749)).
- PNG 알파를 JPEG로 쓸 때 합성 없이 버려, 투명 영역의 숨은 색이 드러난다(1.12.1과 같음). 감지 사본도 같은 RGB를 보므로 그 안의 얼굴은 가려진다.
- Windows에서는 `.cloakframe-tmp-*`에 숨김 속성이 없고, 백신 등이 임시 파일을 잡고 있을 때 rename을 다시 시도하지 않는다(확인하지 못함).

### 21. 시험 helper 중복

- BMP V5 profile 삽입이 [color.rs:575](../crates/cloakframe-engine/src/photo/color.rs#L575)와 [photo_pipeline.rs:620](../crates/cloakframe-engine/tests/photo_pipeline.rs#L620)에 따로 있다.
- `crc32`(decode.rs:407, photo_pipeline.rs:742), 헤더만 있는 PNG(decode.rs:390 `png_declaring`, photo_pipeline.rs:758 `png_header`), 임시 폴더 구조체와 `required_env`·`rgb_of`가 시험 파일 다섯 곳에 있다.
- 추천: `tests/common` 모듈로 모은다.

## 문서

"이번 갱신"은 이 기록과 같은 날 고친 것이다.

| 위치 | 내용 | 이번 갱신 |
| --- | --- | --- |
| CHANGES 사진 입력(CMYK) | "2단계에서 다시 정한다"가 2단계가 끝난 뒤에도 남음 | 문구를 고치고 6번으로 연결 |
| CHANGES 임시 파일 | "숨김 임시 파일"은 Windows에서 숨김 파일이 아님 | 고침 |
| CHANGES | 출력 폴더 안의 링크를 따라 밖에 씀. 1.12.1 `testDestinationRejectsSymlinkEscape`는 거부했고, 0.1의 게시 결정으로 바뀜 | 줄 추가 |
| `legacy/index.json` | 위 시험이 `intent`(실제로는 바뀜) | `changed`와 note |
| `defects/D11/expected.json` | `stage`가 M3(계획은 "M2(YOLO5Face), M3") | 고침 |
| 계획 M2 | 3개 OS CI 전에 완료로 기록, 표의 D2·D6·P-1 행이 17번·1번만큼 넓게 적힘 | 재검토 줄과 표 주석 추가 |
| `manifests/rust.toml` | windows-sys feature에 `Win32_System_SystemInformation`이 없음(Cargo.toml에는 있음) | 고침 |
| 계획 4.2 | PublishPermit "engine 내부에서만 생성", ReviewRevision "해시에 묶임"이 코드와 다름 | 처리 때 고침(19·20번) |
| THIRD_PARTY_NOTICES | Rust crate와 ORT 고지가 없음. CONTRIBUTING은 런타임 의존성을 들일 때마다 적으라고 함 | 남김(M7에서 생성할지 정함) |
| README | SCRFD·Redactly·`PATH`의 FFmpeg 안내가 1.12.1 기준 | 남김(배포 중인 1.12.1 설명) |
| ROADMAP.md | C++ 구현 시절의 내용인데 CONTRIBUTING이 참고하라고 안내함 | 남김 |
| `legacy/index.json` logging | "기본 로그에 경로·파일명 없음"인데 CLI 보고서와 stderr는 절대 경로를 출력함. 로그 기능은 아직 없음 | 남김(CLI 출력을 로그로 볼지 정함) |
| 계획 8장 시간 예산 | 125% 규칙은 초과할 때만 작동하는데 M1·M2는 각 2일. 실제 제약은 M3·M7·M8로 옮긴 실기기 확인. ③ 타일 모드는 14번과 관련 | 남김 |

## 판단 재검토

- 타당하다고 본 것: CLI 검토 결정은 같은 설정에만 적용, BMP 출력의 sRGB 변환(마스크 전에 변환해도 감지 좌표·마스크·인코딩이 일관됨), BMP LINK profile 버림, YOLO5Face-n의 NMS 전 후보 상한 제거, 블러 σ 하한, 타원 규칙.
- D2 판정 허용오차: macOS의 실제 profile로도 맞았다. Apple sRGB(3,144 B, LUT 곡선), Little CMS sRGB, Apple Display P3, HEIC에 들어 있던 Display P3, Apple Adobe RGB는 알려진 색 공간으로, DCI-P3·ITU-709·Generic RGB는 변환 대상으로 판정됐다.
- 근거가 약하거나 사용자에게 불리한 것: NMS 뒤 상한으로 감지를 버림(1번), 모자이크 상한 16(13번), CMYK 불일치(6번), 재명명의 확장자(20번), 감지 0건 무표시(14번).
- 0.1절의 결정을 뒤집을 근거는 찾지 못했다. 찾은 문제는 그 결정 안에서 고칠 수 있다. 시험 게이트(fuzz는 parser 두 곳만)는 fuzz 대상을 늘리기보다 3번(항목 단위 panic 격리)과 11번(디코더 limit)으로 보완하는 편이 싸다.

## 확인하지 못한 것

- Windows: 긴 경로의 `MoveFileExW`, FAT32·exFAT·SMB의 `FileIdInfo`, `:`·예약 이름, 공유 위반.
- Linux: 마운트 지점에서의 대소문자 판정(같은 구조를 macOS 링크로만 재현).
- 네트워크 볼륨의 폴더 fsync, moxcms의 CMYK 변환, m2-verify의 3개 OS CI.

## 문제없음으로 확인한 영역

- 원본 불변: 입력은 읽기로만 열고, 게시 직전과 검토 재개 때 원본 변경을 검출한다.
- 덮어쓰기 금지: Linux·macOS·Windows·FAT 대체 경로 모두 no-replace다. 이름 자리의 끊긴 링크도 충돌로 보고, 동시 게시 시험이 있다.
- P-1 상태 전이: 검토를 꺼도 미해결이면 보류하고, 미해결 승인은 `review::apply`와 runner 양쪽에서 거부하며, 두 가지 내보내기는 정상 완료에서 빠지고, 허가는 항목·근거별로 발급된다.
- 메타데이터: allowlist 30개 태그, GPS 별도 동의, orientation 1, ColorSpace는 출력에서 계산한다. 썸네일·XMP·IPTC·주석·MakerNote는 쓰지 않고, 원본 ICC 바이트는 옮기지 않고 번들 profile만 붙인다.
- D9: 원본 바이트 복사 경로가 없고, 시험이 16×16 전체 감지로 마스크 여부까지 확인한다.
- D3(fixture 입력으로 셀 수·평균·영역 밖 불변), D6 JPEG(IJG q92 양자화 표, 서브샘플링), TIFF LZW, 무손실 pixel, D11(280/280, 상한 초과 0).
- BMP V5 profile 판독의 경계 검사(헤더 크기, `checked_add`, `get`, 크기 0, LINK).
- 9.1: 자산이 없으면 시험이 실패하고 `#[ignore]`가 없다. cargo-deny가 GPL·LGPL·MPL crate를 막는다. ORT는 `ORT_DISABLE_TELEMETRY=1`이 아니면 로드를 거부하고, CLI는 `main` 첫 줄에서 이 값을 설정한다.
- M2 범위의 1.12.1 사진 시험은 바뀐 항목(위 링크 시험 등)을 빼고 모두 대응하는 Rust 시험이 있다.

## 처리 (2026-10-19)

메인테이너가 정한 순서(1·2·12 → 3·11 → 4·8·7·6 → 17 → 5·9·10·15·16 → 18–21, 이어서 결정한 13·14)로 `m2-verify`에서 고쳤다. 항목마다 실패하는 시험을 먼저 넣어 실패를 확인한 뒤 고쳤고(17번은 mutation을 다시 넣어 잡히는지 확인), 커밋은 항목별로 나눴다. 끝난 뒤 macOS arm64에서 `cargo test --workspace --locked --all-features` 216건, fmt, clippy 두 조합, cargo-deny가 통과했다. Windows·Linux는 `m2-verify`의 3개 OS CI로 확인한다.

결정(메인테이너, 2026-10-19): 1번은 (a), 6번은 CMYK TIFF도 거부, 13번은 강도 4–12, 14번은 감지 0건 표시 + 타일 모드를 범위 조정 목록에서 뺌. 2번은 형식을 유지하는 (a)로, 19번은 해시를 실제로 비교하는 쪽으로 정했다(0.1의 결정은 뒤집지 않음).

| # | 처리 | 커밋 | 시험 |
| --- | --- | --- | --- |
| 1 | YOLO5Face-n의 NMS 뒤 상한(300) 제거, 사진은 NMS를 통과한 감지를 모두 가림. `over_limit` 경로는 출력 수가 제한된 M3 모델용으로 남김. D11·P-1 expected, CHANGES, 계획 D11 갱신 | `8034628` | `p1_photo.rs` `every_face_left_after_nms_is_masked`(NMS 0.9, 601개) |
| 2 | 검토 결정을 평평한 형식으로 받아 모르는 key 거부(형식 그대로) | `c2f8d45` | `review` `unknown_keys_are_refused` |
| 12 | "수동 마스크로 덮음"은 약한 후보 box를 추가 영역의 합집합이 덮을 때만. 위치가 없는 근거(잘린 후보, 실패 frame, gap)는 기각만 가능(gap은 M5 영상 편집에서 다시 정함) | `7c5d05a` | core `geometry` `coverage_needs_the_union_of_areas_to_hold_the_box`, `review` `a_manual_mask_resolves_only_the_area_it_covers` |
| 3 | 실행기가 항목 단위로 panic을 잡아 그 항목만 "internal error"로 실패 | `31e18f7` | `run` `a_panicking_step_fails_only_that_item` |
| 11 | PNG 읽기의 할당 상한 = 파일 크기 + ICC 16 MiB + 디코드 예산. 16 MiB 넘는 profile은 읽을 수 없음으로 표시 | `177f736` | `allocations.rs` `a_compressed_profile_inflates_only_to_its_limit`(256 MiB로 풀리는 iCCP의 최대 할당), `color` `a_profile_over_the_limit_is_unreadable` |
| 4 | TIFF Compression 6·7을 디코드 전에 명확한 이유로 거부, CHANGES에 추가 | `8544f00` | `decode` `refuses_a_jpeg_compressed_tiff` |
| 8 | libjpeg-turbo가 쓸 수 없는 서브샘플링(`Unknown`)이면 4:4:4. fixture `decode/mixed-sampling.jpg` | `5a20f97` | `encode` `a_source_with_unusual_subsampling_gets_full_chroma` |
| 7 | 회색 사진의 회색 profile을 sRGB 곡선의 회색으로 변환, `Converted` 표시. D2 expected·CHANGES 갱신 | `ca17d39` | `color` `gray_profiles_convert_gray_pixels_to_the_srgb_curve` |
| 6 | CMYK TIFF(Photometric 5) 거부, CHANGES 확정 | `72dd1cb` | `decode` `refuses_a_cmyk_tiff` |
| 17 | 표의 다섯 mutation과 빈 기록 호출을 잡는 시험 추가: libwebp q90 출력 비교, 버린 profile의 note, 렌더 중 취소, 실제 파이프라인의 D12, Adobe RGB 감지 사본(Little CMS 비교), mask fixture의 엄격한 판독, 모든 방식의 감지 box 가림 property | `7ac3c71` | 같은 커밋. property 확장 중 찾은 것: 블러는 f32 누적합이 창 시작점에 따라 1 단계 다를 수 있어(soft edge가 창을 넓힘) 블러만 허용오차 1 |
| 5 | 대소문자 판정을 링크를 따라간 폴더 안에서: 기존 항목, 없으면 임시 파일(`.cloakframe-tmp-case-*`) | `d7d0e05` | `fs_case` `the_rule_is_the_folders_own_not_its_parents`(macOS, 대소문자 구분 APFS 이미지를 마운트), `probing_an_empty_folder_leaves_it_empty` |
| 9 | 폴더 규칙에 정규화 무시 여부를 더해 key를 NFC로. `unicode-normalization` 0.1.25(MIT OR Apache-2.0, 0.1.26은 사흘 전 공개라 피함) | `6499de5` | `output_plan` `duplicates_inside_the_run_follow_the_folder_normalization_rule`, `fs_case` key 시험 |
| 10 | 세션에서 빠진 항목을 이유와 함께 출력하고 종료 코드 1. 런타임 오류는 종료 코드 1과 오류 문구만, 2는 명령줄 오류에만 | `52986e7` | `batch` `items_left_out_of_the_session_are_reported`, `cli.rs` `runtime_errors_exit_with_one`, `cli_photo.rs` `only_the_pinned_model_runs` |
| 15 | rename 뒤 폴더 fsync 실패와 hard link 대체의 임시 이름 삭제 실패는 게시 성공 + note | `2335417` | `publish` `a_folder_that_cannot_be_flushed_still_publishes`(fsync 실패 주입) |
| 16 | `MoveFileExW`에 정규화한 `\\?\` 경로, `FileIdInfo`가 안 되면 `GetFileInformationByHandle` 파일 인덱스. Windows CI가 VHD로 FAT32 볼륨을 만들어 시험 | `d106765` | `publish` `a_destination_past_max_path_publishes`(모든 OS), `windows_fat32.rs`. **Windows에서는 아직 돌려 보지 못함**(CI에서 확인) |
| 18 | 저장된 Detection의 `kind`를 읽을 때 선택으로: 없으면 모델에서 유도, 있으면 모델과 맞아야 함 | `3cc42cd` | core `serde_forms.rs`(M1 형식) |
| 19 | 분석이 읽은 바이트의 SHA-256을 `Analysis.input`에 두고 세션 키가 그것을 씀(파일을 다시 읽지 않음). 검토를 렌더할 때 다시 읽은 바이트와 비교. 계획 4.2 문구 갱신 | `1d6c25c` | `photo_pipeline.rs` `a_review_of_other_bytes_with_the_same_stamp_is_not_rendered` |
| 20 | 재명명은 같은 종류의 확장자(사진은 이미지, 영상은 `.mp4`)와 Windows가 받는 이름만 | `15f3cc3` | `review` `a_taken_name_needs_a_free_rename` |
| 20 | 메모리 예산을 디코드 형식에서 계산: 2 × pixel 바이트 + 4 × 채널 + 4(블러 f32 창 포함). 측정한 최대치(16-bit RGBA 블러 37 B/px)를 덮음 | `81e05af` | `allocations.rs` `the_estimate_covers_the_peak_of_one_image` |
| 20 | 넓은 색역을 유지하는 사진에 그리는 사용자 이미지를 그 색 공간으로 변환 | `80a5d95` | `photo_pipeline.rs` `the_users_image_is_drawn_in_the_photos_color_space` |
| 20 | `PublishPermit::issue`는 crate가 나뉘어 타입으로 막을 수 없어 계획 4.2 문구를 코드에 맞춤. 알파를 버리는 JPEG 출력은 1.12.1과 같고 감지 사본도 같은 RGB를 보므로 유지. Windows 임시 파일의 숨김 속성(CHANGES에 "Windows에서는 보임")과 공유 위반 재시도는 원인 확인 전이라 바꾸지 않음 | 문서 | — |
| 21 | 시험 helper를 `tests/common`으로 모음(단위 시험은 `src/lib.rs`의 `test_support`로 포함), 할당 측정 시험 두 개는 한 바이너리에서 차례로 | `d54b918` | 시험 수 그대로(216) |
| 13 | 모자이크 강도 상한 16 → 12. D3 expected·계획 D3·CHANGES 갱신 | `be2ed6b` | core `mask` `invalid_inputs_are_refused`, `mask_fixtures.rs` |
| 14 | 감지 0건 사진에 "nothing detected" note(게시는 그대로). 계획의 범위 조정 순서에서 타일 모드를 뺌 | `e8889ce` | `cli_photo.rs` `a_rerun_keeps_the_earlier_output`, D2 파이프라인 시험의 note |

남은 것: 3개 OS CI(특히 Windows의 16번), 문서 표의 "남김" 항목(THIRD_PARTY_NOTICES, README, ROADMAP, 로그 정책, 시간 예산 문구).

# M0 spike: allowlist EXIF writer

실행일: 2026-10-16. 장치: Apple M4, macOS 27.0.1. 코드: [`spikes/exif-writer`](../spikes/exif-writer). [Rust 포팅 계획](RUST_PORT_PLAN.md) M0의 "EXIF writer spike" 항목이다.

## 결론

- `kamadak-exif` 0.6.1로 읽고, 1.12.1 allowlist만 골라 자체 직렬화기로 새 EXIF 블록을 만든 뒤 **JPEG(APP1)·PNG(eXIf)·WebP(VP8X + EXIF)·TIFF(IFD0 + Exif·GPS sub-IFD) 네 형식 모두에 넣었다.** 계획 11장의 "TIFF EXIF writer" 위험은 해소됐다.
- 28개 출력(원본 2종 × GPS 동의 유무 × 컨테이너 7종) 전부 [metadata fixture](../tests/fixtures/metadata/expected.json)의 기대값과 같고, 금지 태그(시리얼·소유자·작가·저작권·소프트웨어·ImageUniqueID·UserComment·MakerNote·XMP·IPTC·JPEG comment)는 하나도 없다. GPS는 동의했을 때만 9개 태그가 옮겨진다. 128 B를 넘는 항목(200 B `Make`)은 버린다.
- 독립 판독기 7개가 모두 읽었다: exiftool 13.55(값과 `-validate`), Exiv2 0.28.9, Pillow 12.3, macOS ImageIO(Finder·미리보기·사진 앱이 쓰는 경로), libwebp `webpinfo`, libtiff `tiffinfo`, ImageMagick 7.1. 모두 orientation 1, `Make`·`LensModel`을 같은 값으로 읽었다.
- `exiftool -validate`는 27개 파일에서 OK, TIFF + GPS 1개에서 minor 경고 하나(`Missing required TIFF GPS tag GPSProcessingMethod`)였다. 이 태그는 UNDEFINED 형식이라 allowlist 정책상 옮기지 않으므로 경고는 남는다.

## 쓰는 방법

| 형식 | 위치 | 비고 |
| --- | --- | --- |
| JPEG | SOI 뒤(JFIF APP0가 있으면 그 뒤) `APP1 "Exif\0\0"` + TIFF 블록 | APP1 하나는 65,533 B까지라 블록 전체를 65,527 B 이하로 제한해야 한다(spike는 넘으면 오류) |
| PNG | IHDR 바로 뒤 `eXIf`(TIFF 블록 그대로, CRC32) | 첫 IDAT 앞이면 된다 |
| WebP | 단순 형식(`VP8 `·`VP8L`)은 `VP8X`를 만들어 앞에 두고 EXIF 플래그(0x08)를 켬. 캔버스 크기는 비트스트림 헤더에서, alpha 플래그는 VP8L의 alpha 비트나 기존 VP8X에서 가져옴. `EXIF` chunk는 이미지 chunk 뒤 | 손실(VP8)·손실+alpha(VP8X+ALPH+VP8)·무손실+alpha(VP8L) 모두 `webpinfo` 오류 없음. 기존 EXIF·XMP chunk는 지움 |
| TIFF | 인코더가 쓴 IFD0 항목(값 필드 4바이트 그대로)에 Make·Model·Orientation·DateTime과 Exif·GPS 포인터를 더한 새 IFD0와 Exif·GPS IFD를 파일 끝에 붙이고 헤더의 첫 IFD offset을 바꿈 | strip 데이터와 기존 out-of-line 값은 그대로라 절대 offset이 유효하다. 옛 IFD0는 죽은 바이트로 남는다(약 200 B). little-endian classic TIFF, 단일 페이지만 |

EXIF 블록은 little-endian TIFF 헤더 + IFD0 + Exif IFD + (동의 시) GPS IFD다. 항목은 태그 번호순, 4바이트 넘는 값은 IFD 뒤에 짝수 정렬로 둔다. 원본 IFD1(썸네일)은 읽지 않는다.

**writer가 만드는 구조 태그**(원본에서 복사하지 않고 출력에서 계산, 사진·소유자 정보 없음):

| 태그 | 값 | 컨테이너 |
| --- | --- | --- |
| Orientation | 1(pixel에 반영한 뒤) | 전부 |
| ExifVersion | `0232` | 전부 |
| FlashpixVersion | `0100` | 전부 |
| ColorSpace | sRGB 출력이면 1, 그 밖이면 0xFFFF | 전부. allowlist에 있지만 원본 값을 쓰지 않는다(D2로 Display P3를 유지한 출력이 1이라고 주장하면 안 됨) |
| PixelXDimension·PixelYDimension | 출력 크기 | TIFF 제외(TIFF는 허용하지 않음) |
| XResolution·YResolution·ResolutionUnit, YCbCrPositioning | 72/1, 72/1, inch, centered | JPEG |
| ComponentsConfiguration | 1 2 3 0 | JPEG |

이 태그가 없으면 exiftool이 JPEG에서 "required tag missing" 경고 4–5개를 낸다(1.12.1 출력도 같은 경고를 냄).

## allowlist

1.12.1 `copyMetadata`의 Exiv2 키 31개는 태그 30개다(`Exif.Photo.ISOSpeedRatings`와 `PhotographicSensitivity`가 같은 0x8827). 필터 규칙은 1.12.1과 같다: 주 이미지 IFD만, UNDEFINED·알 수 없는 형식은 버림, 항목 128 B·합계 64 KiB 상한, GPS는 별도 동의 시 GPS IFD 전체(BYTE·ASCII·RATIONAL 항목만 남음). fixture가 기대값을 태그 이름과 값으로 고정한다.

## 계획에 반영할 것

- 계획 3장 이미지 항목의 "자체 allowlist EXIF writer"는 이 구조(약 500줄 중 직렬화·컨테이너 부분 약 300줄)로 M2에 옮긴다. `kamadak-exif`(BSD-2-Clause)는 읽기에만 쓴다.
- JPEG APP1 크기 상한 때문에 합계 상한은 "값 64 KiB"가 아니라 "블록 65,527 B"로 정의한다.
- D2의 ICC 부착과 함께 쓸 때: JPEG은 APP2 `ICC_PROFILE`, PNG는 `iCCP`(IDAT 앞, eXIf와 순서 무관), WebP는 VP8X ICC 플래그(0x20)와 `ICCP` chunk(VP8X 바로 뒤), TIFF는 IFD0 태그 34675. 이번 spike는 ICC를 다루지 않았다.
- 손실 WebP 인코딩(D6)은 `image` crate에 없다. 이 spike는 `cwebp`로 만든 손실 파일에 태그를 넣어 경로만 확인했다. M2에서 libwebp 바인딩(예: `webp` crate)을 정하고 라이선스(BSD-3-Clause)를 manifest에 적는다.

## 다시 돌리기

```bash
cd spikes/exif-writer && cargo build --release
python3 -I check.py "$(git rev-parse --show-toplevel)" <작업 폴더>
```

exiftool, exiv2, libwebp(`cwebp`, `webpinfo`), libtiff(`tiffinfo`), ImageMagick, Pillow, Xcode의 `swiftc`가 필요하다. 결과는 `<작업 폴더>/results.json`.

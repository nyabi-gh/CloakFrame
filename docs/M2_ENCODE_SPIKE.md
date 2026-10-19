# M2 0단계 spike: JPEG·WebP 코덱과 CI의 ORT 런타임

실행일: 2026-10-18. 장치: Apple M4, macOS 27.0.1. 코드: [`spikes/encode`](../spikes/encode)(`run.sh`), [`tools/fetch_native.py`](../tools/fetch_native.py), [`.github/workflows/ort.yml`](../.github/workflows/ort.yml). [계획](RUST_PORT_PLAN.md) M2의 D6(JPEG q92·원본 서브샘플링, 손실 WebP q90) 구현에 앞서 코덱을 정하는 spike다.

## 결론

- **`image` 0.25.10의 JPEG 디코더(zune-jpeg 0.5.15)는 non-interleaved(성분마다 scan이 따로인) baseline 4:2:0 JPEG를 조용히 잘못 디코드한다.** PSNR 5–9 dB(사실상 다른 그림)이고 오류도 내지 않는다. libjpeg로 만든 같은 형식(`cjpeg -scans`)에서도 재현된다. upstream에 같은 계열 이슈가 여럿 있고(#341, #419, #422, #448, #449, #482) 수정은 정식판이 아닌 0.5.16-rc에만 있다. 사진 입력에서 이 결함은 감지·마스크 위치를 틀리게 만들 수 있다.
- **libjpeg-turbo(`turbojpeg` crate, libjpeg-turbo 3.1.0을 소스에서 빌드)는 위 파일을 포함해 djpeg와 pixel 단위로 같게 디코드하고, 1.12.1이 쓰는 OpenCV 5.0.0과도 pixel 단위로 같다.** 인코딩은 4:4:4·4:2:2·4:2:0 모두 `cjpeg -optimize`(libjpeg-turbo 3.2.0)와 바이트까지 같다.
- `jpeg-encoder` 0.7.1(pure Rust)은 서브샘플링을 고를 수 있지만, optimized Huffman 표를 켜면 위의 non-interleaved 형식을 만들고, 4:2:2에서 화질이 libjpeg-turbo보다 낮다(lena RGB PSNR 46.3 vs 50.5 dB, djpeg로 디코드).
- **손실 WebP는 `libwebp-sys` 0.14.4(libwebp 1.6.0을 소스에서 빌드)로 충분하다.** q90 결과가 `cwebp` 1.6.0과 크기·화질이 같다. `webp` 0.3.1 crate는 CVE-2023-4863 수정 전의 libwebp 1.3.1을 묶어 쓰지 않는다.
- 제안: **JPEG 디코드·인코드 모두 libjpeg-turbo**, 손실 WebP는 libwebp, PNG·TIFF·무손실 WebP 디코드와 인코드는 `image`. libjpeg-turbo·libwebp는 C 소스를 crate 빌드에서 정적 링크한다(cmake·C 컴파일러 필요, x86 SIMD는 nasm).

## JPEG q92

`run.sh`의 표. PSNR은 원본 대비 RGB(dB), 디코더는 `image`(zune-jpeg) — 4:2:0 `jpeg-encoder` 행의 낮은 값이 그 디코더 결함이다. 같은 파일을 djpeg·Pillow·libjpeg-turbo로 풀면 astronaut 36.47 dB로 정상이다.

| 이미지 | 서브샘플링 | jpeg-encoder 바이트 / PSNR | cjpeg 바이트 / PSNR |
| --- | --- | --- | --- |
| astronaut 512² | 4:4:4 | 93,854 / 39.50 | 93,857 / 39.50 |
| | 4:2:2 | 82,021 / 37.31 | 80,883 / 38.06 |
| | 4:2:0 | 74,206 / **5.44** | 73,281 / 37.22 |
| lena 512²(원본 4:2:2) | 4:2:2 | 89,329 / 46.42 | 89,422 / 51.96 |
| messi5 548×342(원본 4:2:0) | 4:2:0 | 58,256 / **8.61** | 58,331 / 39.58 |
| 24 MP | 4:2:0 | 1,815,224 / **5.43** | 1,826,682 / 49.53 |

- 시간(24 MP, q92, 4:2:0): jpeg-encoder 104 ms, cjpeg 100 ms. libjpeg-turbo는 arm64에서 NEON을 쓴다.
- 서브샘플링 판독: 자체 SOF 파서, Pillow `get_sampling`, exiftool `YCbCrSubSampling`이 모든 출력에서 같은 값을 읽었다. 원본 서브샘플링은 SOF 하나로 정해지므로 M2에서 이 파서를 그대로 쓴다.
- ICC: `add_icc_profile`(APP2)로 붙인 Display P3를 exiftool·ImageMagick이 읽었다. libjpeg-turbo에도 같은 기능(`tj3SetICCProfile`)이 있다.

## 손실 WebP q90

| 이미지 | 알파 | libwebp-sys 바이트 / PSNR | cwebp 바이트 / PSNR | 구조 |
| --- | --- | --- | --- | --- |
| astronaut | 없음 | 52,912 / 37.68 | 52,912 / 37.68 | VP8 |
| car_image(원본 손실 WebP) | 없음 | 24,994 / 44.46 | 25,360 / 44.95 | VP8 |
| 24 MP | 없음 | 776,714 / 46.84 | 776,714 / 46.84 | VP8 |
| 위 셋 | 있음 | — | — | ALPH + VP8, `webpinfo` 오류 없음 |

- 알파가 있으면 libwebp 기본값은 알파를 무손실로 압축한다.
- car_image는 두 경로의 입력 디코더가 다르다(spike는 `image`, cwebp는 libwebp). PNG에서 같은 RGB를 넣은 astronaut·24 MP는 결과가 바이트까지 같다.
- 시간(24 MP): 847 ms(cwebp 1,148 ms, 파일 입출력 포함).

## CI의 ORT 런타임

- `tools/fetch_native.py <id> <platform> <dest>`는 `manifests/native.toml`의 URL만 받고, SHA-256이 다르면 멈추며, `library` 항목(`ort::init_from`에 줄 파일)의 경로를 출력한다. 두 번째 호출은 캐시를 쓴다. 이번에 ORT 항목마다 `library`를 추가했다.
- [`ort.yml`](../.github/workflows/ort.yml): Linux(공식 1.30.0)·macOS(공식 1.30.0)·Windows(Windows ML 2.4.89의 ORT 1.27.1)에서 ORT spike를 `ORT_DISABLE_TELEMETRY=1`로 돌려 감지 fixture 54건과 비교한다. 입력은 1.12.1과 같게 OpenCV 5.0.0(pip)으로 디코드한다.
- 2026-10-18 실행([run 39102924148](https://github.com/nyabi-gh/CloakFrame/actions/runs/39102924148)): 세 OS 모두 세 모델 × 18건이 개수 같고 빠진 감지 없음. 텔레메트리 꺼짐으로 로드됐다.

| OS(runner) | ORT | 최소 IoU(YOLO5Face / YuNet / 번호판) | 최대 score 차 | 최대 roll 차(rad) |
| --- | --- | --- | --- | --- |
| macOS 15 arm64 | 공식 1.30.0 | 0.999996 / 0.999996 / 1.0 | 6.3e-7 | 5.1e-6 |
| Ubuntu 24.04 x86_64 | 공식 1.30.0 | 0.999995 / 0.999990 / 1.0 | 4.5e-7 | 4.9e-6 |
| Windows Server 2025 x64 | Windows ML 2.4.89(1.27.1) | 0.999995 / 0.999990 / 1.0 | 5.1e-7 | 4.9e-6 |

- 이 workflow는 `manifests/native.toml`·`tools/fetch_native.py`·`spikes/ort`가 바뀌면 돈다. M2 3단계에서 engine의 감지기 시험이 같은 방식으로 런타임을 받는다.

## 결정 (2026-10-18)

- **JPEG 디코드·인코드는 libjpeg-turbo로 한다**(메인테이너 결정). 라이선스는 IJG + BSD-3-Clause + zlib이고 crate(`turbojpeg`, `turbojpeg-sys`)는 `Unlicense OR MIT`라 cargo-deny 허용 목록은 그대로 통과한다. C 라이브러리의 라이선스는 `manifests/native.toml`과 THIRD_PARTY_NOTICES에 적는다. IJG 라이선스는 문서에 "this software is based in part on the work of the Independent JPEG Group" 고지를 요구한다.
- 버린 대안: zune-jpeg 0.5.16 정식판을 기다리고 인코드는 jpeg-encoder. C 빌드는 없지만 디코드 결과가 1.12.1(OpenCV)과 달라(정상 파일에서도 PSNR 52–55 dB) 감지 fixture 비교가 근사가 되고, 디코더 결함의 수정 시점을 우리가 정할 수 없다.
- 손실 WebP는 libwebp(`libwebp-sys` 0.14.4, libwebp 1.6.0)로 한다. PNG·TIFF·BMP·WebP 디코드와 PNG·TIFF·무손실 WebP 인코드는 `image`가 맡는다. (M2 5단계에서 바뀜: TIFF 인코드는 LZW를 쓰려고 `tiff` crate로 직접 한다.)
- 릴리스 빌드(M7)는 x86에서 libjpeg-turbo SIMD를 쓰도록 nasm을 둔다. CI 시험 빌드는 nasm 없이 SIMD 없는 빌드도 허용한다(`require-simd` 끔).

## Windows의 LNK4098 경고 (2026-10-19)

Windows CI의 debug 시험 빌드는 engine의 모든 시험 바이너리에서 `LNK4098: defaultlib 'LIBCMTD' conflicts with use of other libs`를 낸다.

- 원인: `turbojpeg-sys` 1.2.0은 libjpeg-turbo 3.1.0을 CMake로 빌드하면서 CRT를 지정하지 않는다. libjpeg-turbo는 MSVC에서 `WITH_CRT_DLL`이 꺼져 있으면(기본값) 정적 CRT를 강제하고, Debug 빌드에서는 그 debug 판(`MultiThreadedDebug`, LIBCMTD)을 쓴다. Rust는 debug 빌드에서도 release CRT(기본 `/MD`의 MSVCRT, `+crt-static`이면 LIBCMT)를 링크하므로 둘이 어긋난다. `libwebp-sys`는 `cc`로 빌드해 Rust의 CRT를 따르므로 관계없다. `dumpbin /directives`로 확인했다([run 39199116791](https://github.com/nyabi-gh/CloakFrame/actions/runs/39199116791)).
- 영향: debug 빌드 전부와 `+crt-static` 없는 release 빌드가 어긋난다. 배포 빌드(release, `+crt-static`, [M0_ORT_SPIKE](M0_ORT_SPIKE.md))는 libjpeg-turbo도 `/MT`라 맞는다. 한 바이너리에 CRT 설정이 둘 섞이지만, `turbojpeg` 1.5.1은 버퍼를 모두 `tj3Alloc`·`tj3Free`로 다뤄 두 heap을 오가는 할당이 없고 시험도 통과한다. 지금 드러난 결함은 없고 잠재 위험으로 본다.
- 처리: 경고를 숨기지 않고 upstream에서 고친다([honzasp/rust-turbojpeg#44](https://github.com/honzasp/rust-turbojpeg/pull/44)). `build_vendor()`가 MSVC에서 `WITH_CRT_DLL=ON`과 Rust의 `crt-static`에 맞춘 `CMAKE_MSVC_RUNTIME_LIBRARY`(`MultiThreaded`/`MultiThreadedDLL`)를 넘기도록 한다. 이 수정을 path patch로 debug·release × 기본·`+crt-static` 네 조합에서 돌려 LNK4098이 없고 CRT가 Rust와 같으며 시험이 모두 통과함을 확인했다([run 39199628024](https://github.com/nyabi-gh/CloakFrame/actions/runs/39199628024)). 수정이 들어간 판이 나오면 Cargo.lock의 `turbojpeg-sys`를 올리고 이 절을 정리한다.

## 다시 돌리기

```bash
spikes/encode/run.sh <작업 폴더>
```

libjpeg-turbo(`cjpeg`, `djpeg`), libwebp 도구(`cwebp`, `webpinfo`), exiftool, ImageMagick, Pillow가 필요하다.

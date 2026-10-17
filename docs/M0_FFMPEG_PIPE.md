# M0 spike: FFmpeg 원시 프레임 파이프 처리량

실행일: 2026-10-16. 장치: Apple M4(10코어: 성능 4 + 효율 6), 24 GB, macOS 27.0.1, Homebrew FFmpeg 9.0.2. 코드: [`spikes/ffmpeg-pipe`](../spikes/ffmpeg-pipe), 1.12.1 기준선은 [`tools/fixture-extract/baseline_video.cpp`](../tools/fixture-extract/baseline_video.cpp). [Rust 포팅 계획](RUST_PORT_PLAN.md) 4.4의 처리량 항목이다.

## 결론

- **파이프는 병목이 아니다.** 프로세스 사이 파이프의 상한은 약 9.5–10 GB/s로, 4K30 bgr24가 방향당 요구하는 약 750 MB/s의 12배 이상이다. 처리 속도를 정하는 것은 인코더다.
- 4K 2패스(디코드 → 파이프 → 마스크 → 파이프 → 인코드)는 VideoToolbox H.264 인코더로 58 fps(실시간 1.9배), libx264 medium crf 18로 15 fps다. 같은 디코드·bgr24 변환·인코드를 FFmpeg 한 프로세스 안에서 하면 libx264 16.8 fps로, 파이프 구조의 비용은 약 10%다.
- 1.12.1 `processVideo` 전체(1패스 + 2패스 + 마무리)는 4K VideoToolbox 44.0 fps, 4K libx264 11.7 fps였다. 새 구조를 같은 방식으로 합산하면 4K VideoToolbox 약 46 fps로 1.12.1과 같은 수준이다. 계획 9.3의 "1.12.1보다 느려지지 않음"은 외부 프로세스 구조로 충족할 수 있다.
- 따라서 **`ffmpeg-next` 기반 프로세스 내 미디어 모듈은 검토하지 않는다**(계획 4.4·11장 반영). 처리량을 더 올릴 여지는 인코더 선택(하드웨어 우선)과 yuv420p 파이프(아래)에 있다.

## 입력

`ffmpeg-pipe-spike gen`이 만드는 10초 30 fps 클립. `testsrc2`에 시간 잡음을 더해 실제 촬영물 수준의 비트레이트가 나오게 했다. BT.709 태그.

| 클립 | 코덱 | 비트레이트 |
| --- | --- | --- |
| 1080p30-h264 | H.264(libx264 fast crf 20) | 15.4 Mbit/s |
| 2160p30-h264 | H.264(같음) | 56.1 Mbit/s |
| 2160p30-hevc | HEVC(libx265 fast crf 24) | 31.7 Mbit/s |

## 결과

fps는 300 프레임 기준, "배"는 실시간(30 fps) 대비, MB/s는 파이프를 지난 원시 데이터량이다. CPU는 FFmpeg 자식 프로세스의 user+sys 합(10코어).

### 디코드(1패스·2패스 reader)

| 경우 | 1080p H.264 | 4K H.264 | 4K HEVC |
| --- | --- | --- | --- |
| FFmpeg 안에서 bgr24 변환 후 버림(파이프 없음), 소프트웨어 | 804 fps | 202 fps | 229 fps |
| 같음, VideoToolbox | 269 | 105 | 149 |
| bgr24 → 파이프 → Rust, 소프트웨어(1.12.1 2패스 reader) | 455(2.8 GB/s) | 117(2.9 GB/s) | 127(3.2 GB/s) |
| bgr24 → 파이프, VideoToolbox | 318 | 111 | 150 |
| yuv420p → 파이프, 소프트웨어 | 677 | 171 | 193 |
| 긴 변 960 분석 해상도 bgr24 → 파이프, 소프트웨어(1패스) | 722 | 220 | 262 |
| 같음, VideoToolbox | 300 | 106 | 152 |

- 4K 디코드는 파이프로 읽어도 실시간의 3.9–5배다.
- 이 장치에서는 H.264를 VideoToolbox로 디코드하는 것이 소프트웨어보다 느렸다(4K 111 vs 117 fps, 1080p는 318 vs 455). HEVC는 VideoToolbox가 빨랐다(150 vs 127). CPU 사용은 VideoToolbox가 1/3 수준이다(4K H.264 3.2 s vs 10.2 s). 디코더 선택은 M4에서 배터리·발열과 함께 정한다.
- yuv420p로 받으면 데이터가 절반이고 변환 하나가 빠져 4K H.264가 117 → 171 fps가 된다. 마스크를 YUV에서 직접 그리면(트랙별 고정 격자 모자이크·채우기는 가능, 사용자 이미지·블러는 변환 필요) 이 이득을 얻는다. 첫 stable은 bgr24로 두고 M4의 선택 사항으로 남긴다.

### 인코드(2패스 writer, 메모리의 8 프레임을 반복해 넣음)

| 경우 | 1080p | 4K(H.264 원본 프레임) | 4K(HEVC 원본 프레임) |
| --- | --- | --- | --- |
| bgr24 → 파이프 → libx264 medium crf 18(1.12.1 기본) | 82 fps | 20.0 | 35.5 |
| 같음 + BT.709 행렬·태그(D1 수정) | 84 | 20.4 | 30.5 |
| yuv420p → 파이프 → libx264 | 88 | 21.8 | 31.0 |
| bgr24 → 파이프 → h264_videotoolbox | 212 | 61.2 | 60.7 |
| bgr24 → 파이프 → hevc_videotoolbox | 196 | 56.8 | 56.9 |

- D1 수정(`scale=out_color_matrix=bt709:out_range=tv`와 색 태그)은 비용 차이가 측정 오차 수준이다.
- libx264의 4K 속도는 프레임 내용에 따라 크게 달라진다(원본 프레임이 다른 두 열 20 vs 35 fps). 4K libx264는 실시간보다 느리므로 하드웨어 인코더가 기본이어야 한다(1.12.1과 같음).

### 2패스 전체(디코드 → 파이프 → 마스크 → 파이프 → 인코드)

마스크는 프레임마다 화면 1/30 크기 사각형을 채우는 대역 작업이다. 디코드와 인코드를 두 프로세스로 동시에 돌리고, 사이는 크기 4의 bounded channel이다.

| 경우 | 1080p | 4K H.264 | 4K HEVC |
| --- | --- | --- | --- |
| 소프트웨어 디코드 → libx264(1.12.1 구조) | 63 fps | 15.1 | 23.2 |
| 비교: FFmpeg 한 프로세스에서 디코드 → bgr24 → libx264(파이프 없음) | 68 | 16.8 | 27.8 |
| VideoToolbox 디코드 → h264_videotoolbox | 195 | 57.9 | 56.6 |
| 소프트웨어 디코드 → h264_videotoolbox | 205 | 58.5 | 59.1 |

### 1.12.1 기준선(`processVideo`, 감지 없음, 같은 클립)

| 클립 | 인코더 | fps(전체) | 실시간 대비 |
| --- | --- | --- | --- |
| 1080p H.264 | libx264 | 48.6 | 1.62배 |
| 1080p H.264 | h264_videotoolbox | 127.7 | 4.26배 |
| 4K H.264 | libx264 | 11.7 | 0.39배 |
| 4K H.264 | h264_videotoolbox | 44.0 | 1.47배 |
| 4K HEVC | libx264 | 22.0 | 0.73배 |
| 4K HEVC | h264_videotoolbox | 44.6 | 1.49배 |

1.12.1의 "fps(전체)"에는 1패스(분석 디코드와 장면 전환), 2패스, 마무리(`+faststart` 재작성과 게시, 4K libx264에서 4.2 s)가 모두 들어 있다. 새 구조의 같은 합산(1패스 220 fps + 2패스 58 fps)은 약 46 fps다. 1.12.1은 처리 전에 원본 전체를 사본으로 복사하는데 이 시간은 따로 재지 않았다. 새 구조는 사본을 만들지 않으므로 큰 파일·느린 디스크에서는 차이가 더 날 수 있다.

## Linux x86_64 (GitHub Actions, 소프트웨어만)

2026-10-17, GitHub Actions `ubuntu-24.04` 러너(AMD EPYC 7763 4 vCPU), Ubuntu FFmpeg 6.1.1, GPU 없음. 공유 VM이라 절대값은 참고용이고 구조 비교만 쓴다.

| 경우 | 1080p H.264 | 4K H.264 | 4K HEVC |
| --- | --- | --- | --- |
| 파이프 상한(bgr24, FFmpeg 없음) | 158 fps(약 0.98 GB/s) | 40 | 41 |
| bgr24 → 파이프 → Rust, 소프트웨어 | 102 | 27 | 47 |
| 긴 변 960 분석 해상도 → 파이프(1패스) | 106 | 31 | 56 |
| 2패스(디코드 → 파이프 → 마스크 → 파이프 → libx264 medium crf 18) | 10.9 | 2.7 | 5.1 |
| 같은 디코드·인코드를 FFmpeg 한 프로세스 안에서 | 11.5 | 2.7 | 5.0 |

- 4 vCPU VM의 파이프 상한은 약 1 GB/s로 macOS(9.5 GB/s)보다 훨씬 낮지만, libx264가 4K에서 3 fps 안팎이라 2패스 처리량은 한 프로세스 구성과 같다. **파이프 구조가 병목이 아니라는 결론은 Linux에서도 같다.**
- 하드웨어 인코더가 없는 Linux에서 4K 소프트웨어 인코딩은 실시간의 1/10 수준이다. M4에서 Linux 기본 프리셋(예: `medium` 대신 `fast`·`veryfast`)과 VAAPI 사용을 정한다.

## 계획에 반영할 것

- 4.4 처리량 항목: 측정 결과 파이프는 병목이 아니고, 외부 프로세스 구조를 유지한다. `ffmpeg-next` 검토 조건은 삭제한다.
- 인코더는 하드웨어 우선(1.12.1과 같음). 4K libx264는 실시간보다 느리므로 소프트웨어로 떨어지면 예상 시간을 그에 맞게 보여 준다.
- yuv420p 파이프와 YUV 마스킹은 M4의 선택적 최적화로 둔다(4K 디코드 46% 빠름, 파이프 데이터 절반).
- Windows·Linux 하드웨어 경로(NVENC·QSV·AMF·VAAPI)의 처리량은 같은 프로그램으로 [실행 절차](M0_PLATFORM_RUNBOOK.md)에서 잰다.

## 다시 돌리기

```bash
cd spikes/ffmpeg-pipe && cargo build --release
./target/release/ffmpeg-pipe-spike gen <작업 폴더>
./target/release/ffmpeg-pipe-spike bench <작업 폴더> > bench.jsonl
```

기본값은 macOS(VideoToolbox)다. 다른 플랫폼은 `--hw-decode <hwaccel|none>`과 `--hw-encoders <이름,...>`으로 바꾼다(Windows·Linux 절차에 명령이 있음).

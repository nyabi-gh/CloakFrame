# M5 4단계: 영상 탐지 품질

작성 2026-10-24. [Rust 포팅 계획](RUST_PORT_PLAN.md) 8장 M5 4단계(`m5-benchmark`)와 9.2의 "영상은 라벨이 있는 짧은 클립에서 노출 프레임 수를 잰다"의 기록이다. 장치는 Apple M4(10코어), macOS, release, CPU, 고정 FFmpeg 9.0.2·ORT 1.30.0. 도구는 `tools/benchmark/`의 `video_fetch.py`·`video_sheets.py`·`video_evaluate.py`·`record_video.sh`·`run_legacy_video.sh`·`legacy/legacy_video.cpp`와 engine 예제 `video_benchmark`, 자료는 `tests/fixtures/benchmark/video/`, CI는 `.github/workflows/video-benchmark.yml`이다.

## 결론

- **1.12.1 대비(9.2 합격선):** 같은 설정(YOLO5Face-n 0.5, 타일 끔)에서 새 엔진이 드러내는 얼굴 프레임은 27,714 중 5,322(19.2%)로 1.12.1의 7,791(28.1%)보다 32% 적다. 세 클립 모두 1.12.1 이하다.
- **기본 설정(타일 켬):** 912(3.3%). 노출은 거의 모두 높이 24 px 미만인 얼굴이다.
- **차이의 출처:** 1.12.1의 미해결 gap(보간 2/3초 < 다시 잇기 1초)과 짧은 track 끊김이 새 엔진에서는 보간(2초 창)으로 메워진다. 1.12.1은 세 클립에 gap 3·3·3곳이 있어 자동 모드에서 모두 "검토 필요"였고, 새 엔진은 0곳이다(M5 계획·2단계 결정의 결과).
- **CI:** 추적·영상 분석 코드가 바뀌는 PR에서 Linux가 기록된 감지를 다시 추적해, 기록보다 얼굴 프레임의 1% 넘게 더 드러나거나 타일 끔이 1.12.1보다 많이 드러나면 실패한다. 영상은 받지 않는다(1분 안쪽).

## 결정(2026-10-24, 메인테이너가 추천대로)

시작할 때 네 구간으로 실험하고 물었다([실험](#시작할-때의-실험)).

- **클립:** 이미 쓰던 Commons 영상의 세 구간(거리 19초·도로 20초·군중 12.5초). 인터뷰는 세 실행 모두 노출 0이라 뺐다. 저조도·측면 계층은 약하다.
- **기준:** 표본 프레임의 상자 태깅. 1.12.1·타일 끔·타일 켬의 track 상자를 표본 프레임(거리·군중 15·60프레임, 도로 6프레임마다)에서 모아 crop 시트로 보고 얼굴/애매/아님으로 태깅하고, 전체 프레임 점검으로 어느 실행도 찾지 못한 얼굴을 더했다. 사람을 track으로 잇지 않고 표본 하나가 간격만큼의 프레임을 대표한다.
- **노출 정의:** 기준 상자 면적의 50% 미만이 마스크(track 상자 + padding 0.18, 렌더러와 같은 바깥 반올림)로 덮이면 그 프레임에서 노출. 주 수치는 "얼굴" 태그, 높이 <16·16–24·≥24 px로 나눠 보고하고 "애매"는 따로 센다.
- **CI:** 추적만 다시 도는 게이트. 엔진 감지 기록(gzip, 4 MB)과 1.12.1 track 기록(460 KB)을 저장소에 넣는다. 감지 변화는 사진 벤치마크가 맡는다.

## 클립

`tests/fixtures/benchmark/video/manifest.json`. 원본의 앞부분만 HTTP range로 받아(WebM 앞부분은 그것만으로 디코드됨) 고정 FFmpeg로 자르고, 원본 조각과 자른 결과를 SHA-256으로 확인한다(`video_fetch.py`). 저장소에는 넣지 않는다.

| 클립 | Commons 파일 | 라이선스 | 받는 양 | 자르기 | 프레임 |
| --- | --- | --- | --- | --- | --- |
| 거리 | Walking in BRISTOL - UK - 4K 60fps (UHD).webm (POPtravel) | CC BY 3.0 | 앞 150 MiB | 153.607초부터 19초, stream copy | 1,140 (1080p60) |
| 도로 | Road traffic on Stritarjeva street.webm (Sounds of Changes) | CC BY 3.0 | 앞 16 MiB | 처음 20.4초, stream copy | 510 (1080p25) |
| 군중 | Pyrkon 2026 - ambient flow of people 1.webm, Commons 1080p VP9 변환본 (Rose Abrams) | CC BY 4.0 | 앞 12 MiB | 처음 12.5초, BT.709 태그를 붙여 libx264 crf 16 한 스레드로 다시 인코드 | 750 (608×1080, 60) |

군중 원본은 HLG·BT.2020이라 두 구현 모두 거부한다(M9 전까지 의도). 같은 pixel에 BT.709 태그만 붙였다. 한 스레드 libx264와 `bitexact`로 다시 만들어도 같은 바이트였다.

## 실행

- **1.12.1:** `run_legacy_video.sh`가 v1.12.1 트리에 `legacy_video.cpp`를 붙여 빌드하고(8초), `processVideo`를 그대로 돌린다. 검토 콜백이 후처리를 마친 track(표시 pixel)을 받아 기록하고 Skip해 2패스는 하지 않는다. 감지는 `ProcessorWorker`처럼 하한 0.1, CPU 세션. 같은 고정 FFmpeg로 디코드한다.
- **엔진:** `video_benchmark` 예제가 run과 같은 1패스(`first_pass`)와 추적(`tracking::track`)을 돌려 같은 형식으로 쓴다. `--detections`는 감지 레코드를 값 그대로 JSON 줄로 쓰고, `--from-detections`는 그것을 레코드로 되돌려 추적만 다시 한다. 도로 클립에서 다시 추적한 track·gap이 직접 실행과 같았다.
- **시간(이 Mac, 감지까지):** 1.12.1 거리 43초·도로 21초·군중 32초. 엔진 타일 끔은 그 절반쯤, 타일 켬은 약 1.5배([실험](#시작할-때의-실험) 표).

## 기준 만들기

1. `video_sheets.py boxes`가 세 실행의 얼굴 track 상자를 표본 프레임에서 모아(같은 자리 IoU 0.5 이상은 하나) 번호 붙은 crop 시트를 만든다. 거리 1,466개(31장), 도로 599개(13장), 군중 843개(18장, 초당 4프레임이면 3,415개라 1초 간격).
2. 시트를 눈으로 보고 번호를 적었다. **얼굴:** 실제 사람 얼굴(측면, 작고 흐린 얼굴, 마스크·모자로 일부 가린 얼굴 포함). **애매:** 뒤통수·거의 뒤로 돈 머리, 판단할 수 없게 흐린 것. **아님:** 손·물건·간판·광고 속 얼굴·코스튬 가면.
3. `video_sheets.py audit`으로 표본 프레임 몇 장을 사람 있는 띠만 두 배로 키워 상자와 함께 보고, 어느 실행도 상자를 두지 않은 얼굴을 `added`로 더했다. 거리에서 3개(높이 10–12 px, 2초 간격으로 셈), 도로·군중은 찾지 못했다(군중의 보이는 얼굴은 모두 어느 실행이든 후보가 있었다).
4. `video_sheets.py reference`가 태그와 색인으로 `<clip>.reference.json`을 쓴다(같은 프레임에서 IoU 0.3 이상 겹치면 하나).

| 클립 | 표본 간격 | 얼굴 | 애매 | 더한 얼굴 | 얼굴 프레임(가중) |
| --- | --- | --- | --- | --- | --- |
| 거리 | 15 | 454 | 405 | 3 | 7,170 |
| 도로 | 6 | 144 | 137 | 0 | 864 |
| 군중 | 60 | 328 | 141 | 0 | 19,680 |

한계: 후보가 실행 결과에서 나오므로 어느 실행도 찾지 못한 얼굴은 점검으로만 들어오고, 점검은 표본 몇 장뿐이다. 그런 얼굴은 세 실행 모두에서 노출로 세지므로 비교는 그대로지만 비율은 낮게 나온다. 군중은 1초 간격이라 표본 하나의 무게가 60프레임이다.

## 결과

얼굴 프레임 중 노출(50% 미만 덮임):

| 클립 | 1.12.1 | 타일 끔 | 타일 켬 |
| --- | --- | --- | --- |
| 거리(7,170) | 2,745 (38.3%) | 2,430 (33.9%) | 480 (6.7%) |
| 도로(864) | 246 (28.5%) | 192 (22.2%) | 12 (1.4%) |
| 군중(19,680) | 4,800 (24.4%) | 2,700 (13.7%) | 420 (2.1%) |
| 전체(27,714) | 7,791 (28.1%) | 5,322 (19.2%) | 912 (3.3%) |

크기별(세 클립 합):

| 높이 | 얼굴 프레임 | 1.12.1 | 타일 끔 | 타일 켬 |
| --- | --- | --- | --- | --- |
| 16 px 미만 | 19,257 | 6,297 | 4,542 | 660 |
| 16–24 px | 5,331 | 1,389 | 675 | 252 |
| 24 px 이상 | 3,126 | 105 | 105 | 0 |

애매 태그의 노출은 1.12.1 8,511 / 타일 끔 7,014 / 타일 켬 585(15,357 중)로 같은 순서다.

track 수와 gap(얼굴): 거리 72·3곳 / 69·0 / 146·0, 도로 61·3곳 / 65·0 / 175·0, 군중 224·3곳 / 208·0 / 335·0(1.12.1 / 타일 끔 / 타일 켬). 24 px 이상의 노출은 1.12.1과 타일 끔이 105로 같고 타일 켬은 0이다.

## CI 게이트

`video-benchmark.yml`(Linux, 추적·장면 전환·레코드·분석·`video_benchmark`·기준 파일이 바뀌는 PR과 main push, 수동 실행): `detections/*.jsonl.gz` 여섯 개를 `--from-detections`로 다시 추적하고 `video_evaluate.py --gate`로 `expected.json`(타일 끔 5,322, 타일 켬 912)과 1.12.1 기록에 비교한다. 허용은 얼굴 프레임의 1%(277). 추적 점수를 0.7로 올린 결과(노출 17,085)로 게이트가 실패하는 것을 확인했다. 기록은 이 Mac(arm64)에서 다시 추적한 결과다. 같은 감지를 x86 Linux에서 추적해도 같은 값이었다(PR #53 첫 실행).

추적 결과를 일부러 바꾸는 변경은 `expected.json`을 새 값으로 바꾸고 기록에 이유를 적는다. 감지를 바꾸는 변경은 `record_video.sh`로 감지 기록을 다시 만든다(클립 필요).

## 시작할 때의 실험

네 구간(인터뷰 16초 포함)을 1.12.1·타일 끔·타일 켬으로 돌렸다(구간을 stream copy로만 잘라 군중은 다시 인코드 전 판).

| 구간 | 프레임 | 1.12.1 track·gap | 타일 끔 | 타일 켬 | 시간(1.12.1/끔/켬) |
| --- | --- | --- | --- | --- | --- |
| 인터뷰 | 384 | 1·0 | 1·0 | 1·0 | 16/9/25초 |
| 도로 | 510 | 61·3곳(62프레임) | 65·0 | 175·0 | 21/13/36초 |
| 거리 | 1,140 | 72·3곳(179프레임) | 69·0 | 146·0 | 56/28/88초 |
| 군중 | 752 | 225·7곳(501프레임) | 197·0 | 331·0 | 37/18/53초 |

거리 구간을 0.5초 간격으로 태깅(확실한 얼굴 150개)해 정의별로 쟀다: 25% 미만 덮임 1,260/1,110/60, 50% 미만 1,320/1,140/60, 90% 미만 1,410/1,260/60, 중심점 미덮임 1,320/1,140/60(1.12.1/끔/켬, 얼굴 프레임 4,500). 순위는 어느 정의로도 같고, 50%와 중심점이 같은 값이었다. 노출은 모두 24 px 미만에서 나왔다. stream copy로 자른 군중 구간에서는 1.12.1이 프레임을 755개로 셌다(엔진 752, 앞 번호는 IoU로 맞음). 다시 인코드한 클립에서는 둘 다 750이고, 평가는 실행의 프레임 수가 클립보다 적으면 거부한다.

## 재현

```bash
clips=$(python3 -I tools/benchmark/video_fetch.py "$CLOAKFRAME_FFMPEG" ~/.cache/cloakframe-video-benchmark)
tools/benchmark/run_legacy_video.sh "$CLOAKFRAME_TEST_ASSETS" "$CLOAKFRAME_FFMPEG" "$CLOAKFRAME_FFPROBE" "$clips" <작업 폴더>
tools/benchmark/record_video.sh "$CLOAKFRAME_ORT_LIBRARY" "$CLOAKFRAME_TEST_ASSETS" "$CLOAKFRAME_FFMPEG" "$CLOAKFRAME_FFPROBE" "$clips"
# 다시 추적과 평가(CI와 같음)
for f in tests/fixtures/benchmark/video/detections/*.jsonl.gz; do n=$(basename "$f" .jsonl.gz)
  gzip -dc "$f" > "$n.jsonl"
  cargo run --release -p cloakframe-engine --example video_benchmark -- --from-detections "$n.jsonl" --out "$n.json"; done
python3 -I tools/benchmark/video_evaluate.py --gate *.json tests/fixtures/benchmark/video/legacy/*.json.gz
# 태깅(Pillow 필요)
python3 tools/benchmark/video_sheets.py boxes "$CLOAKFRAME_FFMPEG" "$CLOAKFRAME_FFPROBE" "$clips/street.webm" 15 sheets/street \
  tests/fixtures/benchmark/video/legacy/street.legacy.json.gz street.tiles-off.json street.tiles-on.json
```

## 남은 것

- 완료: PR #53(2026-10-24, main `ed070a6`). 3개 OS CI·탐지 벤치마크(87.9%)와 영상 추적 벤치마크 첫 실행이 통과했다. x86 Linux에서 다시 추적한 노출이 기록과 같았다(타일 끔 5,322, 타일 켬 912, 1.12.1 7,791). 같은 실행의 1080p 파이프: 디코드 Linux 87 fps·macOS runner 219·Windows 102, 인코드 Linux 78·macOS runner 125·Windows 84. 1패스(debug, spin 끔/켬/끔/타일): macOS runner 6.5/6.9/5.7/2.0, Linux 6.8/6.4/6.9/2.4, Windows 6.6/5.8/6.6/2.1 fps.
- 저조도·측면 위주 클립, 번호판 영상 기준(번호판은 기본 꺼짐).
- 표본 사이 구멍의 길이는 표본 간격보다 잘게 재지 못한다. 연속 노출 길이가 필요하면 사람별 keyframe track이 필요하다.
- 5단계 `m5-verify`: 완료 기준 표, 1시간 영상 RAM, 처리량(9.3), 범위 조정 점검.

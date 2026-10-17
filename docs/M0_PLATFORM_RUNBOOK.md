# M0 실행 절차: Windows · Linux

[Rust 포팅 계획](RUST_PORT_PLAN.md) M0의 남은 실기기 항목을 메인테이너가 직접 돌리기 위한 절차다. macOS 결과는 [ORT](M0_ORT_SPIKE.md), [EXIF](M0_EXIF_WRITER.md), [egui](M0_EGUI.md), [FFmpeg 파이프](M0_FFMPEG_PIPE.md) 기록에 있다. 스크립트는 [`spikes/platform-runs`](../spikes/platform-runs)에 있다.

| 항목 | Linux | Windows |
| --- | --- | --- |
| ORT `api-27` CPU 결과가 1.12.1 fixture와 같은지 | 스크립트 | 스크립트 |
| DML 결과·실행 중 DML 실패 시 같은 입력 CPU 재실행 | — | 스크립트 + GPU 강제 리셋 |
| `INTER_LINEAR` 이식이 x86 OpenCV와 바이트 일치하는지(36쌍, IPP 켬·끔) | 스크립트 | 스크립트 |
| ORT 텔레메트리를 끈 상태의 네트워크 시도 | 스크립트(strace, 네트워크 namespace) | 스크립트(프로세스 소켓 감시) |
| 1.12.1 패키지의 ORT가 텔레메트리를 보내는지 | 스크립트(AppImage 안 ORT) | — (DirectML 1.24.4는 ETW 경로) |
| FFmpeg 파이프 처리량, 하드웨어 인코더 | 스크립트(VAAPI·NVENC 자동 감지) | 스크립트(NVENC·QSV·AMF 자동 감지) |
| egui 성능 | 스크립트 | 스크립트 |
| egui IME·스크린리더·드롭·메뉴·GPU 없음·원격(M6 점검으로 옮김, M0에서는 하지 않음) | 사람(아래) | 사람(아래) |
| 깨끗한 최소 OS에서 오프라인 실행 | — | Windows 10 1903 VM(아래) |
| Intel·AMD GPU | — | 해당 PC에서 같은 스크립트 |

2026-10-17 기준 Linux는 GitHub Actions(`.github/workflows/m0-linux.yml`, `--no-gui`)로 돌리고, Windows 항목은 계획 8장의 M3·M7·M8 완료 기준으로 옮겼다. 이 절차는 그 단계에서 그대로 쓴다.

결과는 각 스크립트가 만드는 폴더(또는 zip)의 `summary.txt`와 JSON·텍스트 파일을 그대로 넘겨 주면 된다. 기록 문서 반영은 그다음에 한다.

## 1. Linux

### 준비

Ubuntu 24.04 이상(glibc 2.39, 지원 하한) x86_64, 데스크톱 세션. GPU가 없어도 된다.

```bash
sudo apt install -y build-essential pkg-config curl git python3-venv ffmpeg strace binutils squashfs-tools \
    libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev libxrandr-dev libxi-dev libgl1-mesa-dev mesa-vulkan-drivers
```

```bash
curl -fsSL https://sh.rustup.rs | sh -s -- -y
```

### 자동 실행

```bash
spikes/platform-runs/linux/run_m0_linux.sh
```

약 15–25분 걸린다. 텔레메트리 시험의 일부는 `sudo unshare --net`으로 네트워크가 없는 namespace에서 돌리므로 암호를 한 번 묻는다(그 안에서는 텔레메트리를 켠 실행도 밖으로 나가지 못한다). sudo를 쓸 수 없으면 `--no-sudo`, 화면이 없는 서버면 `--no-gui`를 붙인다. 결과 폴더는 `m0-linux-<호스트>-<날짜>`다.

스크립트가 하는 일:

1. 모델·표본 이미지를 해시로 받고, 공식 ORT 1.30.0 linux-x64를 받는다(SHA-256 `a5ed5a3c…3b3fd`).
2. `opencv-python-headless` 5.0.0.93으로 기준 decode·resize를 만들고 이식 resize와 36쌍을 비교한다.
3. ORT spike(CPU)를 [감지 fixture](../tests/fixtures/detect)와 비교하고, 폴백 경로를 시뮬레이션으로 한 번 확인한다.
4. 텔레메트리: ① `ORT_DISABLE_TELEMETRY=1`, 네트워크 허용, 90초 ② 라이브러리 기본값, 네트워크 namespace ③ 1.12.1 AppImage 안의 ORT를 1.12.1 방식(텔레메트리 호출 없음)으로, 네트워크 namespace ④ 같은 ORT에 환경 변수만. 각각 `LD_PRELOAD` 감시 라이브러리(`spikes/ort/netwatch/netwatch_linux.c`, libc의 getaddrinfo·connect·sendto 기록)와 `strace` 두 가지로 시도를 세고, `~` 아래 `onnxruntime.db*` 변경을 찾는다. 에뮬레이션(예: Apple silicon의 x86 컨테이너)에서는 `strace` 출력이 깨지므로 libnetwatch 수치를 본다.
5. FFmpeg 파이프 처리량을 잰다. `/dev/dri/renderD128`과 `h264_vaapi`가 있으면 VAAPI, `nvidia-smi`가 있으면 NVENC를 함께 잰다.
6. egui spike를 빌드하고 자동 시험과 성능을 잰다.

판정 기준:

- `SUMMARY`의 세 모델 모두 `count_mismatches` 0, `unmatched` 0, `min_iou` ≥ 0.9999.
- `RESIZE`의 `mismatching`이 빈 배열(IPP 켬·끔 둘 다). 아니면 macOS의 KleidiCV처럼 HAL이 있는지 `python -c "import cv2; print(cv2.getBuildInformation())"`의 `HAL` 줄을 같이 보낸다.
- `disabled-open`의 attempts가 libnetwatch·strace 모두 0이고 저장소 변경 0. `default-netns`와 `v1121-untouched-netns`에서 `mobile.events.data.microsoft.com` 조회가 보이면 "기본값으로 전송 시도"가 Linux에서도 확인된 것이다. libnetwatch의 `loaded` 값이 0이면 감시가 붙지 않은 것이므로 결과를 쓰지 않는다.

### 사람이 할 확인 (egui)

```bash
cd spikes/egui && ./target/release/egui-spike
```

X11 세션과 Wayland 세션에서 각각 한 번씩 한다(로그인 화면의 톱니 메뉴에서 선택). 매번 `echo $XDG_SESSION_TYPE`을 같이 적는다.

1. **IME**: 한국어는 `fcitx5-hangul`(또는 `ibus-hangul`), 일본어는 `fcitx5-mozc`(또는 `ibus-mozc`). macOS 절차 1의 2–5와 같은 순서로 "이름"·"메모" 칸을 시험한다(조합 표시 위치, Backspace, Enter, 다른 칸 클릭, 후보 창 위치). Wayland에서 fcitx5는 `text-input-v3`를 써야 하므로 GNOME이면 ibus가 더 안정적일 수 있다. 쓴 입력기 이름과 버전(`fcitx5 --version`)을 적는다.
2. **Orca**: Super+Alt+S로 켜고 Tab·화살표로 이동하며 읽히는 내용을 적는다(macOS 절차 2의 기대 목록과 같음). AccessKit의 AT-SPI 연결은 `at-spi2-core`가 필요하다.
3. **드롭**: 파일 관리자(Nautilus·Dolphin)에서 파일·폴더를 끌어 놓는다. Wayland에서 경로가 들어오는지 특히 본다.
4. **메뉴**: Linux는 muda를 쓰지 않으므로 네이티브 메뉴 없음이 정상이다. Ctrl+C·Ctrl+V·Ctrl+Z가 텍스트 칸에서 되는지만 본다.
5. **GPU 없는 환경**: 다음 세 가지로 실행해 창이 뜨는지와 `--bench upload`의 fps를 적는다.

```bash
WGPU_BACKEND=vulkan VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json ./target/release/egui-spike --bench upload
```

```bash
LIBGL_ALWAYS_SOFTWARE=1 ./target/release/egui-spike --renderer glow --bench upload
```

```bash
WGPU_BACKEND=gl LIBGL_ALWAYS_SOFTWARE=1 ./target/release/egui-spike --bench upload
```

6. **원격 데스크톱**: GNOME 원격 데스크톱(RDP) 또는 xrdp로 접속한 세션에서 앱을 열고, 창이 뜨는지·글자 입력·드롭(원격 클라이언트에서 끌어오기 가능하면)을 본다.
7. **배율**: 디스플레이 배율 100%·200%·분수 배율(125%)에서 글자가 흐린지.

## 2. Windows

### 2.1 번들 만들기 (개발 PC)

필요한 것: Rust(`x86_64-pc-windows-msvc`), Visual Studio Build Tools, Git for Windows, Python 3.11 이상. FFmpeg를 번들에 넣으려면 gyan.dev의 full 빌드처럼 NVENC·QSV·AMF가 들어 있는 빌드를 받아 폴더를 지정한다.

```powershell
powershell -ExecutionPolicy Bypass -File spikes\platform-runs\windows\build_bundle.ps1 -Out C:\m0\m0-windows-bundle -FfmpegDir C:\tools\ffmpeg\bin
```

결과 폴더에는 정적 CRT로 빌드한 `ort-spike.exe`·`ffmpeg-pipe-spike.exe`·`egui-spike.exe`, Windows ML 2.4.89의 `onnxruntime.dll`(ORT 1.27.1)·`DirectML.dll`(1.15.4)과 약관, 모델, fixture, OpenCV 기준 이미지, `run_m0_windows.ps1`, `bundle-sha256.txt`가 들어간다. 이 폴더를 그대로 다른 PC로 옮긴다.

### 2.2 연결된 PC에서 (RTX, Intel, AMD 각각)

관리자 PowerShell에서:

```powershell
powershell -ExecutionPolicy Bypass -File C:\m0\m0-windows-bundle\run_m0_windows.ps1 -ForceTdr
```

`-ForceTdr`는 폴백 시험 중 `dxcap -forcetdr`로 GPU를 한 번 리셋한다(화면이 1–2초 꺼졌다 돌아온다). `dxcap`은 선택적 기능 "그래픽 도구"에 들어 있다.

```powershell
Add-WindowsCapability -Online -Name Tools.Graphics.DirectX~~~~0.0.1.0
```

`dxcap`이 없거나 관리자가 아니면 스크립트가 안내 문구를 찍고 기다리므로, 그동안 다른 관리자 창에서 `dxcap -forcetdr`을 직접 실행한다.

판정 기준:

- `ort-cpu`, `ort-dml`의 `SUMMARY`가 Linux와 같은 기준. RTX 5080에서는 이미 통과했으므로 Intel·AMD에서 같은지가 핵심이다.
- `module:` 두 줄이 번들 폴더의 `onnxruntime.dll`·`DirectML.dll`을 가리키고 `System32`가 아니어야 한다.
- `sockets owned by the process` 0.
- `fallback-simulated`에 `cpu_rerun` `matches_fixture: true`.
- `fallback-tdr`: `accelerator_failure` 이벤트가 찍히고 `cpu_rerun`이 `true`, 이후 `cpu_runs`가 늘었으면 계획 3장의 GPU 실패 정책이 실제 장치 오류에서 동작한 것이다. 리셋 뒤에도 오류 없이 DML로 계속 돌았다면(`failure: null`) 그 사실을 적는다. 프로세스가 죽었다면 `fallback-tdr.txt.err`와 Windows 이벤트 뷰어의 응용 프로그램 오류를 함께 보낸다(계획 0.1의 "driver crash가 측정되면 프로세스로 분리" 판단 근거).

### 2.3 egui 사람 확인 (Windows)

```powershell
C:\m0\m0-windows-bundle\egui-spike.exe --menu predefined
```

1. **IME**: "Microsoft 입력기"(한국어)와 "Microsoft IME"(일본어). macOS 절차 1의 2–6과 같은 순서. 한자 변환은 한국어 입력 중 한자 키(또는 오른쪽 Ctrl).
2. **Narrator**: Win+Ctrl+Enter로 켜고 Tab·Caps Lock+화살표로 이동하며 읽히는 내용을 적는다. 입력란에서 한글을 칠 때 읽는지.
3. **드롭**: 탐색기에서 파일·폴더를 끌어 놓는다. 관리자 권한으로 실행한 앱에는 일반 탐색기에서 드롭이 막히는 것이 Windows의 정상 동작이므로, 일반 권한으로 실행한다.
4. **메뉴**: `--menu predefined`, `--menu custom`, `--menu plain` 각각에서 창 위 메뉴 막대가 보이는지, 파일 > 파일 추가…(Ctrl+O)가 메뉴와 단축키 모두로 "메뉴 이벤트"를 늘리는지, 텍스트 칸에서 Ctrl+A·C·V·Z가 되는지, Alt 키로 메뉴에 들어가는지.
5. **배율**: 디스플레이 배율 100%·150%·175%, 모니터 사이 이동.
6. **원격 데스크톱**: 다른 PC에서 RDP로 접속해 `egui-spike.exe`와 `egui-spike.exe --renderer glow`를 각각 실행, 창이 뜨는지와 `--bench upload` fps.
7. **GPU 없는 환경**: 2.4의 VM(기본 디스플레이 어댑터)에서 같은 두 실행. wgpu는 WARP(DX12 소프트웨어)로 뜰 수 있고 glow는 OpenGL 1.1만 있어 실패할 수 있다. 어느 쪽이 뜨는지가 결과다.

### 2.4 깨끗한 Windows 10 1903 VM, 오프라인

목적: Windows ML 패키지가 정한 최소 OS(빌드 18362)에서, 업데이트·VC++ 런타임·네트워크 없이 번들이 실행되는지. 설치 프로그램은 M7에서 만들므로 지금은 폴더 복사가 "설치"다.

1. Windows 10 1903(빌드 18362) ISO를 준비한다. Media Creation Tool은 최신판만 주므로 Visual Studio 구독(my.visualstudio.com)이나 볼륨 라이선스의 "Windows 10 (business editions), version 1903"을 쓴다.
2. Windows PC의 Hyper-V에서 2세대 VM을 만든다(vCPU 4, 메모리 8 GB, 디스크 64 GB). **네트워크 어댑터는 "연결 안 됨"**으로 두고 설치한다. 설치 중 로컬 계정을 만들고, 설치 후에도 네트워크를 연결하지 않는다(업데이트로 빌드가 올라가지 않게).
3. `winver`로 18362.x인지 확인하고 화면을 남긴다.
4. 번들 전달: 호스트에서 VHDX를 만들어 번들을 복사한 뒤 VM에 SCSI 디스크로 붙인다.

```powershell
New-VHD -Path C:\m0\bundle.vhdx -SizeBytes 4GB -Dynamic | Mount-VHD -Passthru | Initialize-Disk -Passthru | New-Partition -AssignDriveLetter -UseMaximumSize | Format-Volume -FileSystem NTFS -NewFileSystemLabel M0
```

위 명령이 출력한 `DriveLetter`(예: `F`)로 복사한다.

```powershell
Copy-Item -Recurse C:\m0\m0-windows-bundle F:\
```

```powershell
Dismount-VHD C:\m0\bundle.vhdx
```

```powershell
Add-VMHardDiskDrive -VMName Win10-1903 -Path C:\m0\bundle.vhdx
```

5. VM 안에서(관리자 PowerShell 5.1):

```powershell
powershell -ExecutionPolicy Bypass -File E:\m0-windows-bundle\run_m0_windows.ps1 -SkipFfmpeg
```

드라이브 문자는 VM에서 확인한다. 결과는 같은 VHDX의 `results-*.zip`에 남으므로 VM을 끄고 호스트에서 VHDX를 마운트해 꺼낸다.

6. 판정: `VC++ … installed: no`, `network reachable: False`이면서 `ort-cpu`가 통과해야 한다. Hyper-V의 기본 디스플레이 어댑터에서 `ort-dml`은 WARP로 돌거나 DML 장치 생성에서 실패할 수 있다. 실패하면 오류 문구가 결과이며, 앱은 이 경우 CPU로 시작해야 한다(계획 3장). GPU-P(GPU 분할)를 붙인 VM이 있으면 DML도 한 번 더 돌린다.
7. 시작 실패(`0xC0000135` 등)가 나면 그 대화상자 내용과 `bundle-sha256.txt`를 함께 보낸다.

### 2.5 Intel·AMD GPU

2.2와 같은 명령을 Intel(예: Arc, Iris Xe)·AMD(예: Radeon RX, Ryzen 내장) PC에서 돌린다. 드라이버 버전이 `summary.txt`의 `GPU:` 줄에 남는다. FFmpeg 처리량에서 `h264_qsv`(Intel)·`h264_amf`(AMD)가 잡혔는지도 본다.

## 3. 결과 넘기기

- Linux: `m0-linux-*/summary.txt`와 같은 폴더의 `*.json`·`*.txt`·`*.strace`. `work/`는 지워도 된다.
- Windows: `results-<PC 이름>-<날짜>.zip`.
- 사람 확인: 항목별 "정상 / 이상(증상)"과 장치·OS·입력기 버전. 이상은 화면 녹화가 있으면 가장 좋다.

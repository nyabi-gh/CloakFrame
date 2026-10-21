# M4 0단계: FFmpeg 고정과 외부 프로세스 감독

작성 2026-10-21. [Rust 포팅 계획](RUST_PORT_PLAN.md) 8장 M4 0단계(`m4-process`)의 기록이다. 코드는 engine `media::process`, CLI의 신호 처리, 시험은 `crates/cloakframe-engine/tests/media_process.rs`와 `media::process` 단위 시험, `tests/cli.rs`의 Ctrl+C 시험이다.

## FFmpeg 판

1.12.1에서 이어받은 8.1.2(macOS·Windows)·n8.1.3(Linux) 대신 세 OS 모두 **9.0.2**로 고정했다(`manifests/native.toml`).

- 9.0.2는 2026-09-17에 나온 현재 안정판이다(n9.0.2 태그). 2주 규칙을 넘었고, M0 파이프 처리량 측정도 Homebrew 9.0.2로 했다.
- macOS arm64: martin-riedl.de 빌드(ffmpeg·ffprobe 따로). `--enable-gpl --enable-version3`, nonfree 없음. 인코더 목록에 `h264_videotoolbox`·`hevc_videotoolbox`·`libx264`·`aac`가 있다.
- Windows x64: gyan.dev **essentials** 9.0.2(해시는 gyan이 함께 올린 `.sha256`과 같음). full을 쓰지 않는다: essentials의 `ffmpeg.exe`에도 `h264_nvenc`·`hevc_nvenc`·`h264_qsv`·`h264_amf`·`h264_mf`·`libx264`가 들어 있다(2026-10-21, 실행 파일의 인코더 이름으로 확인. 실제 하드웨어 동작은 3단계와 장치가 생긴 뒤).
- Linux x86_64: BtbN `autobuild-2026-09-30-13-08`의 n9.0.2-17. BtbN은 날마다 빌드를 올리고 지난 빌드는 월말 것만 남겨서, 월말 빌드에 고정했다(8.1 고정값도 같은 날짜였다).
- `tools/fetch_native.py`가 zip을 풀 때 Unix 실행 권한을 버려 macOS 도구가 실행되지 않던 것을 고쳤다(zip 항목의 mode를 다시 씀).

## 프로세스 감독

계획 0.1에 따라 FFmpeg는 보안 경계가 아니다. 감독은 정리 목적이다.

- **명령:** `ToolCommand::ffmpeg`·`ffprobe`가 `-hide_banner -loglevel error`(ffmpeg는 `-nostdin`도)를 붙인다. 파일 입력은 `-protocol_whitelist file -i file:<경로>`, 파이프 입력은 `-protocol_whitelist pipe -i pipe:0`이다. `file:` 접두어는 `concat:a|b` 같은 이름이 다른 프로토콜로 읽히지 않게 하고, 허용 목록은 파일 안의 플레이리스트·demuxer가 여는 하위 입력에도 적용된다. FFmpeg의 HLS 기본값은 로컬 플레이리스트의 네트워크 세그먼트를 이미 거부하지만 `data:`는 연다. 시험은 `data:` 세그먼트가 우리 목록 때문에 거부되는 것으로 목록이 적용됨을 확인한다.
- **프로세스 트리:** Unix는 도구를 자기 프로세스 그룹(`process_group(0)`)에서 시작하고 취소하면 그룹 전체에 SIGKILL. Windows는 `CREATE_NEW_PROCESS_GROUP`으로 시작해 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` Job Object에 넣고 취소하면 `TerminateJobObject`. 별도 그룹이라 터미널의 Ctrl+C는 CLI에만 가고, CLI가 도구를 끝낸다. Windows는 CLI가 정리 없이 죽어도 마지막 Job 핸들이 닫히며 도구가 끝난다.
- **취소:** 도구마다 감시 스레드가 취소 토큰을 50 ms마다 보고 트리를 죽인다. 파이프 읽기·쓰기가 막혀 있어도 취소가 닿는다. 그룹 종료와 회수(reap)는 같은 잠금 아래에서만 해서, 이미 회수한 pid(OS가 다른 프로세스에 줄 수 있음)에 신호를 보내지 않는다. 그래서 회수는 막는 `wait` 대신 짧은 간격(1 → 50 ms)의 `try_wait`다.
- **stderr:** 끝 8 KiB만 남겨 실패 메시지에 붙인다(4.7의 상한).
- **drop:** 끝나지 않은 도구를 놓으면 트리를 죽이고 회수한다.
- **늦은 kill:** 도구가 stdout을 닫고 끝났지만 아직 회수되지 않았을 때 취소가 오면 kill은 끝난 프로세스에 닿는다(macOS에서 좀비에 보낸 SIGKILL은 성공하고 종료 상태는 그대로). 그래서 `wait`는 종료 상태를 먼저 보고, 성공이면 정상 종료로, 아니면서 kill했을 때만 취소로 보고한다. 처음 판은 kill 표시를 먼저 봐서 #41 CI의 macOS runner에서 끝난 도구를 취소로 보고했다(그 시험은 200 ms sleep에 기대고 있었다).

## CLI Ctrl+C

M1에서 넘어온 항목이다. `ctrlc` 3.5.2(MIT/Apache-2.0, `termination` feature로 SIGTERM·SIGHUP과 Windows 콘솔 닫기도 받음).

- 첫 신호: 실행 전체의 취소 토큰을 취소하고 stderr에 안내를 쓴다. 스캔·분석·렌더·다운로드가 토큰을 보고 멈추며, 렌더 중이던 임시 파일은 지워지고 아무것도 게시되지 않는다. 결과를 출력한 뒤 종료 코드 **130**.
- 두 번째 신호: 바로 종료(130). 남은 `.cloakframe-tmp-*`는 다음 실행이 하루 지난 것을 지운다(4.5).
- 시작할 때 무시로 설정된 신호(`nohup`의 SIGHUP, 스크립트의 백그라운드 작업이 받는 SIGINT)는 처리기를 건 뒤 다시 무시로 둔다. `ctrlc::set_handler`는 그런 신호에도 처리기를 덮어써, 처음 판은 `nohup`으로 띄운 실행이 터미널을 닫으면 취소됐다([재검토](M4_REVIEW.md) 5번).

## 확인

macOS arm64(2026-10-21): 전체 시험(`--all-features`), clippy 두 조합, cargo-deny 0.20.2 통과. 새 crate(`ctrlc` 3.5.2와 그 의존성 nix·objc2·block2·dispatch2·cfg_aliases·objc2-encode)는 모두 공개 2주가 지났고 허용 목록 라이선스다.

- `media_process.rs` 7건: 끝까지 실행한 출력, 실패 시 stderr 끝, 취소한 도구가 끝남, drop한 도구가 끝남, stdin으로 넣은 프레임이 stdout으로 돌아옴, `data:` 세그먼트가 허용 목록에 막힘, ffprobe.
- `media::process` 단위 5건: 취소·drop이 손자 프로세스까지 끝냄(Unix는 `sh`, Windows는 PowerShell이 띄운 `Start-Sleep` PowerShell. 처음에 쓴 `ping`은 같은 stdout에 빈 줄부터 써서 시험이 pid 대신 그 줄을 읽는 일이 CI에서 한 번 있었다), 끝났지만 아직 회수되지 않은 도구에 닿은 kill은 취소로 보고하지 않음(Unix, `waitid(WNOWAIT)`로 그 상태를 만듦), 없는 프로그램, stderr 상한.
- `run` 단위 `cancelling_the_callers_token_cancels_the_run`, `cli.rs` `interrupt` 2건(Unix): 첫 SIGINT에 렌더 중인 항목·대기 항목이 모두 취소되고 임시 파일 없이 130, 취소를 무시하는 항목이 있어도 두 번째 SIGINT에 바로 130.
- mutation(시험이 잡는지): 그룹 대신 자식만 kill, 허용 목록 뺌, 감시 스레드가 취소를 무시함, CLI가 실행에 토큰을 넘기지 않음 모두 시험이 실패한다. 처음 판의 시험은 앞의 셋을 놓쳤다. 손자가 stderr를 물고 있어 `wait`가 손자 수명만큼 걸린 것을 시간 상한 없이 통과했고, FFmpeg HLS 기본값이 http 세그먼트를 이미 막아 우리 목록과 무관하게 통과했으며, 끝없는 출력을 읽는 시험은 취소가 안 되면 실패 대신 멈췄다. 세 시험을 고쳤다.
- Windows의 Job Object·`CREATE_NEW_PROCESS_GROUP` 코드와 트리 시험은 이 Mac에서 컴파일할 수 없어 CI가 처음 확인한다. Windows 콘솔 Ctrl+C는 자동 시험이 없다(시험에서 다른 프로세스에 콘솔 Ctrl+C를 보내기 어렵다). Unix 시험이 같은 취소 경로를 확인한다.

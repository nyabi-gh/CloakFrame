# M0 platform runs

Scripts the maintainer runs on Linux and Windows hardware for the remaining M0 checks. They build and drive the spikes next to this folder (`ort`, `ffmpeg-pipe`, `egui`). Procedure, pass criteria and what to send back: [docs/M0_PLATFORM_RUNBOOK.md](../../docs/M0_PLATFORM_RUNBOOK.md).

- `linux/run_m0_linux.sh` — Ubuntu 24.04+ x86_64
- `windows/build_bundle.ps1` — on a Windows development machine, makes a portable bundle
- `windows/run_m0_windows.ps1` — from the bundle, on each target PC and on the clean Windows 10 1903 VM

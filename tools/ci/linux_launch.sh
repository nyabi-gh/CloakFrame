#!/usr/bin/env bash
# Starts the AppImage on a bare distribution image and waits for its window, so a library the
# package expects from the host fails the release instead of the user's first launch. Installs
# only what any graphical desktop has, and runs the tools the package carries.
#
# Run inside a container with the package mounted, as root.
# Usage: linux_launch.sh <AppImage>
set -euo pipefail

app=$1

. /etc/os-release
case " $ID ${ID_LIKE:-} " in
    *" debian "* | *" ubuntu "*)
        export DEBIAN_FRONTEND=noninteractive
        apt-get update -qq
        apt-get install -y -qq --no-install-recommends \
            xvfb xauth x11-utils libgl1 libegl1 libgl1-mesa-dri libxkbcommon0 libxkbcommon-x11-0 \
            libxcursor1 libxrandr2 libxi6 ca-certificates >/dev/null
        ;;
    *" fedora "*)
        dnf install -y -q xorg-x11-server-Xvfb xorg-x11-xauth xwininfo mesa-libGL mesa-libEGL \
            mesa-dri-drivers libxkbcommon libxkbcommon-x11 libXcursor libXrandr libXi \
            ca-certificates >/dev/null
        ;;
    *" arch "*)
        pacman -Sy --noconfirm --needed xorg-server-xvfb xorg-xauth xorg-xwininfo mesa libglvnd \
            libxkbcommon libxkbcommon-x11 libxcursor libxrandr libxi ca-certificates >/dev/null
        ;;
    *)
        echo "Unsupported distribution: $ID" >&2
        exit 1
        ;;
esac

# A container has no FUSE.
export APPIMAGE_EXTRACT_AND_RUN=1 LC_ALL=C.UTF-8 HOME="$(mktemp -d)"
log=$(mktemp)

cd "$HOME"
"$app" --appimage-extract >/dev/null
squashfs-root/usr/bin/ffmpeg -hide_banner -version | head -n 1
squashfs-root/usr/bin/ffprobe -hide_banner -version | head -n 1

xvfb-run -a -s "-screen 0 1280x800x24" bash -c '
    "$1" > "$2" 2>&1 &
    pid=$!
    for _ in $(seq 120); do
        if ! kill -0 "$pid" 2>/dev/null; then
            echo "CloakFrame exited before showing its window." >&2
            exit 1
        fi
        if grep -q "^graphics: " "$2" && xwininfo -root -tree | grep -q "\"CloakFrame\""; then
            kill "$pid"
            exit 0
        fi
        sleep 1
    done
    echo "CloakFrame showed no window within two minutes." >&2
    kill "$pid"
    exit 1
' _ "$app" "$log" || { cat "$log" >&2; exit 1; }
grep '^graphics: ' "$log"
echo "CloakFrame started on $PRETTY_NAME."

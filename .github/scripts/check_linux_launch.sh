#!/usr/bin/env bash
set -euo pipefail

# Starts CloakFrame on a bare distribution image and waits for its main window, so a library the
# package expects from the host fails the release instead of the user's first launch.
#
# Run inside a container with the package mounted; installs only what any graphical desktop has.
# Usage: check_linux_launch.sh <AppImage or AppDir/AppRun>

app="${1:?usage: check_linux_launch.sh <AppImage or AppRun>}"

. /etc/os-release
case " $ID ${ID_LIKE:-} " in
    *" debian "* | *" ubuntu "*)
        export DEBIAN_FRONTEND=noninteractive
        apt-get update -qq
        apt-get install -y -qq --no-install-recommends \
            xvfb xauth x11-utils libgl1 libegl1 libopengl0 libfontconfig1 fonts-dejavu-core \
            libxkbcommon0 libxkbcommon-x11-0 libdbus-1-3 libglib2.0-0t64 libgssapi-krb5-2 \
            > /dev/null
        ;;
    *" fedora "*)
        dnf install -y -q xorg-x11-server-Xvfb xorg-x11-xauth xwininfo mesa-libGL mesa-libEGL \
            libglvnd-opengl fontconfig dejavu-sans-fonts libxkbcommon libxkbcommon-x11 dbus-libs \
            glib2 krb5-libs > /dev/null
        ;;
    *" arch "*)
        pacman -Sy --noconfirm --needed xorg-server-xvfb xorg-xauth xorg-xwininfo mesa \
            libglvnd fontconfig ttf-dejavu libxkbcommon libxkbcommon-x11 dbus glib2 krb5 > /dev/null
        ;;
    *)
        echo "Unsupported distribution: $ID" >&2
        exit 1
        ;;
esac

export APPIMAGE_EXTRACT_AND_RUN=1 QT_QPA_PLATFORM=xcb LC_ALL=C.UTF-8 HOME="$(mktemp -d)"
log="$(mktemp)"
xvfb-run -a -s "-screen 0 1280x800x24" bash -c '
    "$1" > "$2" 2>&1 &
    pid=$!
    for _ in $(seq 60); do
        if ! kill -0 "$pid" 2> /dev/null; then
            echo "CloakFrame exited before showing its window." >&2
            exit 1
        fi
        if xwininfo -root -tree | grep -q "\"CloakFrame"; then
            kill "$pid"
            exit 0
        fi
        sleep 1
    done
    echo "CloakFrame showed no window within a minute." >&2
    kill "$pid"
    exit 1
' _ "$app" "$log" || { cat "$log" >&2; exit 1; }
echo "CloakFrame started on $PRETTY_NAME."

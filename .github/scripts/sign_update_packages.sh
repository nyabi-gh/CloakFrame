#!/usr/bin/env bash
set -euo pipefail

# Signs every Velopack package in a directory with the offline update key, writing the detached
# signature beside each package as <package>.sig.
#
# The signature is made over a short text naming the channel, version, file name and lowercase
# hex SHA-256 of the package (UpdateSignature.cpp builds the same text), not over its bytes: it
# keeps a hundred-megabyte package out of memory on both sides, and it stops an older signed
# package from being republished as a newer version.
#
# Usage: sign_update_packages.sh <directory> <channel> <version>
# Reads CLOAKFRAME_UPDATE_PRIVATE_KEY (PEM) and CLOAKFRAME_UPDATE_PUBLIC_KEY (base64, optional).

directory="${1:?usage: sign_update_packages.sh <directory> <channel> <version>}"
channel="${2:?usage: sign_update_packages.sh <directory> <channel> <version>}"
version="${3:?usage: sign_update_packages.sh <directory> <channel> <version>}"

if [[ -z "${CLOAKFRAME_UPDATE_PRIVATE_KEY:-}" ]]; then
    # Matches the client: with no key configured there is nothing to check a signature against,
    # so producing one would be theatre. Release identity is verified separately.
    echo "No update signing key is configured; packages are published unsigned."
    exit 0
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
key="$work/update-key.pem"
(umask 077; printf '%s\n' "$CLOAKFRAME_UPDATE_PRIVATE_KEY" > "$key")

# A private key that does not belong to the pinned public key would be caught by every client
# and by nothing before them, so the two halves are compared here while it is still cheap.
derived="$(openssl pkey -in "$key" -pubout -outform DER | tail -c 32 | base64 | tr -d '\n')"
if [[ -n "${CLOAKFRAME_UPDATE_PUBLIC_KEY:-}" && "$derived" != "$CLOAKFRAME_UPDATE_PUBLIC_KEY" ]]; then
    echo "CLOAKFRAME_UPDATE_PRIVATE_KEY does not match CLOAKFRAME_UPDATE_PUBLIC_KEY." >&2
    echo "The key in the secret has public half '$derived'." >&2
    echo "Every client would reject this update. Refusing to sign." >&2
    exit 1
fi
openssl pkey -in "$key" -pubout -out "$work/update-key.pub"

shopt -s nullglob
packages=("$directory"/*.nupkg)
if [[ ${#packages[@]} -eq 0 ]]; then
    echo "No packages to sign in '$directory'." >&2
    exit 1
fi

# Signs the file $1 and writes the base64 signature to $2.
sign() {
    openssl pkeyutl -sign -inkey "$key" -rawin -in "$1" -out "$work/signature"
    # Signing silently producing something unverifiable is the one failure this step cannot
    # notice later, so the signature is read back through the public half before it is kept.
    openssl pkeyutl -verify -pubin -inkey "$work/update-key.pub" -rawin \
        -in "$1" -sigfile "$work/signature" > /dev/null
    base64 < "$work/signature" | tr -d '\n' > "$2"
}

for package in "${packages[@]}"; do
    digest="$(openssl dgst -sha256 -hex "$package" | awk '{print $NF}')"
    printf 'CloakFrame update v2\nchannel: %s\nversion: %s\nfile: %s\nsha256: %s\n' \
        "$channel" "$version" "$(basename "$package")" "$digest" > "$work/release"
    sign "$work/release" "$package.sig"
    echo "signed $(basename "$package") $channel $version $digest"
done

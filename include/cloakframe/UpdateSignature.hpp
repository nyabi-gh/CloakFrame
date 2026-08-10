#pragma once

#include <QByteArray>
#include <QString>

#include <optional>

namespace cloakframe
{
    // Ed25519 verification for in-app updates.
    //
    // The release feed, the package it names and the hash it publishes for that package all
    // live in one trust domain: whoever can publish a release controls all three. A signature
    // made with a key held offline is the only part of an update that domain cannot forge, so
    // the public key is compiled into the binary and an update that does not verify against it
    // is not applied. The signature itself may travel with the release; a forged one fails.
    //
    // Signatures are made over the lowercase hex SHA-256 of the package rather than over its
    // bytes, so neither the signer nor the client has to hold a whole package in memory.

    // Returns true only on a positive match. Malformed input is a failure, never a pass.
    [[nodiscard]] bool verifyUpdateSignature(const QByteArray &payload,
        const QString &signatureBase64,
        const QString &publicKeyBase64,
        QString *error = nullptr);

    enum class UpdateTrust
    {
        // The pinned key signed this update's digest.
        Trusted,
        // This build pins no key, so there is nothing to check the update against. Sparkle
        // behaves the same way on macOS: the check exists exactly when a key was configured.
        Unpinned,
        // A key is pinned and the update did not verify against it. Never apply one of these.
        Rejected,
    };

    // Decides whether an update may be applied, given the SHA-256 the release feed declares for
    // its package and a detached signature over that digest.
    //
    // Only the digest is checked, because the updater exposes no path to the package it
    // downloaded. That is sound as long as the updater enforces the digest it was given: the
    // signature fixes which digest is legitimate, and the digest fixes which package is. The
    // digest is compared in lowercase, since that is the form the signature is made over.
    [[nodiscard]] UpdateTrust evaluateUpdateTrust(const QString &declaredSha256Hex,
        const QString &signatureBase64,
        const QString &pinnedPublicKeyBase64,
        QString *error = nullptr);

    // Lowercase hex SHA-256 of a file, streamed so cost stays bounded regardless of size.
    [[nodiscard]] std::optional<QByteArray> sha256HexOfFile(
        const QString &path, QString *error = nullptr);
}

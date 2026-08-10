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

    // Lowercase hex SHA-256 of a file, streamed so cost stays bounded regardless of size.
    [[nodiscard]] std::optional<QByteArray> sha256HexOfFile(
        const QString &path, QString *error = nullptr);
}

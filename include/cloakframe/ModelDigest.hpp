#pragma once

#include <QByteArray>
#include <QString>

#include <atomic>

namespace cloakframe
{
    // The identity of a model file as a run sees it: where it resolves to and what its bytes
    // hash to. The digest is the one that consent is checked against and that the detector
    // requires of the bytes it loads.
    struct ModelFileDigest
    {
        QString canonicalPath;
        qint64 size = -1;
        qint64 lastModifiedMs = -1;
        // Raw 32-byte SHA-256; empty when the file could not be read in full.
        QByteArray sha256;

        [[nodiscard]] bool isValid() const
        {
            return !canonicalPath.isEmpty() && !sha256.isEmpty();
        }
    };

    // Hashes the model at `path`. A custom model may be up to `kMaxCustomModelBytes`, so this
    // can take seconds and belongs off the GUI thread. Files that are empty, over the limit, or
    // change size while being read get no digest, and neither does a read that `cancel` stops.
    [[nodiscard]] ModelFileDigest digestModelFile(
        const QString &path, const std::atomic_bool *cancel = nullptr);
}

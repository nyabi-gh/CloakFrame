#pragma once

#include <QByteArray>
#include <QString>

#include <atomic>

namespace cloakframe
{
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

    // Reads up to `kMaxCustomModelBytes`, so call it off the GUI thread. No digest for files
    // that are empty, over the limit, or change size while read, or when `cancel` is set.
    [[nodiscard]] ModelFileDigest digestModelFile(
        const QString &path, const std::atomic_bool *cancel = nullptr);
}

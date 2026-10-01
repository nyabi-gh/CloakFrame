#include "cloakframe/ModelDigest.hpp"

#include "cloakframe/ModelCatalog.hpp"

#include <QCryptographicHash>
#include <QDateTime>
#include <QDir>
#include <QFile>
#include <QFileInfo>

namespace cloakframe
{
    ModelFileDigest digestModelFile(const QString &path, const std::atomic_bool *cancel)
    {
        if (path.isEmpty())
        {
            return {};
        }

        const QFileInfo info(path);
        ModelFileDigest digest;
        digest.canonicalPath = info.canonicalFilePath();
        if (digest.canonicalPath.isEmpty())
        {
            digest.canonicalPath = QDir::cleanPath(info.absoluteFilePath());
        }
        if (!info.exists() || !info.isFile() || info.size() <= 0
            || info.size() > kMaxCustomModelBytes)
        {
            return digest;
        }
        digest.size = info.size();
        digest.lastModifiedMs = info.lastModified().toMSecsSinceEpoch();

        QFile file(digest.canonicalPath);
        if (!file.open(QIODevice::ReadOnly))
        {
            return digest;
        }
        QCryptographicHash hash(QCryptographicHash::Sha256);
        qint64 hashedBytes = 0;
        while (!file.atEnd())
        {
            if (cancel != nullptr && cancel->load())
            {
                return digest;
            }
            const QByteArray chunk = file.read(qint64{1024} * 1024);
            if (chunk.isEmpty())
            {
                break;
            }
            if (hashedBytes > kMaxCustomModelBytes - chunk.size())
            {
                return digest;
            }
            hashedBytes += chunk.size();
            hash.addData(chunk);
        }
        if (file.error() == QFileDevice::NoError && hashedBytes == digest.size)
        {
            digest.sha256 = hash.result();
        }
        return digest;
    }
}

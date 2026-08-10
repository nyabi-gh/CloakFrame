#include "cloakframe/UpdateCache.hpp"

#include "cloakframe/UpdateSignature.hpp"

#include <QFile>
#include <QFileInfo>

#ifndef _WIN32
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>
#endif

namespace cloakframe
{
    CacheDirectoryTrust inspectCacheDirectory(const QString &path)
    {
#ifdef _WIN32
        Q_UNUSED(path);
        return CacheDirectoryTrust::Unsupported;
#else
        const QByteArray native = QFile::encodeName(path);
        struct stat info{};
        // lstat, not stat: a symlink planted here points the updater somewhere else entirely,
        // and following it would report on the target rather than on what is in the cache.
        if (::lstat(native.constData(), &info) != 0)
        {
            return CacheDirectoryTrust::Absent;
        }
        if (!S_ISDIR(info.st_mode))
        {
            return CacheDirectoryTrust::Shared;
        }
        if (info.st_uid != ::getuid())
        {
            return CacheDirectoryTrust::Shared;
        }
        if ((info.st_mode & (S_IWGRP | S_IWOTH)) != 0)
        {
            return CacheDirectoryTrust::Shared;
        }
        return CacheDirectoryTrust::Private;
#endif
    }

    bool fileMatchesDigest(const QString &path, const QString &expectedHex)
    {
        const QString expected = expectedHex.trimmed().toLower();
        if (expected.isEmpty())
        {
            return false;
        }
        const QFileInfo info(path);
        if (!info.exists() || !info.isFile())
        {
            return false;
        }
        const auto digest = sha256HexOfFile(path);
        return digest && QString::fromLatin1(*digest) == expected;
    }
}

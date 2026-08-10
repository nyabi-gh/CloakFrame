#pragma once

#include <QString>

namespace cloakframe
{
    // The Windows and Linux updater keeps downloaded packages in a cache directory, and on
    // Linux that directory is under /var/tmp, a location every account on the machine shares.
    // The updater skips downloading when a package of the expected name is already there and
    // never hashes it, so another user who creates the tree first and leaves a package with
    // the right filename decides what the next update installs.
    //
    // These two checks are what a client can do about that without replacing the updater's
    // own path discovery: refuse a cache another account controls, and refuse to trust a file
    // that is not the one the release feed describes.

    enum class CacheDirectoryTrust
    {
        // Exists, this user owns it, and no one else can write to it.
        Private,
        // Exists, but another user owns it or group/other can write to it.
        Shared,
        // Nothing is there, so there is nothing to distrust yet.
        Absent,
        // Not a POSIX filesystem; ownership means something different and is not checked here.
        Unsupported,
    };

    [[nodiscard]] CacheDirectoryTrust inspectCacheDirectory(const QString &path);

    // True only when `path` is a regular file whose SHA-256 is exactly `expectedHex`. A missing
    // file, an unreadable one, or a mismatch all return false: each means the caller has not
    // been shown the package the feed described.
    [[nodiscard]] bool fileMatchesDigest(const QString &path, const QString &expectedHex);
}

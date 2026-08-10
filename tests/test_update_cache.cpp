#include "cloakframe/UpdateCache.hpp"

#include <QDir>
#include <QFile>
#include <QTemporaryDir>

#include <cassert>
#include <cstdio>
#include <fstream>

#ifndef _WIN32
#include <sys/stat.h>
#endif

namespace
{
    QString makeDirectory(const QTemporaryDir &root, const QString &name)
    {
        const QString path = root.filePath(name);
        assert(QDir().mkpath(path));
        return path;
    }

    void testAPrivateDirectoryIsAccepted()
    {
        QTemporaryDir root;
        assert(root.isValid());
        const QString path = makeDirectory(root, QStringLiteral("private"));
#ifndef _WIN32
        assert(::chmod(QFile::encodeName(path).constData(), 0700) == 0);
        assert(cloakframe::inspectCacheDirectory(path) == cloakframe::CacheDirectoryTrust::Private);
        // Read and traverse for others is not a way in; only writing is.
        assert(::chmod(QFile::encodeName(path).constData(), 0755) == 0);
        assert(cloakframe::inspectCacheDirectory(path) == cloakframe::CacheDirectoryTrust::Private);
#else
        assert(cloakframe::inspectCacheDirectory(path)
               == cloakframe::CacheDirectoryTrust::Unsupported);
#endif
    }

#ifndef _WIN32
    void testAWritableDirectoryIsRejected()
    {
        QTemporaryDir root;
        assert(root.isValid());
        const QString path = makeDirectory(root, QStringLiteral("open"));

        // Either bit is enough: anyone who can write the directory can replace the package
        // inside it, whoever happens to own it.
        assert(::chmod(QFile::encodeName(path).constData(), 0770) == 0);
        assert(cloakframe::inspectCacheDirectory(path) == cloakframe::CacheDirectoryTrust::Shared);
        assert(::chmod(QFile::encodeName(path).constData(), 0707) == 0);
        assert(cloakframe::inspectCacheDirectory(path) == cloakframe::CacheDirectoryTrust::Shared);
        assert(::chmod(QFile::encodeName(path).constData(), 0777) == 0);
        assert(cloakframe::inspectCacheDirectory(path) == cloakframe::CacheDirectoryTrust::Shared);
    }

    void testSomethingThatIsNotADirectoryIsRejected()
    {
        QTemporaryDir root;
        assert(root.isValid());

        const QString file = root.filePath(QStringLiteral("file"));
        {
            std::ofstream out(file.toStdString());
            out << "not a directory";
        }
        assert(cloakframe::inspectCacheDirectory(file) == cloakframe::CacheDirectoryTrust::Shared);

        // A symlink is judged as itself, not as its target: following it would report on
        // somewhere else while the updater still uses this path.
        const QString target = makeDirectory(root, QStringLiteral("target"));
        assert(::chmod(QFile::encodeName(target).constData(), 0700) == 0);
        const QString link = root.filePath(QStringLiteral("link"));
        assert(QFile::link(target, link));
        assert(cloakframe::inspectCacheDirectory(link) == cloakframe::CacheDirectoryTrust::Shared);
    }
#endif

    void testAMissingDirectoryIsNotADecision()
    {
        QTemporaryDir root;
        assert(root.isValid());
        assert(cloakframe::inspectCacheDirectory(root.filePath(QStringLiteral("absent")))
#ifdef _WIN32
               == cloakframe::CacheDirectoryTrust::Unsupported);
#else
               == cloakframe::CacheDirectoryTrust::Absent);
#endif
    }

    void testDigestMatchingAcceptsOnlyTheDescribedFile()
    {
        QTemporaryDir root;
        assert(root.isValid());
        const QString path = root.filePath(QStringLiteral("package.nupkg"));
        {
            std::ofstream out(path.toStdString(), std::ios::binary);
            out << "abc";
        }
        const QString digest =
            QStringLiteral("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");

        assert(cloakframe::fileMatchesDigest(path, digest));
        assert(cloakframe::fileMatchesDigest(path, digest.toUpper()));

        // Everything that is not a positive match has to be false, because the caller deletes
        // or refuses on false and would otherwise keep a package nobody vouched for.
        assert(!cloakframe::fileMatchesDigest(path, QString()));
        assert(!cloakframe::fileMatchesDigest(path, QStringLiteral("not-a-digest")));
        assert(!cloakframe::fileMatchesDigest(path,
            QStringLiteral("00000000000000000000000000000000000000000000000000000000000000"
                           "00")));
        assert(!cloakframe::fileMatchesDigest(root.filePath(QStringLiteral("absent")), digest));
        assert(!cloakframe::fileMatchesDigest(root.path(), digest));
    }

    void testATamperedPackageStopsMatching()
    {
        QTemporaryDir root;
        assert(root.isValid());
        const QString path = root.filePath(QStringLiteral("package.nupkg"));
        {
            std::ofstream out(path.toStdString(), std::ios::binary);
            out << "abc";
        }
        const QString digest =
            QStringLiteral("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert(cloakframe::fileMatchesDigest(path, digest));

        {
            std::ofstream out(path.toStdString(), std::ios::binary | std::ios::app);
            out << "!";
        }
        assert(!cloakframe::fileMatchesDigest(path, digest));
    }
}

int main()
{
    testAPrivateDirectoryIsAccepted();
#ifndef _WIN32
    testAWritableDirectoryIsRejected();
    testSomethingThatIsNotADirectoryIsRejected();
#endif
    testAMissingDirectoryIsNotADecision();
    testDigestMatchingAcceptsOnlyTheDescribedFile();
    testATamperedPackageStopsMatching();
    std::puts("update cache tests passed");
    return 0;
}

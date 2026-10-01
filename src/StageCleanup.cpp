#include "cloakframe/StageCleanup.hpp"

#include "cloakframe/PathUtil.hpp"
#include "cloakframe/UpdateCache.hpp"

#include <QDateTime>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QLockFile>
#include <QSaveFile>
#include <QStandardPaths>
#include <QTemporaryDir>
#include <QThread>

#include <spdlog/spdlog.h>

#include <algorithm>
#include <filesystem>
#include <system_error>

#ifdef Q_OS_UNIX
#include <unistd.h>
#endif

namespace cloakframe
{
    namespace
    {
        // Every name QTemporaryDir produces from the template is the prefix followed by exactly
        // six characters, so the sweep can insist on that shape rather than on a prefix alone.
        constexpr auto kStageNameGlob = ".cloakframe-stage-??????";
        constexpr auto kStageTemplate = ".cloakframe-stage-XXXXXX";
        constexpr auto kStageLockName = ".lock";
        constexpr int kMaxRememberedRoots = 32;
        constexpr qint64 kDefaultNewStageGraceMs = 60'000;
        // Publication leftovers carry no lock, so only age says the writer is gone. One
        // publication takes seconds even on a slow share.
        constexpr qint64 kDefaultPublicationGraceMs = 600'000;
        constexpr auto kPartialSuffix = ".cloakframe-partial";

        qint64 &newStageGraceMs()
        {
            static qint64 grace = kDefaultNewStageGraceMs;
            return grace;
        }

        qint64 &publicationGraceMs()
        {
            static qint64 grace = kDefaultPublicationGraceMs;
            return grace;
        }

        // What ImageIo names the directory a file is published through, in the destination's
        // own folder: ".cloakframe-<digits>-<digits>.tmp".
        bool isPublicationStageName(const QString &name)
        {
            const QString prefix = QStringLiteral(".cloakframe-");
            const QString suffix = QStringLiteral(".tmp");
            if (!name.startsWith(prefix) || !name.endsWith(suffix))
            {
                return false;
            }
            const auto parts = QStringView(name)
                                   .mid(prefix.size(), name.size() - prefix.size() - suffix.size())
                                   .split(QLatin1Char('-'));
            return parts.size() == 2
                   && std::ranges::all_of(parts,
                       [](const QStringView part)
                       {
                           return !part.isEmpty()
                                  && std::ranges::all_of(part,
                                      [](const QChar ch)
                                      {
                                          return ch >= QLatin1Char('0') && ch <= QLatin1Char('9');
                                      });
                       });
        }

        bool olderThan(const QFileInfo &info, const qint64 milliseconds)
        {
            const QDateTime modified = info.lastModified();
            return modified.isValid()
                   && modified.msecsTo(QDateTime::currentDateTime()) >= milliseconds;
        }

        bool stageIsOurs(const QString &path);

        QString &stageRootsFileOverride()
        {
            static QString override;
            return override;
        }

        QString &privateStageRootOverride()
        {
            static QString override;
            return override;
        }

        bool isAlwaysSwept(const QString &canonicalRoot)
        {
            if (canonicalRoot == QDir(QDir::tempPath()).absolutePath())
            {
                return true;
            }
            const QString privateRoot = privateStageRoot();
            return !privateRoot.isEmpty() && canonicalRoot == QDir(privateRoot).absolutePath();
        }

        QString stageRootsFile()
        {
            if (!stageRootsFileOverride().isEmpty())
            {
                return stageRootsFileOverride();
            }
            const auto dataDir =
                QStandardPaths::writableLocation(QStandardPaths::GenericDataLocation);
            if (dataDir.isEmpty())
            {
                return {};
            }
            return dataDir + QStringLiteral("/CloakFrame/stage-roots.txt");
        }

        QStringList readRememberedRoots()
        {
            const auto file = stageRootsFile();
            if (file.isEmpty())
            {
                return {};
            }
            QFile handle(file);
            if (!handle.open(QIODevice::ReadOnly | QIODevice::Text))
            {
                return {};
            }
            QStringList roots;
            while (!handle.atEnd() && roots.size() < kMaxRememberedRoots)
            {
                const auto line = QString::fromUtf8(handle.readLine()).trimmed();
                if (!line.isEmpty() && !roots.contains(line))
                {
                    roots.push_back(line);
                }
            }
            return roots;
        }

        void writeRememberedRoots(const QString &file, const QStringList &roots)
        {
            if (roots.isEmpty())
            {
                QFile::remove(file);
                return;
            }
            QSaveFile out(file);
            if (!out.open(QIODevice::WriteOnly | QIODevice::Text))
            {
                return;
            }
            for (const auto &entry : roots)
            {
                out.write(entry.toUtf8());
                out.write("\n");
            }
            out.commit();
        }

        void rememberRoot(const QString &root)
        {
            if (root.isEmpty())
            {
                return;
            }
            // These roots are swept unconditionally, and this runs once per image in a batch,
            // so leave before touching the filesystem.
            const QString canonical = QDir(root).absolutePath();
            if (isAlwaysSwept(canonical))
            {
                return;
            }
            const auto file = stageRootsFile();
            if (file.isEmpty())
            {
                return;
            }
            if (!QDir().mkpath(QFileInfo(file).absolutePath()))
            {
                return;
            }

            // Two instances can be starting a run at the same moment. Losing the race only costs
            // one remembered root, so the wait is short and failure is silent.
            QLockFile guard(file + QStringLiteral(".lock"));
            if (!guard.tryLock(500))
            {
                return;
            }

            auto roots = readRememberedRoots();
            if (!roots.isEmpty() && roots.front() == canonical)
            {
                return;
            }
            roots.removeAll(canonical);
            roots.push_front(canonical);
            while (roots.size() > kMaxRememberedRoots)
            {
                roots.pop_back();
            }
            writeRememberedRoots(file, roots);
        }

        void forgetRoot(const QString &root)
        {
            const auto file = stageRootsFile();
            if (file.isEmpty() || !QFileInfo::exists(file))
            {
                return;
            }
            QLockFile guard(file + QStringLiteral(".lock"));
            if (!guard.tryLock(500))
            {
                return;
            }
            auto roots = readRememberedRoots();
            if (roots.removeAll(QDir(root).absolutePath()) > 0)
            {
                writeRememberedRoots(file, roots);
            }
        }

        bool hasStages(const QString &root)
        {
            return !QDir(root)
                        .entryList({QString::fromLatin1(kStageNameGlob)},
                            QDir::Dirs | QDir::Hidden | QDir::NoDotAndDotDot)
                        .isEmpty();
        }

        bool fileIsOurs(const QFileInfo &info)
        {
#ifdef Q_OS_UNIX
            return info.ownerId() == ::getuid();
#else
            (void)info;
            return true;
#endif
        }

        // Publication leftovers sit next to the file being published, anywhere under the output
        // root. Links and junctions are not followed: what they point to is not the run's.
        int sweepPublicationLeftovers(const QString &root)
        {
            int removed = 0;
            QStringList pending{root};
            while (!pending.isEmpty())
            {
                const QDir directory(pending.takeLast());
                const auto entries = directory.entryInfoList(
                    QDir::AllEntries | QDir::Hidden | QDir::System | QDir::NoDotAndDotDot);
                for (const auto &entry : entries)
                {
                    if (entry.isSymLink() || entry.isJunction())
                    {
                        continue;
                    }
                    const QString name = entry.fileName();
                    if (entry.isDir())
                    {
                        if (!isPublicationStageName(name))
                        {
                            if (!name.startsWith(QString::fromLatin1(kStagePrefix)))
                            {
                                pending.push_back(entry.absoluteFilePath());
                            }
                            continue;
                        }
                        if (!stageIsOurs(entry.absoluteFilePath())
                            || !olderThan(entry, publicationGraceMs()))
                        {
                            continue;
                        }
                        std::error_code error;
                        std::filesystem::remove_all(
                            pathFromQString(entry.absoluteFilePath()), error);
                        if (!error)
                        {
                            ++removed;
                        }
                    }
                    else if (entry.isFile() && name.endsWith(QString::fromLatin1(kPartialSuffix))
                             && fileIsOurs(entry) && olderThan(entry, publicationGraceMs())
                             && QFile::remove(entry.absoluteFilePath()))
                    {
                        ++removed;
                    }
                }
            }
            return removed;
        }

        bool stageIsOurs(const QString &path)
        {
            switch (inspectCacheDirectory(path))
            {
            case CacheDirectoryTrust::Private:
                return true;
            case CacheDirectoryTrust::Shared:
            case CacheDirectoryTrust::Absent:
                return false;
            case CacheDirectoryTrust::Unsupported:
                break;
            }
            // Windows, where ownership is an ACL question rather than a uid one. The name schema
            // and the lock are what is left; both temporary and output roots are normally
            // per-user there.
            const QFileInfo info(path);
            return info.isDir() && !info.isSymLink();
        }

        int sweepRoot(const QString &root)
        {
            QDir directory(root);
            if (!directory.exists())
            {
                return 0;
            }
            const auto entries = directory.entryList({QString::fromLatin1(kStageNameGlob)},
                QDir::Dirs | QDir::Hidden | QDir::NoDotAndDotDot);

            int removed = 0;
            for (const auto &entry : entries)
            {
                const QString path = directory.absoluteFilePath(entry);
                if (!stageIsOurs(path))
                {
                    continue;
                }

                const QString lockPath = QDir(path).filePath(QString::fromLatin1(kStageLockName));
                if (!QFileInfo::exists(lockPath))
                {
                    // Either a directory another instance created a moment ago and has not
                    // locked yet, or a run that died inside that same gap. Age is the only thing
                    // that separates the two, and it is used nowhere else: for every directory
                    // that has a lock, whether its owner is gone is a better question than how
                    // old it is.
                    const QDateTime modified = QFileInfo(path).lastModified();
                    if (!modified.isValid()
                        || modified.msecsTo(QDateTime::currentDateTime()) < newStageGraceMs())
                    {
                        continue;
                    }
                }

                QLockFile lock(lockPath);
                lock.setStaleLockTime(0);
                if (!lock.tryLock(0))
                {
                    continue;
                }
                lock.unlock();

                // remove_all does not follow symlinks, and the check above established that no
                // other account can write inside this directory, so nothing can be swapped
                // underneath the walk.
                std::error_code error;
                std::filesystem::remove_all(pathFromQString(path), error);
                if (error)
                {
                    spdlog::warn(
                        "Could not remove stale stage {}: {}", path.toStdString(), error.message());
                    continue;
                }
                ++removed;
            }
            return removed;
        }
    }

    QString privateStageRoot()
    {
        if (!privateStageRootOverride().isEmpty())
        {
            return privateStageRootOverride();
        }
        const QString cache = QStandardPaths::writableLocation(QStandardPaths::CacheLocation);
        if (cache.isEmpty())
        {
            return {};
        }
        return cache + QStringLiteral("/staging");
    }

    StageDirectory::StageDirectory(const QString &root)
    {
        // QTemporaryDir would resolve a template under an empty root against the working
        // directory.
        if (root.isEmpty())
        {
            return;
        }
        dir_ = std::make_unique<QTemporaryDir>(
            QDir(root).filePath(QString::fromLatin1(kStageTemplate)));
        if (!dir_->isValid())
        {
            return;
        }
        lock_ = std::make_unique<QLockFile>(dir_->filePath(QString::fromLatin1(kStageLockName)));
        lock_->setStaleLockTime(0);
        if (!lock_->tryLock(0))
        {
            // Only reachable if something else already holds a lock inside a directory this
            // process just created. Nothing is unsafe about continuing; the sweep will simply
            // leave this directory alone if the run dies.
            lock_.reset();
        }
        rememberRoot(root);
    }

    StageDirectory::~StageDirectory()
    {
        // The lock has to go first: Windows will not remove a file another handle holds open.
        lock_.reset();
        if (!dir_)
        {
            return;
        }
        for (int attempt = 0; dir_->isValid() && !dir_->remove() && attempt < 20; ++attempt)
        {
            QThread::msleep(100);
        }
    }

    bool StageDirectory::isValid() const
    {
        return dir_ && dir_->isValid();
    }

    QString StageDirectory::path() const
    {
        return dir_ ? dir_->path() : QString();
    }

    QString StageDirectory::filePath(const QString &name) const
    {
        return dir_ ? dir_->filePath(name) : QString();
    }

    void setStageRootsFileForTesting(const QString &path)
    {
        stageRootsFileOverride() = path;
    }

    void setPrivateStageRootForTesting(const QString &path)
    {
        privateStageRootOverride() = path;
    }

    void setNewStageGraceForTesting(const qint64 milliseconds)
    {
        newStageGraceMs() = milliseconds < 0 ? kDefaultNewStageGraceMs : milliseconds;
    }

    int removeStaleStagesIn(const QStringList &roots)
    {
        int removed = 0;
        QStringList seen;
        for (const auto &root : roots)
        {
            const QString canonical = QDir(root).absolutePath();
            if (canonical.isEmpty() || seen.contains(canonical))
            {
                continue;
            }
            seen.push_back(canonical);
            removed += sweepRoot(canonical);
        }
        return removed;
    }

    int removeStaleStages()
    {
        QStringList roots{QDir::tempPath()};
        if (const QString privateRoot = privateStageRoot(); !privateRoot.isEmpty())
        {
            roots.push_back(privateRoot);
        }
        int removed = removeStaleStagesIn(roots);
        // A remembered output root is one a run was writing into when it stopped without
        // finishing. Once nothing of that run is left there, its path is not kept any longer.
        for (const auto &root : readRememberedRoots())
        {
            removed += sweepRoot(root);
            if (QDir(root).exists())
            {
                removed += sweepPublicationLeftovers(root);
            }
            if (!hasStages(root))
            {
                forgetRoot(root);
            }
        }
        if (removed > 0)
        {
            spdlog::info("Removed {} temporary items left by an earlier run", removed);
        }
        return removed;
    }

    bool clearRememberedStageRoots()
    {
        const auto file = stageRootsFile();
        return file.isEmpty() || !QFileInfo::exists(file) || QFile::remove(file);
    }

    OutputRootGuard::OutputRootGuard(const QString &root)
        : root_(root)
        , marker_(std::make_unique<StageDirectory>(root))
    {
    }

    OutputRootGuard::~OutputRootGuard()
    {
        marker_.reset();
        // Another instance writing into the same folder keeps its own marker there, and with
        // it the folder's place on the list.
        if (!root_.isEmpty() && !hasStages(root_))
        {
            forgetRoot(root_);
        }
    }

    void setPublicationGraceForTesting(const qint64 milliseconds)
    {
        publicationGraceMs() = milliseconds < 0 ? kDefaultPublicationGraceMs : milliseconds;
    }
}

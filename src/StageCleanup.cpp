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

#include <filesystem>
#include <system_error>

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

        qint64 &newStageGraceMs()
        {
            static qint64 grace = kDefaultNewStageGraceMs;
            return grace;
        }

        QString &stageRootsFileOverride()
        {
            static QString override;
            return override;
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

        void rememberRoot(const QString &root)
        {
            if (root.isEmpty())
            {
                return;
            }
            // The system temporary directory is swept unconditionally, and this runs once per
            // image in a batch, so leave before touching the filesystem.
            const QString canonical = QDir(root).absolutePath();
            if (canonical == QDir(QDir::tempPath()).absolutePath())
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

    StageDirectory::StageDirectory(const QString &root)
        : dir_(std::make_unique<QTemporaryDir>(
              QDir(root).filePath(QString::fromLatin1(kStageTemplate))))
    {
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
        roots += readRememberedRoots();
        const int removed = removeStaleStagesIn(roots);
        if (removed > 0)
        {
            spdlog::info("Removed {} stage directories left by an earlier run", removed);
        }
        return removed;
    }
}

#include "cloakframe/PathUtil.hpp"
#include "cloakframe/StageCleanup.hpp"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QString>
#include <QStringList>
#include <QTemporaryDir>

#include <cassert>
#include <chrono>
#include <cstdio>
#include <filesystem>
#include <fstream>

#ifndef _WIN32
#include <sys/stat.h>
#endif

namespace
{
    QString stagePath(const QTemporaryDir &root, const QString &suffix)
    {
        return root.filePath(QString::fromLatin1(cloakframe::kStagePrefix) + suffix);
    }

    QString makeAbandonedStage(const QTemporaryDir &root, const QString &suffix)
    {
        const QString path = stagePath(root, suffix);
        assert(QDir().mkpath(path));
        std::ofstream out(QDir(path).filePath(QStringLiteral("source.jpg")).toStdString());
        out << "the whole original";
        out.close();
        return path;
    }

    void testAStageLeftByACrashedRunIsRemoved()
    {
        QTemporaryDir root;
        assert(root.isValid());
        const QString stage = makeAbandonedStage(root, QStringLiteral("aB3xY9"));

        assert(cloakframe::removeStaleStagesIn({root.path()}) == 1);
        assert(!QDir(stage).exists());
    }

    void testAStageAnotherInstanceHoldsIsLeftAlone()
    {
        QTemporaryDir root;
        assert(root.isValid());

        cloakframe::StageDirectory live(root.path());
        assert(live.isValid());
        const QString path = live.path();
        {
            std::ofstream out(QDir(path).filePath(QStringLiteral("source.mp4")).toStdString());
            out << "still being written";
        }

        assert(cloakframe::removeStaleStagesIn({root.path()}) == 0);
        assert(QDir(path).exists());
    }

    void testOnlyTheExactNameShapeIsSwept()
    {
        QTemporaryDir root;
        assert(root.isValid());

        const QStringList survivors = {
            root.filePath(QStringLiteral("holiday-photos")),
            root.filePath(QStringLiteral(".cloakframe-stage-")),
            root.filePath(QStringLiteral(".cloakframe-stage-short")),
            root.filePath(QStringLiteral(".cloakframe-stage-toolong")),
            root.filePath(QStringLiteral(".cloakframe-snapshot-aB3xY9")),
        };
        for (const auto &path : survivors)
        {
            assert(QDir().mkpath(path));
        }

        assert(cloakframe::removeStaleStagesIn({root.path()}) == 0);
        for (const auto &path : survivors)
        {
            assert(QDir(path).exists());
        }
    }

    void testAnAbsentRootIsNotAnError()
    {
        QTemporaryDir root;
        assert(root.isValid());
        assert(cloakframe::removeStaleStagesIn({root.filePath(QStringLiteral("gone"))}) == 0);
        assert(cloakframe::removeStaleStagesIn({QString()}) == 0);
    }

    void testEachRootIsSweptOnce()
    {
        QTemporaryDir root;
        assert(root.isValid());
        makeAbandonedStage(root, QStringLiteral("aB3xY9"));
        makeAbandonedStage(root, QStringLiteral("cD4wZ8"));

        const QStringList repeated = {root.path(), root.path(), root.path() + QStringLiteral("/.")};
        assert(cloakframe::removeStaleStagesIn(repeated) == 2);
    }

    void testAStageDirectoryTakesItsContentsWithIt()
    {
        QTemporaryDir root;
        assert(root.isValid());

        QString path;
        {
            cloakframe::StageDirectory stage(root.path());
            assert(stage.isValid());
            path = stage.path();
            std::ofstream out(stage.filePath(QStringLiteral("source.jpg")).toStdString());
            out << "the whole original";
        }
        assert(!QDir(path).exists());
    }

    void testAnEmptyRootGivesNoStage()
    {
        const cloakframe::StageDirectory stage{QString()};
        assert(!stage.isValid());
        assert(stage.path().isEmpty());
    }

    void testThePrivateRootIsSweptWithoutBeingRemembered(const QString &rootsFile)
    {
        QTemporaryDir root;
        assert(root.isValid());
        cloakframe::setPrivateStageRootForTesting(root.path());
        {
            const cloakframe::StageDirectory live(cloakframe::privateStageRoot());
            assert(live.isValid());
        }
        QFile remembered(rootsFile);
        const QString roots =
            remembered.open(QIODevice::ReadOnly) ? QString::fromUtf8(remembered.readAll()) : "";
        assert(!roots.contains(QDir(root.path()).absolutePath()));

        // The full sweep also visits the real temporary directory, where other tests may be
        // creating stages, so it runs with the normal grace period and an aged fixture.
        cloakframe::setNewStageGraceForTesting(-1);
        const QString stage = makeAbandonedStage(root, QStringLiteral("aB3xY9"));
        std::filesystem::last_write_time(cloakframe::pathFromQString(stage),
            std::filesystem::file_time_type::clock::now() - std::chrono::hours(2));
        cloakframe::removeStaleStages();
        assert(!QDir(stage).exists());
        cloakframe::setNewStageGraceForTesting(0);
        cloakframe::setPrivateStageRootForTesting({});
    }

    QString readRoots(const QString &rootsFile)
    {
        QFile file(rootsFile);
        return file.open(QIODevice::ReadOnly) ? QString::fromUtf8(file.readAll()) : QString();
    }

    void age(const QString &path)
    {
        std::filesystem::last_write_time(cloakframe::pathFromQString(path),
            std::filesystem::file_time_type::clock::now() - std::chrono::hours(2));
    }

    void testAFinishedRunForgetsItsOutputRoot(const QString &rootsFile)
    {
        QTemporaryDir root;
        assert(root.isValid());
        const QString canonical = QDir(root.path()).absolutePath();
        {
            const cloakframe::OutputRootGuard guard(root.path());
            assert(readRoots(rootsFile).contains(canonical));
            assert(!QDir(root.path())
                    .entryList({QString::fromLatin1(cloakframe::kStagePrefix) + "*"},
                        QDir::Dirs | QDir::Hidden)
                    .isEmpty());
        }
        assert(!readRoots(rootsFile).contains(canonical));
        assert(QDir(root.path())
                .entryList(QDir::AllEntries | QDir::Hidden | QDir::NoDotAndDotDot)
                .isEmpty());
    }

    void testACrashedRunsOutputRootIsSweptAndForgotten(const QString &rootsFile)
    {
        QTemporaryDir root;
        assert(root.isValid());
        const QString canonical = QDir(root.path()).absolutePath();
        {
            QFile file(rootsFile);
            assert(file.open(QIODevice::WriteOnly | QIODevice::Truncate));
            file.write(canonical.toUtf8() + "\n");
        }
        const QString stage = makeAbandonedStage(root, QStringLiteral("aB3xY9"));
        age(stage);

        // An interrupted image publication: its directory next to the destination, and the
        // partial file of the copy fallback.
        QDir nested(root.filePath(QStringLiteral("album/2026")));
        assert(nested.mkpath(QStringLiteral(".")));
        const QString published = nested.filePath(QStringLiteral(".cloakframe-123-4.tmp"));
        assert(QDir().mkpath(published));
        {
            std::ofstream out(QDir(published).filePath(QStringLiteral("payload")).toStdString());
            out << "masked result";
        }
        age(published);
        const QString partial = nested.filePath(QStringLiteral("photo.jpg.cloakframe-partial"));
        {
            std::ofstream out(partial.toStdString());
            out << "half a result";
        }
        age(partial);

        // Recent, so possibly another instance's, and a name that only looks similar.
        const QString recent = nested.filePath(QStringLiteral(".cloakframe-5-6.tmp"));
        assert(QDir().mkpath(recent));
        const QString lookalike = nested.filePath(QStringLiteral(".cloakframe-12a-3.tmp"));
        assert(QDir().mkpath(lookalike));
        age(lookalike);
        const QString kept = nested.filePath(QStringLiteral("photo.jpg"));
        {
            std::ofstream out(kept.toStdString());
            out << "a finished result";
        }

        // The full sweep also visits the real temporary directory, where other tests may be
        // creating stages, so it runs with the normal grace period.
        cloakframe::setNewStageGraceForTesting(-1);
        cloakframe::removeStaleStages();
        cloakframe::setNewStageGraceForTesting(0);

        assert(!QDir(stage).exists());
        assert(!QDir(published).exists());
        assert(!QFileInfo::exists(partial));
        assert(QDir(recent).exists());
        assert(QDir(lookalike).exists());
        assert(QFileInfo::exists(kept));
        assert(!readRoots(rootsFile).contains(canonical));
    }

    void testDeletingLogsForgetsTheOutputRoots(const QString &rootsFile)
    {
        QTemporaryDir root;
        assert(root.isValid());
        {
            QFile file(rootsFile);
            assert(file.open(QIODevice::WriteOnly | QIODevice::Truncate));
            file.write(QDir(root.path()).absolutePath().toUtf8() + "\n");
        }
        assert(cloakframe::clearRememberedStageRoots());
        assert(!QFileInfo::exists(rootsFile));
        assert(cloakframe::clearRememberedStageRoots());
    }

#ifndef _WIN32
    void testAStageAnotherAccountCouldWriteIsLeftAlone()
    {
        QTemporaryDir root;
        assert(root.isValid());
        const QString stage = makeAbandonedStage(root, QStringLiteral("aB3xY9"));

        // Not ours to delete, whoever owns it: anyone who can write the directory could have
        // put something there for this sweep to walk into.
        assert(::chmod(QFile::encodeName(stage).constData(), 0777) == 0);
        assert(cloakframe::removeStaleStagesIn({root.path()}) == 0);
        assert(QDir(stage).exists());

        assert(::chmod(QFile::encodeName(stage).constData(), 0700) == 0);
        assert(cloakframe::removeStaleStagesIn({root.path()}) == 1);
        assert(!QDir(stage).exists());
    }

    void testASymlinkWearingAStageNameIsLeftAlone()
    {
        QTemporaryDir root;
        assert(root.isValid());

        const QString target = root.filePath(QStringLiteral("pictures"));
        assert(QDir().mkpath(target));
        const QString kept = QDir(target).filePath(QStringLiteral("keep.jpg"));
        {
            std::ofstream out(kept.toStdString());
            out << "not the application's to delete";
        }

        const QString link = stagePath(root, QStringLiteral("aB3xY9"));
        assert(QFile::link(target, link));

        assert(cloakframe::removeStaleStagesIn({root.path()}) == 0);
        assert(QFileInfo(link).isSymLink());
        assert(QFile::exists(kept));
    }
#endif
}

int main()
{
    QTemporaryDir dataDir;
    assert(dataDir.isValid());
    const QString rootsFile = dataDir.filePath(QStringLiteral("stage-roots.txt"));
    cloakframe::setStageRootsFileForTesting(rootsFile);

    // A directory with no lock file yet is normally spared in case another instance is still
    // setting it up. These fixtures are all older than that in spirit and none of them is
    // being set up, so the grace period is what would make the test sleep for a minute.
    cloakframe::setNewStageGraceForTesting(0);

    testAStageLeftByACrashedRunIsRemoved();
    testAStageAnotherInstanceHoldsIsLeftAlone();
    testOnlyTheExactNameShapeIsSwept();
    testAnAbsentRootIsNotAnError();
    testEachRootIsSweptOnce();
    testAStageDirectoryTakesItsContentsWithIt();
    testAnEmptyRootGivesNoStage();
    testThePrivateRootIsSweptWithoutBeingRemembered(rootsFile);
    testAFinishedRunForgetsItsOutputRoot(rootsFile);
    testACrashedRunsOutputRootIsSweptAndForgotten(rootsFile);
    testDeletingLogsForgetsTheOutputRoots(rootsFile);
#ifndef _WIN32
    testAStageAnotherAccountCouldWriteIsLeftAlone();
    testASymlinkWearingAStageNameIsLeftAlone();
#endif
    std::puts("stage cleanup tests passed");
    return 0;
}

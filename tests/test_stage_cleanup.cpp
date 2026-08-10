#include "cloakframe/StageCleanup.hpp"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QString>
#include <QStringList>
#include <QTemporaryDir>

#include <cassert>
#include <cstdio>
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
    cloakframe::setStageRootsFileForTesting(dataDir.filePath(QStringLiteral("stage-roots.txt")));

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
#ifndef _WIN32
    testAStageAnotherAccountCouldWriteIsLeftAlone();
    testASymlinkWearingAStageNameIsLeftAlone();
#endif
    std::puts("stage cleanup tests passed");
    return 0;
}

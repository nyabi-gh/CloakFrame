#pragma once

#include <QString>
#include <QStringList>

#include <memory>

class QLockFile;
class QTemporaryDir;

namespace cloakframe
{
    // A run copies the whole original into a temporary directory before it touches it, and
    // stages the encoded result in another one. Both are removed when the owning object goes
    // away, which does not happen on SIGKILL, on power loss, or on a crash. What is left behind
    // can be a full copy of someone's photo or video.
    //
    // Every such directory is therefore named with one prefix and carries a lock file, so a
    // later start can recognize its own leftovers and tell a crashed run's directory from one
    // a second running instance is still using.

    inline constexpr auto kStagePrefix = ".cloakframe-stage-";

    // Where copies of originals are staged: a per-user directory on disk. Not the output root,
    // which the user chose because they mean to share or sync it, and not the system temporary
    // directory, which is RAM-backed on many Linux systems and too small for a video. The
    // directory may not exist yet. Empty if the platform reports no cache location.
    [[nodiscard]] QString privateStageRoot();

    // A private directory for one run's temporary files, removed on destruction. `root` is the
    // directory to create it in: the output root for an anonymized result that has to be renamed
    // onto the same filesystem, `privateStageRoot()` or `QDir::tempPath()` for anything else. An
    // empty root gives an invalid directory. Check `isValid()` before using it.
    class StageDirectory
    {
    public:
        explicit StageDirectory(const QString &root);
        ~StageDirectory();

        StageDirectory(const StageDirectory &) = delete;
        StageDirectory &operator=(const StageDirectory &) = delete;

        [[nodiscard]] bool isValid() const;
        [[nodiscard]] QString path() const;
        [[nodiscard]] QString filePath(const QString &name) const;

    private:
        std::unique_ptr<QTemporaryDir> dir_;
        std::unique_ptr<QLockFile> lock_;
    };

    // Keeps `root` on the list of output roots swept at start while a run writes into it.
    class OutputRootGuard
    {
    public:
        explicit OutputRootGuard(const QString &root);
        ~OutputRootGuard();

        OutputRootGuard(const OutputRootGuard &) = delete;
        OutputRootGuard &operator=(const OutputRootGuard &) = delete;

    private:
        QString root_;
        std::unique_ptr<StageDirectory> marker_;
    };

    // Remove stage directories left by earlier runs, in the system temporary directory, in
    // `privateStageRoot()`, and in every remembered output root, where interrupted publications
    // are removed too. A remembered root with nothing left in it is forgotten. Returns how many
    // items were removed. Safe to call while other instances are running: a directory whose
    // lock is held is left alone.
    int removeStaleStages();

    // The same stage sweep over an explicit list of roots, without consulting or updating the
    // remembered ones. Neither the system temporary directory nor the private root is added.
    int removeStaleStagesIn(const QStringList &roots);

    // Deletes the list of remembered output roots. False if it could not be deleted.
    [[nodiscard]] bool clearRememberedStageRoots();

    // Test seam: redirect the file that remembers output roots, so a test does not write into
    // the real data directory. An empty path restores the default.
    void setStageRootsFileForTesting(const QString &path);

    // Test seam: redirect `privateStageRoot()`. An empty path restores the default.
    void setPrivateStageRootForTesting(const QString &path);

    // Test seam: how recently a directory with no lock file yet must have been touched to be
    // treated as one another instance is still setting up. A negative value restores the
    // default; zero makes a lockless directory eligible immediately.
    void setNewStageGraceForTesting(qint64 milliseconds);

    // Test seam: the minimum age of a publication leftover the sweep removes. Negative restores
    // the default.
    void setPublicationGraceForTesting(qint64 milliseconds);
}

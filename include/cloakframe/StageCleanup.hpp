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
    // is a full copy of someone's photo or video, in the system temporary directory or next to
    // their output.
    //
    // Every such directory is therefore named with one prefix and carries a lock file, so a
    // later start can recognize its own leftovers and tell a crashed run's directory from one
    // a second running instance is still using.

    inline constexpr auto kStagePrefix = ".cloakframe-stage-";

    // A private directory for one run's temporary files, removed on destruction. `root` is the
    // directory to create it in - the output root when the staged file has to be renamed onto
    // the same filesystem, `QDir::tempPath()` otherwise. Check `isValid()` before using it.
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

    // Remove stage directories left by earlier runs, in the system temporary directory and in
    // every output root a `StageDirectory` has been created in before. Returns how many were
    // removed. Safe to call while other instances are running: a directory whose lock is held
    // is left alone.
    int removeStaleStages();

    // The same sweep over an explicit list of roots, without consulting or updating the
    // remembered ones. The system temporary directory is not added.
    int removeStaleStagesIn(const QStringList &roots);

    // Test seam: redirect the file that remembers output roots, so a test does not write into
    // the real data directory. An empty path restores the default.
    void setStageRootsFileForTesting(const QString &path);

    // Test seam: how recently a directory with no lock file yet must have been touched to be
    // treated as one another instance is still setting up. A negative value restores the
    // default; zero makes a lockless directory eligible immediately.
    void setNewStageGraceForTesting(qint64 milliseconds);
}

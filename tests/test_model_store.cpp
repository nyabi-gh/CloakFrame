#include "cloakframe/ModelStore.hpp"

#include <QByteArray>
#include <QCryptographicHash>
#include <QDir>
#include <QFile>
#include <QTemporaryDir>
#include <QThread>

#include <array>
#include <cassert>
#include <cstdio>
#include <memory>

#ifdef _WIN32
#include <windows.h>
#else
#include <sys/stat.h>
#endif

namespace
{
    constexpr auto kModelBytes = "a model, near enough for a filesystem test";

    QString digestOf(const QByteArray &bytes)
    {
        return QString::fromLatin1(
            QCryptographicHash::hash(bytes, QCryptographicHash::Sha256).toHex());
    }

    QByteArray readAll(const QString &path)
    {
        QFile file(path);
        return file.open(QIODevice::ReadOnly) ? file.readAll() : QByteArray();
    }

    void writeFile(const QString &path, const QByteArray &bytes)
    {
        QFile file(path);
        assert(file.open(QIODevice::WriteOnly));
        assert(file.write(bytes) == bytes.size());
    }

    // Nothing may be left behind under a name that looks like a half-finished download.
    bool noResidue(const QString &folder)
    {
        const auto leftovers =
            QDir(folder).entryList({QStringLiteral("*.part")}, QDir::Files | QDir::Hidden);
        return leftovers.isEmpty();
    }

    class Folder
    {
    public:
        Folder()
        {
            assert(root_.isValid());
            path_ = root_.filePath(QStringLiteral("models"));
            assert(QDir().mkpath(path_));
        }

        [[nodiscard]] QString path() const
        {
            return path_;
        }

        [[nodiscard]] QString filePath(const QString &name) const
        {
            return path_ + "/" + name;
        }

        [[nodiscard]] QString modelPath() const
        {
            return filePath(QStringLiteral("model.onnx"));
        }

    private:
        QTemporaryDir root_;
        QString path_;
    };

    void testAVerifiedModelIsPublishedAndLeavesNothingBehind()
    {
        const Folder folder;
        assert(cloakframe::saveModelFile(folder.modelPath(), kModelBytes, digestOf(kModelBytes))
               == cloakframe::ModelSaveResult::Saved);
        assert(readAll(folder.modelPath()) == kModelBytes);
        assert(noResidue(folder.path()));
    }

    void testTheTemporaryNameIsNotTheOneAnAttackerWouldPrepare()
    {
        const Folder folder;
        // The name the old code used, and the only one anyone could have guessed.
        const QString predictable = folder.modelPath() + ".part";
        writeFile(predictable, QByteArrayLiteral("squatted"));

        assert(cloakframe::saveModelFile(folder.modelPath(), kModelBytes, digestOf(kModelBytes))
               == cloakframe::ModelSaveResult::Saved);
        assert(readAll(folder.modelPath()) == kModelBytes);
        // Untouched: the download never went near it.
        assert(readAll(predictable) == QByteArrayLiteral("squatted"));
    }

    void testAnExistingModelSurvivesAFailedPublication()
    {
        const Folder folder;
        // A directory cannot be renamed over, so publication fails after a good write.
        const QString destination = folder.filePath(QStringLiteral("occupied"));
        assert(QDir().mkpath(destination));
        writeFile(destination + "/keep", QByteArrayLiteral("still here"));

        assert(cloakframe::saveModelFile(destination, kModelBytes, digestOf(kModelBytes))
               == cloakframe::ModelSaveResult::PublishFailed);
        assert(readAll(destination + "/keep") == QByteArrayLiteral("still here"));
        assert(noResidue(folder.path()));
    }

    void testBytesThatDoNotMatchTheDigestNeverTakeTheModelName()
    {
        const Folder folder;
        const QByteArray previous = QByteArrayLiteral("the model that was already there");
        writeFile(folder.modelPath(), previous);

        assert(
            cloakframe::saveModelFile(
                folder.modelPath(), kModelBytes, digestOf(QByteArrayLiteral("a different model")))
            == cloakframe::ModelSaveResult::ContentMismatch);
        assert(readAll(folder.modelPath()) == previous);
        assert(noResidue(folder.path()));
    }

    void testAReplacementTakesOverFromTheModelAlreadyThere()
    {
        const Folder folder;
        writeFile(folder.modelPath(), QByteArrayLiteral("an older model"));

        assert(cloakframe::saveModelFile(folder.modelPath(), kModelBytes, digestOf(kModelBytes))
               == cloakframe::ModelSaveResult::Saved);
        assert(readAll(folder.modelPath()) == kModelBytes);
        assert(noResidue(folder.path()));
    }

#ifdef _WIN32
    void testAContendedPublishIsSuccessWhenTheModelAlreadyMatches()
    {
        const Folder folder;
        writeFile(folder.modelPath(), kModelBytes);

        const auto native = QDir::toNativeSeparators(folder.modelPath()).toStdWString();
        const HANDLE reader = ::CreateFileW(native.c_str(),
            GENERIC_READ,
            FILE_SHARE_READ,
            nullptr,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            nullptr);
        assert(reader != INVALID_HANDLE_VALUE);

        assert(cloakframe::saveModelFile(folder.modelPath(), kModelBytes, digestOf(kModelBytes))
               == cloakframe::ModelSaveResult::Saved);
        assert(readAll(folder.modelPath()) == kModelBytes);
        assert(noResidue(folder.path()));
        assert(::CloseHandle(reader) != 0);
    }
#endif

    // Two downloads of the same model at once must both finish, and neither may delete or
    // publish the other's temporary file.
    void testConcurrentDownloadsBothFinish()
    {
        const Folder folder;
        const QString destination = folder.modelPath();
        const QString digest = digestOf(kModelBytes);

        std::array<cloakframe::ModelSaveResult, 4> results;
        results.fill(cloakframe::ModelSaveResult::WriteFailed);
        std::array<std::unique_ptr<QThread>, 4> threads{};
        for (std::size_t i = 0; i < threads.size(); ++i)
        {
            threads[i].reset(QThread::create(
                [&results, i, destination, digest]
                {
                    results[i] = cloakframe::saveModelFile(destination, kModelBytes, digest);
                }));
            threads[i]->start();
        }
        for (auto &thread : threads)
        {
            assert(thread->wait(30000));
        }

        for (const auto result : results)
        {
            assert(result == cloakframe::ModelSaveResult::Saved);
        }
        assert(readAll(destination) == kModelBytes);
        assert(noResidue(folder.path()));
    }

#ifndef _WIN32
    void testAFolderOthersCanWriteIsRefused()
    {
        const Folder folder;
        assert(::chmod(QFile::encodeName(folder.path()).constData(), 0777) == 0);

        assert(cloakframe::saveModelFile(folder.modelPath(), kModelBytes, digestOf(kModelBytes))
               == cloakframe::ModelSaveResult::FolderNotPrivate);
        assert(!QFile::exists(folder.modelPath()));
        assert(noResidue(folder.path()));

        assert(::chmod(QFile::encodeName(folder.path()).constData(), 0700) == 0);
    }

    void testTheWriteDoesNotFollowALinkLeftInItsPlace()
    {
        const Folder folder;
        QTemporaryDir elsewhere;
        assert(elsewhere.isValid());
        const QString victim = elsewhere.filePath(QStringLiteral("victim"));
        writeFile(victim, QByteArrayLiteral("someone else's file"));

        // The temporary name is unpredictable, so the closest an attacker gets is claiming
        // every name the folder already holds. Exclusive creation is what refuses those.
        for (const auto &name : {QStringLiteral("model.onnx.part"), QStringLiteral("model.onnx")})
        {
            assert(QFile::link(victim, folder.filePath(name)));
        }

        const auto result =
            cloakframe::saveModelFile(folder.modelPath(), kModelBytes, digestOf(kModelBytes));
        assert(result == cloakframe::ModelSaveResult::Saved);
        assert(readAll(victim) == QByteArrayLiteral("someone else's file"));
        assert(readAll(folder.modelPath()) == kModelBytes);
    }
#endif
}

int main()
{
    testAVerifiedModelIsPublishedAndLeavesNothingBehind();
    testTheTemporaryNameIsNotTheOneAnAttackerWouldPrepare();
    testAnExistingModelSurvivesAFailedPublication();
    testBytesThatDoNotMatchTheDigestNeverTakeTheModelName();
    testAReplacementTakesOverFromTheModelAlreadyThere();
#ifdef _WIN32
    testAContendedPublishIsSuccessWhenTheModelAlreadyMatches();
#endif
    testConcurrentDownloadsBothFinish();
#ifndef _WIN32
    testAFolderOthersCanWriteIsRefused();
    testTheWriteDoesNotFollowALinkLeftInItsPlace();
#endif
    std::puts("model store tests passed");
    return 0;
}

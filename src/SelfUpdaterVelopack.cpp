#include "cloakframe/SelfUpdater.hpp"
#include "cloakframe/UpdateCache.hpp"
#include "cloakframe/UpdateSignature.hpp"

#include <QDir>
#include <QEventLoop>
#include <QFile>
#include <QFileInfo>
#include <QMetaObject>
#include <QNetworkAccessManager>
#include <QNetworkReply>
#include <QNetworkRequest>
#include <QString>
#include <QThread>
#include <QUrl>

#include <Velopack.hpp>
#include <exception>
#include <memory>
#include <optional>
#include <vector>

#ifndef CLOAKFRAME_UPDATE_PUBLIC_KEY
#define CLOAKFRAME_UPDATE_PUBLIC_KEY ""
#endif

namespace cloakframe
{
    namespace
    {
        constexpr auto kRepoUrl = "https://github.com/nyattic/CloakFrame";
        constexpr auto kReleaseDownloadPrefix =
            "https://github.com/nyattic/CloakFrame/releases/download/v";

#ifdef __linux__
        // Velopack 1.2.0 caches Linux packages here (locator.rs). It offers no way to ask for
        // the path, and supplying a locator replaces its whole path discovery rather than this
        // one field, so the path is derived. A derivation that is ever wrong costs nothing: the
        // checks that use it find no file and leave the behaviour exactly as it was.
        constexpr auto kVelopackCacheRoot = "/var/tmp/velopack";

        QString packagesDirectory(const std::string &appId)
        {
            return QStringLiteral("%1/%2/packages")
                .arg(QString::fromLatin1(kVelopackCacheRoot), QString::fromStdString(appId));
        }
#endif

        class VelopackWorker final : public QObject
        {
            Q_OBJECT

        public slots:
            void check()
            {
                try
                {
                    if (!manager_)
                    {
                        manager_ = std::make_unique<Velopack::UpdateManager>(
                            std::make_unique<Velopack::GithubSource>(kRepoUrl));
                    }
                    update_ = manager_->CheckForUpdates();
                }
                catch (const std::exception &error)
                {
                    emit checkFailed(QString::fromUtf8(error.what()));
                    return;
                }
                if (!update_)
                {
                    return;
                }
                if (QString rejection; !updateIsTrusted(*update_, &rejection))
                {
                    update_.reset();
                    emit checkFailed(rejection);
                    return;
                }
                emit updateAvailable(QString::fromStdString(update_->TargetFullRelease.Version),
                    QString::fromStdString(update_->TargetFullRelease.NotesMarkdown));
            }

            void download()
            {
                if (!manager_ || !update_)
                {
                    emit downloadFailed(QStringLiteral("no update is pending"));
                    return;
                }
                if (QString rejection; !cacheIsUsable(&rejection))
                {
                    emit downloadFailed(rejection);
                    return;
                }
                discardCachedPackagesThatDoNotMatch(*update_);
                try
                {
                    manager_->DownloadUpdates(*update_, &VelopackWorker::forwardProgress, this);
                }
                catch (const std::exception &error)
                {
                    emit downloadFailed(QString::fromUtf8(error.what()));
                    return;
                }
                if (QString rejection;
                    !cachedPackageIsTheOneDescribed(update_->TargetFullRelease, &rejection))
                {
                    emit downloadFailed(rejection);
                    return;
                }
                emit downloadFinished();
            }

            void apply()
            {
                if (!manager_ || !update_)
                {
                    return;
                }
                // Checked again here rather than trusting the download: this is the last moment
                // the package is ours to look at, and on a shared cache it is not ours alone.
                if (QString rejection;
                    !cacheIsUsable(&rejection)
                    || !cachedPackageIsTheOneDescribed(update_->TargetFullRelease, &rejection))
                {
                    emit downloadFailed(rejection);
                    return;
                }
                try
                {
                    manager_->WaitExitThenApplyUpdates(*update_);
                }
                catch (const std::exception &error)
                {
                    emit downloadFailed(QString::fromUtf8(error.what()));
                    return;
                }
                emit applied();
            }

        signals:
            void updateAvailable(const QString &version, const QString &releaseNotes);

            void checkFailed(const QString &error);

            void progress(int percent);

            void downloadFinished();

            void downloadFailed(const QString &error);

            void applied();

        private:
            static void forwardProgress(void *userData, std::size_t value)
            {
                auto *worker = static_cast<VelopackWorker *>(userData);
                emit worker->progress(static_cast<int>(value));
            }

            // Fetches `<asset>.sig` from the release the feed points at. The file lives in the
            // same place an attacker who can publish a release controls, which is fine: the
            // signature is checked against a key that is not there.
            std::optional<QString> fetchSignature(const Velopack::VelopackAsset &asset)
            {
                const QUrl url(QString::fromLatin1(kReleaseDownloadPrefix)
                               + QString::fromStdString(asset.Version) + QLatin1Char('/')
                               + QString::fromStdString(asset.FileName) + QLatin1String(".sig"));
                QNetworkRequest request{url};
                request.setTransferTimeout(15000);
                request.setAttribute(QNetworkRequest::RedirectPolicyAttribute,
                    QNetworkRequest::NoLessSafeRedirectPolicy);

                QNetworkReply *reply = network_.get(request);
                QEventLoop loop;
                connect(reply, &QNetworkReply::finished, &loop, &QEventLoop::quit);
                loop.exec();
                reply->deleteLater();
                if (reply->error() != QNetworkReply::NoError)
                {
                    return std::nullopt;
                }
                // Base64 of 64 bytes, so a signature file is tiny; anything else is not one.
                const QByteArray body = reply->read(256);
                return QString::fromLatin1(body).trimmed();
            }

            // Every asset that could end up being applied has to verify, not just the full
            // package: with deltas enabled the bytes that reach the installer are the deltas.
            bool assetIsTrusted(const Velopack::VelopackAsset &asset, QString *rejection)
            {
                const QString pinnedKey = QString::fromLatin1(CLOAKFRAME_UPDATE_PUBLIC_KEY);
                const auto signature = fetchSignature(asset);
                if (!signature && !pinnedKey.isEmpty())
                {
                    *rejection = tr("Could not download the signature for %1. The update was not "
                                    "applied.")
                                     .arg(QString::fromStdString(asset.FileName));
                    return false;
                }

                QString error;
                switch (evaluateUpdateTrust(QString::fromStdString(asset.SHA256),
                    signature.value_or(QString()),
                    pinnedKey,
                    &error))
                {
                case UpdateTrust::Trusted:
                    return true;
                case UpdateTrust::Unpinned:
                    // Matches the macOS build configured without a Sparkle key: the check exists
                    // exactly when a key was configured at build time.
                    return true;
                case UpdateTrust::Rejected:
                    *rejection = tr("The update %1 failed its signature check and was not applied "
                                    "(%2).")
                                     .arg(QString::fromStdString(asset.FileName), error);
                    return false;
                }
                return false;
            }

            // The Linux cache is shared with every account on the machine. Whoever owns the
            // tree decides what is in it, so a tree this user does not own is not a place an
            // update may come from.
            bool cacheIsUsable(QString *rejection)
            {
#ifdef __linux__
                const QString packages = packagesDirectory(manager_->GetAppId());
                for (const QString &directory :
                    {QString::fromLatin1(kVelopackCacheRoot), QFileInfo(packages).path(), packages})
                {
                    if (inspectCacheDirectory(directory) == CacheDirectoryTrust::Shared)
                    {
                        *rejection =
                            tr("The update cache at %1 is owned or writable by another account "
                               "on this computer, so an update taken from it cannot be trusted. "
                               "Remove that directory and try again.")
                                .arg(directory);
                        return false;
                    }
                }
#else
                Q_UNUSED(rejection);
#endif
                return true;
            }

            // The updater skips downloading whenever a file of the expected name is already
            // present and never hashes it, so a package that is not the one the feed describes
            // has to go before the download runs or it is what gets installed.
            void discardCachedPackagesThatDoNotMatch(const Velopack::UpdateInfo &update)
            {
#ifdef __linux__
                const QDir packages(packagesDirectory(manager_->GetAppId()));
                auto discard = [&packages](const Velopack::VelopackAsset &asset)
                {
                    const QString path = packages.filePath(QString::fromStdString(asset.FileName));
                    if (QFileInfo::exists(path)
                        && !fileMatchesDigest(path, QString::fromStdString(asset.SHA256)))
                    {
                        QFile::remove(path);
                    }
                };
                discard(update.TargetFullRelease);
                for (const auto &delta : update.DeltasToTarget)
                {
                    discard(delta);
                }
#else
                Q_UNUSED(update);
#endif
            }

            bool cachedPackageIsTheOneDescribed(
                const Velopack::VelopackAsset &asset, QString *rejection)
            {
#ifdef __linux__
                const QString path = QDir(packagesDirectory(manager_->GetAppId()))
                                         .filePath(QString::fromStdString(asset.FileName));
                if (QFileInfo::exists(path)
                    && !fileMatchesDigest(path, QString::fromStdString(asset.SHA256)))
                {
                    *rejection = tr("The cached update %1 is not the package this release "
                                    "describes and was not applied.")
                                     .arg(QString::fromStdString(asset.FileName));
                    return false;
                }
#else
                Q_UNUSED(asset);
                Q_UNUSED(rejection);
#endif
                return true;
            }

            bool updateIsTrusted(const Velopack::UpdateInfo &update, QString *rejection)
            {
                if (!assetIsTrusted(update.TargetFullRelease, rejection))
                {
                    return false;
                }
                for (const auto &delta : update.DeltasToTarget)
                {
                    if (!assetIsTrusted(delta, rejection))
                    {
                        return false;
                    }
                }
                return true;
            }

            std::unique_ptr<Velopack::UpdateManager> manager_;
            std::optional<Velopack::UpdateInfo> update_;
            QNetworkAccessManager network_;
        };

        class VelopackSelfUpdater final : public SelfUpdater
        {
            Q_OBJECT

        public:
            explicit VelopackSelfUpdater(QObject *parent)
                : SelfUpdater(parent)
                , thread_(new QThread)
                , worker_(new VelopackWorker)
            {
                worker_->moveToThread(thread_);
                connect(thread_, &QThread::finished, worker_, &QObject::deleteLater);
                connect(thread_, &QThread::finished, thread_, &QObject::deleteLater);
                connect(
                    worker_, &VelopackWorker::updateAvailable, this, &SelfUpdater::updateAvailable);
                connect(worker_, &VelopackWorker::checkFailed, this, &SelfUpdater::checkFailed);
                connect(worker_, &VelopackWorker::progress, this, &SelfUpdater::downloadProgress);
                connect(worker_,
                    &VelopackWorker::downloadFinished,
                    this,
                    &SelfUpdater::downloadFinished);
                connect(
                    worker_, &VelopackWorker::downloadFailed, this, &SelfUpdater::downloadFailed);
                connect(worker_, &VelopackWorker::applied, this, &SelfUpdater::readyToQuit);
                thread_->start();
            }

            ~VelopackSelfUpdater() override
            {
                thread_->quit();
            }

            Mode mode() const override
            {
                return Mode::InApp;
            }

            void checkForUpdates() override
            {
                QMetaObject::invokeMethod(worker_, &VelopackWorker::check, Qt::QueuedConnection);
            }

            void downloadUpdate() override
            {
                QMetaObject::invokeMethod(worker_, &VelopackWorker::download, Qt::QueuedConnection);
            }

            void restartToApply() override
            {
                QMetaObject::invokeMethod(worker_, &VelopackWorker::apply, Qt::QueuedConnection);
            }

        private:
            QThread *thread_;
            VelopackWorker *worker_;
        };
    }

    void SelfUpdater::runStartupHooks()
    {
        Velopack::VelopackApp::Build().Run();
    }

    SelfUpdater *SelfUpdater::create(QObject *parent)
    {
        return new VelopackSelfUpdater(parent);
    }
}

#include "SelfUpdaterVelopack.moc"

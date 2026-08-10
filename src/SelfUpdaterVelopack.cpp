#include "cloakframe/SelfUpdater.hpp"
#include "cloakframe/UpdateSignature.hpp"

#include <QEventLoop>
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
                try
                {
                    manager_->DownloadUpdates(*update_, &VelopackWorker::forwardProgress, this);
                }
                catch (const std::exception &error)
                {
                    emit downloadFailed(QString::fromUtf8(error.what()));
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

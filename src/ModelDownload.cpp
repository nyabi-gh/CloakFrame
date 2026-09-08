#include "cloakframe/ModelDownload.hpp"

#include <QCryptographicHash>
#include <QElapsedTimer>
#include <QEventLoop>
#include <QNetworkAccessManager>
#include <QNetworkReply>
#include <QNetworkRequest>
#include <QScopedPointer>
#include <QTimer>

#include <algorithm>
#include <limits>

namespace cloakframe
{
    ModelDownloadResult downloadModelData(const QUrl &url,
        qint64 maxBytes,
        const QString &sha256,
        const std::function<bool()> &cancelled,
        const std::function<void(qint64, qint64)> &progress,
        ModelDownloadPolicy policy)
    {
        ModelDownloadResult result;
        if (maxBytes <= 0 || maxBytes >= std::numeric_limits<qint64>::max())
            return result;
        QNetworkAccessManager manager;
        QElapsedTimer elapsed;
        elapsed.start();
        const auto isCancelled = [&]
        {
            return cancelled && cancelled();
        };
        for (int attempt = 0; attempt < std::clamp(policy.attempts, 1, 3); ++attempt)
        {
            if (isCancelled())
            {
                result.status = ModelDownloadStatus::Cancelled;
                return result;
            }
            if (elapsed.elapsed() >= policy.totalMs)
            {
                result.status = ModelDownloadStatus::TimedOut;
                return result;
            }
            ++result.attempts;
            QNetworkRequest request(url);
            request.setAttribute(QNetworkRequest::RedirectPolicyAttribute,
                QNetworkRequest::NoLessSafeRedirectPolicy);
            QScopedPointer<QNetworkReply> reply(manager.get(request));
            reply->setReadBufferSize(64LL * 1024);
            QEventLoop loop;
            QTimer idle;
            QTimer deadline;
            QTimer poll;
            idle.setSingleShot(true);
            deadline.setSingleShot(true);
            poll.setInterval(25);
            bool timedOut = false;
            bool tooLarge = false;
            bool wasCancelled = false;
            QByteArray data;
            const auto timeout = [&]
            {
                timedOut = true;
                reply->abort();
                loop.quit();
            };
            QObject::connect(&idle, &QTimer::timeout, &loop, timeout);
            QObject::connect(&deadline, &QTimer::timeout, &loop, timeout);
            QObject::connect(&poll,
                &QTimer::timeout,
                &loop,
                [&]
                {
                    if (isCancelled())
                    {
                        wasCancelled = true;
                        reply->abort();
                        loop.quit();
                    }
                });
            const auto read = [&]
            {
                if (tooLarge || timedOut || wasCancelled)
                    return;
                const QByteArray chunk = reply->read(maxBytes - data.size() + 1);
                if (!chunk.isEmpty())
                    idle.start(std::max(1, policy.inactivityMs));
                data += chunk;
                if (data.size() > maxBytes)
                {
                    tooLarge = true;
                    reply->abort();
                    loop.quit();
                }
            };
            QObject::connect(reply.data(), &QIODevice::readyRead, &loop, read);
            QObject::connect(reply.data(),
                &QNetworkReply::metaDataChanged,
                &loop,
                [&]
                {
                    const qint64 size =
                        reply->header(QNetworkRequest::ContentLengthHeader).toLongLong();
                    if (size > maxBytes)
                    {
                        tooLarge = true;
                        reply->abort();
                        loop.quit();
                    }
                });
            QObject::connect(reply.data(),
                &QNetworkReply::downloadProgress,
                &loop,
                [&](qint64 received, qint64 total)
                {
                    if (progress)
                        progress(received, total);
                });
            QObject::connect(reply.data(), &QNetworkReply::finished, &loop, &QEventLoop::quit);
            idle.start(std::max(1, policy.inactivityMs));
            deadline.start(
                static_cast<int>(std::max<qint64>(1, policy.totalMs - elapsed.elapsed())));
            poll.start();
            if (!reply->isFinished())
                loop.exec();
            idle.stop();
            deadline.stop();
            poll.stop();
            if (wasCancelled || isCancelled())
            {
                result.status = ModelDownloadStatus::Cancelled;
                return result;
            }
            if (!timedOut && !tooLarge)
                read();
            if (tooLarge)
            {
                result.status = ModelDownloadStatus::TooLarge;
                return result;
            }
            if (!timedOut && reply->error() == QNetworkReply::NoError)
            {
                const QString actual = QString::fromLatin1(
                    QCryptographicHash::hash(data, QCryptographicHash::Sha256).toHex());
                if (actual.compare(sha256, Qt::CaseInsensitive) != 0)
                {
                    result.status = ModelDownloadStatus::IntegrityError;
                    return result;
                }
                result.status = ModelDownloadStatus::Success;
                result.data = std::move(data);
                return result;
            }
            result.status =
                timedOut ? ModelDownloadStatus::TimedOut : ModelDownloadStatus::NetworkError;
            const int http = reply->attribute(QNetworkRequest::HttpStatusCodeAttribute).toInt();
            const auto error = reply->error();
            const bool transient = timedOut || http == 429 || http >= 500
                                   || error == QNetworkReply::TimeoutError
                                   || error == QNetworkReply::TemporaryNetworkFailureError
                                   || error == QNetworkReply::RemoteHostClosedError
                                   || error == QNetworkReply::ConnectionRefusedError;
            if (!transient || attempt + 1 >= std::clamp(policy.attempts, 1, 3))
                return result;
            QEventLoop backoff;
            QTimer delay;
            QTimer cancelPoll;
            delay.setSingleShot(true);
            QObject::connect(&delay, &QTimer::timeout, &backoff, &QEventLoop::quit);
            QObject::connect(&cancelPoll,
                &QTimer::timeout,
                &backoff,
                [&]
                {
                    if (isCancelled() || elapsed.elapsed() >= policy.totalMs)
                        backoff.quit();
                });
            delay.start(std::max(1, policy.retryDelayMs));
            cancelPoll.start(25);
            backoff.exec();
        }
        return result;
    }
}

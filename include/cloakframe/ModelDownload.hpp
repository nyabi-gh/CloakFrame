#pragma once
#include <QByteArray>
#include <QUrl>

#include <functional>

namespace cloakframe
{
    enum class ModelDownloadStatus
    {
        Success,
        Cancelled,
        TimedOut,
        NetworkError,
        TooLarge,
        IntegrityError
    };
    struct ModelDownloadPolicy
    {
        int inactivityMs = 30'000;
        int totalMs = 15 * 60'000;
        int attempts = 3;
        int retryDelayMs = 1000;
    };
    struct ModelDownloadResult
    {
        ModelDownloadStatus status = ModelDownloadStatus::NetworkError;
        QByteArray data;
        int attempts = 0;
    };
    [[nodiscard]] ModelDownloadResult downloadModelData(const QUrl &url,
        qint64 maxBytes,
        const QString &sha256,
        const std::function<bool()> &cancelled,
        const std::function<void(qint64, qint64)> &progress = {},
        ModelDownloadPolicy policy = {});
}

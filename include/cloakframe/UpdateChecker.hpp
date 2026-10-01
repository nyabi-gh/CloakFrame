#pragma once

#include <QObject>
#include <QString>

class QNetworkAccessManager;

namespace cloakframe
{
    class UpdateChecker final : public QObject
    {
        Q_OBJECT

    public:
        explicit UpdateChecker(QString currentVersion, QObject *parent = nullptr);

        ~UpdateChecker() override;

        static QString releasesPageUrl();

        void check();

    signals:
        void updateAvailable(const QString &latestVersion, const QString &releaseUrl);

    private:
        QString currentVersion_;
        QNetworkAccessManager *manager_;
    };
}

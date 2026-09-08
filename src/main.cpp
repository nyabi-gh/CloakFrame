#include "cloakframe/Logging.hpp"
#include "cloakframe/MainWindow.hpp"
#include "cloakframe/ProcessorWorker.hpp"
#include "cloakframe/ReviewTypes.hpp"
#include "cloakframe/SelfUpdater.hpp"
#include "cloakframe/StageCleanup.hpp"
#include "cloakframe/Theme.hpp"

#include <QApplication>
#include <QDir>
#include <QFont>
#include <QMetaType>
#include <QRectF>
#include <QSettings>
#include <QStandardPaths>
#include <QStyleFactory>
#include <QThreadPool>
#include <QVector>

#include <spdlog/sinks/rotating_file_sink.h>
#include <spdlog/sinks/stdout_color_sinks.h>
#include <spdlog/spdlog.h>

#include <exception>
#include <memory>
#include <vector>

#ifndef CLOAKFRAME_VERSION
#define CLOAKFRAME_VERSION "0.0.0"
#endif

namespace
{
    constexpr auto kOrganizationName = "CloakFrame";
    constexpr auto kOrganizationDomain = "cloakframe.app";
    constexpr auto kApplicationName = "CloakFrame";

    void configureApplicationAndMigrateSettings()
    {
        QCoreApplication::setOrganizationName("Redactly");
        QCoreApplication::setOrganizationDomain("redactly.app");
        QCoreApplication::setApplicationName("Redactly");
        QSettings legacySettings;

        QCoreApplication::setOrganizationName(kOrganizationName);
        QCoreApplication::setOrganizationDomain(kOrganizationDomain);
        QCoreApplication::setApplicationName(kApplicationName);
        QCoreApplication::setApplicationVersion(CLOAKFRAME_VERSION);

        QSettings currentSettings;
        constexpr auto kMigrationKey = "migration/redactlySettingsImported";
        if (currentSettings.value(kMigrationKey, false).toBool())
        {
            return;
        }

        for (const auto &key : legacySettings.allKeys())
        {
            if (!currentSettings.contains(key))
            {
                currentSettings.setValue(key, legacySettings.value(key));
            }
        }
        currentSettings.setValue(kMigrationKey, true);
        currentSettings.sync();
    }

}

int main(int argc, char *argv[])
{
    cloakframe::SelfUpdater::runStartupHooks();

    QApplication app(argc, argv);

    configureApplicationAndMigrateSettings();

    cloakframe::configureLogging(
        cloakframe::localLogDirectory(), QSettings().value("detailedLogging", false).toBool());

    // A run that was killed leaves its private copy of the source behind. Sweep on a worker
    // thread: one of the roots may be on a slow mount, and nothing here has to finish before
    // the window appears.
    QThreadPool::globalInstance()->start(
        []
        {
            cloakframe::removeStaleStages();
        });

    QApplication::setStyle(QStyleFactory::create("Fusion"));
    {
        QSettings settings;
        const auto mode =
            cloakframe::themeModeFromString(settings.value("theme", "system").toString());
        cloakframe::applyTheme(app, mode);
    }

    qRegisterMetaType<cloakframe::ReviewResult>("cloakframe::ReviewResult");
    qRegisterMetaType<cloakframe::RunOutcome>("cloakframe::RunOutcome");
    qRegisterMetaType<cloakframe::RunSummary>("cloakframe::RunSummary");
    qRegisterMetaType<QVector<QRectF>>("QVector<QRectF>");

#ifdef Q_OS_MACOS
    QFont defaultFont("SF Pro Text", 13);
#elif defined(Q_OS_WIN)
    QFont defaultFont("Segoe UI", 10);
#else
    QFont defaultFont;
    defaultFont.setPointSize(10);
#endif
    app.setFont(defaultFont);

    cloakframe::MainWindow window;
    window.show();
    return QApplication::exec();
}

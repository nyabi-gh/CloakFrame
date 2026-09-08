#include "cloakframe/Logging.hpp"

#include <QCoreApplication>
#include <QFile>
#include <QTemporaryDir>

#include <spdlog/spdlog.h>

#include <cassert>

int main(int argc, char **argv)
{
    QCoreApplication app(argc, argv);
    QTemporaryDir dir;
    assert(dir.isValid());
    const auto contents = [&]
    {
        QFile file(dir.filePath("cloakframe.log"));
        assert(file.open(QIODevice::ReadOnly));
        return file.readAll();
    };
    cloakframe::configureLogging(dir.path(), false);
    spdlog::warn("private/person.jpg secret name");
    cloakframe::logDiagnostic(QStringLiteral("Run outcome=0 total=1"));
    assert(!contents().contains("person.jpg") && contents().contains("total=1"));
    cloakframe::setDetailedLogging(true);
    spdlog::info("detail.jpg");
    assert(contents().contains("detail.jpg"));
    cloakframe::setDetailedLogging(false);
    spdlog::error("hidden.jpg");
    assert(!contents().contains("hidden.jpg"));
    QFile unrelated(dir.filePath("keep.txt"));
    assert(unrelated.open(QIODevice::WriteOnly));
    unrelated.close();
    assert(cloakframe::clearLocalLogs());
    assert(contents().isEmpty() && unrelated.exists());
    cloakframe::logDiagnostic(QStringLiteral("Diagnostics resumed"));
    assert(contents().contains("Diagnostics resumed"));
    return 0;
}

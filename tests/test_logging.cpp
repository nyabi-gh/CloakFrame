#include "cloakframe/Logging.hpp"

#include <QCoreApplication>
#include <QFile>
#include <QTemporaryDir>

#include <spdlog/spdlog.h>

#include <cassert>
#include <sstream>

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
    std::ostringstream console;
    cloakframe::setConsoleLogStreamForTesting(&console);
    cloakframe::configureLogging(dir.path(), false);
    spdlog::warn("private/person.jpg secret name");
    cloakframe::logDiagnostic(QStringLiteral("Run outcome=0 total=1"));
    assert(!contents().contains("person.jpg") && contents().contains("total=1"));
    assert(console.str().find("person.jpg") == std::string::npos);
    assert(console.str().find("total=1") != std::string::npos);
    cloakframe::setDetailedLogging(true);
    spdlog::info("detail.jpg");
    assert(contents().contains("detail.jpg"));
    assert(console.str().find("detail.jpg") != std::string::npos);
    cloakframe::setDetailedLogging(false);
    spdlog::error("hidden.jpg");
    assert(!contents().contains("hidden.jpg"));
    assert(console.str().find("hidden.jpg") == std::string::npos);
    QFile unrelated(dir.filePath("keep.txt"));
    assert(unrelated.open(QIODevice::WriteOnly));
    unrelated.close();
    assert(cloakframe::clearLocalLogs());
    assert(contents().isEmpty() && unrelated.exists());
    cloakframe::logDiagnostic(QStringLiteral("Diagnostics resumed"));
    assert(contents().contains("Diagnostics resumed"));
    return 0;
}

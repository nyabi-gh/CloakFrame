#pragma once
#include <QString>

#include <iosfwd>

namespace cloakframe
{
    [[nodiscard]] QString localLogDirectory();
    // Messages reach the log file and standard output only while detailed logging is on;
    // `logDiagnostic` reaches both always.
    void configureLogging(const QString &directory, bool detailed);
    // Test seam: send console output to `stream` instead of standard output. Takes effect at the
    // next `configureLogging`; null restores standard output.
    void setConsoleLogStreamForTesting(std::ostream *stream);
    void setDetailedLogging(bool enabled);
    [[nodiscard]] bool clearLocalLogs();
    // Accept only fixed diagnostics and numeric metrics, never paths or exception messages.
    void logDiagnostic(const QString &message);
}

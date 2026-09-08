#pragma once
#include <QString>

namespace cloakframe
{
    [[nodiscard]] QString localLogDirectory();
    void configureLogging(const QString &directory, bool detailed);
    void setDetailedLogging(bool enabled);
    [[nodiscard]] bool clearLocalLogs();
    // Accept only fixed diagnostics and numeric metrics, never paths or exception messages.
    void logDiagnostic(const QString &message);
}

#pragma once

#include <QMetaType>
#include <QString>
#include <QStringList>

namespace cloakframe
{
    enum class FileResultStatus
    {
        Saved,
        NeedsReview,
        Skipped,
        Failed,
        Cancelled,
        UnreadableInput,
    };

    struct FileResult
    {
        QString sourcePath;
        // Empty unless this run actually published an output for this input.
        QString outputPath;
        FileResultStatus status = FileResultStatus::Failed;
        QStringList messages;
    };
}

Q_DECLARE_METATYPE(cloakframe::FileResult)

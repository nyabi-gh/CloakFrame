#pragma once

#include <QMetaType>
#include <QString>
#include <QStringList>
#include <QVector>

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

    enum class FileIssueKind
    {
        OmittedRegions,
        TrackingGap,
        DroppedTracks,
        ExcludedTracks,
        ScanFailure,
        MetadataWarning,
        OutputConflict,
        ProcessingFailure,
        UnredactedOutput
    };
    struct FileIssue
    {
        FileIssueKind kind = FileIssueKind::ProcessingFailure;
        qint64 count = 0;
        int firstFrame = -1;
        int lastFrame = -1;
        int trackId = 0;
        bool acknowledged = false;
    };

    struct FileResult
    {
        QString sourcePath;
        // Empty unless this run actually published an output for this input.
        QString outputPath;
        FileResultStatus status = FileResultStatus::Failed;
        QStringList messages;
        QVector<FileIssue> issues{};
    };
}

Q_DECLARE_METATYPE(cloakframe::FileResult)

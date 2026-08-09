#pragma once

#include "cloakframe/Tracking.hpp"

#include <QMetaType>
#include <QRectF>
#include <QSize>
#include <QString>
#include <QStringList>
#include <QVector>

#include <optional>

namespace cloakframe
{
    struct VideoReviewBox
    {
        int frame = 0;
        QRectF rect;
        bool interpolated = false;
    };

    struct VideoReviewTrack
    {
        int id = 0;
        bool lowConfidence = false;
        QVector<VideoReviewBox> boxes;
    };

    struct VideoReviewManualTrack
    {
        int id = 0;
        int startFrame = 0;
        int endFrame = 0;
        QVector<VideoReviewBox> keyframes;
    };

    struct VideoReviewRequest
    {
        QString sourcePath;
        QString ffmpegPath;
        QString sourceName;
        QSize frameSize;
        double fps = 0.0;
        // Must match VideoInfo::startTimeSeconds so preview frame indices mean the same thing
        // as the indices the encoder masks.
        double startTimeSeconds = 0.0;
        int fpsNum = 0;
        int fpsDen = 1;
        int frameCount = 0;
        QVector<VideoReviewTrack> tracks;
        // Frame ranges post-processing refused to interpolate. They are exactly the places a
        // manual track is worth drawing, so the timeline marks them.
        QVector<UncoveredSpan> uncoveredSpans;
    };

    enum class VideoReviewDecision
    {
        Encode,
        CancelAll,
    };

    // FFmpeg arguments that extract exactly the frame the encoder knows as `frame`.
    [[nodiscard]] QStringList videoPreviewFrameArguments(
        const VideoReviewRequest &request, int frame);

    struct VideoReviewResult
    {
        VideoReviewDecision decision = VideoReviewDecision::Encode;
        QVector<int> excludedTrackIds;
        QVector<VideoReviewManualTrack> addedTracks;
    };

    [[nodiscard]] std::optional<QRectF> manualTrackRectAtFrame(
        const VideoReviewManualTrack &track, int frame);

    [[nodiscard]] std::optional<Track> materializeManualVideoTrack(
        const VideoReviewManualTrack &track,
        int frameCount,
        const QSize &frameSize,
        int assignedId);
}

Q_DECLARE_METATYPE(cloakframe::VideoReviewBox)
Q_DECLARE_METATYPE(cloakframe::VideoReviewTrack)
Q_DECLARE_METATYPE(cloakframe::VideoReviewManualTrack)
Q_DECLARE_METATYPE(cloakframe::VideoReviewRequest)
Q_DECLARE_METATYPE(cloakframe::VideoReviewResult)

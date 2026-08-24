#pragma once

#include "cloakframe/FaceDetection.hpp"
#include "cloakframe/Mosaic.hpp"
#include "cloakframe/Tracking.hpp"
#include "cloakframe/VideoIo.hpp"

#include <QString>

#include <atomic>
#include <functional>
#include <vector>

namespace cloakframe
{
    struct VideoProcessOptions
    {
        float scoreThreshold = 0.5F;
        float nmsThreshold = 0.4F;
        int mosaicBlockSize = 14;
        float paddingRatio = 0.18F;
        AnonymizationMethod method = AnonymizationMethod::Mosaic;
        cv::Mat customImage;
        MaskShape shape = MaskShape::Rectangle;
        bool softEdges = false;
        int crf = 18;
        VideoCodec codec = VideoCodec::H264;
        int analysisLongEdge = 960;
        bool hardwareEncoder = true;
        QString outputRootPath;
        QString outputRelativePath;
        TrackerConfig tracker;
        TrackPostProcessConfig postProcess;
    };

    enum class VideoProcessStatus
    {
        Completed,
        Cancelled,
        Failed,
    };

    struct VideoProcessResult
    {
        VideoProcessStatus status = VideoProcessStatus::Failed;
        QString error;
        qint64 frameCount = 0;
        int trackCount = 0;
        // Frames inside a retained track's span that the output does not mask, and where
        // they are. These are frames, not objects, and must not be added to a count of
        // detections.
        int uncoveredFrames = 0;
        std::vector<UncoveredSpan> uncoveredSpans;
        // Tracks discarded for holding no confident detection at all.
        int droppedTracks = 0;
        QString encoderName;
    };

    using VideoProgressFn = std::function<void(int pass, qint64 frame, qint64 totalEstimate)>;
    using VideoDetectFn = std::function<FaceDetections(const cv::Mat &frame)>;
    using VideoTrackReviewFn = std::function<bool(std::vector<Track> &tracks,
        const std::vector<UncoveredSpan> &uncoveredSpans,
        qint64 frameCount,
        const QString &sourcePath,
        const VideoInfo &info)>;

    struct VideoMaskingPlan
    {
        qint64 frameBytes = 0;
        int workerCount = 1;
        int batchFrames = 1;
        // Decoded frames the prefetching reader may hold beyond the batch. Sized like the
        // batch so the decoder can produce a whole batch while the previous one is masked
        // and encoded.
        int prefetchFrames = 1;
    };

    [[nodiscard]] float videoStrongScoreThreshold(float scoreThreshold);

    [[nodiscard]] VideoMaskingPlan videoMaskingPlan(
        int width, int height, unsigned int hardwareThreads, qint64 memoryBudget = 0);

    VideoProcessResult processVideo(const FfmpegTools &tools,
        const QString &sourcePath,
        const QString &destinationPath,
        const VideoInfo &info,
        const VideoProcessOptions &options,
        const VideoDetectFn &detect,
        const std::atomic<bool> &cancelled,
        const VideoProgressFn &progress = {},
        const VideoTrackReviewFn &review = {});
}

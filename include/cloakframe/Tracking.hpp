#pragma once

#include "cloakframe/FaceDetection.hpp"
#include "cloakframe/SceneCut.hpp"

#include <opencv2/core.hpp>

#include <exception>
#include <functional>
#include <vector>

namespace cloakframe
{
    using TrackingContinueGuard = std::function<bool()>;

    class TrackingCancelled final : public std::exception
    {
    public:
        [[nodiscard]] const char *what() const noexcept override
        {
            return "Tracking cancelled.";
        }
    };

    struct TrackedBox
    {
        int frame = 0;
        cv::Rect2f box;
        float score = 0.0F;
        bool interpolated = false;
        float rollRadians = 0.0F;
        bool hasPose = false;
    };

    struct Track
    {
        int id = 0;
        bool lowConfidence = false;
        std::vector<TrackedBox> boxes;

        [[nodiscard]] int firstFrame() const;
        [[nodiscard]] int lastFrame() const;
        [[nodiscard]] const TrackedBox *boxAtFrame(int frame) const;
    };

    struct TrackerConfig
    {
        float highScoreThreshold = 0.5F;
        float lowScoreThreshold = 0.1F;
        float iouThreshold = 0.3F;
        int maxFramesLost = 30;
        int maxFramesSinceHighScore = 30;
        int maxFramesSinceHighScoreMoving = 45;
        float coastMotionThreshold = 0.05F;
        float velocityBlend = 0.3F;
    };

    class ByteTracker
    {
    public:
        explicit ByteTracker(TrackerConfig config = {}, SceneCuts cuts = {});

        void update(int frame,
            const FaceDetections &detections,
            const TrackingContinueGuard &continueGuard = {});

        [[nodiscard]] std::vector<Track> finish();

    private:
        struct ActiveTrack
        {
            Track track;
            cv::Point2f velocity{0.0F, 0.0F};
            int lastFrame = 0;
            int lastHighScoreFrame = 0;

            [[nodiscard]] cv::Rect2f predictedBox(int frame) const;
        };

        void extendTrack(ActiveTrack &active, int frame, const FaceDetection &detection);

        TrackerConfig config_;
        SceneCuts cuts_;
        std::vector<ActiveTrack> active_;
        std::vector<Track> finished_;
        std::size_t boxCount_ = 0;
        int nextId_ = 1;
    };

    struct TrackPostProcessConfig
    {
        int maxInterpolationGap = 20;
        int smoothingRadius = 2;
        int extensionFrames = 3;
        float strongScoreThreshold = 0.5F;
        int minStrongDetections = 3;
        int shortTrackMinStrong = 1;
        float shortTrackStrongRatio = 0.5F;
        bool retainLowConfidenceTracks = false;
    };

    [[nodiscard]] std::vector<Track> buildTracks(const std::vector<FaceDetections> &frameDetections,
        const TrackerConfig &config = {},
        const SceneCuts &cuts = {},
        const TrackingContinueGuard &continueGuard = {});

    [[nodiscard]] std::vector<Track> buildBidirectionalTracks(
        const std::vector<FaceDetections> &frameDetections,
        const TrackerConfig &config = {},
        float mergeIouThreshold = 0.5F,
        const SceneCuts &cuts = {},
        const TrackingContinueGuard &continueGuard = {});

    // A run of frames inside one track's own span that stayed unmasked, closed at both
    // ends. A total on its own gives a reviewer nowhere to look, so the range travels
    // with it.
    struct UncoveredSpan
    {
        int trackId = 0;
        int firstFrame = 0;
        int lastFrame = 0;

        [[nodiscard]] int frameCount() const
        {
            return lastFrame - firstFrame + 1;
        }
    };

    std::vector<UncoveredSpan> interpolateGaps(Track &track,
        int maxGap,
        const SceneCuts &cuts = {},
        const TrackingContinueGuard &continueGuard = {});

    void smoothTrack(Track &track, int radius, const TrackingContinueGuard &continueGuard = {});

    void extendTrackEnds(Track &track,
        int frames,
        int frameCount,
        const SceneCuts &cuts = {},
        const TrackingContinueGuard &continueGuard = {});

    // Coverage that post-processing could not deliver, in three kinds that have different
    // remedies and must not be added together: `uncoveredFrames` and the matching
    // `uncoveredSpans` are frames inside a retained track's own span that end up with no
    // mask, and `droppedTracks` counts tracks discarded for holding no confident detection
    // at all. A caller must not report a clean result while any of them is nonzero.
    struct TrackCoverageReport
    {
        int uncoveredFrames = 0;
        int droppedTracks = 0;
        std::vector<UncoveredSpan> uncoveredSpans;
    };

    TrackCoverageReport postProcessTracks(std::vector<Track> &tracks,
        const TrackPostProcessConfig &config,
        int frameCount,
        const SceneCuts &cuts = {},
        const TrackingContinueGuard &continueGuard = {});

    [[nodiscard]] std::vector<cv::Rect2f> trackRegionsForFrame(
        const std::vector<Track> &tracks, int frame);
}

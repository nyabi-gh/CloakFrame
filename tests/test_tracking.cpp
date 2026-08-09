#include "cloakframe/SceneCut.hpp"
#include "cloakframe/Tracking.hpp"
#include "cloakframe/VideoReviewTypes.hpp"

#include <opencv2/core.hpp>

#include <cassert>
#include <cmath>
#include <cstdio>
#include <stdexcept>
#include <vector>

namespace
{
    void testManualVideoTrackInterpolationAndBounds()
    {
        cloakframe::VideoReviewManualTrack manual;
        manual.id = 90;
        manual.startFrame = 2;
        manual.endFrame = 8;
        manual.keyframes = {
            {3, QRectF(10.0, 20.0, 30.0, 40.0), false},
            {7, QRectF(30.0, 40.0, 50.0, 60.0), false},
        };

        const auto beforeFirst = cloakframe::manualTrackRectAtFrame(manual, 2);
        assert(beforeFirst && *beforeFirst == manual.keyframes.front().rect);
        const auto middle = cloakframe::manualTrackRectAtFrame(manual, 5);
        assert(middle && *middle == QRectF(20.0, 30.0, 40.0, 50.0));
        const auto afterLast = cloakframe::manualTrackRectAtFrame(manual, 8);
        assert(afterLast && *afterLast == manual.keyframes.back().rect);
        assert(!cloakframe::manualTrackRectAtFrame(manual, 1));

        const auto track = cloakframe::materializeManualVideoTrack(manual, 10, QSize(100, 100), 12);
        assert(track && track->id == 12 && track->boxes.size() == 7);
        assert(track->firstFrame() == 2 && track->lastFrame() == 8);
        assert(track->boxAtFrame(3) && !track->boxAtFrame(3)->interpolated);
        assert(track->boxAtFrame(5) && track->boxAtFrame(5)->interpolated);

        auto invalid = manual;
        std::swap(invalid.keyframes[0], invalid.keyframes[1]);
        assert(!cloakframe::materializeManualVideoTrack(invalid, 10, QSize(100, 100), 13));
        assert(!cloakframe::materializeManualVideoTrack(manual, 8, QSize(100, 100), 13));
    }

    cloakframe::FaceDetection det(float x, float y, float score = 0.9F, float size = 40.0F)
    {
        return {cv::Rect2f(x, y, size, size), score};
    }

    cloakframe::FaceDetection posedDet(
        float x, float y, float rollRadians, float score = 0.9F, float size = 40.0F)
    {
        auto detection = det(x, y, score, size);
        detection.rollRadians = rollRadians;
        detection.hasPose = true;
        return detection;
    }

    std::vector<cloakframe::FaceDetections> movingObjectSequence(
        int frames, float startX, float stepX, float y = 100.0F, float score = 0.9F)
    {
        std::vector<cloakframe::FaceDetections> sequence(frames);
        for (int frame = 0; frame < frames; ++frame)
        {
            sequence[frame].push_back(det(startX + stepX * static_cast<float>(frame), y, score));
        }
        return sequence;
    }

    bool contains(const cv::Rect2f &outer, const cv::Rect2f &inner)
    {
        constexpr float kEpsilon = 0.001F;
        return outer.x <= inner.x + kEpsilon && outer.y <= inner.y + kEpsilon
               && outer.x + outer.width >= inner.x + inner.width - kEpsilon
               && outer.y + outer.height >= inner.y + inner.height - kEpsilon;
    }

    void testConstantVelocitySingleTrack()
    {
        const auto tracks = cloakframe::buildTracks(movingObjectSequence(30, 50.0F, 5.0F));
        assert(tracks.size() == 1);
        assert(tracks[0].boxes.size() == 30);
        assert(tracks[0].firstFrame() == 0);
        assert(tracks[0].lastFrame() == 29);
        for (const auto &box : tracks[0].boxes)
        {
            assert(!box.interpolated);
        }
    }

    void testGapInterpolationFillsMissedFrames()
    {
        auto sequence = movingObjectSequence(20, 50.0F, 5.0F);
        sequence[10].clear();
        sequence[11].clear();
        sequence[12].clear();

        auto tracks = cloakframe::buildTracks(sequence);
        assert(tracks.size() == 1);
        assert(tracks[0].boxAtFrame(11) == nullptr);

        cloakframe::interpolateGaps(tracks[0], 30);
        assert(tracks[0].boxes.size() == 20);

        const auto *filled = tracks[0].boxAtFrame(11);
        assert(filled != nullptr);
        assert(filled->interpolated);
        const float expectedX = 50.0F + 5.0F * 11.0F;
        assert(std::abs(filled->box.x - expectedX) < 1.0F);
    }

    void testGapLargerThanLimitIsReportedAsUncovered()
    {
        auto sequence = movingObjectSequence(20, 50.0F, 5.0F);
        for (int frame = 5; frame <= 12; ++frame)
        {
            sequence[frame].clear();
        }

        auto tracks = cloakframe::buildTracks(sequence);
        assert(tracks.size() == 1);
        const auto uncovered = cloakframe::interpolateGaps(tracks[0], 3);
        assert(tracks[0].boxAtFrame(8) == nullptr);
        assert(uncovered.size() == 1);
        assert(uncovered[0].frameCount() == 8);
        // The gap runs between the last detection before it and the first one after.
        assert(uncovered[0].firstFrame == 5);
        assert(uncovered[0].lastFrame == 12);

        auto reported = cloakframe::buildTracks(sequence);
        cloakframe::TrackPostProcessConfig config;
        config.maxInterpolationGap = 3;
        config.extensionFrames = 0;
        const auto report = cloakframe::postProcessTracks(reported, config, 20);
        assert(report.uncoveredFrames == 8);
        assert(report.uncoveredSpans.size() == 1);
        assert(report.uncoveredSpans[0].firstFrame == 5);
        assert(report.uncoveredSpans[0].lastFrame == 12);
        assert(report.uncoveredSpans[0].trackId == reported[0].id);
    }

    void testGuardRejectedGapIsCoveredConservatively()
    {
        std::vector<cloakframe::FaceDetections> sequence(3);
        sequence[0].push_back(det(100.0F, 100.0F, 0.9F, 40.0F));
        sequence[2].push_back(det(100.0F, 100.0F, 0.9F, 58.0F));

        auto tracks = cloakframe::buildTracks(sequence);
        assert(tracks.size() == 1);
        const auto uncovered = cloakframe::interpolateGaps(tracks[0], 20);
        assert(uncovered.empty());

        const auto *middle = tracks[0].boxAtFrame(1);
        assert(middle != nullptr);
        const auto first = tracks[0].boxAtFrame(0)->box;
        const auto last = tracks[0].boxAtFrame(2)->box;
        assert(middle->box.x <= std::min(first.x, last.x) + 0.001F);
        assert(middle->box.y <= std::min(first.y, last.y) + 0.001F);
        assert(middle->box.br().x >= std::max(first.br().x, last.br().x) - 0.001F);
        assert(middle->box.br().y >= std::max(first.br().y, last.br().y) - 0.001F);
    }

    void testPoseIsTrackedAndInterpolated()
    {
        std::vector<cloakframe::FaceDetections> sequence(3);
        sequence[0].push_back(posedDet(50.0F, 100.0F, 0.0F));
        sequence[2].push_back(posedDet(60.0F, 100.0F, 0.4F));

        auto tracks = cloakframe::buildTracks(sequence);
        assert(tracks.size() == 1);
        assert(tracks[0].boxAtFrame(0)->hasPose);
        assert(std::abs(tracks[0].boxAtFrame(0)->rollRadians) < 0.001F);

        cloakframe::interpolateGaps(tracks[0], 3);
        const auto *middle = tracks[0].boxAtFrame(1);
        assert(middle != nullptr);
        assert(middle->interpolated);
        assert(middle->hasPose);
        assert(std::abs(middle->rollRadians - 0.2F) < 0.001F);
    }

    void testPoseSmoothingReducesAngleJitter()
    {
        cloakframe::Track track;
        const float angles[] = {0.2F, 0.2F, -0.6F, 0.2F, 0.2F};
        for (int frame = 0; frame < 5; ++frame)
        {
            track.boxes.push_back(
                {frame, cv::Rect2f(50.0F, 100.0F, 40.0F, 40.0F), 0.9F, false, angles[frame], true});
        }

        cloakframe::smoothTrack(track, 2);
        assert(track.boxes[2].hasPose);
        assert(std::abs(track.boxes[2].rollRadians) < 0.1F);
    }

    void testLowConfidenceDetectionsExtendButNeverStartTracks()
    {
        auto sequence = movingObjectSequence(15, 50.0F, 5.0F);
        for (int frame = 5; frame <= 7; ++frame)
        {
            sequence[frame][0].score = 0.2F;
        }
        const auto tracks = cloakframe::buildTracks(sequence);
        assert(tracks.size() == 1);
        assert(tracks[0].boxes.size() == 15);
        assert(tracks[0].boxAtFrame(6) != nullptr);

        const auto lowOnly =
            cloakframe::buildTracks(movingObjectSequence(15, 50.0F, 5.0F, 100.0F, 0.2F));
        assert(lowOnly.empty());
    }

    void testLowConfidenceCoastingExpires()
    {
        auto sequence = movingObjectSequence(60, 50.0F, 0.0F, 100.0F, 0.2F);
        for (int frame = 0; frame <= 4; ++frame)
        {
            sequence[frame][0].score = 0.9F;
        }

        const auto tracks = cloakframe::buildTracks(sequence);
        assert(tracks.size() == 1);
        assert(tracks[0].lastFrame() == 34);
    }

    void testWeakTracksKeepTheirConfidentFrames()
    {
        auto weak = movingObjectSequence(20, 50.0F, 5.0F, 100.0F, 0.2F);
        weak[3][0].score = 0.9F;
        weak[4][0].score = 0.9F;
        auto weakTracks = cloakframe::buildBidirectionalTracks(weak);
        assert(weakTracks.size() == 1);
        cloakframe::TrackPostProcessConfig noExtension;
        noExtension.extensionFrames = 0;
        cloakframe::postProcessTracks(weakTracks, noExtension, 20);
        assert(weakTracks.size() == 1);
        assert(weakTracks[0].lowConfidence);
        assert(weakTracks[0].boxAtFrame(3) != nullptr);
        assert(weakTracks[0].boxAtFrame(4) != nullptr);

        auto nothingStrong = cloakframe::buildBidirectionalTracks(weak);
        cloakframe::TrackPostProcessConfig strict;
        strict.strongScoreThreshold = 0.95F;
        const auto report = cloakframe::postProcessTracks(nothingStrong, strict, 20);
        assert(nothingStrong.empty());
        assert(report.droppedTracks == 1);

        auto solid = movingObjectSequence(20, 50.0F, 5.0F);
        auto solidTracks = cloakframe::buildBidirectionalTracks(solid);
        assert(solidTracks.size() == 1);
        cloakframe::postProcessTracks(solidTracks, {}, 20);
        assert(solidTracks.size() == 1);
    }

    void testRetainedLowConfidenceTracksAreMarkedNotDropped()
    {
        auto weak = movingObjectSequence(20, 50.0F, 5.0F, 100.0F, 0.2F);
        weak[3][0].score = 0.9F;
        weak[4][0].score = 0.9F;
        auto tracks = cloakframe::buildBidirectionalTracks(weak);
        auto solid = movingObjectSequence(20, 300.0F, 5.0F, 300.0F);
        auto solidTracks = cloakframe::buildBidirectionalTracks(solid);
        tracks.insert(tracks.end(), solidTracks.begin(), solidTracks.end());
        assert(tracks.size() == 2);

        cloakframe::TrackPostProcessConfig retain;
        retain.retainLowConfidenceTracks = true;
        cloakframe::postProcessTracks(tracks, retain, 20);
        assert(tracks.size() == 2);
        int lowConfidence = 0;
        for (const auto &track : tracks)
        {
            assert(!track.boxes.empty());
            if (track.lowConfidence)
            {
                ++lowConfidence;
                assert(track.boxAtFrame(3) != nullptr);
                assert(track.boxAtFrame(4) != nullptr);
            }
        }
        assert(lowConfidence == 1);
    }

    void testShortStrongBurstIsKept()
    {
        auto burst = movingObjectSequence(2, 50.0F, 5.0F);
        auto tracks = cloakframe::buildBidirectionalTracks(burst);
        assert(tracks.size() == 1);
        cloakframe::postProcessTracks(tracks, {}, 2);
        assert(tracks.size() == 1);
    }

    void testHighConfidenceSingletonIsKept()
    {
        std::vector<cloakframe::FaceDetections> sequence(6);
        sequence[5].push_back(det(50.0F, 100.0F));

        auto tracks =
            cloakframe::buildBidirectionalTracks(sequence, {}, 0.5F, cloakframe::SceneCuts({5}));
        assert(tracks.size() == 1);
        cloakframe::postProcessTracks(tracks, {}, 6, cloakframe::SceneCuts({5}));
        assert(tracks.size() == 1);
        assert(tracks[0].boxAtFrame(5) != nullptr);

        // A stricter policy may call the track low confidence, but the frame the detector
        // was sure about still has to be covered.
        auto strictTracks =
            cloakframe::buildBidirectionalTracks(sequence, {}, 0.5F, cloakframe::SceneCuts({5}));
        cloakframe::TrackPostProcessConfig strict;
        strict.shortTrackMinStrong = 2;
        const auto report =
            cloakframe::postProcessTracks(strictTracks, strict, 6, cloakframe::SceneCuts({5}));
        assert(strictTracks.size() == 1);
        assert(strictTracks[0].lowConfidence);
        assert(strictTracks[0].boxAtFrame(5) != nullptr);
        assert(report.droppedTracks == 0);
    }

    void testMovingCoastingSurvivesLongerThanStatic()
    {
        auto stationary = movingObjectSequence(60, 50.0F, 0.0F, 100.0F, 0.2F);
        for (int frame = 0; frame <= 4; ++frame)
        {
            stationary[frame][0].score = 0.9F;
        }
        const auto stationaryTracks = cloakframe::buildTracks(stationary);
        assert(stationaryTracks.size() == 1);
        assert(stationaryTracks[0].lastFrame() == 34);

        auto moving = movingObjectSequence(60, 50.0F, 6.0F, 100.0F, 0.2F);
        for (int frame = 0; frame <= 4; ++frame)
        {
            moving[frame][0].score = 0.9F;
        }
        const auto movingTracks = cloakframe::buildTracks(moving);
        assert(movingTracks.size() == 1);
        assert(movingTracks[0].lastFrame() == 49);
    }

    void testCrossingObjectsKeepTwoTracks()
    {
        std::vector<cloakframe::FaceDetections> sequence(21);
        for (int frame = 0; frame <= 20; ++frame)
        {
            const auto offset = 10.0F * static_cast<float>(frame);
            sequence[frame].push_back(det(0.0F + offset, 100.0F));
            sequence[frame].push_back(det(200.0F - offset, 100.0F));
        }

        const auto tracks = cloakframe::buildTracks(sequence);
        assert(tracks.size() == 2);
        for (int frame = 0; frame <= 20; ++frame)
        {
            assert(cloakframe::trackRegionsForFrame(tracks, frame).size() == 2);
        }
    }

    void testBidirectionalMergeProducesSingleTrack()
    {
        const auto tracks =
            cloakframe::buildBidirectionalTracks(movingObjectSequence(20, 50.0F, 5.0F));
        assert(tracks.size() == 1);
        assert(tracks[0].boxes.size() == 20);
    }

    void testBackwardPassRecoversLowConfidenceStart()
    {
        auto sequence = movingObjectSequence(20, 50.0F, 5.0F);
        for (int frame = 0; frame <= 4; ++frame)
        {
            sequence[frame][0].score = 0.2F;
        }

        const auto forwardOnly = cloakframe::buildTracks(sequence);
        assert(forwardOnly.size() == 1);
        assert(forwardOnly[0].firstFrame() == 5);

        const auto merged = cloakframe::buildBidirectionalTracks(sequence);
        assert(merged.size() == 1);
        assert(merged[0].firstFrame() == 0);
        assert(merged[0].boxes.size() == 20);
    }

    void testSmoothingNeverUncoversDetections()
    {
        std::vector<cloakframe::FaceDetections> sequence(20);
        for (int frame = 0; frame < 20; ++frame)
        {
            const float jitter = (frame % 2 == 0) ? 3.0F : -3.0F;
            sequence[frame].push_back(
                det(100.0F + 5.0F * static_cast<float>(frame) + jitter, 100.0F + jitter));
        }

        auto tracks = cloakframe::buildTracks(sequence);
        assert(tracks.size() == 1);
        const auto original = tracks[0];

        cloakframe::postProcessTracks(tracks, {}, 20);
        for (const auto &box : original.boxes)
        {
            const auto *processed = tracks[0].boxAtFrame(box.frame);
            assert(processed != nullptr);
            assert(contains(processed->box, box.box));
        }
    }

    void testSmoothingDoesNotInflateMovingBoxes()
    {
        const auto sequence = movingObjectSequence(30, 0.0F, 12.0F);
        auto tracks = cloakframe::buildBidirectionalTracks(sequence);
        cloakframe::postProcessTracks(tracks, {}, 30);
        assert(tracks.size() == 1);

        for (int frame = 0; frame < 30; ++frame)
        {
            const auto *processed = tracks[0].boxAtFrame(frame);
            assert(processed != nullptr);
            if (processed->interpolated)
            {
                continue;
            }
            const auto &detected = sequence[frame][0].box;
            assert(contains(processed->box, detected));
            assert(processed->box.area() <= detected.area() * 1.26F);
        }
    }

    void testExtendTrackEndsClampsToVideoBounds()
    {
        auto sequence = movingObjectSequence(12, 50.0F, 5.0F);
        for (int frame = 0; frame < 5; ++frame)
        {
            sequence[frame].clear();
        }
        for (int frame = 11; frame < 12; ++frame)
        {
            sequence[frame].clear();
        }

        auto tracks = cloakframe::buildTracks(sequence);
        assert(tracks.size() == 1);
        assert(tracks[0].firstFrame() == 5);
        assert(tracks[0].lastFrame() == 10);

        cloakframe::extendTrackEnds(tracks[0], 100, 12);
        assert(tracks[0].firstFrame() == 0);
        assert(tracks[0].lastFrame() == 11);
        assert(tracks[0].boxes.size() == 12);
        assert(tracks[0].boxAtFrame(2)->interpolated);
    }

    void testRegionsForFrameCollectsAllTracks()
    {
        std::vector<cloakframe::FaceDetections> sequence(10);
        for (int frame = 0; frame < 10; ++frame)
        {
            sequence[frame].push_back(det(50.0F, 50.0F));
            sequence[frame].push_back(det(300.0F, 200.0F));
        }
        const auto tracks = cloakframe::buildTracks(sequence);
        assert(tracks.size() == 2);
        assert(cloakframe::trackRegionsForFrame(tracks, 5).size() == 2);
        assert(cloakframe::trackRegionsForFrame(tracks, 42).empty());
    }

    void testSceneCutsContainer()
    {
        const cloakframe::SceneCuts cuts({15, 5, 5, 0, -3});
        assert(cuts.frames() == std::vector<int>({5, 15}));
        assert(cuts.isCut(5));
        assert(!cuts.isCut(6));
        assert(cuts.spansCut(4, 5));
        assert(cuts.spansCut(0, 19));
        assert(cuts.spansCut(14, 16));
        assert(!cuts.spansCut(5, 14));
        assert(!cuts.spansCut(15, 19));
        assert(!cloakframe::SceneCuts{}.spansCut(0, 1000));

        const auto reversed = cuts.reversed(20);
        assert(reversed.frames() == std::vector<int>({5, 15}));
        const auto asymmetric = cloakframe::SceneCuts({7}).reversed(20);
        assert(asymmetric.frames() == std::vector<int>({13}));
    }

    void testTrackerSplitsTracksAtSceneCut()
    {
        const auto sequence = movingObjectSequence(20, 50.0F, 0.0F);

        const auto unbroken = cloakframe::buildTracks(sequence);
        assert(unbroken.size() == 1);

        const auto split = cloakframe::buildTracks(sequence, {}, cloakframe::SceneCuts({10}));
        assert(split.size() == 2);
        for (const auto &track : split)
        {
            assert(track.firstFrame() >= 10 || track.lastFrame() < 10);
        }
    }

    void testNoInterpolationAcrossSceneCut()
    {
        cloakframe::Track track;
        track.id = 1;
        track.boxes.push_back({5, cv::Rect2f(50.0F, 50.0F, 40.0F, 40.0F), 0.9F, false});
        track.boxes.push_back({15, cv::Rect2f(50.0F, 50.0F, 40.0F, 40.0F), 0.9F, false});

        auto gated = track;
        cloakframe::interpolateGaps(gated, 30, cloakframe::SceneCuts({10}));
        assert(gated.boxes.size() == 2);

        cloakframe::interpolateGaps(track, 30);
        assert(track.boxes.size() == 11);
    }

    void testExtendTrackEndsStopsAtSceneCut()
    {
        cloakframe::Track track;
        track.id = 1;
        for (int frame = 10; frame <= 15; ++frame)
        {
            track.boxes.push_back({frame, cv::Rect2f(50.0F, 50.0F, 40.0F, 40.0F), 0.9F, false});
        }

        cloakframe::extendTrackEnds(track, 5, 30, cloakframe::SceneCuts({10, 16}));
        assert(track.firstFrame() == 10);
        assert(track.lastFrame() == 15);
    }

    void testBidirectionalTracksRespectSceneCuts()
    {
        const auto sequence = movingObjectSequence(20, 50.0F, 5.0F);
        const auto tracks =
            cloakframe::buildBidirectionalTracks(sequence, {}, 0.5F, cloakframe::SceneCuts({10}));
        assert(tracks.size() == 2);
        for (const auto &track : tracks)
        {
            assert(track.firstFrame() >= 10 || track.lastFrame() < 10);
        }
        for (int frame = 0; frame < 20; ++frame)
        {
            assert(cloakframe::trackRegionsForFrame(tracks, frame).size() == 1);
        }
    }

    cv::Mat gradientFrame(bool horizontal, int width = 480, int height = 270)
    {
        cv::Mat frame(height, width, CV_8UC1);
        for (int y = 0; y < height; ++y)
        {
            for (int x = 0; x < width; ++x)
            {
                const int value = horizontal ? x * 255 / (width - 1) : y * 255 / (height - 1);
                frame.at<unsigned char>(y, x) = static_cast<unsigned char>(value);
            }
        }
        return frame;
    }

    void testSceneCutDetectorFindsHardCut()
    {
        const auto sceneA = gradientFrame(true);
        const auto sceneB = gradientFrame(false);

        cloakframe::SceneCutDetector detector;
        for (int frame = 0; frame < 10; ++frame)
        {
            detector.push(sceneA);
        }
        for (int frame = 0; frame < 5; ++frame)
        {
            detector.push(sceneB);
        }
        const auto cuts = detector.finish();
        assert(cuts.frames() == std::vector<int>({10}));
    }

    void testSceneCutDetectorIgnoresFlash()
    {
        const auto scene = gradientFrame(true);
        const cv::Mat flash(270, 480, CV_8UC1, cv::Scalar(255));

        cloakframe::SceneCutDetector detector;
        for (int frame = 0; frame < 10; ++frame)
        {
            detector.push(scene);
        }
        detector.push(flash);
        for (int frame = 0; frame < 10; ++frame)
        {
            detector.push(scene);
        }
        assert(detector.finish().empty());
    }

    void testSceneCutDetectorIgnoresStaticScene()
    {
        const auto scene = gradientFrame(true);
        cloakframe::SceneCutDetector detector;
        for (int frame = 0; frame < 30; ++frame)
        {
            detector.push(scene);
        }
        assert(detector.finish().empty());
    }

    void testSceneCutDetectorCommitsTrailingCandidate()
    {
        const auto sceneA = gradientFrame(true);
        const auto sceneB = gradientFrame(false);

        cloakframe::SceneCutDetector detector;
        for (int frame = 0; frame < 5; ++frame)
        {
            detector.push(sceneA);
        }
        detector.push(sceneB);
        const auto cuts = detector.finish();
        assert(cuts.frames() == std::vector<int>({5}));
    }

    void testSizeJumpStartsNewTrackInsteadOfAssociating()
    {
        std::vector<cloakframe::FaceDetections> sequence(8);
        for (int frame = 0; frame < 4; ++frame)
        {
            sequence[frame].push_back({cv::Rect2f(100.0F, 100.0F, 100.0F, 100.0F), 0.9F});
        }
        for (int frame = 4; frame < 8; ++frame)
        {
            sequence[frame].push_back({cv::Rect2f(65.0F, 65.0F, 170.0F, 170.0F), 0.9F});
        }
        const auto tracks = cloakframe::buildTracks(sequence);
        assert(tracks.size() == 2);
        for (const auto &track : tracks)
        {
            for (const auto &tracked : track.boxes)
            {
                assert(tracked.box.width == track.boxes.front().box.width);
            }
        }
    }

    void testGradualGrowthKeepsOneTrack()
    {
        std::vector<cloakframe::FaceDetections> sequence(10);
        float size = 40.0F;
        for (int frame = 0; frame < 10; ++frame)
        {
            const float half = size * 0.5F;
            sequence[frame].push_back({cv::Rect2f(150.0F - half, 150.0F - half, size, size), 0.9F});
            size *= 1.1F;
        }
        assert(cloakframe::buildTracks(sequence).size() == 1);
    }

    void testInterpolationCoversSizeJumpConservatively()
    {
        cloakframe::Track track;
        track.boxes.push_back({0, cv::Rect2f(100.0F, 100.0F, 40.0F, 40.0F), 0.9F, false});
        track.boxes.push_back({5, cv::Rect2f(60.0F, 60.0F, 160.0F, 160.0F), 0.9F, false});
        const auto first = track.boxes.front().box;
        const auto last = track.boxes.back().box;

        const auto uncovered = cloakframe::interpolateGaps(track, 10);
        assert(uncovered.empty());
        assert(track.boxes.size() == 6);
        for (const auto &tracked : track.boxes)
        {
            if (!tracked.interpolated)
            {
                continue;
            }
            assert(contains(tracked.box, first));
            assert(contains(tracked.box, last));
        }
    }

    void testSmoothingDoesNotInflateInterpolatedBoxes()
    {
        cloakframe::Track track;
        track.boxes.push_back({0, cv::Rect2f(0.0F, 0.0F, 200.0F, 200.0F), 0.9F, false});
        track.boxes.push_back({1, cv::Rect2f(75.0F, 75.0F, 50.0F, 50.0F), 0.9F, true});
        track.boxes.push_back({2, cv::Rect2f(0.0F, 0.0F, 200.0F, 200.0F), 0.9F, false});

        cloakframe::smoothTrack(track, 2);

        const float maxArea = 50.0F * 50.0F * 1.25F;
        assert(track.boxes[1].box.area() <= maxArea + 0.001F);
    }

    void testTrackingSafetyLimitsRejectExcessiveData()
    {
        std::vector<cloakframe::FaceDetections> crowded(1);
        for (int index = 0; index < 1025; ++index)
        {
            crowded[0].push_back(det(static_cast<float>(index), 100.0F));
        }
        bool crowdedRejected = false;
        try
        {
            (void)cloakframe::buildTracks(crowded);
        }
        catch (const std::length_error &)
        {
            crowdedRejected = true;
        }
        assert(crowdedRejected);

        constexpr int frameCount = 65'540;
        std::vector<cloakframe::FaceDetections> singletons(frameCount);
        std::vector<int> cuts;
        cuts.reserve(frameCount);
        for (int frame = 0; frame < frameCount; ++frame)
        {
            singletons[frame].push_back(det(50.0F, 100.0F));
            cuts.push_back(frame);
        }
        bool trackCountRejected = false;
        try
        {
            (void)cloakframe::buildTracks(singletons, {}, cloakframe::SceneCuts(cuts));
        }
        catch (const std::length_error &)
        {
            trackCountRejected = true;
        }
        assert(trackCountRejected);
    }

    void testTrackingCancellationGuard()
    {
        const auto sequence = movingObjectSequence(120, 50.0F, 2.0F);
        int checks = 0;
        bool buildCancelled = false;
        try
        {
            (void)cloakframe::buildBidirectionalTracks(sequence,
                {},
                0.5F,
                {},
                [&checks]
                {
                    return ++checks < 5;
                });
        }
        catch (const cloakframe::TrackingCancelled &)
        {
            buildCancelled = true;
        }
        assert(buildCancelled);

        auto tracks = cloakframe::buildBidirectionalTracks(sequence);
        bool postProcessCancelled = false;
        try
        {
            cloakframe::postProcessTracks(tracks,
                {},
                120,
                {},
                []
                {
                    return false;
                });
        }
        catch (const cloakframe::TrackingCancelled &)
        {
            postProcessCancelled = true;
        }
        assert(postProcessCancelled);
    }
}

int main()
{
    testManualVideoTrackInterpolationAndBounds();
    testConstantVelocitySingleTrack();
    testGapInterpolationFillsMissedFrames();
    testGapLargerThanLimitIsReportedAsUncovered();
    testGuardRejectedGapIsCoveredConservatively();
    testPoseIsTrackedAndInterpolated();
    testPoseSmoothingReducesAngleJitter();
    testLowConfidenceDetectionsExtendButNeverStartTracks();
    testLowConfidenceCoastingExpires();
    testWeakTracksKeepTheirConfidentFrames();
    testRetainedLowConfidenceTracksAreMarkedNotDropped();
    testShortStrongBurstIsKept();
    testHighConfidenceSingletonIsKept();
    testMovingCoastingSurvivesLongerThanStatic();
    testCrossingObjectsKeepTwoTracks();
    testBidirectionalMergeProducesSingleTrack();
    testBackwardPassRecoversLowConfidenceStart();
    testSmoothingNeverUncoversDetections();
    testSmoothingDoesNotInflateMovingBoxes();
    testExtendTrackEndsClampsToVideoBounds();
    testRegionsForFrameCollectsAllTracks();
    testSceneCutsContainer();
    testTrackerSplitsTracksAtSceneCut();
    testNoInterpolationAcrossSceneCut();
    testExtendTrackEndsStopsAtSceneCut();
    testBidirectionalTracksRespectSceneCuts();
    testSceneCutDetectorFindsHardCut();
    testSceneCutDetectorIgnoresFlash();
    testSceneCutDetectorIgnoresStaticScene();
    testSceneCutDetectorCommitsTrailingCandidate();
    testSizeJumpStartsNewTrackInsteadOfAssociating();
    testGradualGrowthKeepsOneTrack();
    testInterpolationCoversSizeJumpConservatively();
    testSmoothingDoesNotInflateInterpolatedBoxes();
    testTrackingSafetyLimitsRejectExcessiveData();
    testTrackingCancellationGuard();
    std::puts("tracking tests passed");
    return 0;
}

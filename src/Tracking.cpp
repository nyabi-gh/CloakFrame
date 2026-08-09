#include "cloakframe/Tracking.hpp"

#include "cloakframe/DetectionGeometry.hpp"

#include <algorithm>
#include <cmath>
#include <stdexcept>
#include <utility>

namespace cloakframe
{
    namespace
    {
        constexpr std::size_t kMaxDetectionsPerFrame = 1024;
        constexpr std::size_t kMaxActiveTracks = 4096;
        constexpr std::size_t kMaxTrackCount = 65'536;
        constexpr std::size_t kMaxTrackedBoxesPerPass = 8'000'000;
        constexpr std::size_t kMaxBidirectionalTrackedBoxes = 16'000'000;
        constexpr std::size_t kMaxFinalTrackedBoxes = 8'000'000;
        constexpr std::size_t kMaxMatchComparisons = std::size_t{1} * 1024 * 1024;
        constexpr std::size_t kMaxTrackMergeComparisons = std::size_t{4} * 1024 * 1024;
        constexpr std::size_t kCancellationCheckInterval = 16'384;

        void requireTrackingContinue(const TrackingContinueGuard &continueGuard)
        {
            if (continueGuard && !continueGuard())
            {
                throw TrackingCancelled{};
            }
        }

        void periodicallyRequireTrackingContinue(
            const TrackingContinueGuard &continueGuard, std::size_t &operations)
        {
            ++operations;
            if ((operations & (kCancellationCheckInterval - 1)) == 0)
            {
                requireTrackingContinue(continueGuard);
            }
        }

        std::size_t trackedBoxCount(const std::vector<Track> &tracks, const std::size_t maximum)
        {
            std::size_t count = 0;
            for (const auto &track : tracks)
            {
                if (count > maximum || track.boxes.size() > maximum - count)
                {
                    throw std::length_error("Tracking data exceeds the safety limit.");
                }
                count += track.boxes.size();
            }
            return count;
        }

        cv::Point2f boxCenter(const cv::Rect2f &box)
        {
            return {box.x + box.width * 0.5F, box.y + box.height * 0.5F};
        }

        cv::Rect2f boxWithCenter(const cv::Rect2f &box, const cv::Point2f &center)
        {
            return {
                center.x - box.width * 0.5F, center.y - box.height * 0.5F, box.width, box.height};
        }

        cv::Rect2f boxUnion(const cv::Rect2f &a, const cv::Rect2f &b)
        {
            const float left = std::min(a.x, b.x);
            const float top = std::min(a.y, b.y);
            const float right = std::max(a.x + a.width, b.x + b.width);
            const float bottom = std::max(a.y + a.height, b.y + b.height);
            return {left, top, right - left, bottom - top};
        }

        cv::Rect2f lerpBox(const cv::Rect2f &a, const cv::Rect2f &b, float t)
        {
            return {a.x + (b.x - a.x) * t,
                a.y + (b.y - a.y) * t,
                a.width + (b.width - a.width) * t,
                a.height + (b.height - a.height) * t};
        }

        float lerpAngle(const float from, const float to, const float t)
        {
            constexpr float kTwoPi = 6.2831853072F;
            return from + std::remainder(to - from, kTwoPi) * t;
        }

        bool gapMotionTooFast(const cv::Rect2f &a, const cv::Rect2f &b, int gap)
        {
            constexpr float kMaxShiftPerFrame = 0.75F;
            const cv::Point2f from = boxCenter(a);
            const cv::Point2f to = boxCenter(b);
            const float distance = std::hypot(to.x - from.x, to.y - from.y);
            const float perFrame = distance / static_cast<float>(gap + 1);
            const float scale = std::max({a.width, a.height, b.width, b.height});
            return scale > 0.0F && perFrame > kMaxShiftPerFrame * scale;
        }

        bool sizeJumpTooLarge(const cv::Rect2f &from, const cv::Rect2f &to, int gap)
        {
            constexpr float kMaxDimensionRatio = 1.6F;
            constexpr float kMaxAreaRatio = 2.0F;
            constexpr float kPerFrameSlack = 0.05F;
            constexpr float kMaxDimensionRatioCap = 2.4F;
            constexpr float kMaxAreaRatioCap = 4.5F;

            if (from.width <= 0.0F || from.height <= 0.0F || to.width <= 0.0F || to.height <= 0.0F)
            {
                return true;
            }
            const float slack = 1.0F + kPerFrameSlack * static_cast<float>(std::max(0, gap - 1));
            const float dimensionLimit =
                std::min(kMaxDimensionRatioCap, kMaxDimensionRatio * slack);
            const float areaLimit = std::min(kMaxAreaRatioCap, kMaxAreaRatio * slack * slack);
            const float widthRatio = std::max(to.width / from.width, from.width / to.width);
            const float heightRatio = std::max(to.height / from.height, from.height / to.height);
            const float areaRatio = std::max(to.area() / from.area(), from.area() / to.area());
            return widthRatio > dimensionLimit || heightRatio > dimensionLimit
                   || areaRatio > areaLimit;
        }

        struct Match
        {
            size_t trackIndex;
            size_t detectionIndex;
            float iou;
        };

        std::vector<Match> greedyIouMatches(const std::vector<cv::Rect2f> &predictions,
            const std::vector<cv::Rect2f> &lastBoxes,
            const std::vector<int> &gaps,
            const std::vector<size_t> &trackIndices,
            const FaceDetections &detections,
            const std::vector<size_t> &detectionIndices,
            float iouThreshold,
            const TrackingContinueGuard &continueGuard)
        {
            if (!detectionIndices.empty()
                && trackIndices.size() > kMaxMatchComparisons / detectionIndices.size())
            {
                throw std::length_error("Tracking data exceeds the safety limit.");
            }
            std::vector<Match> candidates;
            std::size_t comparisons = 0;
            for (const size_t trackIndex : trackIndices)
            {
                for (const size_t detectionIndex : detectionIndices)
                {
                    periodicallyRequireTrackingContinue(continueGuard, comparisons);
                    const float iou = intersectionOverUnion(
                        predictions[trackIndex], detections[detectionIndex].box);
                    if (iou >= iouThreshold
                        && !sizeJumpTooLarge(lastBoxes[trackIndex],
                            detections[detectionIndex].box,
                            gaps[trackIndex]))
                    {
                        candidates.push_back({trackIndex, detectionIndex, iou});
                    }
                }
            }

            std::ranges::sort(candidates,
                [](const Match &a, const Match &b)
                {
                    return a.iou > b.iou;
                });

            std::vector<Match> matches;
            std::vector<bool> trackUsed(predictions.size(), false);
            std::vector<bool> detectionUsed(detections.size(), false);
            std::size_t selectionOperations = 0;
            for (const auto &candidate : candidates)
            {
                periodicallyRequireTrackingContinue(continueGuard, selectionOperations);
                if (trackUsed[candidate.trackIndex] || detectionUsed[candidate.detectionIndex])
                {
                    continue;
                }
                trackUsed[candidate.trackIndex] = true;
                detectionUsed[candidate.detectionIndex] = true;
                matches.push_back(candidate);
            }
            return matches;
        }
    }

    int Track::firstFrame() const
    {
        return boxes.empty() ? 0 : boxes.front().frame;
    }

    int Track::lastFrame() const
    {
        return boxes.empty() ? 0 : boxes.back().frame;
    }

    const TrackedBox *Track::boxAtFrame(int frame) const
    {
        const auto it = std::ranges::lower_bound(boxes, frame, {}, &TrackedBox::frame);
        if (it != boxes.end() && it->frame == frame)
        {
            return &*it;
        }
        return nullptr;
    }

    cv::Rect2f ByteTracker::ActiveTrack::predictedBox(int frame) const
    {
        const auto &last = track.boxes.back();
        const float dt = static_cast<float>(frame - lastFrame);
        cv::Point2f shift = velocity * dt;
        const float maxShift = std::max(last.box.width, last.box.height);
        const float shiftLength = std::hypot(shift.x, shift.y);
        if (shiftLength > maxShift)
        {
            shift *= maxShift / shiftLength;
        }
        const cv::Point2f center = boxCenter(last.box) + shift;
        return boxWithCenter(last.box, center);
    }

    ByteTracker::ByteTracker(TrackerConfig config, SceneCuts cuts)
        : config_(config)
        , cuts_(std::move(cuts))
    {
    }

    void ByteTracker::extendTrack(ActiveTrack &active, int frame, const FaceDetection &detection)
    {
        if (boxCount_ >= kMaxTrackedBoxesPerPass)
        {
            throw std::length_error("Tracking data exceeds the safety limit.");
        }
        const auto &last = active.track.boxes.back();
        const float dt = static_cast<float>(frame - active.lastFrame);
        if (dt > 0.0F)
        {
            const cv::Point2f observed = (boxCenter(detection.box) - boxCenter(last.box)) / dt;
            active.velocity =
                active.velocity * (1.0F - config_.velocityBlend) + observed * config_.velocityBlend;
        }
        active.track.boxes.push_back({frame,
            detection.box,
            detection.score,
            false,
            detection.rollRadians,
            hasValidFacePose(detection)});
        ++boxCount_;
        active.lastFrame = frame;
        if (detection.score >= config_.highScoreThreshold)
        {
            active.lastHighScoreFrame = frame;
        }
    }

    void ByteTracker::update(
        int frame, const FaceDetections &detections, const TrackingContinueGuard &continueGuard)
    {
        requireTrackingContinue(continueGuard);
        if (detections.size() > kMaxDetectionsPerFrame || active_.size() > kMaxActiveTracks)
        {
            throw std::length_error("Tracking data exceeds the safety limit.");
        }
        if (cuts_.isCut(frame))
        {
            for (auto &active : active_)
            {
                finished_.push_back(std::move(active.track));
            }
            active_.clear();
        }

        std::erase_if(active_,
            [&](ActiveTrack &active)
            {
                const cv::Rect2f &lastBox = active.track.boxes.back().box;
                const float scale = std::max(lastBox.width, lastBox.height);
                const float speed = std::hypot(active.velocity.x, active.velocity.y);
                const bool moving = scale > 0.0F && speed > config_.coastMotionThreshold * scale;
                const int coastLimit = moving ? config_.maxFramesSinceHighScoreMoving
                                              : config_.maxFramesSinceHighScore;
                if (frame - active.lastFrame > config_.maxFramesLost
                    || frame - active.lastHighScoreFrame > coastLimit)
                {
                    finished_.push_back(std::move(active.track));
                    return true;
                }
                return false;
            });

        std::vector<cv::Rect2f> predictions(active_.size());
        std::vector<cv::Rect2f> lastBoxes(active_.size());
        std::vector<int> gaps(active_.size());
        std::vector<size_t> unmatchedTracks(active_.size());
        for (size_t i = 0; i < active_.size(); ++i)
        {
            predictions[i] = active_[i].predictedBox(frame);
            lastBoxes[i] = active_[i].track.boxes.back().box;
            gaps[i] = frame - active_[i].lastFrame;
            unmatchedTracks[i] = i;
        }

        std::vector<size_t> highDetections;
        std::vector<size_t> lowDetections;
        for (size_t i = 0; i < detections.size(); ++i)
        {
            if (detections[i].score >= config_.highScoreThreshold)
            {
                highDetections.push_back(i);
            }
            else if (detections[i].score >= config_.lowScoreThreshold)
            {
                lowDetections.push_back(i);
            }
        }

        const auto removeMatched = [](std::vector<size_t> &indices, size_t value)
        {
            std::erase(indices, value);
        };

        const auto highMatches = greedyIouMatches(predictions,
            lastBoxes,
            gaps,
            unmatchedTracks,
            detections,
            highDetections,
            config_.iouThreshold,
            continueGuard);
        for (const auto &match : highMatches)
        {
            extendTrack(active_[match.trackIndex], frame, detections[match.detectionIndex]);
            removeMatched(unmatchedTracks, match.trackIndex);
            removeMatched(highDetections, match.detectionIndex);
        }

        const auto lowMatches = greedyIouMatches(predictions,
            lastBoxes,
            gaps,
            unmatchedTracks,
            detections,
            lowDetections,
            config_.iouThreshold,
            continueGuard);
        for (const auto &match : lowMatches)
        {
            extendTrack(active_[match.trackIndex], frame, detections[match.detectionIndex]);
            removeMatched(unmatchedTracks, match.trackIndex);
        }

        for (const size_t detectionIndex : highDetections)
        {
            if (active_.size() >= kMaxActiveTracks
                || active_.size() + finished_.size() >= kMaxTrackCount
                || boxCount_ >= kMaxTrackedBoxesPerPass)
            {
                throw std::length_error("Tracking data exceeds the safety limit.");
            }
            ActiveTrack fresh;
            fresh.track.id = nextId_++;
            fresh.track.boxes.push_back({frame,
                detections[detectionIndex].box,
                detections[detectionIndex].score,
                false,
                detections[detectionIndex].rollRadians,
                hasValidFacePose(detections[detectionIndex])});
            fresh.lastFrame = frame;
            fresh.lastHighScoreFrame = frame;
            active_.push_back(std::move(fresh));
            ++boxCount_;
        }
    }

    std::vector<Track> ByteTracker::finish()
    {
        for (auto &active : active_)
        {
            finished_.push_back(std::move(active.track));
        }
        active_.clear();

        std::vector<Track> result = std::move(finished_);
        finished_ = {};
        std::erase_if(result,
            [](const Track &track)
            {
                return track.boxes.empty();
            });
        std::ranges::sort(result, {}, &Track::id);
        return result;
    }

    std::vector<Track> buildTracks(const std::vector<FaceDetections> &frameDetections,
        const TrackerConfig &config,
        const SceneCuts &cuts,
        const TrackingContinueGuard &continueGuard)
    {
        ByteTracker tracker(config, cuts);
        for (size_t frame = 0; frame < frameDetections.size(); ++frame)
        {
            tracker.update(static_cast<int>(frame), frameDetections[frame], continueGuard);
        }
        requireTrackingContinue(continueGuard);
        return tracker.finish();
    }

    std::vector<Track> buildBidirectionalTracks(const std::vector<FaceDetections> &frameDetections,
        const TrackerConfig &config,
        float mergeIouThreshold,
        const SceneCuts &cuts,
        const TrackingContinueGuard &continueGuard)
    {
        auto forward = buildTracks(frameDetections, config, cuts, continueGuard);

        const int frameCount = static_cast<int>(frameDetections.size());
        ByteTracker backwardTracker(config, cuts.reversed(frameCount));
        for (int frame = 0; frame < frameCount; ++frame)
        {
            backwardTracker.update(frame,
                frameDetections[static_cast<std::size_t>(frameCount - frame - 1)],
                continueGuard);
        }
        requireTrackingContinue(continueGuard);
        auto backward = backwardTracker.finish();
        if (!backward.empty() && forward.size() > kMaxTrackMergeComparisons / backward.size())
        {
            throw std::length_error("Tracking data exceeds the safety limit.");
        }
        std::size_t totalBoxes = trackedBoxCount(forward, kMaxFinalTrackedBoxes);
        const auto backwardBoxes = trackedBoxCount(backward, kMaxTrackedBoxesPerPass);
        if (totalBoxes > kMaxBidirectionalTrackedBoxes
            || backwardBoxes > kMaxBidirectionalTrackedBoxes - totalBoxes)
        {
            throw std::length_error("Tracking data exceeds the safety limit.");
        }
        std::size_t reframeOperations = 0;
        for (auto &track : backward)
        {
            for (auto &box : track.boxes)
            {
                periodicallyRequireTrackingContinue(continueGuard, reframeOperations);
                box.frame = frameCount - 1 - box.frame;
            }
            std::ranges::reverse(track.boxes);
        }

        int nextId = 1;
        for (const auto &track : forward)
        {
            nextId = std::max(nextId, track.id + 1);
        }

        for (auto &candidate : backward)
        {
            requireTrackingContinue(continueGuard);
            Track *bestMatch = nullptr;
            float bestScore = 0.0F;
            std::size_t mergeOperations = 0;
            for (auto &existing : forward)
            {
                int sharedFrames = 0;
                float iouSum = 0.0F;
                for (const auto &box : candidate.boxes)
                {
                    periodicallyRequireTrackingContinue(continueGuard, mergeOperations);
                    if (const auto *other = existing.boxAtFrame(box.frame))
                    {
                        ++sharedFrames;
                        iouSum += intersectionOverUnion(box.box, other->box);
                    }
                }
                if (sharedFrames == 0)
                {
                    continue;
                }
                const float meanIou = iouSum / static_cast<float>(sharedFrames);
                if (meanIou >= mergeIouThreshold && meanIou > bestScore)
                {
                    bestScore = meanIou;
                    bestMatch = &existing;
                }
            }

            if (bestMatch == nullptr)
            {
                if (forward.size() >= kMaxTrackCount || totalBoxes > kMaxFinalTrackedBoxes
                    || candidate.boxes.size() > kMaxFinalTrackedBoxes - totalBoxes)
                {
                    throw std::length_error("Tracking data exceeds the safety limit.");
                }
                totalBoxes += candidate.boxes.size();
                candidate.id = nextId++;
                forward.push_back(std::move(candidate));
                continue;
            }

            std::vector<TrackedBox> missing;
            for (const auto &box : candidate.boxes)
            {
                periodicallyRequireTrackingContinue(continueGuard, mergeOperations);
                if (bestMatch->boxAtFrame(box.frame) == nullptr)
                {
                    missing.push_back(box);
                }
            }
            if (totalBoxes > kMaxFinalTrackedBoxes
                || missing.size() > kMaxFinalTrackedBoxes - totalBoxes)
            {
                throw std::length_error("Tracking data exceeds the safety limit.");
            }
            totalBoxes += missing.size();
            bestMatch->boxes.insert(bestMatch->boxes.end(), missing.begin(), missing.end());
            std::ranges::sort(bestMatch->boxes, {}, &TrackedBox::frame);
        }

        return forward;
    }

    std::vector<UncoveredSpan> interpolateGaps(
        Track &track, int maxGap, const SceneCuts &cuts, const TrackingContinueGuard &continueGuard)
    {
        if (track.boxes.size() < 2 || maxGap < 1)
        {
            return {};
        }
        std::vector<UncoveredSpan> uncovered;

        std::vector<TrackedBox> filled;
        filled.reserve(track.boxes.size());
        const auto append = [&](const TrackedBox &box)
        {
            if (filled.size() >= kMaxFinalTrackedBoxes)
            {
                throw std::length_error("Tracking data exceeds the safety limit.");
            }
            filled.push_back(box);
        };
        std::size_t operations = 0;
        for (size_t i = 0; i + 1 < track.boxes.size(); ++i)
        {
            periodicallyRequireTrackingContinue(continueGuard, operations);
            const auto &current = track.boxes[i];
            const auto &next = track.boxes[i + 1];
            append(current);

            const int gap = next.frame - current.frame - 1;
            if (gap < 1)
            {
                continue;
            }
            // A cut means the subject legitimately left; anything else is a hole inside
            // one track's own span and must be covered or reported.
            if (cuts.spansCut(current.frame, next.frame))
            {
                continue;
            }
            if (gap > maxGap)
            {
                uncovered.push_back({track.id, current.frame + 1, next.frame - 1});
                continue;
            }

            // Motion and size guards mean the path between the anchors is unknown, so
            // cover the gap with the union of both ends rather than leaving it bare.
            const bool unreliable = gapMotionTooFast(current.box, next.box, gap)
                                    || sizeJumpTooLarge(current.box, next.box, gap);
            const cv::Rect2f conservative = current.box | next.box;
            for (int step = 1; step <= gap; ++step)
            {
                const float t = static_cast<float>(step) / static_cast<float>(gap + 1);
                TrackedBox interpolated{current.frame + step,
                    unreliable ? conservative : lerpBox(current.box, next.box, t),
                    std::min(current.score, next.score),
                    true};
                if (!unreliable && isValidFacePose(current.rollRadians, current.hasPose)
                    && isValidFacePose(next.rollRadians, next.hasPose))
                {
                    interpolated.rollRadians = lerpAngle(current.rollRadians, next.rollRadians, t);
                    interpolated.hasPose = isValidFacePose(interpolated.rollRadians, true);
                }
                append(interpolated);
            }
        }
        append(track.boxes.back());
        track.boxes = std::move(filled);
        return uncovered;
    }

    void smoothTrack(Track &track, int radius, const TrackingContinueGuard &continueGuard)
    {
        if (radius < 1 || track.boxes.size() < 3)
        {
            return;
        }

        const auto original = track.boxes;
        std::size_t operations = 0;
        for (size_t i = 0; i < track.boxes.size(); ++i)
        {
            periodicallyRequireTrackingContinue(continueGuard, operations);
            const size_t begin =
                i >= static_cast<size_t>(radius) ? i - static_cast<size_t>(radius) : 0;
            const size_t end = std::min(track.boxes.size() - 1, i + static_cast<size_t>(radius));

            cv::Rect2f accumulated{0.0F, 0.0F, 0.0F, 0.0F};
            int count = 0;
            float rollSin = 0.0F;
            float rollCos = 0.0F;
            int rollCount = 0;
            for (size_t j = begin; j <= end; ++j)
            {
                const int frameDelta = original[j].frame - original[i].frame;
                const int indexDelta = static_cast<int>(j) - static_cast<int>(i);
                if (frameDelta != indexDelta)
                {
                    continue;
                }
                accumulated.x += original[j].box.x;
                accumulated.y += original[j].box.y;
                accumulated.width += original[j].box.width;
                accumulated.height += original[j].box.height;
                ++count;
                if (isValidFacePose(original[j].rollRadians, original[j].hasPose))
                {
                    rollSin += std::sin(original[j].rollRadians);
                    rollCos += std::cos(original[j].rollRadians);
                    ++rollCount;
                }
            }
            if (count < 2)
            {
                continue;
            }
            if (rollCount >= 2)
            {
                const float smoothedRoll = std::atan2(rollSin, rollCos);
                if (isValidFacePose(smoothedRoll, true))
                {
                    track.boxes[i].rollRadians = smoothedRoll;
                    track.boxes[i].hasPose = true;
                }
            }
            const float inverse = 1.0F / static_cast<float>(count);
            const cv::Rect2f smoothed{accumulated.x * inverse,
                accumulated.y * inverse,
                accumulated.width * inverse,
                accumulated.height * inverse};
            constexpr float kMaxSmoothingAreaGrowth = 1.25F;
            const float originalArea = original[i].box.area();
            if (original[i].interpolated)
            {
                track.boxes[i].box =
                    (originalArea > 0.0F
                        && smoothed.area() > originalArea * kMaxSmoothingAreaGrowth)
                        ? original[i].box
                        : smoothed;
                continue;
            }
            const cv::Rect2f covered = boxUnion(smoothed, original[i].box);
            track.boxes[i].box =
                (originalArea > 0.0F && covered.area() > originalArea * kMaxSmoothingAreaGrowth)
                    ? original[i].box
                    : covered;
        }
    }

    void extendTrackEnds(Track &track,
        int frames,
        int frameCount,
        const SceneCuts &cuts,
        const TrackingContinueGuard &continueGuard)
    {
        if (track.boxes.empty() || frames < 1 || frameCount < 1)
        {
            return;
        }

        std::vector<TrackedBox> prefix;
        const auto &first = track.boxes.front();
        for (int frame = std::max(0, first.frame - frames); frame < first.frame; ++frame)
        {
            requireTrackingContinue(continueGuard);
            if (cuts.spansCut(frame, first.frame))
            {
                continue;
            }
            if (track.boxes.size() + prefix.size() >= kMaxFinalTrackedBoxes)
            {
                throw std::length_error("Tracking data exceeds the safety limit.");
            }
            prefix.push_back(
                {frame, first.box, first.score, true, first.rollRadians, first.hasPose});
        }

        std::vector<TrackedBox> suffix;
        const auto &last = track.boxes.back();
        const int lastAllowed = frameCount - 1;
        for (int frame = last.frame + 1; frame <= std::min(lastAllowed, last.frame + frames);
            ++frame)
        {
            requireTrackingContinue(continueGuard);
            if (cuts.spansCut(last.frame, frame))
            {
                break;
            }
            if (track.boxes.size() + prefix.size() + suffix.size() >= kMaxFinalTrackedBoxes)
            {
                throw std::length_error("Tracking data exceeds the safety limit.");
            }
            suffix.push_back({frame, last.box, last.score, true, last.rollRadians, last.hasPose});
        }

        track.boxes.insert(track.boxes.begin(), prefix.begin(), prefix.end());
        track.boxes.insert(track.boxes.end(), suffix.begin(), suffix.end());
    }

    TrackCoverageReport postProcessTracks(std::vector<Track> &tracks,
        const TrackPostProcessConfig &config,
        int frameCount,
        const SceneCuts &cuts,
        const TrackingContinueGuard &continueGuard)
    {
        TrackCoverageReport report;
        std::size_t filteringOperations = 0;
        const auto isLowConfidence = [&](const Track &track)
        {
            int strong = 0;
            int real = 0;
            for (const auto &tracked : track.boxes)
            {
                periodicallyRequireTrackingContinue(continueGuard, filteringOperations);
                if (tracked.interpolated)
                {
                    continue;
                }
                ++real;
                if (tracked.score >= config.strongScoreThreshold)
                {
                    ++strong;
                }
            }
            if (strong >= config.minStrongDetections)
            {
                return false;
            }
            const bool cleanShortBurst =
                strong >= config.shortTrackMinStrong
                && static_cast<float>(strong)
                       >= config.shortTrackStrongRatio * static_cast<float>(real);
            return !cleanShortBurst;
        };
        if (config.retainLowConfidenceTracks)
        {
            for (auto &track : tracks)
            {
                track.lowConfidence = isLowConfidence(track);
            }
        }
        else
        {
            // Dropping a whole track also drops the frames the detector was confident
            // about. Keep those, and only discard tracks with nothing strong in them.
            for (auto &track : tracks)
            {
                if (!isLowConfidence(track))
                {
                    continue;
                }
                track.lowConfidence = true;
                std::erase_if(track.boxes,
                    [&](const TrackedBox &box)
                    {
                        return box.interpolated || box.score < config.strongScoreThreshold;
                    });
            }
            const auto emptied = std::erase_if(tracks,
                [](const Track &track)
                {
                    return track.boxes.empty();
                });
            report.droppedTracks = static_cast<int>(emptied);
        }
        std::size_t totalBoxes = trackedBoxCount(tracks, kMaxFinalTrackedBoxes);
        for (auto &track : tracks)
        {
            requireTrackingContinue(continueGuard);
            const auto previous = track.boxes.size();
            for (const auto &span :
                interpolateGaps(track, config.maxInterpolationGap, cuts, continueGuard))
            {
                report.uncoveredFrames += span.frameCount();
                report.uncoveredSpans.push_back(span);
            }
            const auto withoutCurrent = totalBoxes - previous;
            if (withoutCurrent > kMaxFinalTrackedBoxes
                || track.boxes.size() > kMaxFinalTrackedBoxes - withoutCurrent)
            {
                throw std::length_error("Tracking data exceeds the safety limit.");
            }
            totalBoxes = withoutCurrent + track.boxes.size();
            smoothTrack(track, config.smoothingRadius, continueGuard);
            const auto beforeExtension = track.boxes.size();
            extendTrackEnds(track, config.extensionFrames, frameCount, cuts, continueGuard);
            const auto withoutExtended = totalBoxes - beforeExtension;
            if (withoutExtended > kMaxFinalTrackedBoxes
                || track.boxes.size() > kMaxFinalTrackedBoxes - withoutExtended)
            {
                throw std::length_error("Tracking data exceeds the safety limit.");
            }
            totalBoxes = withoutExtended + track.boxes.size();
        }
        return report;
    }

    std::vector<cv::Rect2f> trackRegionsForFrame(const std::vector<Track> &tracks, int frame)
    {
        std::vector<cv::Rect2f> regions;
        for (const auto &track : tracks)
        {
            if (const auto *box = track.boxAtFrame(frame))
            {
                regions.push_back(box->box);
            }
        }
        return regions;
    }
}

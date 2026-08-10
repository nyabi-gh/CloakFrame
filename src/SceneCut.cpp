#include "cloakframe/SceneCut.hpp"

#include <opencv2/imgproc.hpp>

#include <algorithm>
#include <cstddef>

namespace cloakframe
{
    namespace
    {
        constexpr int kSampleWidth = 48;
        constexpr int kSampleHeight = 27;
        constexpr float kMinCutDiff = 22.0F;
        constexpr float kMedianRatio = 2.5F;
        constexpr int kRecentWindow = 8;

        // A candidate is confirmed only after this many further frames fail to return to the
        // pre-candidate content, so the shortest shot the detector can resolve is this long. A
        // shot shorter than that — a one-frame insert or a flash between two scenes — surfaces as
        // the single boundary that starts it rather than as two.
        constexpr int kConfirmFrames = 2;

        cv::Mat toThumbnail(const cv::Mat &frame)
        {
            cv::Mat gray;
            if (frame.channels() == 1)
            {
                gray = frame;
            }
            else
            {
                cv::cvtColor(frame, gray, cv::COLOR_BGR2GRAY);
            }
            cv::Mat small;
            cv::resize(gray, small, {kSampleWidth, kSampleHeight}, 0.0, 0.0, cv::INTER_AREA);
            return small;
        }

        float meanAbsDiff(const cv::Mat &a, const cv::Mat &b)
        {
            cv::Mat diff;
            cv::absdiff(a, b, diff);
            return static_cast<float>(cv::mean(diff)[0]);
        }
    }

    SceneCuts::SceneCuts(std::vector<int> cutFrames)
        : frames_(std::move(cutFrames))
    {
        std::erase_if(frames_,
            [](const int frame)
            {
                return frame <= 0;
            });
        std::ranges::sort(frames_);
        frames_.erase(std::ranges::unique(frames_).begin(), frames_.end());
    }

    bool SceneCuts::empty() const
    {
        return frames_.empty();
    }

    const std::vector<int> &SceneCuts::frames() const
    {
        return frames_;
    }

    bool SceneCuts::isCut(const int frame) const
    {
        return std::ranges::binary_search(frames_, frame);
    }

    bool SceneCuts::spansCut(const int fromFrame, const int toFrame) const
    {
        const auto [low, high] = std::minmax(fromFrame, toFrame);
        const auto it = std::ranges::upper_bound(frames_, low);
        return it != frames_.end() && *it <= high;
    }

    SceneCuts SceneCuts::reversed(const int frameCount) const
    {
        std::vector<int> mapped;
        mapped.reserve(frames_.size());
        for (const int frame : frames_)
        {
            if (frame > 0 && frame < frameCount)
            {
                mapped.push_back(frameCount - frame);
            }
        }
        return SceneCuts(std::move(mapped));
    }

    float SceneCutDetector::recentMedian() const
    {
        if (recentDiffs_.empty())
        {
            return 0.0F;
        }
        auto sorted = recentDiffs_;
        const auto middle = sorted.begin() + static_cast<std::ptrdiff_t>(sorted.size() / 2);
        std::ranges::nth_element(sorted, middle);
        return *middle;
    }

    void SceneCutDetector::recordDiff(const float diff)
    {
        if (recentDiffs_.size() >= kRecentWindow)
        {
            recentDiffs_.erase(recentDiffs_.begin());
        }
        recentDiffs_.push_back(diff);
    }

    void SceneCutDetector::push(const cv::Mat &frame)
    {
        ++frameIndex_;
        cv::Mat thumbnail = toThumbnail(frame);
        if (previous_.empty())
        {
            previous_ = std::move(thumbnail);
            return;
        }

        const float diff = meanAbsDiff(thumbnail, previous_);

        if (candidateFrame_ >= 0)
        {
            const float returnDiff = meanAbsDiff(thumbnail, preCandidate_);
            if (returnDiff < std::min(kMinCutDiff, candidateDiff_ * 0.5F))
            {
                candidateFrame_ = -1;
                preCandidate_.release();
                previous_ = std::move(thumbnail);
                return;
            }
            if (++framesSinceCandidate_ < kConfirmFrames)
            {
                previous_ = std::move(thumbnail);
                return;
            }
            cuts_.push_back(candidateFrame_);
            candidateFrame_ = -1;
            preCandidate_.release();
        }

        const float median = recentMedian();
        const bool spike =
            diff >= kMinCutDiff && (recentDiffs_.empty() || diff >= median * kMedianRatio);
        if (spike)
        {
            candidateFrame_ = frameIndex_;
            candidateDiff_ = diff;
            framesSinceCandidate_ = 0;
            preCandidate_ = previous_.clone();
        }
        else
        {
            recordDiff(diff);
        }
        previous_ = std::move(thumbnail);
    }

    SceneCuts SceneCutDetector::finish()
    {
        if (candidateFrame_ >= 0)
        {
            cuts_.push_back(candidateFrame_);
            candidateFrame_ = -1;
            preCandidate_.release();
        }
        return SceneCuts(std::move(cuts_));
    }
}

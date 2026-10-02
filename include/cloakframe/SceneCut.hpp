#pragma once

#include <opencv2/core.hpp>

#include <optional>
#include <vector>

namespace cloakframe
{
    class SceneCuts
    {
    public:
        SceneCuts() = default;
        explicit SceneCuts(std::vector<int> cutFrames);

        [[nodiscard]] bool empty() const;
        [[nodiscard]] const std::vector<int> &frames() const;

        [[nodiscard]] bool isCut(int frame) const;

        [[nodiscard]] bool spansCut(int fromFrame, int toFrame) const;

        [[nodiscard]] std::optional<int> firstCutAfter(int frame) const;

        [[nodiscard]] SceneCuts reversed(int frameCount) const;

    private:
        std::vector<int> frames_;
    };

    class SceneCutDetector
    {
    public:
        void push(const cv::Mat &frame);

        [[nodiscard]] SceneCuts finish();

    private:
        [[nodiscard]] float recentMedian() const;
        void recordDiff(float diff);

        cv::Mat previous_;
        cv::Mat preCandidate_;
        std::vector<float> recentDiffs_;
        std::vector<int> cuts_;
        int frameIndex_ = -1;
        int candidateFrame_ = -1;
        float candidateDiff_ = 0.0F;
        int framesSinceCandidate_ = 0;
    };
}

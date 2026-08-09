#pragma once

#include "cloakframe/FaceDetection.hpp"

#include <opencv2/core.hpp>

#include <atomic>
#include <chrono>

namespace cloakframe
{
    // Where a detector's own time goes, accumulated across every call so far. A video run
    // spends most of its wall clock inside detect(), and preparing the tensor, running the
    // session and decoding the output have nothing in common as optimisation targets, so a
    // single total cannot say which one to attack.
    struct DetectorStageTimings
    {
        long long preprocessMicros = 0;
        long long inferenceMicros = 0;
        long long postprocessMicros = 0;
    };

    class Detector
    {
    public:
        virtual ~Detector() = default;

        virtual DetectionResult detect(
            const cv::Mat &bgrImage, float scoreThreshold, float nmsThreshold) = 0;

        [[nodiscard]] virtual int inputSize() const noexcept
        {
            return 0;
        }

        [[nodiscard]] virtual const char *backendName() const noexcept
        {
            return "CPU";
        }

        [[nodiscard]] DetectorStageTimings stageTimings() const noexcept
        {
            return {preprocessMicros_.load(std::memory_order_relaxed),
                inferenceMicros_.load(std::memory_order_relaxed),
                postprocessMicros_.load(std::memory_order_relaxed)};
        }

    protected:
        // Adds its own lifetime to one accumulator. Relaxed ordering is enough: the counters
        // are read for reporting, never to establish an ordering with the detection itself.
        class StageScope
        {
        public:
            explicit StageScope(std::atomic<long long> &target)
                : target_(target)
                , started_(std::chrono::steady_clock::now())
            {
            }

            ~StageScope()
            {
                const auto elapsed = std::chrono::duration_cast<std::chrono::microseconds>(
                    std::chrono::steady_clock::now() - started_)
                                         .count();
                target_.fetch_add(elapsed, std::memory_order_relaxed);
            }

            StageScope(const StageScope &) = delete;
            StageScope &operator=(const StageScope &) = delete;

        private:
            std::atomic<long long> &target_;
            std::chrono::steady_clock::time_point started_;
        };

        std::atomic<long long> preprocessMicros_{0};
        std::atomic<long long> inferenceMicros_{0};
        std::atomic<long long> postprocessMicros_{0};
    };
}

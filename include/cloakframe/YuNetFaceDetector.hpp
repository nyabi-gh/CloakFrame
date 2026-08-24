#pragma once

#include "cloakframe/Detector.hpp"

#include <QByteArray>

#include <opencv2/objdetect/face.hpp>

#include <mutex>
#include <string>

namespace cloakframe
{
    class YuNetFaceDetector final : public Detector
    {
    public:
        explicit YuNetFaceDetector(
            const std::string &modelPath, const QByteArray &expectedSha256 = {});

        DetectionResult detect(
            const cv::Mat &bgrImage, float scoreThreshold, float nmsThreshold) override;

        [[nodiscard]] int inputSize() const noexcept override
        {
            return 640;
        }

        [[nodiscard]] const char *backendName() const noexcept override
        {
            return "OpenCV CPU";
        }

    private:
        // cv::FaceDetectorYN keeps mutable state across detect calls, so the whole inference
        // is serialized; only the letterbox preparation and the output parsing run unlocked.
        std::mutex detectMutex_;
        cv::Ptr<cv::FaceDetectorYN> detector_;
    };
}

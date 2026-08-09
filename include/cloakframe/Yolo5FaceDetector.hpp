#pragma once

#include "cloakframe/Detector.hpp"
#include "cloakframe/OrtAcceleration.hpp"

#include <QByteArray>

#include <opencv2/core.hpp>

#include <onnxruntime_cxx_api.h>

#include <string>
#include <vector>

namespace cloakframe
{
    class Yolo5FaceDetector final : public Detector
    {
    public:
        explicit Yolo5FaceDetector(const std::string &modelPath,
            bool enableAcceleration = false,
            const QByteArray &expectedSha256 = {});

        DetectionResult detect(
            const cv::Mat &bgrImage, float scoreThreshold, float nmsThreshold) override;

        [[nodiscard]] int inputSize() const noexcept override
        {
            return 640;
        }

        [[nodiscard]] const char *backendName() const noexcept override
        {
            return ortAcceleratorName(accelerator_);
        }

        [[nodiscard]] OrtAccelerator accelerator() const noexcept
        {
            return accelerator_;
        }

    private:
        OrtAccelerator accelerator_ = OrtAccelerator::None;
        Ort::Env env_;
        Ort::SessionOptions sessionOptions_;
        Ort::Session session_;
        std::string inputName_;
        std::string outputName_;
        // Reused across calls; detect() is never entered concurrently on one detector.
        Ort::MemoryInfo memoryInfo_ =
            Ort::MemoryInfo::CreateCpu(OrtArenaAllocator, OrtMemTypeDefault);
        cv::Mat canvas_;
        cv::Mat blob_;
    };
}

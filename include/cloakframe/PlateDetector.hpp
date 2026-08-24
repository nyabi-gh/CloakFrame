#pragma once

#include "cloakframe/Detector.hpp"
#include "cloakframe/FaceDetection.hpp"
#include "cloakframe/OrtAcceleration.hpp"

#include <QByteArray>

#include <opencv2/core.hpp>

#include <onnxruntime_cxx_api.h>

#include <mutex>
#include <string>
#include <vector>

namespace cloakframe
{
    class PlateDetector final : public Detector
    {
    public:
        explicit PlateDetector(const std::string &modelPath,
            bool enableAcceleration = false,
            const QByteArray &expectedSha256 = {});

        DetectionResult detect(
            const cv::Mat &bgrImage, float scoreThreshold, float nmsThreshold) override;

        [[nodiscard]] OrtAccelerator accelerator() const noexcept
        {
            return accelerator_;
        }

    private:
        int inputWidth_;
        int inputHeight_;
        OrtAccelerator accelerator_ = OrtAccelerator::None;
        Ort::Env env_;
        Ort::SessionOptions sessionOptions_;
        Ort::Session session_;
        std::vector<std::string> inputNames_;
        std::vector<std::string> outputNames_;
        std::vector<const char *> inputNamePtrs_;
        std::vector<const char *> outputNamePtrs_;
        Ort::MemoryInfo memoryInfo_ =
            Ort::MemoryInfo::CreateCpu(OrtArenaAllocator, OrtMemTypeDefault);
        // Serializes session_.Run: the DirectML execution provider does not support
        // concurrent Run calls on one session.
        std::mutex runMutex_;
    };
}

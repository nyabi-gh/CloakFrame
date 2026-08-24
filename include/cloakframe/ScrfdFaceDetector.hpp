#pragma once

#include "cloakframe/DetectionGeometry.hpp"
#include "cloakframe/Detector.hpp"
#include "cloakframe/FaceDetection.hpp"
#include "cloakframe/OrtAcceleration.hpp"

#include <QByteArray>

#include <opencv2/core.hpp>

#include <onnxruntime_cxx_api.h>

#include <cstdint>
#include <mutex>
#include <string>
#include <vector>

namespace cloakframe
{
    class ScrfdFaceDetector final : public Detector
    {
    public:
        explicit ScrfdFaceDetector(const std::string &modelPath,
            int inputSize = 640,
            bool enableAcceleration = false,
            const QByteArray &expectedSha256 = {});

        DetectionResult detect(
            const cv::Mat &bgrImage, float scoreThreshold, float nmsThreshold) override;

        [[nodiscard]] OrtAccelerator accelerator() const noexcept
        {
            return accelerator_;
        }

        [[nodiscard]] int inputSize() const noexcept override
        {
            return inputSize_;
        }

        [[nodiscard]] const char *backendName() const noexcept override
        {
            return ortAcceleratorName(accelerator_);
        }

    private:
        struct PreparedImage
        {
            std::vector<float> tensor;
            int originalWidth = 0;
            int originalHeight = 0;
            float scale = 1.0F;
            int resizedWidth = 0;
            int resizedHeight = 0;
        };

        void adoptOriginalSessionIfAccelerated(const std::vector<std::uint8_t> &modelBytes);

        void adoptFixedInputSession(const std::vector<std::uint8_t> &modelBytes, int fallbackSize);

        [[nodiscard]] PreparedImage prepare(const cv::Mat &bgrImage) const;

        [[nodiscard]] DetectionResult decode(const std::vector<Ort::Value> &outputs,
            const PreparedImage &prepared,
            float scoreThreshold,
            float nmsThreshold) const;

        [[nodiscard]] static std::vector<cv::Point2f> anchorCenters(
            int featureHeight, int featureWidth, int stride);

        int inputSize_;
        OrtAccelerator accelerator_ = OrtAccelerator::None;
        Ort::Env env_;
        Ort::SessionOptions sessionOptions_;
        Ort::Session session_;
        std::vector<std::string> inputNames_;
        std::vector<std::string> outputNames_;
        std::vector<const char *> inputNamePtrs_;
        std::vector<const char *> outputNamePtrs_;
        // Serializes session_.Run: the DirectML execution provider does not support
        // concurrent Run calls on one session.
        std::mutex runMutex_;
    };
}

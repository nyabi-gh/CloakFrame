#include "cloakframe/PlateDetector.hpp"

#include <QByteArrayView>
#include <QCryptographicHash>

#include <opencv2/dnn.hpp>
#include <opencv2/imgproc.hpp>

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <optional>
#include <stdexcept>
#include <string>
#include <thread>

namespace cloakframe
{
    namespace
    {
        constexpr int kChannels = 3;
        constexpr std::size_t kMaxPlateCandidates = 16'384;
        constexpr std::size_t kMaxPlateOutputRows = 65'536;
        constexpr std::uintmax_t kMaxModelFileBytes = 512ULL * 1024ULL * 1024ULL;

        std::filesystem::path modelPathFromUtf8(const std::string &modelPath)
        {
            const std::u8string u8(modelPath.begin(), modelPath.end());
            return std::filesystem::path(u8);
        }

        std::vector<std::uint8_t> readModelFile(
            const std::filesystem::path &path, const QByteArray &expectedSha256)
        {
            std::error_code sizeError;
            const auto size = std::filesystem::file_size(path, sizeError);
            if (sizeError || size == 0 || size > kMaxModelFileBytes)
            {
                throw std::runtime_error("Could not read the model file.");
            }
            std::ifstream stream(path, std::ios::binary);
            std::vector<std::uint8_t> bytes(static_cast<std::size_t>(size));
            if (!stream.read(reinterpret_cast<char *>(bytes.data()),
                    static_cast<std::streamsize>(bytes.size())))
            {
                throw std::runtime_error("Could not read the model file.");
            }
            if (!expectedSha256.isEmpty())
            {
                QCryptographicHash hash(QCryptographicHash::Sha256);
                hash.addData(QByteArrayView(reinterpret_cast<const char *>(bytes.data()),
                    static_cast<qsizetype>(bytes.size())));
                if (hash.result() != expectedSha256)
                {
                    throw std::runtime_error("The model file changed before it was loaded.");
                }
            }
            return bytes;
        }
    }

    PlateDetector::PlateDetector(
        const std::string &modelPath, bool enableAcceleration, const QByteArray &expectedSha256)
        : inputWidth_(512)
        , inputHeight_(512)
        , env_(ORT_LOGGING_LEVEL_WARNING, "CloakFrame-Plate")
        , sessionOptions_()
        , session_(nullptr)
    {
        sessionOptions_.SetGraphOptimizationLevel(GraphOptimizationLevel::ORT_ENABLE_EXTENDED);
        accelerator_ = applyOrtAcceleration(sessionOptions_, enableAcceleration);
        sessionOptions_.SetIntraOpNumThreads(
            accelerator_ == OrtAccelerator::None
                ? static_cast<int>(std::max(1U, std::thread::hardware_concurrency()))
                : 1);
        const std::filesystem::path modelFsPath = modelPathFromUtf8(modelPath);
        const auto modelBytes = readModelFile(modelFsPath, expectedSha256);
        session_ = Ort::Session(env_, modelBytes.data(), modelBytes.size(), sessionOptions_);

        Ort::AllocatorWithDefaultOptions allocator;

        if (session_.GetInputCount() != 1 || session_.GetOutputCount() < 1)
        {
            throw std::runtime_error(
                "The selected model does not look like a license-plate ONNX model.");
        }

        inputNames_.reserve(session_.GetInputCount());
        for (size_t i = 0; i < session_.GetInputCount(); ++i)
        {
            auto name = session_.GetInputNameAllocated(i, allocator);
            inputNames_.emplace_back(name.get());
        }
        outputNames_.reserve(session_.GetOutputCount());
        for (size_t i = 0; i < session_.GetOutputCount(); ++i)
        {
            auto name = session_.GetOutputNameAllocated(i, allocator);
            outputNames_.emplace_back(name.get());
        }

        const auto inputTypeInfo = session_.GetInputTypeInfo(0);
        const auto inputType = inputTypeInfo.GetTensorTypeAndShapeInfo();
        if (inputType.GetElementType() != ONNX_TENSOR_ELEMENT_DATA_TYPE_FLOAT)
        {
            throw std::runtime_error("License-plate model input must be a float tensor.");
        }
        const auto outputTypeInfo = session_.GetOutputTypeInfo(0);
        if (outputTypeInfo.GetONNXType() != ONNX_TYPE_TENSOR)
        {
            throw std::runtime_error("License-plate model output must be a float tensor.");
        }
        const auto outputType = outputTypeInfo.GetTensorTypeAndShapeInfo();
        if (outputType.GetElementType() != ONNX_TENSOR_ELEMENT_DATA_TYPE_FLOAT)
        {
            throw std::runtime_error("License-plate model output must be a float tensor.");
        }

        const auto inputShape = inputType.GetShape();
        if (inputShape.size() != 4 || (inputShape[0] != -1 && inputShape[0] != 1)
            || (inputShape[1] != -1 && inputShape[1] != kChannels)
            || (inputShape[2] != -1 && (inputShape[2] <= 0 || inputShape[2] > 2048))
            || (inputShape[3] != -1 && (inputShape[3] <= 0 || inputShape[3] > 2048)))
        {
            throw std::runtime_error("License-plate model input must be a [1, 3, H, W] tensor.");
        }
        if (inputShape[2] > 0)
        {
            inputHeight_ = static_cast<int>(inputShape[2]);
        }
        if (inputShape[3] > 0)
        {
            inputWidth_ = static_cast<int>(inputShape[3]);
        }
        const auto declaredOutputShape = outputType.GetShape();
        if (declaredOutputShape.size() != 2
            || (declaredOutputShape[0] > 0
                && static_cast<std::uint64_t>(declaredOutputShape[0]) > kMaxPlateOutputRows)
            || (declaredOutputShape[1] >= 0 && declaredOutputShape[1] < 7))
        {
            throw std::runtime_error("License-plate model output must use an [N, 7+] tensor.");
        }

        inputNamePtrs_.reserve(inputNames_.size());
        for (const auto &name : inputNames_)
        {
            inputNamePtrs_.push_back(name.c_str());
        }
        outputNamePtrs_.reserve(outputNames_.size());
        for (const auto &name : outputNames_)
        {
            outputNamePtrs_.push_back(name.c_str());
        }
    }

    DetectionResult PlateDetector::detect(
        const cv::Mat &bgrImage, float scoreThreshold, float /*nmsThreshold*/)
    {
        if (bgrImage.empty())
        {
            return {};
        }

        // Each emplace ends the previous stage and starts the next, so the three accumulators
        // partition detect() exactly.
        std::optional<StageScope> stage;
        stage.emplace(preprocessMicros_);
        const float ratio =
            std::min(static_cast<float>(inputWidth_) / static_cast<float>(bgrImage.cols),
                static_cast<float>(inputHeight_) / static_cast<float>(bgrImage.rows));
        const int resizedWidth =
            std::max(1, static_cast<int>(std::round(static_cast<float>(bgrImage.cols) * ratio)));
        const int resizedHeight =
            std::max(1, static_cast<int>(std::round(static_cast<float>(bgrImage.rows) * ratio)));
        const float padX = static_cast<float>(inputWidth_ - resizedWidth) / 2.0F;
        const float padY = static_cast<float>(inputHeight_ - resizedHeight) / 2.0F;

        const int top = static_cast<int>(std::round(padY - 0.1F));
        const int left = static_cast<int>(std::round(padX - 0.1F));

        // Members rather than locals: a dashcam pass calls this once per frame alongside the
        // face detector, so the allocations were being paid twice over.
        if (canvas_.empty())
        {
            canvas_.create(inputHeight_, inputWidth_, CV_8UC3);
        }
        canvas_.setTo(cv::Scalar(114, 114, 114));
        cv::Mat letterbox = canvas_(cv::Rect(left, top, resizedWidth, resizedHeight));
        cv::resize(bgrImage, letterbox, letterbox.size(), 0.0, 0.0, cv::INTER_LINEAR);

        // Same normalise and HWC→CHW as the scalar loop it replaces — scale 1/255, and
        // swapRB because the source is BGR — but vectorised, and into a reused buffer.
        cv::dnn::blobFromImage(canvas_,
            blob_,
            1.0 / 255.0,
            cv::Size(),
            cv::Scalar(),
            /*swapRB=*/true,
            /*crop=*/false,
            CV_32F);

        std::array<int64_t, 4> inputShape = {1, kChannels, inputHeight_, inputWidth_};
        Ort::Value inputTensor = Ort::Value::CreateTensor<float>(memoryInfo_,
            blob_.ptr<float>(),
            static_cast<std::size_t>(kChannels) * inputHeight_ * inputWidth_,
            inputShape.data(),
            inputShape.size());

        stage.emplace(inferenceMicros_);
        auto outputs = session_.Run(Ort::RunOptions{nullptr},
            inputNamePtrs_.data(),
            &inputTensor,
            1,
            outputNamePtrs_.data(),
            1);
        stage.emplace(postprocessMicros_);

        if (outputs.empty() || !outputs.front().IsTensor())
        {
            throw std::runtime_error("License-plate model did not return a tensor.");
        }

        const auto outputInfo = outputs.front().GetTensorTypeAndShapeInfo();
        if (outputInfo.GetElementType() != ONNX_TENSOR_ELEMENT_DATA_TYPE_FLOAT)
        {
            throw std::runtime_error("License-plate model output must be a float tensor.");
        }
        const auto outputShape = outputInfo.GetShape();
        if (outputShape.size() != 2 || outputShape[0] < 0 || outputShape[1] < 7)
        {
            throw std::runtime_error("License-plate model returned an invalid tensor shape.");
        }
        const auto rows = static_cast<size_t>(outputShape[0]);
        const auto stride = static_cast<size_t>(outputShape[1]);
        const auto elementCount = outputInfo.GetElementCount();
        if (rows > kMaxPlateOutputRows)
        {
            throw std::length_error("License plate detection data exceeds the safety limit.");
        }
        if (rows > elementCount / stride)
        {
            throw std::runtime_error("License-plate model returned an invalid tensor size.");
        }
        if (rows == 0)
        {
            return {};
        }
        const float *data = outputs.front().GetTensorData<float>();
        if (data == nullptr)
        {
            throw std::runtime_error("License-plate model returned no tensor data.");
        }

        FaceDetections detections;
        for (size_t i = 0; i < rows; ++i)
        {
            const float *row = data + i * stride;
            const float score = row[6];
            if (!std::isfinite(score) || score < scoreThreshold)
            {
                continue;
            }

            const float x1 = (row[1] - padX) / ratio;
            const float y1 = (row[2] - padY) / ratio;
            const float x2 = (row[3] - padX) / ratio;
            const float y2 = (row[4] - padY) / ratio;
            if (!std::isfinite(x1) || !std::isfinite(y1) || !std::isfinite(x2)
                || !std::isfinite(y2))
            {
                continue;
            }

            const float boxLeft =
                std::clamp(std::min(x1, x2), 0.0F, static_cast<float>(bgrImage.cols));
            const float boxTop =
                std::clamp(std::min(y1, y2), 0.0F, static_cast<float>(bgrImage.rows));
            const float boxRight =
                std::clamp(std::max(x1, x2), 0.0F, static_cast<float>(bgrImage.cols));
            const float boxBottom =
                std::clamp(std::max(y1, y2), 0.0F, static_cast<float>(bgrImage.rows));
            if (boxRight - boxLeft < 1.0F || boxBottom - boxTop < 1.0F)
            {
                continue;
            }

            FaceDetection detection;
            detection.box = cv::Rect2f(boxLeft, boxTop, boxRight - boxLeft, boxBottom - boxTop);
            detection.score = score;
            detections.push_back(detection);
            if (detections.size() > kMaxPlateCandidates)
            {
                throw std::length_error("License plate detection data exceeds the safety limit.");
            }
        }

        return {std::move(detections), 0};
    }
}

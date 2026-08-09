#include "cloakframe/Yolo5FaceDetector.hpp"

#include "cloakframe/DetectionGeometry.hpp"

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
#include <limits>
#include <optional>
#include <stdexcept>
#include <thread>

namespace cloakframe
{
    namespace
    {
        constexpr int kInputSize = 640;
        constexpr int kChannels = 3;
        constexpr int kOutputColumns = 16;
        constexpr int kExpectedRows = 25'200;
        constexpr std::size_t kMaxCandidatesBeforeNms = 2'000;
        constexpr std::size_t kMaxDetections = 300;
        constexpr std::uintmax_t kMaxModelFileBytes = 512ULL * 1024ULL * 1024ULL;

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

        std::optional<float> faceRollFromLandmarks(
            const std::array<cv::Point2f, 5> &landmarks, const cv::Rect2f &box)
        {
            if (box.width <= 0.0F || box.height <= 0.0F)
            {
                return std::nullopt;
            }
            const float marginX = box.width * 0.35F;
            const float marginY = box.height * 0.35F;
            for (const auto &point : landmarks)
            {
                if (!std::isfinite(point.x) || !std::isfinite(point.y) || point.x < box.x - marginX
                    || point.x > box.x + box.width + marginX || point.y < box.y - marginY
                    || point.y > box.y + box.height + marginY)
                {
                    return std::nullopt;
                }
            }

            const auto angleFor = [&](const int left, const int right) -> std::optional<float>
            {
                const float dx = landmarks[right].x - landmarks[left].x;
                const float dy = landmarks[right].y - landmarks[left].y;
                const float distance = std::hypot(dx, dy);
                if (dx <= 0.0F || distance < box.width * 0.08F || distance > box.width * 1.25F)
                {
                    return std::nullopt;
                }
                const float angle = std::atan2(dy, dx);
                return isValidFacePose(angle, true) ? std::optional<float>(angle) : std::nullopt;
            };

            const auto eyeAngle = angleFor(0, 1);
            if (!eyeAngle)
            {
                return std::nullopt;
            }
            const auto mouthAngle = angleFor(3, 4);
            if (!mouthAngle || std::abs(*mouthAngle - *eyeAngle) > 0.45F)
            {
                return eyeAngle;
            }
            return std::atan2(std::sin(*eyeAngle) + std::sin(*mouthAngle),
                std::cos(*eyeAngle) + std::cos(*mouthAngle));
        }
    }

    Yolo5FaceDetector::Yolo5FaceDetector(const std::string &modelPath,
        const bool enableAcceleration,
        const QByteArray &expectedSha256)
        : env_(ORT_LOGGING_LEVEL_WARNING, "CloakFrame-YOLO5Face")
        , sessionOptions_()
        , session_(nullptr)
    {
        sessionOptions_.SetGraphOptimizationLevel(GraphOptimizationLevel::ORT_ENABLE_EXTENDED);
        accelerator_ = applyOrtAcceleration(sessionOptions_, enableAcceleration);
        sessionOptions_.SetIntraOpNumThreads(
            accelerator_ == OrtAccelerator::None
                ? static_cast<int>(std::max(1U, std::thread::hardware_concurrency()))
                : 1);

        const std::u8string modelU8(modelPath.begin(), modelPath.end());
        const auto modelBytes = readModelFile(std::filesystem::path(modelU8), expectedSha256);
        session_ = Ort::Session(env_, modelBytes.data(), modelBytes.size(), sessionOptions_);

        if (session_.GetInputCount() != 1 || session_.GetOutputCount() != 1)
        {
            throw std::runtime_error(
                "The selected model does not look like a YOLO5Face-n ONNX model.");
        }

        const auto inputTypeInfo = session_.GetInputTypeInfo(0);
        const auto inputInfo = inputTypeInfo.GetTensorTypeAndShapeInfo();
        const auto inputShape = inputInfo.GetShape();
        const auto dimensionMatches = [](const int64_t actual, const int64_t expected)
        {
            return actual <= 0 || actual == expected;
        };
        if (inputInfo.GetElementType() != ONNX_TENSOR_ELEMENT_DATA_TYPE_FLOAT)
        {
            throw std::runtime_error(
                "YOLO5Face-n model input must be a float tensor; received type "
                + std::to_string(static_cast<int>(inputInfo.GetElementType())) + ".");
        }
        if (inputShape.size() != 4 || !dimensionMatches(inputShape[0], 1)
            || !dimensionMatches(inputShape[1], kChannels)
            || !dimensionMatches(inputShape[2], kInputSize)
            || !dimensionMatches(inputShape[3], kInputSize))
        {
            throw std::runtime_error(
                "YOLO5Face-n model input must be a [1, 3, 640, 640] float tensor.");
        }

        const auto outputTypeInfo = session_.GetOutputTypeInfo(0);
        if (outputTypeInfo.GetONNXType() != ONNX_TYPE_TENSOR)
        {
            throw std::runtime_error("YOLO5Face-n model output must be a float tensor.");
        }
        const auto outputInfo = outputTypeInfo.GetTensorTypeAndShapeInfo();
        const auto outputShape = outputInfo.GetShape();
        if (outputInfo.GetElementType() != ONNX_TENSOR_ELEMENT_DATA_TYPE_FLOAT)
        {
            throw std::runtime_error(
                "YOLO5Face-n model output must be a float tensor; received type "
                + std::to_string(static_cast<int>(outputInfo.GetElementType())) + ".");
        }
        if (outputShape.size() != 3 || !dimensionMatches(outputShape[0], 1)
            || !dimensionMatches(outputShape[1], kExpectedRows)
            || !dimensionMatches(outputShape[2], kOutputColumns))
        {
            throw std::runtime_error(
                "YOLO5Face-n model output must be a [1, 25200, 16] float tensor.");
        }

        Ort::AllocatorWithDefaultOptions allocator;
        inputName_ = session_.GetInputNameAllocated(0, allocator).get();
        outputName_ = session_.GetOutputNameAllocated(0, allocator).get();
    }

    DetectionResult Yolo5FaceDetector::detect(
        const cv::Mat &bgrImage, const float scoreThreshold, const float nmsThreshold)
    {
        if (bgrImage.empty())
        {
            return {};
        }
        if (bgrImage.type() != CV_8UC3)
        {
            throw std::invalid_argument("YOLO5Face-n requires an 8-bit BGR image.");
        }

        // Each emplace ends the previous stage and starts the next, so the three accumulators
        // partition detect() exactly.
        std::optional<StageScope> stage;
        stage.emplace(preprocessMicros_);
        const float scale =
            std::min(static_cast<float>(kInputSize) / static_cast<float>(bgrImage.cols),
                static_cast<float>(kInputSize) / static_cast<float>(bgrImage.rows));
        const int resizedWidth =
            std::max(1, static_cast<int>(static_cast<float>(bgrImage.cols) * scale));
        const int resizedHeight =
            std::max(1, static_cast<int>(static_cast<float>(bgrImage.rows) * scale));
        const float padX = static_cast<float>(kInputSize - resizedWidth) / 2.0F;
        const float padY = static_cast<float>(kInputSize - resizedHeight) / 2.0F;
        const int left = static_cast<int>(padX);
        const int top = static_cast<int>(padY);

        cv::Mat resized;
        cv::resize(
            bgrImage, resized, cv::Size(resizedWidth, resizedHeight), 0.0, 0.0, cv::INTER_LINEAR);
        cv::Mat canvas(kInputSize, kInputSize, CV_8UC3, cv::Scalar(114, 114, 114));
        resized.copyTo(canvas(cv::Rect(left, top, resizedWidth, resizedHeight)));

        const int planeSize = kInputSize * kInputSize;
        std::vector<float> tensor(static_cast<std::size_t>(kChannels) * planeSize);
        for (int y = 0; y < kInputSize; ++y)
        {
            const auto *row = canvas.ptr<cv::Vec3b>(y);
            for (int x = 0; x < kInputSize; ++x)
            {
                const int offset = y * kInputSize + x;
                tensor[offset] = static_cast<float>(row[x][2]) / 255.0F;
                tensor[planeSize + offset] = static_cast<float>(row[x][1]) / 255.0F;
                tensor[2 * planeSize + offset] = static_cast<float>(row[x][0]) / 255.0F;
            }
        }

        std::array<int64_t, 4> inputShape = {1, kChannels, kInputSize, kInputSize};
        auto memoryInfo = Ort::MemoryInfo::CreateCpu(OrtArenaAllocator, OrtMemTypeDefault);
        auto inputTensor = Ort::Value::CreateTensor<float>(
            memoryInfo, tensor.data(), tensor.size(), inputShape.data(), inputShape.size());
        const char *inputName = inputName_.c_str();
        const char *outputName = outputName_.c_str();
        stage.emplace(inferenceMicros_);
        auto outputs =
            session_.Run(Ort::RunOptions{nullptr}, &inputName, &inputTensor, 1, &outputName, 1);
        stage.emplace(postprocessMicros_);

        if (outputs.size() != 1 || !outputs.front().IsTensor())
        {
            throw std::runtime_error("YOLO5Face-n did not return a tensor.");
        }
        const auto outputInfo = outputs.front().GetTensorTypeAndShapeInfo();
        const auto outputShape = outputInfo.GetShape();
        if (outputInfo.GetElementType() != ONNX_TENSOR_ELEMENT_DATA_TYPE_FLOAT
            || outputShape.size() != 3 || outputShape[0] != 1 || outputShape[1] != kExpectedRows
            || outputShape[2] != kOutputColumns
            || outputInfo.GetElementCount()
                   != static_cast<std::size_t>(kExpectedRows) * kOutputColumns)
        {
            throw std::runtime_error("YOLO5Face-n returned an invalid tensor shape.");
        }

        const float *data = outputs.front().GetTensorData<float>();
        if (data == nullptr)
        {
            throw std::runtime_error("YOLO5Face-n returned no tensor data.");
        }

        FaceDetections candidates;
        for (int index = 0; index < kExpectedRows; ++index)
        {
            const float *row = data + static_cast<std::ptrdiff_t>(index) * kOutputColumns;
            const float score = row[4];
            if (!std::isfinite(score) || score < scoreThreshold)
            {
                continue;
            }

            const float x1 = (row[0] - row[2] * 0.5F - padX) / scale;
            const float y1 = (row[1] - row[3] * 0.5F - padY) / scale;
            const float x2 = (row[0] + row[2] * 0.5F - padX) / scale;
            const float y2 = (row[1] + row[3] * 0.5F - padY) / scale;
            if (!std::isfinite(x1) || !std::isfinite(y1) || !std::isfinite(x2)
                || !std::isfinite(y2))
            {
                continue;
            }

            const float boxLeft = std::clamp(x1, 0.0F, static_cast<float>(bgrImage.cols));
            const float boxTop = std::clamp(y1, 0.0F, static_cast<float>(bgrImage.rows));
            const float boxRight = std::clamp(x2, 0.0F, static_cast<float>(bgrImage.cols));
            const float boxBottom = std::clamp(y2, 0.0F, static_cast<float>(bgrImage.rows));
            if (boxRight - boxLeft < 1.0F || boxBottom - boxTop < 1.0F)
            {
                continue;
            }

            FaceDetection detection;
            detection.box = {boxLeft, boxTop, boxRight - boxLeft, boxBottom - boxTop};
            detection.score = score;

            std::array<cv::Point2f, 5> landmarks{};
            bool landmarksValid = true;
            for (int point = 0; point < 5; ++point)
            {
                landmarks[point] = {
                    (row[5 + point * 2] - padX) / scale, (row[6 + point * 2] - padY) / scale};
                landmarksValid = landmarksValid && std::isfinite(landmarks[point].x)
                                 && std::isfinite(landmarks[point].y);
            }
            if (landmarksValid)
            {
                if (const auto roll = faceRollFromLandmarks(landmarks, detection.box))
                {
                    detection.rollRadians = *roll;
                    detection.hasPose = true;
                }
            }
            candidates.push_back(detection);
        }

        std::size_t omitted = 0;
        if (candidates.size() > kMaxCandidatesBeforeNms)
        {
            std::sort(candidates.begin(),
                candidates.end(),
                [](const FaceDetection &a, const FaceDetection &b)
                {
                    return a.score > b.score;
                });
            omitted += candidates.size() - kMaxCandidatesBeforeNms;
            candidates.resize(kMaxCandidatesBeforeNms);
        }
        auto detections = nonMaxSuppression(std::move(candidates), nmsThreshold);
        if (detections.size() > kMaxDetections)
        {
            omitted += detections.size() - kMaxDetections;
            detections.resize(kMaxDetections);
        }
        return {std::move(detections),
            static_cast<int>(std::min<std::size_t>(omitted, std::numeric_limits<int>::max()))};
    }
}

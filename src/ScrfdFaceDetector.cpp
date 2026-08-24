#include "cloakframe/ScrfdFaceDetector.hpp"

#include "cloakframe/OnnxGraphPatch.hpp"

#include <QByteArrayView>
#include <QCryptographicHash>

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
#include <string>

namespace cloakframe
{
    namespace
    {
        constexpr std::array<int, 3> kStrides = {8, 16, 32};
        constexpr int kChannels = 3;
        constexpr int kMaxAnchorsPerLocation = 2;
        constexpr int kFaceLandmarkCount = 5;
        constexpr int kLandmarkCoordinateCount = kFaceLandmarkCount * 2;
        constexpr size_t kMaxCandidatesBeforeNms = 2000;
        constexpr int kMaxInputSize = 2048;
        constexpr std::uintmax_t kMaxModelFileBytes = 512ULL << 20;

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

        cv::Rect2f distanceToBox(const cv::Point2f &center, const float *distances)
        {
            const float left = distances[0];
            const float top = distances[1];
            const float right = distances[2];
            const float bottom = distances[3];
            return {center.x - left, center.y - top, left + right, top + bottom};
        }

        std::optional<float> faceRollFromLandmarks(
            const std::array<cv::Point2f, kFaceLandmarkCount> &landmarks, const cv::Rect2f &box)
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

    ScrfdFaceDetector::ScrfdFaceDetector(const std::string &modelPath,
        int inputSize,
        bool enableAcceleration,
        const QByteArray &expectedSha256)
        : inputSize_(inputSize)
        , env_(ORT_LOGGING_LEVEL_WARNING, "CloakFrame")
        , sessionOptions_()
        , session_(nullptr)
    {
        // No compiled-model cache tag here: these options create sessions for both the
        // original and the dimension-patched graph, which share input/output names — the
        // only key ONNX Runtime has for a from-memory model — so their cached CoreML models
        // would collide.
        accelerator_ = configureOrtSessionOptions(sessionOptions_, enableAcceleration);
        const std::u8string modelU8(modelPath.begin(), modelPath.end());
        const std::filesystem::path modelFsPath(modelU8);
        const auto modelBytes = readModelFile(modelFsPath, expectedSha256);
        if (accelerator_ == OrtAccelerator::None)
        {
            session_ = Ort::Session(env_, modelBytes.data(), modelBytes.size(), sessionOptions_);
        }
        else
        {
            Ort::SessionOptions metadataOptions;
            metadataOptions.SetGraphOptimizationLevel(GraphOptimizationLevel::ORT_DISABLE_ALL);
            metadataOptions.SetIntraOpNumThreads(1);
            session_ = Ort::Session(env_, modelBytes.data(), modelBytes.size(), metadataOptions);
        }

        Ort::AllocatorWithDefaultOptions allocator;

        const auto inputCount = session_.GetInputCount();
        inputNames_.reserve(inputCount);
        for (size_t i = 0; i < inputCount; ++i)
        {
            auto name = session_.GetInputNameAllocated(i, allocator);
            inputNames_.emplace_back(name.get());
        }

        const auto outputCount = session_.GetOutputCount();
        outputNames_.reserve(outputCount);
        for (size_t i = 0; i < outputCount; ++i)
        {
            auto name = session_.GetOutputNameAllocated(i, allocator);
            outputNames_.emplace_back(name.get());
        }

        if (inputNames_.empty() || outputNames_.size() < 6)
        {
            throw std::runtime_error("The selected model does not look like an SCRFD ONNX model.");
        }

        const auto inputTypeInfo = session_.GetInputTypeInfo(0);
        const auto inputInfo = inputTypeInfo.GetTensorTypeAndShapeInfo();
        if (inputInfo.GetElementType() != ONNX_TENSOR_ELEMENT_DATA_TYPE_FLOAT)
        {
            throw std::runtime_error("SCRFD model input must be a float tensor.");
        }
        const auto inputShape = inputInfo.GetShape();
        if (inputShape.size() != 4)
        {
            throw std::runtime_error("SCRFD model input must be a 4D NCHW tensor.");
        }
        const auto dimensionMatches = [](int64_t actual, int64_t expected)
        {
            return actual < 0 || actual == expected;
        };
        if (!dimensionMatches(inputShape[0], 1) || !dimensionMatches(inputShape[1], kChannels))
        {
            throw std::runtime_error("SCRFD model input must be a [1, 3, H, W] tensor.");
        }

        const int64_t modelHeight = inputShape[2];
        const int64_t modelWidth = inputShape[3];
        const bool heightFixed = modelHeight > 0;
        const bool widthFixed = modelWidth > 0;
        if (heightFixed != widthFixed)
        {
            throw std::runtime_error(
                "SCRFD model must have both spatial dimensions fixed or both dynamic.");
        }
        if (heightFixed)
        {
            if (modelHeight != modelWidth)
            {
                throw std::runtime_error("SCRFD model input must be square.");
            }
            if (modelHeight % kStrides.back() != 0)
            {
                throw std::runtime_error("SCRFD model input size must be a multiple of 32.");
            }
            if (modelHeight > kMaxInputSize)
            {
                throw std::runtime_error("SCRFD model input size is too large.");
            }
            if (static_cast<int>(modelHeight) != inputSize_)
            {
                adoptFixedInputSession(modelBytes, static_cast<int>(modelHeight));
            }
            else
            {
                inputSize_ = static_cast<int>(modelHeight);
                adoptOriginalSessionIfAccelerated(modelBytes);
            }
        }
        else
        {
            adoptFixedInputSession(modelBytes, 0);
        }

        for (size_t i = 0; i < std::min<size_t>(outputNames_.size(), kStrides.size() * 2U); ++i)
        {
            const auto outputTypeInfo = session_.GetOutputTypeInfo(i);
            const auto outputInfo = outputTypeInfo.GetTensorTypeAndShapeInfo();
            if (outputInfo.GetElementType() != ONNX_TENSOR_ELEMENT_DATA_TYPE_FLOAT)
            {
                throw std::runtime_error("SCRFD model outputs must be float tensors.");
            }
        }

        inputNamePtrs_.reserve(inputNames_.size());
        outputNamePtrs_.reserve(outputNames_.size());
        for (const auto &name : inputNames_)
        {
            inputNamePtrs_.push_back(name.c_str());
        }
        for (const auto &name : outputNames_)
        {
            outputNamePtrs_.push_back(name.c_str());
        }
    }

    void ScrfdFaceDetector::adoptOriginalSessionIfAccelerated(
        const std::vector<std::uint8_t> &modelBytes)
    {
        if (accelerator_ == OrtAccelerator::None)
        {
            return;
        }
        session_ = Ort::Session(env_, modelBytes.data(), modelBytes.size(), sessionOptions_);
    }

    void ScrfdFaceDetector::adoptFixedInputSession(
        const std::vector<std::uint8_t> &modelBytes, int fallbackSize)
    {
        const bool patchable =
            inputSize_ > 0 && inputSize_ <= kMaxInputSize && inputSize_ % kStrides.back() == 0;
        const auto patched =
            patchable ? makeOnnxSpatialDimsFixed(modelBytes, inputSize_) : std::nullopt;
        if (patched)
        {
            try
            {
                Ort::Session fixedSession(env_, patched->data(), patched->size(), sessionOptions_);

                std::vector<float> probe(
                    static_cast<std::size_t>(kChannels) * inputSize_ * inputSize_, 0.0F);
                const std::array<int64_t, 4> probeShape = {1, kChannels, inputSize_, inputSize_};
                Ort::MemoryInfo memoryInfo =
                    Ort::MemoryInfo::CreateCpu(OrtArenaAllocator, OrtMemTypeDefault);
                Ort::Value probeTensor = Ort::Value::CreateTensor<float>(
                    memoryInfo, probe.data(), probe.size(), probeShape.data(), probeShape.size());

                std::vector<const char *> inputPtrs;
                std::vector<const char *> outputPtrs;
                inputPtrs.reserve(inputNames_.size());
                outputPtrs.reserve(outputNames_.size());
                for (const auto &name : inputNames_)
                {
                    inputPtrs.push_back(name.c_str());
                }
                for (const auto &name : outputNames_)
                {
                    outputPtrs.push_back(name.c_str());
                }
                fixedSession.Run(Ort::RunOptions{nullptr},
                    inputPtrs.data(),
                    &probeTensor,
                    1,
                    outputPtrs.data(),
                    outputPtrs.size());

                session_ = std::move(fixedSession);
                return;
            }
            catch (const Ort::Exception &)
            {
                if (fallbackSize > 0 && accelerator_ != OrtAccelerator::None)
                {
                    throw;
                }
            }
            // The probe only decides whether this input size works; the fallback below
            // retries with another one.
            // NOLINTNEXTLINE(bugprone-empty-catch)
            catch (const std::exception &)
            {
            }
        }
        if (fallbackSize > 0)
        {
            inputSize_ = fallbackSize;
        }
        adoptOriginalSessionIfAccelerated(modelBytes);
    }

    DetectionResult ScrfdFaceDetector::detect(
        const cv::Mat &bgrImage, float scoreThreshold, float nmsThreshold)
    {
        if (bgrImage.empty())
        {
            return {};
        }

        auto prepared = prepare(bgrImage);
        std::array<int64_t, 4> inputShape = {1, kChannels, inputSize_, inputSize_};

        Ort::MemoryInfo memoryInfo =
            Ort::MemoryInfo::CreateCpu(OrtArenaAllocator, OrtMemTypeDefault);
        Ort::Value inputTensor = Ort::Value::CreateTensor<float>(memoryInfo,
            prepared.tensor.data(),
            prepared.tensor.size(),
            inputShape.data(),
            inputShape.size());

        std::vector<Ort::Value> outputs;
        {
            const std::scoped_lock runLock(runMutex_);
            outputs = session_.Run(Ort::RunOptions{nullptr},
                inputNamePtrs_.data(),
                &inputTensor,
                1,
                outputNamePtrs_.data(),
                outputNamePtrs_.size());
        }

        return decode(outputs, prepared, scoreThreshold, nmsThreshold);
    }

    ScrfdFaceDetector::PreparedImage ScrfdFaceDetector::prepare(const cv::Mat &bgrImage) const
    {
        PreparedImage prepared;
        prepared.originalWidth = bgrImage.cols;
        prepared.originalHeight = bgrImage.rows;

        const float scale =
            std::min(static_cast<float>(inputSize_) / static_cast<float>(bgrImage.cols),
                static_cast<float>(inputSize_) / static_cast<float>(bgrImage.rows));
        prepared.scale = scale;
        prepared.resizedWidth =
            std::max(1, static_cast<int>(std::round(static_cast<float>(bgrImage.cols) * scale)));
        prepared.resizedHeight =
            std::max(1, static_cast<int>(std::round(static_cast<float>(bgrImage.rows) * scale)));

        cv::Mat resized;
        cv::resize(bgrImage, resized, cv::Size(prepared.resizedWidth, prepared.resizedHeight));

        cv::Mat canvas(inputSize_, inputSize_, CV_8UC3, cv::Scalar(0, 0, 0));
        resized.copyTo(canvas(cv::Rect(0, 0, resized.cols, resized.rows)));

        cv::Mat rgb;
        cv::cvtColor(canvas, rgb, cv::COLOR_BGR2RGB);

        prepared.tensor.resize(static_cast<std::size_t>(kChannels) * inputSize_ * inputSize_);
        const int planeSize = inputSize_ * inputSize_;
        for (int y = 0; y < inputSize_; ++y)
        {
            const auto *row = rgb.ptr<cv::Vec3b>(y);
            for (int x = 0; x < inputSize_; ++x)
            {
                const int offset = y * inputSize_ + x;
                prepared.tensor[offset] = (static_cast<float>(row[x][0]) - 127.5F) / 128.0F;
                prepared.tensor[planeSize + offset] =
                    (static_cast<float>(row[x][1]) - 127.5F) / 128.0F;
                prepared.tensor[2 * planeSize + offset] =
                    (static_cast<float>(row[x][2]) - 127.5F) / 128.0F;
            }
        }

        return prepared;
    }

    DetectionResult ScrfdFaceDetector::decode(const std::vector<Ort::Value> &outputs,
        const PreparedImage &prepared,
        float scoreThreshold,
        float nmsThreshold) const
    {
        constexpr auto featureMapCount = kStrides.size();
        if (outputs.size() < featureMapCount * 2U)
        {
            throw std::runtime_error("SCRFD output tensor count is too small.");
        }

        struct StrideOutput
        {
            const Ort::Value *value = nullptr;
            size_t elementCount = 0;
        };

        std::vector<StrideOutput> scoreOutputs;
        std::vector<StrideOutput> bboxOutputs;
        std::vector<StrideOutput> keypointOutputs;
        for (const auto &output : outputs)
        {
            if (!output.IsTensor())
            {
                continue;
            }
            const auto info = output.GetTensorTypeAndShapeInfo();
            if (info.GetElementType() != ONNX_TENSOR_ELEMENT_DATA_TYPE_FLOAT)
            {
                continue;
            }
            const auto shape = info.GetShape();
            if (shape.empty())
            {
                continue;
            }
            const auto elementCount = info.GetElementCount();
            if (elementCount == 0
                || elementCount > static_cast<size_t>(std::numeric_limits<int>::max()))
            {
                continue;
            }
            const auto lastDim = shape.back();
            if (lastDim == 1)
            {
                scoreOutputs.push_back({&output, elementCount});
            }
            else if (lastDim == 4)
            {
                bboxOutputs.push_back({&output, elementCount});
            }
            else if (lastDim == kLandmarkCoordinateCount)
            {
                keypointOutputs.push_back({&output, elementCount});
            }
        }

        FaceDetections detections;

        for (size_t index = 0; index < featureMapCount; ++index)
        {
            const int stride = kStrides[index];
            const int featureHeight = inputSize_ / stride;
            const int featureWidth = inputSize_ / stride;
            auto anchors = anchorCenters(featureHeight, featureWidth, stride);
            const int baseAnchorCount = static_cast<int>(anchors.size());
            if (baseAnchorCount <= 0)
            {
                continue;
            }

            const StrideOutput *scoreMatch = nullptr;
            for (const auto &candidate : scoreOutputs)
            {
                if (candidate.elementCount == static_cast<size_t>(baseAnchorCount)
                    || candidate.elementCount
                           == static_cast<size_t>(baseAnchorCount) * kMaxAnchorsPerLocation)
                {
                    scoreMatch = &candidate;
                    break;
                }
            }

            const StrideOutput *bboxMatch = nullptr;
            if (scoreMatch != nullptr)
            {
                for (const auto &candidate : bboxOutputs)
                {
                    if (candidate.elementCount == scoreMatch->elementCount * 4U)
                    {
                        bboxMatch = &candidate;
                        break;
                    }
                }
            }

            const StrideOutput *keypointMatch = nullptr;
            if (scoreMatch != nullptr)
            {
                for (const auto &candidate : keypointOutputs)
                {
                    if (candidate.elementCount
                        == scoreMatch->elementCount * kLandmarkCoordinateCount)
                    {
                        keypointMatch = &candidate;
                        break;
                    }
                }
            }

            if (scoreMatch == nullptr || bboxMatch == nullptr)
            {
                throw std::runtime_error(
                    "Could not match SCRFD score and box outputs for this model.");
            }

            const auto scoreCount = static_cast<int>(scoreMatch->elementCount);
            const int maxExpectedScores = baseAnchorCount * kMaxAnchorsPerLocation;
            if (scoreCount > maxExpectedScores)
            {
                throw std::runtime_error("SCRFD output tensor shape is unexpectedly large.");
            }
            if (scoreCount > baseAnchorCount)
            {
                const int repeats = std::max(1, scoreCount / baseAnchorCount);
                std::vector<cv::Point2f> repeatedAnchors;
                repeatedAnchors.reserve(
                    static_cast<size_t>(baseAnchorCount) * static_cast<size_t>(repeats));
                for (const auto &anchor : anchors)
                {
                    for (int repeat = 0; repeat < repeats; ++repeat)
                    {
                        repeatedAnchors.push_back(anchor);
                    }
                }
                anchors = std::move(repeatedAnchors);
            }

            const auto *scores = scoreMatch->value->GetTensorData<float>();
            const auto *boxes = bboxMatch->value->GetTensorData<float>();
            const auto *keypoints =
                keypointMatch != nullptr ? keypointMatch->value->GetTensorData<float>() : nullptr;
            if (scores == nullptr || boxes == nullptr)
            {
                continue;
            }
            const int anchorsToRead = std::min(scoreCount, static_cast<int>(anchors.size()));

            for (int i = 0; i < anchorsToRead; ++i)
            {
                const float score = scores[i];
                if (!std::isfinite(score) || score < scoreThreshold)
                {
                    continue;
                }

                std::array<float, 4> distances = {
                    boxes[static_cast<std::ptrdiff_t>(i) * 4] * static_cast<float>(stride),
                    boxes[i * 4 + 1] * static_cast<float>(stride),
                    boxes[i * 4 + 2] * static_cast<float>(stride),
                    boxes[i * 4 + 3] * static_cast<float>(stride),
                };
                if (!std::ranges::all_of(distances,
                        [](const float value)
                        {
                            return std::isfinite(value);
                        }))
                {
                    continue;
                }
                const auto modelBox = distanceToBox(anchors[i], distances.data());
                float x = modelBox.x / prepared.scale;
                float y = modelBox.y / prepared.scale;
                float width = modelBox.width / prepared.scale;
                float height = modelBox.height / prepared.scale;
                if (!std::isfinite(x) || !std::isfinite(y) || !std::isfinite(width)
                    || !std::isfinite(height))
                {
                    continue;
                }

                x = std::clamp(x, 0.0F, static_cast<float>(prepared.originalWidth - 1));
                y = std::clamp(y, 0.0F, static_cast<float>(prepared.originalHeight - 1));
                const float right =
                    std::clamp(x + width, x + 1.0F, static_cast<float>(prepared.originalWidth));
                const float bottom =
                    std::clamp(y + height, y + 1.0F, static_cast<float>(prepared.originalHeight));
                FaceDetection detection{cv::Rect2f(x, y, right - x, bottom - y), score};
                if (keypoints != nullptr)
                {
                    std::array<cv::Point2f, kFaceLandmarkCount> landmarks;
                    for (int landmark = 0; landmark < kFaceLandmarkCount; ++landmark)
                    {
                        const int offset = i * kLandmarkCoordinateCount + landmark * 2;
                        landmarks[landmark] = {
                            (anchors[i].x + keypoints[offset] * static_cast<float>(stride))
                                / prepared.scale,
                            (anchors[i].y + keypoints[offset + 1] * static_cast<float>(stride))
                                / prepared.scale,
                        };
                    }
                    if (const auto roll = faceRollFromLandmarks(landmarks, detection.box))
                    {
                        detection.rollRadians = *roll;
                        detection.hasPose = true;
                    }
                }
                detections.push_back(detection);
            }
        }

        std::size_t omitted = 0;
        if (detections.size() > kMaxCandidatesBeforeNms)
        {
            std::ranges::partial_sort(detections,
                detections.begin() + static_cast<std::ptrdiff_t>(kMaxCandidatesBeforeNms),
                [](const FaceDetection &a, const FaceDetection &b)
                {
                    return a.score > b.score;
                });
            omitted = detections.size() - kMaxCandidatesBeforeNms;
            detections.resize(kMaxCandidatesBeforeNms);
        }

        return {nonMaxSuppression(std::move(detections), nmsThreshold),
            static_cast<int>(std::min<std::size_t>(omitted, std::numeric_limits<int>::max()))};
    }

    std::vector<cv::Point2f> ScrfdFaceDetector::anchorCenters(
        int featureHeight, int featureWidth, int stride)
    {
        std::vector<cv::Point2f> anchors;
        anchors.reserve(static_cast<size_t>(featureHeight) * static_cast<size_t>(featureWidth));
        for (int y = 0; y < featureHeight; ++y)
        {
            for (int x = 0; x < featureWidth; ++x)
            {
                anchors.emplace_back(
                    static_cast<float>(x * stride), static_cast<float>(y * stride));
            }
        }
        return anchors;
    }
}

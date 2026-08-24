#include "cloakframe/YuNetFaceDetector.hpp"

#include "cloakframe/FaceDetection.hpp"

#include <QByteArrayView>
#include <QCryptographicHash>

#include <opencv2/imgproc.hpp>

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <optional>
#include <stdexcept>
#include <vector>

namespace cloakframe
{
    namespace
    {
        constexpr int kInputSize = 640;
        constexpr int kFaceOutputColumns = 15;
        constexpr int kMaxDetections = 5'000;
        constexpr std::uintmax_t kMaxModelFileBytes = 512ULL * 1024ULL * 1024ULL;

        std::vector<uchar> readModelFile(
            const std::filesystem::path &path, const QByteArray &expectedSha256)
        {
            std::error_code sizeError;
            const auto size = std::filesystem::file_size(path, sizeError);
            if (sizeError || size == 0 || size > kMaxModelFileBytes)
            {
                throw std::runtime_error("Could not read the model file.");
            }
            std::ifstream stream(path, std::ios::binary);
            std::vector<uchar> bytes(static_cast<std::size_t>(size));
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

    YuNetFaceDetector::YuNetFaceDetector(
        const std::string &modelPath, const QByteArray &expectedSha256)
    {
        const std::u8string modelU8(modelPath.begin(), modelPath.end());
        const auto modelBytes = readModelFile(std::filesystem::path(modelU8), expectedSha256);
        detector_ = cv::FaceDetectorYN::create(
            "ONNX", modelBytes, {}, cv::Size(kInputSize, kInputSize), 0.5F, 0.4F, kMaxDetections);
        if (detector_.empty())
        {
            throw std::runtime_error("Could not initialize the YuNet face model.");
        }
    }

    DetectionResult YuNetFaceDetector::detect(
        const cv::Mat &bgrImage, const float scoreThreshold, const float nmsThreshold)
    {
        if (bgrImage.empty())
        {
            return {};
        }
        if (bgrImage.type() != CV_8UC3)
        {
            throw std::invalid_argument("YuNet requires an 8-bit BGR image.");
        }

        const float scale =
            std::min(static_cast<float>(kInputSize) / static_cast<float>(bgrImage.cols),
                static_cast<float>(kInputSize) / static_cast<float>(bgrImage.rows));
        const int resizedWidth =
            std::max(1, static_cast<int>(static_cast<float>(bgrImage.cols) * scale));
        const int resizedHeight =
            std::max(1, static_cast<int>(static_cast<float>(bgrImage.rows) * scale));
        const float padX = static_cast<float>(kInputSize - resizedWidth) / 2.0F;
        const float padY = static_cast<float>(kInputSize - resizedHeight) / 2.0F;

        cv::Mat resized;
        cv::resize(
            bgrImage, resized, cv::Size(resizedWidth, resizedHeight), 0.0, 0.0, cv::INTER_LINEAR);
        cv::Mat canvas(kInputSize, kInputSize, CV_8UC3, cv::Scalar(0, 0, 0));
        resized.copyTo(canvas(
            cv::Rect(static_cast<int>(padX), static_cast<int>(padY), resizedWidth, resizedHeight)));

        cv::Mat faces;
        {
            const std::scoped_lock lock(detectMutex_);
            detector_->setScoreThreshold(std::clamp(scoreThreshold, 0.0F, 1.0F));
            detector_->setNMSThreshold(std::clamp(nmsThreshold, 0.0F, 1.0F));
            detector_->detect(canvas, faces);
        }
        if (faces.empty())
        {
            return {};
        }
        if (faces.type() != CV_32F || faces.cols < kFaceOutputColumns || faces.rows < 0
            || faces.rows > kMaxDetections)
        {
            throw std::runtime_error("YuNet returned an invalid detection matrix.");
        }

        FaceDetections detections;
        detections.reserve(static_cast<std::size_t>(faces.rows));
        for (int rowIndex = 0; rowIndex < faces.rows; ++rowIndex)
        {
            const float *row = faces.ptr<float>(rowIndex);
            const float score = row[14];
            const float x1 = (row[0] - padX) / scale;
            const float y1 = (row[1] - padY) / scale;
            const float x2 = (row[0] + row[2] - padX) / scale;
            const float y2 = (row[1] + row[3] - padY) / scale;
            if (!std::isfinite(score) || score < scoreThreshold || !std::isfinite(x1)
                || !std::isfinite(y1) || !std::isfinite(x2) || !std::isfinite(y2))
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
                    (row[4 + point * 2] - padX) / scale, (row[5 + point * 2] - padY) / scale};
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
            detections.push_back(detection);
        }
        return {std::move(detections), 0};
    }
}

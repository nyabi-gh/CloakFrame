#include "cloakframe/ProcessorWorker.hpp"

#include "cloakframe/DetectionGeometry.hpp"
#include "cloakframe/ImageIo.hpp"
#include "cloakframe/ImageScanner.hpp"
#include "cloakframe/MemoryBudget.hpp"
#include "cloakframe/Mosaic.hpp"
#include "cloakframe/OrderedParallel.hpp"
#include "cloakframe/OutputPlan.hpp"
#include "cloakframe/PathSafety.hpp"
#include "cloakframe/PathUtil.hpp"
#include "cloakframe/PlateDetector.hpp"
#include "cloakframe/ReviewTypes.hpp"
#include "cloakframe/ScrfdFaceDetector.hpp"
#include "cloakframe/StageCleanup.hpp"
#include "cloakframe/VideoIo.hpp"
#include "cloakframe/VideoProcessor.hpp"
#include "cloakframe/VideoReviewTypes.hpp"
#include "cloakframe/Yolo5FaceDetector.hpp"
#include "cloakframe/YuNetFaceDetector.hpp"

#include <QCoreApplication>
#include <QDir>
#include <QElapsedTimer>
#include <QImageReader>
#include <QMetaObject>
#include <QRectF>
#include <QSize>
#include <QStringList>

#include <opencv2/imgcodecs.hpp>
#include <opencv2/imgproc.hpp>

#include <spdlog/spdlog.h>

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <exception>
#include <filesystem>
#include <limits>
#include <system_error>
#include <thread>

namespace cloakframe
{
    namespace
    {
        constexpr std::uintmax_t kMaxInputFileBytes = 2ULL * 1024ULL * 1024ULL * 1024ULL;
        constexpr long long kMaxPixelCount = 512LL * 1000LL * 1000LL;
        constexpr long long kMaxJpegPixelCount = 512LL * 1000LL * 1000LL;
        constexpr std::uint64_t kImageMinimumMemoryBudget = 512ULL * 1024ULL * 1024ULL;
        constexpr std::uint64_t kImageMaximumMemoryBudget = 16ULL * 1024ULL * 1024ULL * 1024ULL;
        constexpr std::uint64_t kEstimatedImageBytesPerPixel = 16;
        constexpr int kVideoDetectionInputSize = 960;

        constexpr int kReviewMaxLongEdge = 1600;

        // A long film can leave dozens of holes. Listing all of them would bury the rest of
        // the log, so the message names the first few and says how many it withheld.
        constexpr std::size_t kMaxReportedUncoveredSpans = 12;

        void logDetectorStages(
            const char *label, const DetectorStageTimings &before, const Detector *detector)
        {
            if (detector == nullptr)
            {
                return;
            }
            const auto after = detector->stageTimings();
            const long long preprocess = after.preprocessMicros - before.preprocessMicros;
            const long long inference = after.inferenceMicros - before.inferenceMicros;
            const long long postprocess = after.postprocessMicros - before.postprocessMicros;
            if (preprocess + inference + postprocess <= 0)
            {
                return;
            }
            spdlog::info("Detector timing ({}): preprocess {} ms, inference {} ms, "
                         "postprocess {} ms",
                label,
                preprocess / 1000,
                inference / 1000,
                postprocess / 1000);
        }

        QString formatUncoveredSpans(const std::vector<UncoveredSpan> &spans)
        {
            QStringList parts;
            const auto listed = std::min(spans.size(), kMaxReportedUncoveredSpans);
            for (std::size_t i = 0; i < listed; ++i)
            {
                const auto &span = spans[i];
                parts << (span.firstFrame == span.lastFrame
                              ? QString::number(span.firstFrame)
                              : QStringLiteral("%1-%2").arg(span.firstFrame).arg(span.lastFrame));
            }
            return parts.join(QStringLiteral(", "));
        }

        std::shared_ptr<Detector> makeFaceDetector(const FaceModelKind kind,
            const QString &modelPath,
            const int scrfdInputSize,
            const bool enableAcceleration,
            const QByteArray &expectedSha256)
        {
            const auto path = pathToUtf8(pathFromQString(modelPath));
            switch (kind)
            {
            case FaceModelKind::Yolo5Face:
                return std::make_shared<Yolo5FaceDetector>(
                    path, enableAcceleration, expectedSha256);
            case FaceModelKind::YuNet:
                return std::make_shared<YuNetFaceDetector>(path, expectedSha256);
            case FaceModelKind::Scrfd:
                return std::make_shared<ScrfdFaceDetector>(
                    path, scrfdInputSize, enableAcceleration, expectedSha256);
            }
            throw std::invalid_argument("Unsupported face model kind.");
        }

        std::uint64_t megabytesForDisplay(const std::uint64_t bytes)
        {
            constexpr std::uint64_t kBytesPerMegabyte = 1024ULL * 1024ULL;
            return (bytes + kBytesPerMegabyte - 1) / kBytesPerMegabyte;
        }

        struct ImageDimensionCheck
        {
            bool ok = false;
            QString reason;
            QSize size;
            long long pixelLimit = kMaxPixelCount;
        };

        long long imagePixelLimit(const QByteArray &format)
        {
            const QByteArray normalized = format.toLower();
            return normalized == "jpeg" || normalized == "jpg" ? kMaxJpegPixelCount
                                                               : kMaxPixelCount;
        }

        class ImageMemoryReservation
        {
        public:
            ImageMemoryReservation(std::mutex &mutex,
                std::condition_variable &condition,
                std::uint64_t &available,
                const std::uint64_t requested,
                const std::atomic<bool> &cancelled)
                : mutex_(mutex)
                , condition_(condition)
                , available_(available)
                , amount_(requested)
            {
                std::unique_lock lock(mutex_);
                condition_.wait(lock,
                    [&]
                    {
                        return cancelled.load(std::memory_order_acquire) || available_ >= amount_;
                    });
                if (!cancelled.load(std::memory_order_acquire))
                {
                    available_ -= amount_;
                    reserved_ = true;
                }
            }

            ~ImageMemoryReservation()
            {
                if (!reserved_)
                {
                    return;
                }
                {
                    const std::lock_guard lock(mutex_);
                    available_ += amount_;
                }
                condition_.notify_all();
            }

            ImageMemoryReservation(const ImageMemoryReservation &) = delete;
            ImageMemoryReservation &operator=(const ImageMemoryReservation &) = delete;

            [[nodiscard]] bool acquired() const
            {
                return reserved_;
            }

        private:
            std::mutex &mutex_;
            std::condition_variable &condition_;
            std::uint64_t &available_;
            std::uint64_t amount_ = 0;
            bool reserved_ = false;
        };

        ImageDimensionCheck inspectImageDimensions(const std::filesystem::path &source)
        {
            QImageReader reader(pathToQString(source));
            reader.setAutoTransform(false);

            const QSize size = reader.size();
            const long long pixelLimit = imagePixelLimit(reader.format());
            if (!size.isValid() || size.width() <= 0 || size.height() <= 0)
            {
                return {false,
                    QCoreApplication::translate(
                        "cloakframe::ProcessorWorker", "cannot inspect image dimensions"),
                    {}};
            }

            const long long pixelCount =
                static_cast<long long>(size.width()) * static_cast<long long>(size.height());
            if (pixelCount > pixelLimit)
            {
                return {false,
                    QCoreApplication::translate(
                        "cloakframe::ProcessorWorker", "image too large, %1 x %2")
                        .arg(size.width())
                        .arg(size.height()),
                    size,
                    pixelLimit};
            }

            return {true, {}, size, pixelLimit};
        }

        unsigned imageParallelism(
            const std::vector<ScanResult> &items, const std::vector<std::size_t> &indexes)
        {
            std::uint64_t largestEstimate = 1;
            for (const auto index : indexes)
            {
                const auto dimensions = inspectImageDimensions(items[index].sourcePath);
                if (!dimensions.size.isValid())
                {
                    continue;
                }
                const auto pixels = static_cast<std::uint64_t>(dimensions.size.width())
                                    * static_cast<std::uint64_t>(dimensions.size.height());
                std::error_code sizeError;
                const auto fileSize =
                    std::filesystem::file_size(items[index].sourcePath, sizeError);
                const auto encodedBytes = sizeError ? 0 : static_cast<std::uint64_t>(fileSize);
                largestEstimate =
                    std::max(largestEstimate, estimatedImageMemoryBytes(pixels, encodedBytes));
            }

            const auto memoryLimit = static_cast<unsigned>(
                std::clamp<std::uint64_t>(imageMemoryBudget() / largestEstimate, 1, 8));
            const unsigned hardware = std::max(1U, std::thread::hardware_concurrency());
            return std::min(memoryLimit, hardware);
        }

        QImage matToQImage(const cv::Mat &bgr)
        {
            QImage image(
                bgr.data, bgr.cols, bgr.rows, static_cast<int>(bgr.step), QImage::Format_BGR888);
            return image.copy();
        }

        std::pair<QImage, double> makeReviewPreview(const cv::Mat &bgr)
        {
            const int longEdge = std::max(bgr.cols, bgr.rows);
            if (longEdge <= kReviewMaxLongEdge)
            {
                return {matToQImage(bgr), 1.0};
            }
            const double scale = static_cast<double>(kReviewMaxLongEdge) / longEdge;
            const int newW = std::max(1, static_cast<int>(std::round(bgr.cols * scale)));
            const int newH = std::max(1, static_cast<int>(std::round(bgr.rows * scale)));
            cv::Mat resized;
            cv::resize(bgr, resized, cv::Size(newW, newH), 0.0, 0.0, cv::INTER_AREA);
            return {matToQImage(resized), scale};
        }

        QVector<QRectF> scaleRects(const QVector<QRectF> &rects, double factor)
        {
            if (factor == 1.0)
            {
                return rects;
            }
            QVector<QRectF> scaled;
            scaled.reserve(rects.size());
            for (const auto &rect : rects)
            {
                scaled.push_back(QRectF(rect.x() * factor,
                    rect.y() * factor,
                    rect.width() * factor,
                    rect.height() * factor));
            }
            return scaled;
        }

        FaceDetections toDetections(
            const QVector<QRectF> &boxes, const FaceDetections &references = {})
        {
            FaceDetections result;
            result.reserve(boxes.size());
            for (const auto &rect : boxes)
            {
                FaceDetection det;
                det.box = cv::Rect2f(static_cast<float>(rect.x()),
                    static_cast<float>(rect.y()),
                    static_cast<float>(rect.width()),
                    static_cast<float>(rect.height()));
                det.score = 1.0F;
                const FaceDetection *bestReference = nullptr;
                float bestIou = 0.0F;
                for (const auto &reference : references)
                {
                    const float iou = intersectionOverUnion(det.box, reference.box);
                    if (iou > bestIou)
                    {
                        bestIou = iou;
                        bestReference = &reference;
                    }
                }
                constexpr float kPoseTransferIou = 0.6F;
                if (bestReference != nullptr && bestIou >= kPoseTransferIou
                    && hasValidFacePose(*bestReference))
                {
                    det.rollRadians = bestReference->rollRadians;
                    det.hasPose = true;
                }
                result.push_back(det);
            }
            return result;
        }

        QVector<QRectF> toQRects(const FaceDetections &faces)
        {
            QVector<QRectF> result;
            result.reserve(static_cast<int>(faces.size()));
            for (const auto &face : faces)
            {
                result.push_back(QRectF(face.box.x, face.box.y, face.box.width, face.box.height));
            }
            return result;
        }
    }

    std::uint64_t estimatedImageMemoryBytes(
        const std::uint64_t pixels, const std::uint64_t encodedBytes)
    {
        constexpr auto maximum = std::numeric_limits<std::uint64_t>::max();
        if (pixels > (maximum - encodedBytes) / kEstimatedImageBytesPerPixel)
        {
            return maximum;
        }
        return pixels * kEstimatedImageBytesPerPixel + encodedBytes;
    }

    std::uint64_t imageMemoryBudget()
    {
        return adaptiveMemoryBudget(kImageMinimumMemoryBudget, kImageMaximumMemoryBudget, 4);
    }

    struct ProcessorWorker::ItemOutcome
    {
        QStringList logs;
        int redacted = 0;
        int copied = 0;
        int skipped = 0;
        int failed = 0;
        int unredacted = 0;
        int warnings = 0;
        qint64 omittedRegions = 0;
        qint64 trackingGapFrames = 0;
        qint64 droppedTracks = 0;
        bool cancelled = false;
    };

    ProcessorWorker::ProcessorWorker(ProcessingRequest request, DetectorCache cache)
        : modelPath_(std::move(request.modelPath))
        , modelSha256_(std::move(request.modelSha256))
        , faceModelKind_(request.faceModelKind)
        , inputs_(std::move(request.inputs))
        , outputDirectory_(std::move(request.outputDirectory))
        , recursive_(request.recursive)
        , scoreThreshold_(request.scoreThreshold)
        , nmsThreshold_(request.nmsThreshold)
        , mosaicBlockSize_(request.mosaicBlockSize)
        , paddingRatio_(request.paddingRatio)
        , method_(request.method)
        , customImage_(std::move(request.customImage))
        , shape_(request.shape)
        , softEdges_(request.softEdges)
        , preserveMetadata_(request.preserveMetadata)
        , reviewEnabled_(request.reviewEnabled)
        , reviewReceiver_(request.reviewReceiver)
        , detectFaces_(request.detectFaces)
        , detectPlates_(request.detectPlates)
        , plateModelPath_(std::move(request.plateModelPath))
        , plateModelSha256_(std::move(request.plateModelSha256))
        , gpuAcceleration_(request.gpuAcceleration)
        , videoCrf_(request.videoCrf)
        , videoCodec_(request.videoCodec)
        , imageMemoryBudget_(imageMemoryBudget())
        , imageMemoryAvailable_(imageMemoryBudget_)
        , detector_(std::move(cache.face))
        , plateDetector_(std::move(cache.plate))
        , videoDetector_(std::move(cache.videoFace))
    {
    }

    ProcessorWorker::~ProcessorWorker() = default;

    std::shared_ptr<Detector> ProcessorWorker::takeDetector()
    {
        return std::move(detector_);
    }

    std::shared_ptr<PlateDetector> ProcessorWorker::takePlateDetector()
    {
        return std::move(plateDetector_);
    }

    std::shared_ptr<Detector> ProcessorWorker::takeVideoDetector()
    {
        return std::move(videoDetector_);
    }

    void ProcessorWorker::process()
    {
        try
        {
            // Review that was promised but cannot be shown has to stop the run. Falling back to
            // writing the unreviewed detections is the coverage claim SECURITY.md treats as a
            // vulnerability, not a degraded mode.
            if (reviewEnabled_ && !reviewReceiver_)
            {
                emit logMessage(tr("Error: review is on but no review window is available. "
                                   "Nothing was processed."));
                emit finished(RunOutcome::Failed);
                return;
            }

            if (detectFaces_)
            {
                if (!detector_)
                {
                    emit logMessage(tr("Loading face detection model..."));
                    try
                    {
                        detector_ = makeFaceDetector(
                            faceModelKind_, modelPath_, 640, gpuAcceleration_, modelSha256_);
                        const cv::Mat warmupFrame(detector_->inputSize(),
                            detector_->inputSize(),
                            CV_8UC3,
                            cv::Scalar(0, 0, 0));
                        detector_->detect(warmupFrame, scoreThreshold_, nmsThreshold_);
                    }
                    catch (const Ort::Exception &)
                    {
                        emit logMessage(tr("GPU acceleration can't run the face model; "
                                           "using the CPU instead."));
                        detector_ =
                            makeFaceDetector(faceModelKind_, modelPath_, 640, false, modelSha256_);
                        const cv::Mat warmupFrame(detector_->inputSize(),
                            detector_->inputSize(),
                            CV_8UC3,
                            cv::Scalar(0, 0, 0));
                        detector_->detect(warmupFrame, scoreThreshold_, nmsThreshold_);
                    }
                }
                else
                {
                    emit logMessage(tr("Reusing loaded face detection model."));
                }
                emit logMessage(tr("Face detection backend: %1")
                        .arg(QString::fromLatin1(detector_->backendName())));
            }

            if (detectPlates_)
            {
                if (!plateDetector_)
                {
                    emit logMessage(tr("Loading license plate detection model..."));
                    try
                    {
                        plateDetector_ = std::make_shared<PlateDetector>(
                            pathToUtf8(pathFromQString(plateModelPath_)),
                            gpuAcceleration_,
                            plateModelSha256_);
                        const cv::Mat warmupFrame(512, 512, CV_8UC3, cv::Scalar(0, 0, 0));
                        plateDetector_->detect(warmupFrame, scoreThreshold_, nmsThreshold_);
                    }
                    catch (const Ort::Exception &)
                    {
                        emit logMessage(tr("GPU acceleration can't run the license plate model; "
                                           "using the CPU instead."));
                        plateDetector_ = std::make_shared<PlateDetector>(
                            pathToUtf8(pathFromQString(plateModelPath_)), false, plateModelSha256_);
                        const cv::Mat warmupFrame(512, 512, CV_8UC3, cv::Scalar(0, 0, 0));
                        plateDetector_->detect(warmupFrame, scoreThreshold_, nmsThreshold_);
                    }
                }
                else
                {
                    emit logMessage(tr("Reusing loaded license plate detection model."));
                }
                emit logMessage(tr("License plate detection backend: %1")
                        .arg(QString::fromLatin1(
                            ortAcceleratorName(plateDetector_->accelerator()))));
            }

            emit logMessage(tr("Scanning inputs..."));
            std::vector<ScanIssue> scanIssues;
            const auto images = scanMedia(inputs_, recursive_, true, &scanIssues);
            const int total = static_cast<int>(images.size());
            emit logMessage(tr("Preflight: found %n supported file(s).", nullptr, total));

            constexpr std::size_t kReportedScanIssues = 10;
            for (std::size_t i = 0; i < scanIssues.size() && i < kReportedScanIssues; ++i)
            {
                emit logMessage(tr("Could not read input '%1': %2")
                        .arg(pathToQString(scanIssues[i].path),
                            QString::fromStdString(scanIssues[i].error.message())));
            }
            if (scanIssues.size() > kReportedScanIssues)
            {
                emit logMessage(tr("Additional unreadable inputs omitted."));
            }
            if (!scanIssues.empty())
            {
                emit logMessage(
                    tr("Warning: %n input(s) could not be read, so nothing below covers them.",
                        nullptr,
                        static_cast<int>(std::min<std::size_t>(
                            scanIssues.size(), std::numeric_limits<int>::max()))));
            }
            emit progressChanged(0, total);

            if (cancelled_.load(std::memory_order_acquire))
            {
                emit finished(RunOutcome::Cancelled);
                return;
            }

            if (total == 0)
            {
                emit logMessage(tr("No supported files were found."));
                emit finished(RunOutcome::Failed);
                return;
            }

            const auto outputRoot = pathFromQString(outputDirectory_);
            std::error_code mkdirError;
            std::filesystem::create_directories(outputRoot, mkdirError);
            if (mkdirError)
            {
                emit logMessage(tr("Cannot create output directory: %1")
                        .arg(QString::fromStdString(mkdirError.message())));
                emit finished(RunOutcome::Failed);
                return;
            }

            std::error_code canonicalError;
            const auto canonicalRoot = std::filesystem::canonical(outputRoot, canonicalError);
            const auto safeRoot = canonicalError ? outputRoot.lexically_normal() : canonicalRoot;

            const auto outputConflicts = findOutputConflicts(images, safeRoot);
            if (!outputConflicts.empty())
            {
                emit logMessage(tr("Refusing to run because an output path is already in use."));
                for (const auto &conflict : outputConflicts)
                {
                    if (conflict.kind == OutputConflict::Kind::DuplicateDestination)
                    {
                        emit logMessage(
                            tr("Output name collision: '%1' and '%2' would both write to '%3'")
                                .arg(pathToQString(conflict.otherSource),
                                    pathToQString(conflict.source),
                                    pathToQString(conflict.destination)));
                    }
                    else
                    {
                        emit logMessage(tr("Existing output would be overwritten: '%1'")
                                .arg(pathToQString(conflict.destination)));
                    }
                }
                if (outputConflicts.size() >= 10)
                    emit logMessage(tr("Additional output conflicts omitted."));
                emit finished(RunOutcome::Failed);
                return;
            }
            emit logMessage(tr("Preflight: output paths are available."));

            int completed = 0;
            int redactedCount = 0;
            int copiedCount = 0;
            int skippedCount = 0;
            int failedCount = 0;
            int unredactedCount = 0;
            int warningCount = 0;
            qint64 omittedRegionCount = 0;
            qint64 trackingGapFrameCount = 0;
            qint64 droppedTrackCount = 0;
            int uncoveredFileCount = 0;

            const auto applyOutcome = [&](ItemOutcome &&outcome)
            {
                for (const auto &message : outcome.logs)
                {
                    emit logMessage(message);
                }
                redactedCount += outcome.redacted;
                copiedCount += outcome.copied;
                skippedCount += outcome.skipped;
                failedCount += outcome.failed;
                unredactedCount += outcome.unredacted;
                warningCount += outcome.warnings;
                omittedRegionCount += outcome.omittedRegions;
                trackingGapFrameCount += outcome.trackingGapFrames;
                droppedTrackCount += outcome.droppedTracks;
                if (outcome.omittedRegions > 0 || outcome.trackingGapFrames > 0
                    || outcome.droppedTracks > 0)
                {
                    ++uncoveredFileCount;
                }
                if (!outcome.cancelled)
                {
                    emit progressChanged(++completed, total);
                }
            };

            if (reviewEnabled_)
            {
                int index = 0;
                for (const auto &item : images)
                {
                    ++index;
                    if (cancelled_.load(std::memory_order_acquire))
                    {
                        break;
                    }
                    auto outcome = processItem(item, safeRoot, index, total, true);
                    const bool stop = outcome.cancelled;
                    applyOutcome(std::move(outcome));
                    if (stop)
                    {
                        break;
                    }
                }
            }
            else
            {
                std::vector<std::size_t> imageIndexes;
                std::vector<std::size_t> videoIndexes;
                for (std::size_t i = 0; i < images.size(); ++i)
                {
                    (isSupportedVideo(images[i].sourcePath) ? videoIndexes : imageIndexes)
                        .push_back(i);
                }

                const unsigned threadCount = imageParallelism(images, imageIndexes);
                processOrdered<ItemOutcome>(
                    imageIndexes.size(),
                    threadCount,
                    threadCount,
                    cancelled_,
                    [&](std::size_t k)
                    {
                        const auto i = imageIndexes[k];
                        return processItem(
                            images[i], safeRoot, static_cast<int>(i) + 1, total, false);
                    },
                    [&](std::size_t, ItemOutcome &&outcome)
                    {
                        applyOutcome(std::move(outcome));
                    });

                for (const auto i : videoIndexes)
                {
                    if (cancelled_.load(std::memory_order_acquire))
                    {
                        break;
                    }
                    auto outcome =
                        processItem(images[i], safeRoot, static_cast<int>(i) + 1, total, false);
                    const bool stop = outcome.cancelled;
                    applyOutcome(std::move(outcome));
                    if (stop)
                    {
                        break;
                    }
                }
            }

            emit logMessage(tr("Summary: %1 redacted, %2 saved without redaction, %3 copied, %4 "
                               "skipped, %5 failed (of %6).")
                    .arg(redactedCount)
                    .arg(unredactedCount)
                    .arg(copiedCount)
                    .arg(skippedCount)
                    .arg(failedCount)
                    .arg(total));

            RunSummary summary;
            summary.total = total;
            summary.redacted = redactedCount;
            summary.copied = copiedCount;
            summary.skipped = skippedCount;
            summary.failed = failedCount;
            summary.unredacted = unredactedCount;
            summary.omittedRegions = omittedRegionCount;
            summary.trackingGapFrames = trackingGapFrameCount;
            summary.droppedTracks = droppedTrackCount;
            summary.coverageWarningFiles = uncoveredFileCount;
            summary.warningFiles = warningCount;
            summary.unreadableInputs = static_cast<qint64>(scanIssues.size());
            emit summaryAvailable(summary);

            if (unredactedCount > 0)
            {
                emit logMessage(tr("Warning: %n image(s) were saved with no regions redacted. "
                                   "Check them before sharing.",
                    nullptr,
                    unredactedCount));
            }

            // The per-file messages already say what was left over and in what unit. This
            // one only has to say how many files carry any of it.
            if (uncoveredFileCount > 0)
            {
                emit logMessage(tr("Warning: %n file(s) have detection or tracking warnings. "
                                   "Review them before sharing.",
                    nullptr,
                    uncoveredFileCount));
            }

            if (cancelled_.load(std::memory_order_acquire))
            {
                emit finished(RunOutcome::Cancelled);
                return;
            }

            if (failedCount > 0 || skippedCount > 0 || unredactedCount > 0 || copiedCount > 0
                || warningCount > 0 || uncoveredFileCount > 0 || !scanIssues.empty())
            {
                emit logMessage(tr("Completed with warnings. Review the summary before sharing."));
                emit finished(RunOutcome::CompletedWithWarnings);
            }
            else
            {
                emit logMessage(tr("Done."));
                emit finished(RunOutcome::Completed);
            }
        }
        catch (const std::exception &exception)
        {
            emit logMessage(tr("Error: %1").arg(exception.what()));
            emit finished(cancelled_.load(std::memory_order_acquire) ? RunOutcome::Cancelled
                                                                     : RunOutcome::Failed);
        }
        catch (...)
        {
            emit logMessage(tr("Unexpected error — processing stopped."));
            emit finished(cancelled_.load(std::memory_order_acquire) ? RunOutcome::Cancelled
                                                                     : RunOutcome::Failed);
        }
    }

    ProcessorWorker::ItemOutcome ProcessorWorker::processItem(const ScanResult &item,
        const std::filesystem::path &safeRoot,
        const int index,
        const int total,
        const bool allowReview)
    {
        ItemOutcome outcome;
        try
        {
            const auto source = item.sourcePath;
            const auto destination = (safeRoot / outputRelativePath(item)).lexically_normal();

            if (!destinationIsSafe(destination, safeRoot))
            {
                outcome.logs.push_back(
                    tr("Skipped unsafe output path for: %1").arg(pathToQString(source.filename())));
                outcome.skipped = 1;
                return outcome;
            }

            std::error_code sameFileError;
            if (std::filesystem::exists(destination, sameFileError)
                && std::filesystem::equivalent(source, destination, sameFileError))
            {
                outcome.logs.push_back(tr("Skipped (source and destination are the same file): %1")
                        .arg(pathToQString(source.filename())));
                outcome.skipped = 1;
                return outcome;
            }

            if (isSupportedVideo(source))
            {
                return processVideoItem(item, safeRoot, destination, index, total);
            }

            const auto sourceIdentity = captureFileIdentity(source);
            if (!sourceIdentity)
            {
                outcome.logs.push_back(
                    tr("Skipped unreadable image: %1").arg(pathToQString(source)));
                outcome.skipped = 1;
                return outcome;
            }
            const auto originalIsUnchanged = [source, expected = *sourceIdentity]
            {
                const auto current = captureFileIdentity(source);
                return current && *current == expected;
            };
            const auto recordChangedSource = [&]
            {
                outcome.logs.push_back(tr("Source file changed during processing: %1")
                        .arg(pathToQString(source.filename())));
                outcome.failed = 1;
            };

            const auto fileSize = sourceIdentity->size;
            if (fileSize > kMaxInputFileBytes)
            {
                outcome.logs.push_back(tr("Skipped (file too large, %1 MB): %2")
                        .arg(static_cast<qulonglong>(fileSize >> 20))
                        .arg(pathToQString(source.filename())));
                outcome.skipped = 1;
                return outcome;
            }

            const QString fileName = pathToQString(source.filename());
            StageDirectory sourceStaging(QDir::tempPath());
            if (!sourceStaging.isValid())
            {
                outcome.logs.push_back(
                    tr("Failed to create a private source snapshot: %1").arg(fileName));
                outcome.failed = 1;
                return outcome;
            }
            std::error_code snapshotError;
            const auto snapshotRoot =
                std::filesystem::canonical(pathFromQString(sourceStaging.path()), snapshotError);
            std::filesystem::path snapshotRelative = "source";
            snapshotRelative += source.extension();
            const auto canCopySource = [&]
            {
                return !cancelled_.load(std::memory_order_acquire) && originalIsUnchanged();
            };
            if (snapshotError
                || !copyFileNoReplaceAtRoot(source, snapshotRoot, snapshotRelative, canCopySource))
            {
                if (cancelled_.load(std::memory_order_acquire))
                {
                    outcome.cancelled = true;
                }
                else if (!originalIsUnchanged())
                {
                    recordChangedSource();
                }
                else
                {
                    outcome.logs.push_back(
                        tr("Failed to create a private source snapshot: %1").arg(fileName));
                    outcome.failed = 1;
                }
                return outcome;
            }
            const auto processingSource = snapshotRoot / snapshotRelative;
            const auto processingIdentity = captureFileIdentity(processingSource);
            if (!processingIdentity)
            {
                outcome.logs.push_back(
                    tr("Failed to create a private source snapshot: %1").arg(fileName));
                outcome.failed = 1;
                return outcome;
            }
            const auto sourceIsUnchanged = [processingSource, expected = *processingIdentity]
            {
                const auto current = captureFileIdentity(processingSource);
                return current && *current == expected;
            };
            const auto canPublish = [&]
            {
                return !cancelled_.load(std::memory_order_acquire) && sourceIsUnchanged();
            };
            emit stageChanged(index, total, tr("Loading"), fileName);

            const auto frameCount = imageFrameCount(processingSource);
            if (frameCount > 1)
            {
                outcome.logs.push_back(
                    tr("Skipped (animated or multi-page images are not supported): %1")
                        .arg(fileName));
                outcome.skipped = 1;
                return outcome;
            }
            if (!sourceIsUnchanged())
            {
                recordChangedSource();
                return outcome;
            }

            const auto dimensions = inspectImageDimensions(processingSource);
            if (!dimensions.ok)
            {
                outcome.logs.push_back(tr("Skipped (%1): %2").arg(dimensions.reason, fileName));
                outcome.skipped = 1;
                return outcome;
            }
            if (!sourceIsUnchanged())
            {
                recordChangedSource();
                return outcome;
            }

            const auto pixels = static_cast<std::uint64_t>(dimensions.size.width())
                                * static_cast<std::uint64_t>(dimensions.size.height());
            const auto estimatedMemory = estimatedImageMemoryBytes(pixels, fileSize);
            if (estimatedMemory > imageMemoryBudget_)
            {
                outcome.logs.push_back(tr("Skipped (%1): %2")
                        .arg(tr("needs about %1 MB of memory, over the %2 MB limit")
                                 .arg(megabytesForDisplay(estimatedMemory))
                                 .arg(megabytesForDisplay(imageMemoryBudget_)),
                            fileName));
                outcome.skipped = 1;
                return outcome;
            }
            ImageMemoryReservation memoryReservation(imageMemoryMutex_,
                imageMemoryCv_,
                imageMemoryAvailable_,
                std::max<std::uint64_t>(1, estimatedMemory),
                cancelled_);
            if (!memoryReservation.acquired())
            {
                outcome.cancelled = true;
                return outcome;
            }

            cv::Mat image = imreadUnicode(processingSource, cv::IMREAD_UNCHANGED);
            if (image.empty())
            {
                if (!sourceIsUnchanged())
                {
                    recordChangedSource();
                    return outcome;
                }
                outcome.logs.push_back(
                    tr("Skipped unreadable image: %1").arg(pathToQString(source)));
                outcome.skipped = 1;
                return outcome;
            }

            if (!sourceIsUnchanged())
            {
                recordChangedSource();
                return outcome;
            }
            const int orientation = readExifOrientation(processingSource);
            if (!sourceIsUnchanged())
            {
                recordChangedSource();
                return outcome;
            }
            applyOrientation(image, orientation);

            if (cancelled_.load(std::memory_order_acquire))
            {
                outcome.cancelled = true;
                return outcome;
            }

            const long long pixelCount =
                static_cast<long long>(image.cols) * static_cast<long long>(image.rows);
            if (pixelCount > dimensions.pixelLimit)
            {
                outcome.logs.push_back(tr("Skipped (image too large, %1 × %2): %3")
                        .arg(image.cols)
                        .arg(image.rows)
                        .arg(fileName));
                outcome.skipped = 1;
                return outcome;
            }

            cv::Mat detectMat = toDetectionBgr(image);

            emit stageChanged(index, total, tr("Detecting"), fileName);
            FaceDetections detected;
            int omittedDetections = 0;
            if (detectFaces_ && detector_)
            {
                auto faces = detector_->detect(detectMat, scoreThreshold_, nmsThreshold_);
                detected = std::move(faces.detections);
                omittedDetections += faces.omitted;
            }
            if (detectPlates_ && plateDetector_)
            {
                auto plates = plateDetector_->detect(detectMat, scoreThreshold_, nmsThreshold_);
                detected.insert(detected.end(),
                    std::make_move_iterator(plates.detections.begin()),
                    std::make_move_iterator(plates.detections.end()));
                omittedDetections += plates.omitted;
            }
            if (cancelled_.load(std::memory_order_acquire))
            {
                outcome.cancelled = true;
                return outcome;
            }
            FaceDetections finalFaces = detected;
            bool doNotSaveThisImage = false;
            bool copyOriginalThisImage = false;

            if (allowReview && reviewEnabled_)
            {
                emit stageChanged(index, total, tr("Reviewing"), fileName);

                auto [preview, previewScale] = makeReviewPreview(detectMat);
                const QVector<QRectF> detectedRects = scaleRects(toQRects(detected), previewScale);

                ReviewResult reviewResult;
                const bool invoked = reviewReceiver_
                                     && QMetaObject::invokeMethod(reviewReceiver_.data(),
                                         "requestReview",
                                         Qt::BlockingQueuedConnection,
                                         Q_RETURN_ARG(cloakframe::ReviewResult, reviewResult),
                                         Q_ARG(QImage, preview),
                                         Q_ARG(QString, fileName),
                                         Q_ARG(QVector<QRectF>, detectedRects),
                                         Q_ARG(int, index),
                                         Q_ARG(int, total),
                                         Q_ARG(double, previewScale));

                if (!invoked)
                {
                    outcome.logs.push_back(
                        tr("Failed (review could not be shown, nothing was saved): %1")
                            .arg(fileName));
                    outcome.failed = 1;
                    return outcome;
                }

                switch (reviewResult.decision)
                {
                case ReviewDecision::CancelAll:
                    cancelled_.store(true, std::memory_order_release);
                    break;
                case ReviewDecision::DoNotSave:
                    doNotSaveThisImage = true;
                    break;
                case ReviewDecision::CopyOriginal:
                    copyOriginalThisImage = true;
                    break;
                case ReviewDecision::Save:
                    finalFaces = toDetections(scaleRects(reviewResult.finalBoxes,
                                                  previewScale != 0.0 ? 1.0 / previewScale : 1.0),
                        detected);
                    break;
                }
            }

            if (cancelled_.load(std::memory_order_acquire))
            {
                outcome.cancelled = true;
                return outcome;
            }
            detectMat.release();

            if (doNotSaveThisImage)
            {
                outcome.logs.push_back(tr("Skipped without saving: %1").arg(fileName));
                outcome.skipped = 1;
                return outcome;
            }
            if (!sourceIsUnchanged())
            {
                recordChangedSource();
                return outcome;
            }

            const auto encodeParams = encodeParamsForExtension(pathToUtf8(destination.extension()));

            if (copyOriginalThisImage)
            {
                emit stageChanged(index, total, tr("Saving"), fileName);
                bool copied = false;
                if (preserveMetadata_)
                {
                    copied = copyFileNoReplaceAtRoot(
                        processingSource, safeRoot, outputRelativePath(item), canPublish);
                }
                else
                {
                    copied =
                        imwriteUnicodeNoReplaceAtRoot(
                            safeRoot, outputRelativePath(item), image, encodeParams, {}, canPublish)
                        != ImageWriteResult::Failed;
                }

                if (!copied)
                {
                    if (cancelled_.load(std::memory_order_acquire))
                    {
                        outcome.cancelled = true;
                    }
                    else if (!sourceIsUnchanged())
                    {
                        recordChangedSource();
                    }
                    else
                    {
                        outcome.logs.push_back(
                            tr("Failed to copy: %1").arg(pathToQString(destination)));
                        outcome.failed = 1;
                    }
                }
                else
                {
                    outcome.logs.push_back(tr("Skipped (original copied): %1").arg(fileName));
                    outcome.copied = 1;
                }
                return outcome;
            }

            emit stageChanged(index, total, tr("Applying anonymization"), fileName);
            applyAnonymization(image,
                finalFaces,
                method_,
                mosaicBlockSize_,
                paddingRatio_,
                shape_,
                softEdges_,
                customImage_);

            if (cancelled_.load(std::memory_order_acquire))
            {
                outcome.cancelled = true;
                return outcome;
            }

            emit stageChanged(index, total, tr("Saving"), fileName);
            const auto writeResult = imwriteUnicodeNoReplaceAtRoot(safeRoot,
                outputRelativePath(item),
                image,
                encodeParams,
                preserveMetadata_ ? processingSource : std::filesystem::path{},
                canPublish);
            if (writeResult == ImageWriteResult::Failed)
            {
                if (cancelled_.load(std::memory_order_acquire))
                {
                    outcome.cancelled = true;
                }
                else if (!sourceIsUnchanged())
                {
                    recordChangedSource();
                }
                else
                {
                    outcome.logs.push_back(
                        tr("Failed to save: %1").arg(pathToQString(destination)));
                    outcome.failed = 1;
                }
            }
            else
            {
                if (writeResult == ImageWriteResult::SavedWithoutMetadata)
                {
                    outcome.logs.push_back(
                        tr("Saved, but could not copy metadata: %1").arg(fileName));
                    outcome.warnings = 1;
                }
                if (finalFaces.empty())
                {
                    outcome.logs.push_back(tr("Saved with no regions redacted: %1").arg(fileName));
                    outcome.unredacted = 1;
                }
                else
                {
                    outcome.logs.push_back(tr(
                        "Redacted %n region(s): %1", nullptr, static_cast<int>(finalFaces.size()))
                            .arg(fileName));
                    outcome.redacted = 1;
                }
                if (omittedDetections > 0)
                {
                    outcome.logs.push_back(
                        tr("Warning: %n detected region(s) exceeded the safety limit and were "
                           "left unredacted in %1. Review before sharing.",
                            nullptr,
                            omittedDetections)
                            .arg(fileName));
                    outcome.omittedRegions = omittedDetections;
                }
            }
        }
        catch (const std::exception &exception)
        {
            outcome.logs.push_back(tr("Error processing %1: %2")
                    .arg(pathToQString(item.sourcePath.filename()),
                        QString::fromUtf8(exception.what())));
            outcome.failed = 1;
        }
        catch (...)
        {
            outcome.logs.push_back(
                tr("Error processing %1").arg(pathToQString(item.sourcePath.filename())));
            outcome.failed = 1;
        }
        return outcome;
    }

    ProcessorWorker::ItemOutcome ProcessorWorker::processVideoItem(const ScanResult &item,
        const std::filesystem::path &safeRoot,
        const std::filesystem::path &destination,
        const int index,
        const int total)
    {
        ItemOutcome outcome;
        const QString fileName = pathToQString(item.sourcePath.filename());

        if (preserveMetadata_)
        {
            outcome.logs.push_back(
                tr("Metadata preservation is not available for videos; metadata was removed: %1")
                    .arg(fileName));
            outcome.warnings = 1;
        }

        QString toolsError;
        const auto tools = locateFfmpegTools(&toolsError);
        if (!tools)
        {
            outcome.logs.push_back(tr("Failed (%1): %2").arg(toolsError, fileName));
            outcome.failed = 1;
            return outcome;
        }

        emit stageChanged(index, total, tr("Inspecting"), fileName);
        QString probeError;
        const auto info = probeVideo(*tools, pathToQString(item.sourcePath), &probeError);
        if (!info)
        {
            outcome.logs.push_back(tr("Failed (%1): %2").arg(probeError, fileName));
            outcome.failed = 1;
            return outcome;
        }
        const auto unsupported = videoUnsupportedReason(*info);
        if (!unsupported.isEmpty())
        {
            outcome.logs.push_back(
                tr("Failed (unsupported video: %1): %2").arg(unsupported, fileName));
            outcome.failed = 1;
            return outcome;
        }
        if (info->isVfr)
        {
            outcome.logs.push_back(
                tr("Note: variable frame rate is converted to a constant frame rate: %1")
                    .arg(fileName));
        }

        VideoProcessOptions options;
        options.scoreThreshold = scoreThreshold_;
        options.nmsThreshold = nmsThreshold_;
        options.mosaicBlockSize = mosaicBlockSize_;
        options.paddingRatio = paddingRatio_;
        options.method = method_;
        options.customImage = customImage_;
        options.shape = shape_;
        options.softEdges = softEdges_;
        options.crf = videoCrf_;
        options.codec = videoCodec_;
        options.hardwareEncoder = gpuAcceleration_;
        options.outputRootPath = pathToQString(safeRoot);
        options.outputRelativePath = pathToQString(outputRelativePath(item));

        if (detectFaces_ && detector_ && !videoDetector_)
        {
            emit logMessage(tr("Loading face detection model for video..."));
            const int videoInputSize =
                faceModelKind_ == FaceModelKind::Scrfd ? kVideoDetectionInputSize : 640;
            try
            {
                videoDetector_ = makeFaceDetector(
                    faceModelKind_, modelPath_, videoInputSize, gpuAcceleration_, modelSha256_);
                const cv::Mat warmupFrame(videoDetector_->inputSize(),
                    videoDetector_->inputSize(),
                    CV_8UC3,
                    cv::Scalar(0, 0, 0));
                videoDetector_->detect(warmupFrame, scoreThreshold_, nmsThreshold_);
            }
            catch (const Ort::Exception &)
            {
                emit logMessage(tr("GPU acceleration can't run the video face model at %1 px; "
                                   "using the CPU instead.")
                        .arg(videoInputSize));
                videoDetector_ = makeFaceDetector(
                    faceModelKind_, modelPath_, videoInputSize, false, modelSha256_);
                const cv::Mat warmupFrame(videoDetector_->inputSize(),
                    videoDetector_->inputSize(),
                    CV_8UC3,
                    cv::Scalar(0, 0, 0));
                videoDetector_->detect(warmupFrame, scoreThreshold_, nmsThreshold_);
            }
            emit logMessage(tr("Video face detection: %1 px · %2")
                    .arg(videoDetector_->inputSize())
                    .arg(QString::fromLatin1(videoDetector_->backendName())));
        }

        const float detectionThreshold =
            std::min(options.tracker.lowScoreThreshold, scoreThreshold_);
        // Only pass 1's single detection loop calls this, so the accumulator needs no lock.
        int omittedDetections = 0;
        const auto detect = [this, detectionThreshold, &omittedDetections](const cv::Mat &frame)
        {
            FaceDetections detections;
            if (detectFaces_ && videoDetector_)
            {
                auto faces = videoDetector_->detect(frame, detectionThreshold, nmsThreshold_);
                detections = std::move(faces.detections);
                omittedDetections += faces.omitted;
            }
            if (detectPlates_ && plateDetector_)
            {
                auto plates = plateDetector_->detect(frame, detectionThreshold, nmsThreshold_);
                detections.insert(detections.end(),
                    std::make_move_iterator(plates.detections.begin()),
                    std::make_move_iterator(plates.detections.end()));
                omittedDetections += plates.omitted;
            }
            return detections;
        };

        QElapsedTimer passTimer;
        QElapsedTimer totalTimer;
        totalTimer.start();
        int lastPass = 0;
        int lastPercent = -1;
        const auto progress = [&](int pass, qint64 frame, qint64 totalFrames)
        {
            if (pass != lastPass)
            {
                passTimer.start();
                lastPass = pass;
                lastPercent = -1;
            }
            const int percent =
                totalFrames > 0 ? static_cast<int>(std::min<qint64>(100, frame * 100 / totalFrames))
                                : 0;
            if (percent == lastPercent)
            {
                return;
            }
            lastPercent = percent;
            QString stage =
                pass == 1 ? tr("Analyzing %1%").arg(percent) : tr("Encoding %1%").arg(percent);
            const qint64 elapsedMs = passTimer.elapsed();
            if (frame > 0 && elapsedMs > 2000 && totalFrames > frame)
            {
                const qint64 remaining = elapsedMs * (totalFrames - frame) / frame / 1000;
                stage += QStringLiteral("  ·  ");
                stage += remaining >= 60
                             ? tr("%1m %2s left").arg(remaining / 60).arg(remaining % 60)
                             : tr("%1s left").arg(remaining);
            }
            emit stageChanged(index, total, stage, fileName);
        };

        VideoTrackReviewFn review;
        if (reviewEnabled_)
        {
            review = [this, &tools, &fileName, index, total](std::vector<Track> &tracks,
                         const std::vector<UncoveredSpan> &uncoveredSpans,
                         qint64 frameCount,
                         const QString &reviewSourcePath,
                         const VideoInfo &reviewInfo)
            {
                emit stageChanged(index, total, tr("Reviewing video tracks"), fileName);
                VideoReviewRequest request;
                request.sourcePath = reviewSourcePath;
                request.ffmpegPath = tools->ffmpegPath;
                request.sourceName = fileName;
                request.frameSize = QSize(reviewInfo.displayWidth(), reviewInfo.displayHeight());
                request.fps = reviewInfo.fps();
                request.startTimeSeconds = reviewInfo.startTimeSeconds;
                request.fpsNum = reviewInfo.fpsNum;
                request.fpsDen = reviewInfo.fpsDen;
                request.frameCount =
                    static_cast<int>(std::min<qint64>(frameCount, std::numeric_limits<int>::max()));
                request.tracks.reserve(static_cast<qsizetype>(tracks.size()));
                for (const auto &track : tracks)
                {
                    VideoReviewTrack reviewTrack;
                    reviewTrack.id = track.id;
                    reviewTrack.lowConfidence = track.lowConfidence;
                    reviewTrack.boxes.reserve(static_cast<qsizetype>(track.boxes.size()));
                    for (const auto &box : track.boxes)
                    {
                        reviewTrack.boxes.push_back({box.frame,
                            QRectF(box.box.x, box.box.y, box.box.width, box.box.height),
                            box.interpolated});
                    }
                    request.tracks.push_back(std::move(reviewTrack));
                }
                request.uncoveredSpans.reserve(static_cast<qsizetype>(uncoveredSpans.size()));
                for (const auto &span : uncoveredSpans)
                {
                    request.uncoveredSpans.push_back(span);
                }

                VideoReviewResult reviewResult;
                const bool invoked = reviewReceiver_
                                     && QMetaObject::invokeMethod(reviewReceiver_.data(),
                                         "requestVideoReview",
                                         Qt::BlockingQueuedConnection,
                                         Q_RETURN_ARG(cloakframe::VideoReviewResult, reviewResult),
                                         Q_ARG(cloakframe::VideoReviewRequest, request));
                if (!invoked || reviewResult.decision == VideoReviewDecision::CancelAll)
                {
                    cancelled_.store(true, std::memory_order_release);
                    return false;
                }
                std::erase_if(tracks,
                    [&](const Track &track)
                    {
                        return reviewResult.excludedTrackIds.contains(track.id);
                    });

                constexpr qsizetype kMaxManualTracks = 64;
                constexpr qsizetype kMaxManualKeyframes = 4096;
                constexpr std::uint64_t kMaxReviewedTrackBoxes = 8'000'000;
                if (reviewResult.addedTracks.size() > kMaxManualTracks)
                {
                    cancelled_.store(true, std::memory_order_release);
                    return false;
                }
                int nextTrackId = 1;
                std::uint64_t reviewedTrackBoxes = 0;
                for (const auto &track : tracks)
                {
                    if (track.id < std::numeric_limits<int>::max())
                    {
                        nextTrackId = std::max(nextTrackId, track.id + 1);
                    }
                    if (track.boxes.size() > kMaxReviewedTrackBoxes - reviewedTrackBoxes)
                    {
                        cancelled_.store(true, std::memory_order_release);
                        return false;
                    }
                    reviewedTrackBoxes += track.boxes.size();
                }
                for (const auto &manual : reviewResult.addedTracks)
                {
                    const std::uint64_t manualBoxCount =
                        manual.endFrame >= manual.startFrame
                            ? static_cast<std::uint64_t>(
                                  static_cast<std::int64_t>(manual.endFrame)
                                  - static_cast<std::int64_t>(manual.startFrame))
                                  + 1
                            : 0;
                    if (manual.keyframes.size() > kMaxManualKeyframes || manualBoxCount == 0
                        || manualBoxCount > kMaxReviewedTrackBoxes - reviewedTrackBoxes
                        || nextTrackId == std::numeric_limits<int>::max())
                    {
                        cancelled_.store(true, std::memory_order_release);
                        return false;
                    }
                    auto track = materializeManualVideoTrack(
                        manual, request.frameCount, request.frameSize, nextTrackId++);
                    if (!track)
                    {
                        cancelled_.store(true, std::memory_order_release);
                        return false;
                    }
                    tracks.push_back(std::move(*track));
                    reviewedTrackBoxes += manualBoxCount;
                }
                return true;
            };
        }

        // The accumulators run for the life of the detector, so a multi-file run needs the
        // difference across this video rather than the running total.
        const DetectorStageTimings faceStagesBefore =
            videoDetector_ ? videoDetector_->stageTimings() : DetectorStageTimings{};
        const DetectorStageTimings plateStagesBefore =
            plateDetector_ ? plateDetector_->stageTimings() : DetectorStageTimings{};

        const auto result = processVideo(*tools,
            pathToQString(item.sourcePath),
            pathToQString(destination),
            *info,
            options,
            detect,
            cancelled_,
            progress,
            review);

        logDetectorStages("face", faceStagesBefore, videoDetector_.get());
        logDetectorStages("plate", plateStagesBefore, plateDetector_.get());

        switch (result.status)
        {
        case VideoProcessStatus::Completed:
            if (result.trackCount == 0)
            {
                outcome.logs.push_back(tr("Saved with no regions redacted: %1").arg(fileName));
                outcome.unredacted = 1;
            }
            else
            {
                outcome.logs.push_back(
                    tr("Redacted %n region(s): %1", nullptr, result.trackCount).arg(fileName));
                outcome.redacted = 1;
            }
            if (const double elapsedSeconds = static_cast<double>(totalTimer.elapsed()) / 1000.0;
                elapsedSeconds > 0 && info->fps() > 0)
            {
                const double videoSeconds = static_cast<double>(result.frameCount) / info->fps();
                outcome.logs.push_back(tr("Processed %1 frames in %2s (%3× real time): %4")
                        .arg(result.frameCount)
                        .arg(elapsedSeconds, 0, 'f', 1)
                        .arg(videoSeconds / elapsedSeconds, 0, 'f', 1)
                        .arg(fileName));
            }
            if (!result.encoderName.isEmpty())
            {
                outcome.logs.push_back(tr("Video encoder: %1").arg(result.encoderName));
            }
            // Three different failures used to be added together and reported as one count
            // of detected regions. They are frames, objects and tracks, and each one sends
            // the user somewhere else, so each is reported on its own terms.
            if (omittedDetections > 0)
            {
                outcome.logs.push_back(
                    tr("Warning: %n detected region(s) exceeded the safety limit and were "
                       "left unredacted in %1. Review before sharing.",
                        nullptr,
                        omittedDetections)
                        .arg(fileName));
            }
            if (result.droppedTracks > 0)
            {
                outcome.logs.push_back(
                    tr("Warning: %n track(s) in %1 held no confident detection and were "
                       "dropped. Review before sharing.",
                        nullptr,
                        result.droppedTracks)
                        .arg(fileName));
            }
            if (result.uncoveredFrames > 0)
            {
                outcome.logs.push_back(
                    tr("Warning: tracking could not locate the subject in %n frame(s) of %1 "
                       "before review. Manual masks do not verify its position. "
                       "Check these frames before sharing.",
                        nullptr,
                        result.uncoveredFrames)
                        .arg(fileName));
                outcome.logs.push_back(tr("Tracking gaps found before review in %1: %2")
                        .arg(fileName, formatUncoveredSpans(result.uncoveredSpans)));
                if (result.uncoveredSpans.size() > kMaxReportedUncoveredSpans)
                {
                    outcome.logs.push_back(tr("%n further tracking gap(s) are not listed.",
                        nullptr,
                        static_cast<int>(
                            result.uncoveredSpans.size() - kMaxReportedUncoveredSpans)));
                }
            }
            outcome.omittedRegions = omittedDetections;
            outcome.trackingGapFrames = result.uncoveredFrames;
            outcome.droppedTracks = result.droppedTracks;
            break;
        case VideoProcessStatus::Cancelled:
            outcome.cancelled = true;
            break;
        case VideoProcessStatus::Failed:
            outcome.logs.push_back(
                tr("Failed to process video %1: %2").arg(fileName, result.error));
            outcome.failed = 1;
            break;
        }
        return outcome;
    }

    void ProcessorWorker::cancel()
    {
        {
            const std::lock_guard<std::mutex> lock(imageMemoryMutex_);
            cancelled_.store(true, std::memory_order_release);
        }
        imageMemoryCv_.notify_all();
    }
}

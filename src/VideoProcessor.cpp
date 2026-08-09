#include "cloakframe/VideoProcessor.hpp"

#include "cloakframe/ImageIo.hpp"
#include "cloakframe/MemoryBudget.hpp"
#include "cloakframe/PathUtil.hpp"

#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QTemporaryDir>
#include <QThread>

#include <opencv2/core.hpp>

#include <spdlog/spdlog.h>

#include <algorithm>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstdint>
#include <functional>
#include <limits>
#include <mutex>
#include <optional>
#include <stdexcept>
#include <thread>
#include <utility>
#include <vector>

#if defined(_WIN32)
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#else
#include <sys/stat.h>
#endif

namespace cloakframe
{
    namespace
    {
        constexpr std::uint64_t kVideoMaskingMinimumBudget = 512ULL * 1024ULL * 1024ULL;
        constexpr std::uint64_t kVideoMaskingMaximumBudget = 1ULL * 1024ULL * 1024ULL * 1024ULL;
        constexpr qint64 kVideoMaskingFixedHeadroom = 48LL * 1024 * 1024;
        constexpr std::uint64_t kVideoDetectionMinimumBudget = 128ULL * 1024ULL * 1024ULL;
        constexpr std::uint64_t kVideoDetectionMaximumBudget = 512ULL * 1024ULL * 1024ULL;
        constexpr qint64 kMaxDetectionReserveFrames = 100'000;
        constexpr std::size_t kMaxVideoDetectionsPerFrame = 256;
        constexpr std::size_t kMaxVideoTrackedRegions = 8'000'000;
        constexpr int kMaxMaskWorkers = 8;
        constexpr int kMaxMaskBatchFrames = 16;

        qint64 videoMaskingMemoryBudget()
        {
            return static_cast<qint64>(
                adaptiveMemoryBudget(kVideoMaskingMinimumBudget, kVideoMaskingMaximumBudget, 16));
        }

        std::uint64_t videoDetectionMemoryBudget()
        {
            return adaptiveMemoryBudget(
                kVideoDetectionMinimumBudget, kVideoDetectionMaximumBudget, 32);
        }

        struct VideoSourceSnapshot
        {
            std::uint64_t device = 0;
            std::uint64_t file = 0;
            std::uint64_t size = 0;
            std::int64_t modifiedSeconds = 0;
            std::int64_t modifiedNanoseconds = 0;
            std::int64_t changedSeconds = 0;
            std::int64_t changedNanoseconds = 0;

            bool operator==(const VideoSourceSnapshot &) const = default;
        };

        std::optional<VideoSourceSnapshot> captureVideoSourceSnapshot(const QString &path)
        {
#if defined(_WIN32)
            const std::wstring nativePath = path.toStdWString();
            const HANDLE handle = CreateFileW(nativePath.c_str(),
                FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                nullptr,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                nullptr);
            if (handle == INVALID_HANDLE_VALUE)
            {
                return std::nullopt;
            }

            BY_HANDLE_FILE_INFORMATION info{};
            FILE_BASIC_INFO basic{};
            const bool valid =
                GetFileInformationByHandle(handle, &info) != 0
                && GetFileInformationByHandleEx(handle, FileBasicInfo, &basic, sizeof(basic)) != 0
                && (info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) == 0;
            CloseHandle(handle);
            if (!valid)
            {
                return std::nullopt;
            }

            VideoSourceSnapshot snapshot;
            snapshot.device = info.dwVolumeSerialNumber;
            snapshot.file =
                (static_cast<std::uint64_t>(info.nFileIndexHigh) << 32U) | info.nFileIndexLow;
            snapshot.size =
                (static_cast<std::uint64_t>(info.nFileSizeHigh) << 32U) | info.nFileSizeLow;
            snapshot.modifiedSeconds = static_cast<std::int64_t>(
                (static_cast<std::uint64_t>(info.ftLastWriteTime.dwHighDateTime) << 32U)
                | info.ftLastWriteTime.dwLowDateTime);
            snapshot.changedSeconds = basic.ChangeTime.QuadPart;
            return snapshot;
#else
            const QByteArray nativePath = QFile::encodeName(path);
            struct stat info{};
            if (::stat(nativePath.constData(), &info) != 0 || !S_ISREG(info.st_mode)
                || info.st_size < 0)
            {
                return std::nullopt;
            }

            VideoSourceSnapshot snapshot;
            snapshot.device = static_cast<std::uint64_t>(info.st_dev);
            snapshot.file = static_cast<std::uint64_t>(info.st_ino);
            snapshot.size = static_cast<std::uint64_t>(info.st_size);
#if defined(__APPLE__)
            snapshot.modifiedSeconds = info.st_mtimespec.tv_sec;
            snapshot.modifiedNanoseconds = info.st_mtimespec.tv_nsec;
            snapshot.changedSeconds = info.st_ctimespec.tv_sec;
            snapshot.changedNanoseconds = info.st_ctimespec.tv_nsec;
#else
            snapshot.modifiedSeconds = info.st_mtim.tv_sec;
            snapshot.modifiedNanoseconds = info.st_mtim.tv_nsec;
            snapshot.changedSeconds = info.st_ctim.tv_sec;
            snapshot.changedNanoseconds = info.st_ctim.tv_nsec;
#endif
            return snapshot;
#endif
        }

        class StageTimer
        {
        public:
            using Clock = std::chrono::steady_clock;

            static Clock::time_point now()
            {
                return Clock::now();
            }

            void add(Clock::time_point since)
            {
                total_ += Clock::now() - since;
            }

            [[nodiscard]] long long ms() const
            {
                return std::chrono::duration_cast<std::chrono::milliseconds>(total_).count();
            }

        private:
            Clock::duration total_{};
        };

        class ScopedCvThreads
        {
        public:
            explicit ScopedCvThreads(int threads)
                : previous_(cv::getNumThreads())
            {
                cv::setNumThreads(threads);
            }

            ~ScopedCvThreads()
            {
                cv::setNumThreads(previous_);
            }

            ScopedCvThreads(const ScopedCvThreads &) = delete;
            ScopedCvThreads &operator=(const ScopedCvThreads &) = delete;

        private:
            int previous_;
        };

        class MaskWorkerPool
        {
        public:
            explicit MaskWorkerPool(int workerCount)
            {
                const int helpers = std::max(0, workerCount - 1);
                workers_.reserve(static_cast<std::size_t>(helpers));
                try
                {
                    for (int index = 0; index < helpers; ++index)
                    {
                        workers_.emplace_back(
                            [this]
                            {
                                workerLoop();
                            });
                    }
                }
                catch (...)
                {
                    // A throwing thread constructor skips ~MaskWorkerPool, so the helpers that
                    // did start must be joined here before workers_ is destroyed.
                    stopAndJoin();
                    throw;
                }
            }

            ~MaskWorkerPool()
            {
                stopAndJoin();
            }

            MaskWorkerPool(const MaskWorkerPool &) = delete;
            MaskWorkerPool &operator=(const MaskWorkerPool &) = delete;

            void run(const std::function<void()> &task)
            {
                {
                    const std::scoped_lock lock(mutex_);
                    task_ = task;
                    pending_ = workers_.size();
                    ++generation_;
                }
                workAvailable_.notify_all();
                task();

                std::unique_lock lock(mutex_);
                workComplete_.wait(lock,
                    [this]
                    {
                        return pending_ == 0;
                    });
                task_ = {};
            }

        private:
            void stopAndJoin() noexcept
            {
                {
                    const std::scoped_lock lock(mutex_);
                    stopping_ = true;
                }
                workAvailable_.notify_all();
                for (auto &worker : workers_)
                {
                    if (worker.joinable())
                    {
                        worker.join();
                    }
                }
            }

            void workerLoop()
            {
                std::uint64_t observedGeneration = 0;
                for (;;)
                {
                    std::unique_lock lock(mutex_);
                    workAvailable_.wait(lock,
                        [this, &observedGeneration]
                        {
                            return stopping_ || generation_ != observedGeneration;
                        });
                    if (stopping_)
                    {
                        return;
                    }
                    observedGeneration = generation_;
                    const auto *task = &task_;
                    lock.unlock();
                    (*task)();
                    lock.lock();
                    --pending_;
                    if (pending_ == 0)
                    {
                        workComplete_.notify_one();
                    }
                }
            }

            std::mutex mutex_;
            std::condition_variable workAvailable_;
            std::condition_variable workComplete_;
            std::vector<std::thread> workers_;
            std::function<void()> task_;
            std::size_t pending_ = 0;
            std::uint64_t generation_ = 0;
            bool stopping_ = false;
        };

        class RetryingStagingDir
        {
        public:
            explicit RetryingStagingDir(const QString &templatePath)
                : dir_(templatePath)
            {
            }

            ~RetryingStagingDir()
            {
                for (int attempt = 0; dir_.isValid() && !dir_.remove() && attempt < 20; ++attempt)
                {
                    QThread::msleep(100);
                }
            }

            RetryingStagingDir(const RetryingStagingDir &) = delete;
            RetryingStagingDir &operator=(const RetryingStagingDir &) = delete;

            [[nodiscard]] bool isValid() const
            {
                return dir_.isValid();
            }
            [[nodiscard]] QString path() const
            {
                return dir_.path();
            }

        private:
            QTemporaryDir dir_;
        };

        QString trVideoProcessor(const char *text)
        {
            return QCoreApplication::translate("cloakframe::VideoProcessor", text);
        }

        void scaleTracksToNative(std::vector<Track> &tracks, float scaleX, float scaleY)
        {
            if (scaleX == 1.0F && scaleY == 1.0F)
            {
                return;
            }
            for (auto &track : tracks)
            {
                for (auto &tracked : track.boxes)
                {
                    tracked.box.x *= scaleX;
                    tracked.box.y *= scaleY;
                    tracked.box.width *= scaleX;
                    tracked.box.height *= scaleY;
                }
            }
        }
    }

    float videoStrongScoreThreshold(float scoreThreshold)
    {
        // A low user threshold may supply weak candidates to continue an existing track, but
        // those candidates must not create and preserve thousands of false-positive tracks.
        // The floor must never exceed the user's own threshold, or video would ignore a low
        // setting that images honour.
        return std::min(scoreThreshold, std::max(0.35F, scoreThreshold - 0.1F));
    }

    VideoMaskingPlan videoMaskingPlan(const int width,
        const int height,
        const unsigned int hardwareThreads,
        const qint64 memoryBudget)
    {
        VideoMaskingPlan plan;
        if (width <= 0 || height <= 0)
        {
            return plan;
        }

        const qint64 pixels = static_cast<qint64>(width) * height;
        plan.frameBytes = pixels > std::numeric_limits<qint64>::max() / 8
                              ? std::numeric_limits<qint64>::max()
                              : pixels * 8;

        const unsigned int availableThreads = hardwareThreads == 0 ? 1 : hardwareThreads;
        const int requestedWorkers =
            static_cast<int>(std::min<unsigned int>(availableThreads, kMaxMaskWorkers));
        const qint64 budget = memoryBudget > 0 ? memoryBudget : videoMaskingMemoryBudget();
        const qint64 framesWithinBudget = std::clamp<qint64>(
            (budget - kVideoMaskingFixedHeadroom) / std::max<qint64>(1, plan.frameBytes),
            1,
            kMaxMaskBatchFrames);
        plan.batchFrames = std::min(requestedWorkers * 2, static_cast<int>(framesWithinBudget));
        plan.workerCount = std::min(requestedWorkers, plan.batchFrames);
        return plan;
    }

    VideoProcessResult processVideo(const FfmpegTools &tools,
        const QString &sourcePath,
        const QString &destinationPath,
        const VideoInfo &info,
        const VideoProcessOptions &options,
        const VideoDetectFn &detect,
        const std::atomic<bool> &cancelled,
        const VideoProgressFn &progress,
        const VideoTrackReviewFn &review)
    {
        VideoProcessResult result;
        (void)info;

        const auto sourceSnapshot = captureVideoSourceSnapshot(sourcePath);
        if (!sourceSnapshot)
        {
            result.error = trVideoProcessor(QT_TRANSLATE_NOOP(
                "cloakframe::VideoProcessor", "Could not inspect the source video."));
            return result;
        }
        const auto originalIsUnchanged = [&]()
        {
            const auto current = captureVideoSourceSnapshot(sourcePath);
            return current && *current == *sourceSnapshot;
        };
        const auto rejectChangedSource = [&]()
        {
            result.error = trVideoProcessor(QT_TRANSLATE_NOOP("cloakframe::VideoProcessor",
                "The source video changed during processing. Start the operation again."));
            spdlog::warn("Video source changed while processing: {}", sourcePath.toStdString());
        };

        const QString stagingBase = !options.outputRootPath.isEmpty()
                                        ? options.outputRootPath
                                        : QFileInfo(destinationPath).absolutePath();
        RetryingStagingDir sourceStaging(
            QDir(stagingBase).filePath(QStringLiteral(".cloakframe-snapshot-XXXXXX")));
        if (!sourceStaging.isValid())
        {
            result.error = trVideoProcessor(QT_TRANSLATE_NOOP("cloakframe::VideoProcessor",
                "Could not create a private snapshot of the source video."));
            return result;
        }
        std::error_code snapshotError;
        const auto snapshotRoot =
            std::filesystem::canonical(pathFromQString(sourceStaging.path()), snapshotError);
        std::filesystem::path snapshotRelative = "source";
        snapshotRelative += pathFromQString(sourcePath).extension();
        const auto canCopySource = [&]()
        {
            return !cancelled.load(std::memory_order_acquire) && originalIsUnchanged();
        };
        if (snapshotError
            || !copyFileNoReplaceAtRoot(
                pathFromQString(sourcePath), snapshotRoot, snapshotRelative, canCopySource))
        {
            if (cancelled.load(std::memory_order_acquire))
            {
                result.status = VideoProcessStatus::Cancelled;
            }
            else if (!originalIsUnchanged())
            {
                rejectChangedSource();
            }
            else
            {
                result.error = trVideoProcessor(QT_TRANSLATE_NOOP("cloakframe::VideoProcessor",
                    "Could not create a private snapshot of the source video."));
            }
            return result;
        }

        const QString processingSource = pathToQString(snapshotRoot / snapshotRelative);
        const auto processingSnapshot = captureVideoSourceSnapshot(processingSource);
        if (!processingSnapshot)
        {
            result.error = trVideoProcessor(QT_TRANSLATE_NOOP("cloakframe::VideoProcessor",
                "Could not create a private snapshot of the source video."));
            return result;
        }
        const auto sourceIsUnchanged = [&]()
        {
            const auto current = captureVideoSourceSnapshot(processingSource);
            return current && *current == *processingSnapshot;
        };
        auto lastSourceCheck = std::chrono::steady_clock::time_point{};
        const auto canContinue = [&]()
        {
            if (cancelled.load(std::memory_order_acquire))
            {
                return false;
            }
            const auto now = std::chrono::steady_clock::now();
            if (now - lastSourceCheck < std::chrono::milliseconds(500))
            {
                return true;
            }
            if (!sourceIsUnchanged())
            {
                return false;
            }
            lastSourceCheck = now;
            return true;
        };

        QString probeError;
        const auto inspectedInfo = probeVideo(tools, processingSource, &probeError);
        if (!inspectedInfo)
        {
            result.error = probeError;
            return result;
        }
        const VideoInfo &activeInfo = *inspectedInfo;
        const QString unsupported = videoUnsupportedReason(activeInfo);
        if (!unsupported.isEmpty())
        {
            result.error = unsupported;
            return result;
        }

        std::vector<FaceDetections> frameDetections;
        std::uint64_t detectionMemory = 0;
        const std::uint64_t detectionBudget = videoDetectionMemoryBudget();
        if (activeInfo.estimatedFrameCount > 0)
        {
            frameDetections.reserve(static_cast<std::size_t>(
                std::min(activeInfo.estimatedFrameCount, kMaxDetectionReserveFrames)));
        }

        float scaleX = 1.0F;
        float scaleY = 1.0F;
        SceneCutDetector cutDetector;
        {
            VideoFrameReader reader;
            if (!reader.open(tools, processingSource, activeInfo, options.analysisLongEdge))
            {
                result.error = reader.errorString();
                return result;
            }
            scaleX = static_cast<float>(activeInfo.displayWidth())
                     / static_cast<float>(reader.frameWidth());
            scaleY = static_cast<float>(activeInfo.displayHeight())
                     / static_cast<float>(reader.frameHeight());

            StageTimer readTimer;
            StageTimer detectTimer;
            cv::Mat frame;
            for (;;)
            {
                const auto readMark = StageTimer::now();
                const bool got = reader.readFrame(frame, canContinue);
                readTimer.add(readMark);
                if (!got)
                {
                    break;
                }
                if (cancelled.load(std::memory_order_acquire))
                {
                    result.status = VideoProcessStatus::Cancelled;
                    return result;
                }
                const auto detectMark = StageTimer::now();
                cutDetector.push(frame);
                if (static_cast<qint64>(frameDetections.size()) >= kMaxVideoFrameCount)
                {
                    result.error = trVideoProcessor(QT_TRANSLATE_NOOP("cloakframe::VideoProcessor",
                        "The video frame count exceeds the safety limit."));
                    return result;
                }
                FaceDetections detections = detect ? detect(frame) : FaceDetections{};
                std::erase_if(detections,
                    [](const FaceDetection &detection)
                    {
                        return !isValidFaceDetection(detection);
                    });
                if (detections.size() > kMaxVideoDetectionsPerFrame)
                {
                    result.error = trVideoProcessor(QT_TRANSLATE_NOOP("cloakframe::VideoProcessor",
                        "Video detection data exceeds the safety limit."));
                    return result;
                }
                const auto capacity = static_cast<std::uint64_t>(detections.capacity());
                const auto maximum = std::numeric_limits<std::uint64_t>::max();
                const auto elementBytes = sizeof(FaceDetection);
                if (capacity > (maximum - sizeof(FaceDetections)) / elementBytes)
                {
                    result.error = trVideoProcessor(QT_TRANSLATE_NOOP("cloakframe::VideoProcessor",
                        "Video detection data exceeds the safety limit."));
                    return result;
                }
                const auto additional = sizeof(FaceDetections) + capacity * elementBytes;
                if (additional > detectionBudget || detectionMemory > detectionBudget - additional)
                {
                    result.error = trVideoProcessor(QT_TRANSLATE_NOOP("cloakframe::VideoProcessor",
                        "Video detection data exceeds the safety limit."));
                    return result;
                }
                detectionMemory += additional;
                frameDetections.push_back(std::move(detections));
                detectTimer.add(detectMark);
                if (progress)
                {
                    progress(1,
                        static_cast<qint64>(frameDetections.size()),
                        std::max<qint64>(activeInfo.estimatedFrameCount,
                            static_cast<qint64>(frameDetections.size())));
                }
            }
            if (!reader.errorString().isEmpty())
            {
                result.error = reader.errorString();
                return result;
            }
            spdlog::info("Pass 1 timing: decode {} ms, detect+scene {} ms ({} frames)",
                readTimer.ms(),
                detectTimer.ms(),
                frameDetections.size());
        }

        if (cancelled.load(std::memory_order_acquire))
        {
            result.status = VideoProcessStatus::Cancelled;
            return result;
        }
        if (!sourceIsUnchanged())
        {
            rejectChangedSource();
            return result;
        }

        const auto frameCount = static_cast<qint64>(frameDetections.size());
        result.frameCount = frameCount;
        if (frameCount == 0)
        {
            result.error = trVideoProcessor(
                QT_TRANSLATE_NOOP("cloakframe::VideoProcessor", "No frames could be decoded."));
            return result;
        }

        const SceneCuts sceneCuts = cutDetector.finish();
        spdlog::info("Video scene cuts detected: {}", sceneCuts.frames().size());

        TrackerConfig trackerConfig = options.tracker;
        trackerConfig.highScoreThreshold = videoStrongScoreThreshold(options.scoreThreshold);
        std::vector<Track> tracks;
        const TrackingContinueGuard trackingContinue = [&]
        {
            return !cancelled.load(std::memory_order_acquire);
        };
        try
        {
            tracks = buildBidirectionalTracks(
                frameDetections, trackerConfig, 0.5F, sceneCuts, trackingContinue);
            TrackPostProcessConfig postProcess = options.postProcess;
            postProcess.strongScoreThreshold = trackerConfig.highScoreThreshold;
            postProcess.retainLowConfidenceTracks = static_cast<bool>(review);
            auto coverage = postProcessTracks(
                tracks, postProcess, static_cast<int>(frameCount), sceneCuts, trackingContinue);
            result.uncoveredFrames = coverage.uncoveredFrames;
            result.uncoveredSpans = std::move(coverage.uncoveredSpans);
            result.droppedTracks = coverage.droppedTracks;
        }
        catch (const TrackingCancelled &)
        {
            result.status = VideoProcessStatus::Cancelled;
            return result;
        }
        catch (const std::exception &)
        {
            result.error = trVideoProcessor(QT_TRANSLATE_NOOP(
                "cloakframe::VideoProcessor", "Video tracking data exceeds the safety limit."));
            return result;
        }
        scaleTracksToNative(tracks, scaleX, scaleY);
        for (auto &detections : frameDetections)
        {
            detections.clear();
        }

        if (review
            && !review(tracks, result.uncoveredSpans, frameCount, processingSource, activeInfo))
        {
            result.status = VideoProcessStatus::Cancelled;
            return result;
        }
        if (cancelled.load(std::memory_order_acquire))
        {
            result.status = VideoProcessStatus::Cancelled;
            return result;
        }
        if (!sourceIsUnchanged())
        {
            rejectChangedSource();
            return result;
        }
        result.trackCount = static_cast<int>(tracks.size());

        std::size_t indexedRegions = 0;
        try
        {
            for (auto &track : tracks)
            {
                for (const auto &tracked : track.boxes)
                {
                    if ((indexedRegions & 0x3FFFU) == 0U
                        && cancelled.load(std::memory_order_acquire))
                    {
                        result.status = VideoProcessStatus::Cancelled;
                        return result;
                    }
                    if (tracked.frame < 0 || tracked.frame >= frameCount
                        || indexedRegions >= kMaxVideoTrackedRegions
                        || !isValidFaceDetection({tracked.box, 1.0F}))
                    {
                        throw std::length_error("Invalid video tracking data.");
                    }
                    FaceDetection detection{tracked.box, 1.0F};
                    detection.rollRadians = tracked.rollRadians;
                    detection.hasPose = isValidFacePose(tracked.rollRadians, tracked.hasPose);
                    frameDetections[static_cast<std::size_t>(tracked.frame)].push_back(detection);
                    ++indexedRegions;
                }
                std::vector<TrackedBox>().swap(track.boxes);
            }
        }
        catch (const std::exception &)
        {
            result.error = trVideoProcessor(QT_TRANSLATE_NOOP(
                "cloakframe::VideoProcessor", "Video tracking data exceeds the safety limit."));
            return result;
        }
        tracks.clear();
        tracks.shrink_to_fit();
        const bool hasTrackedRegions = indexedRegions > 0;
        if (!hasTrackedRegions)
        {
            frameDetections.clear();
            frameDetections.shrink_to_fit();
        }

        struct EncodeAttempt
        {
            VideoProcessStatus status = VideoProcessStatus::Failed;
            // Set when the encoder process itself failed, so the same frames may still encode
            // with a different encoder.
            bool encoderFailed = false;
            bool usedHardwareEncoder = false;
        };

        // A hardware encoder can clear its start-up probe and still fail once real frames reach
        // it. Nothing is published until `finish` succeeds, so a failed attempt can be dropped
        // and the whole pass replayed on the software encoder.
        const auto runEncodePass = [&](const bool allowHardwareEncoder) -> EncodeAttempt
        {
            EncodeAttempt attempt;
            VideoFrameReader reader;
            if (!reader.open(tools, processingSource, activeInfo))
            {
                result.error = reader.errorString();
                return attempt;
            }
            if (!sourceIsUnchanged())
            {
                rejectChangedSource();
                return attempt;
            }
            VideoFrameWriter writer;
            if (!writer.open(tools,
                    destinationPath,
                    processingSource,
                    activeInfo,
                    options.crf,
                    allowHardwareEncoder,
                    options.codec,
                    options.outputRootPath,
                    options.outputRelativePath))
            {
                attempt.usedHardwareEncoder = writer.usedHardwareEncoder();
                attempt.encoderFailed = writer.encoderFailed();
                result.error = writer.errorString();
                return attempt;
            }
            attempt.usedHardwareEncoder = writer.usedHardwareEncoder();
            if (!sourceIsUnchanged())
            {
                rejectChangedSource();
                writer.abort();
                return attempt;
            }
            result.encoderName = writer.encoderName();

            StageTimer readTimer;
            StageTimer maskTimer;
            StageTimer writeTimer;
            const VideoMaskingPlan maskingPlan = videoMaskingPlan(
                reader.frameWidth(), reader.frameHeight(), std::thread::hardware_concurrency());
            const int workerCount = maskingPlan.workerCount;
            const int batchCap = maskingPlan.batchFrames;
            spdlog::info("Video masking plan: {} worker(s), {} frame batch, {} MiB raw-frame cap",
                workerCount,
                batchCap,
                (maskingPlan.frameBytes * static_cast<qint64>(batchCap)) / (qint64{1024} * 1024));
            const ScopedCvThreads maskThreads(1);
            MaskWorkerPool maskPool(hasTrackedRegions ? workerCount : 1);
            qint64 frameIndex = 0;
            std::vector<cv::Mat> batch;
            batch.reserve(static_cast<std::size_t>(batchCap));
            for (;;)
            {
                batch.clear();
                const auto readMark = StageTimer::now();
                for (int slot = 0; slot < batchCap; ++slot)
                {
                    if (cancelled.load(std::memory_order_acquire))
                    {
                        break;
                    }
                    cv::Mat frame;
                    if (!reader.readFrame(frame, canContinue))
                    {
                        break;
                    }
                    batch.push_back(std::move(frame));
                }
                readTimer.add(readMark);
                if (batch.empty())
                {
                    break;
                }
                if (cancelled.load(std::memory_order_acquire))
                {
                    writer.abort();
                    attempt.status = VideoProcessStatus::Cancelled;
                    return attempt;
                }
                if (frameIndex + static_cast<qint64>(batch.size()) > frameCount)
                {
                    result.error = trVideoProcessor(QT_TRANSLATE_NOOP("cloakframe::VideoProcessor",
                        "The source video changed during processing (frame "
                        "count differs between passes)."));
                    writer.abort();
                    return attempt;
                }

                const auto maskMark = StageTimer::now();
                const qint64 baseIndex = frameIndex;
                std::atomic<int> nextSlot{0};
                std::atomic<bool> maskFailed{false};
                std::atomic<bool> maskCancelled{false};
                const auto maskWorker = [&]()
                {
                    int slot;
                    while ((slot = nextSlot.fetch_add(1, std::memory_order_relaxed))
                           < static_cast<int>(batch.size()))
                    {
                        if (cancelled.load(std::memory_order_acquire))
                        {
                            maskCancelled.store(true, std::memory_order_relaxed);
                            return;
                        }
                        try
                        {
                            const auto &toRedact =
                                frameDetections[static_cast<std::size_t>(baseIndex + slot)];
                            applyAnonymization(batch[static_cast<std::size_t>(slot)],
                                toRedact,
                                options.method,
                                options.mosaicBlockSize,
                                options.paddingRatio,
                                options.shape,
                                options.softEdges,
                                options.customImage);
                        }
                        catch (...)
                        {
                            maskFailed.store(true, std::memory_order_relaxed);
                        }
                    }
                };
                if (hasTrackedRegions)
                {
                    maskPool.run(maskWorker);
                }
                maskTimer.add(maskMark);
                if (maskFailed.load(std::memory_order_relaxed))
                {
                    result.error = trVideoProcessor(
                        QT_TRANSLATE_NOOP("cloakframe::VideoProcessor", "Video redaction failed."));
                    writer.abort();
                    return attempt;
                }
                if (maskCancelled.load(std::memory_order_relaxed)
                    || cancelled.load(std::memory_order_acquire))
                {
                    writer.abort();
                    attempt.status = VideoProcessStatus::Cancelled;
                    return attempt;
                }

                const auto writeMark = StageTimer::now();
                for (auto &frame : batch)
                {
                    if (cancelled.load(std::memory_order_acquire))
                    {
                        writer.abort();
                        attempt.status = VideoProcessStatus::Cancelled;
                        return attempt;
                    }
                    if (!writer.writeFrame(frame, canContinue))
                    {
                        writer.abort();
                        writeTimer.add(writeMark);
                        if (cancelled.load(std::memory_order_acquire))
                        {
                            attempt.status = VideoProcessStatus::Cancelled;
                        }
                        else if (!sourceIsUnchanged())
                        {
                            rejectChangedSource();
                        }
                        else
                        {
                            attempt.encoderFailed = writer.encoderFailed();
                            result.error = writer.errorString();
                        }
                        return attempt;
                    }
                    ++frameIndex;
                    if (progress)
                    {
                        progress(2, frameIndex, frameCount);
                    }
                }
                writeTimer.add(writeMark);
            }
            if (cancelled.load(std::memory_order_acquire))
            {
                writer.abort();
                attempt.status = VideoProcessStatus::Cancelled;
                return attempt;
            }
            if (!sourceIsUnchanged())
            {
                rejectChangedSource();
                writer.abort();
                return attempt;
            }
            if (!reader.errorString().isEmpty())
            {
                result.error = reader.errorString();
                writer.abort();
                return attempt;
            }
            if (frameIndex == 0)
            {
                result.error = trVideoProcessor(
                    QT_TRANSLATE_NOOP("cloakframe::VideoProcessor", "No frames could be decoded."));
                writer.abort();
                return attempt;
            }
            if (frameIndex != frameCount)
            {
                result.error = trVideoProcessor(QT_TRANSLATE_NOOP("cloakframe::VideoProcessor",
                    "The source video changed during processing (frame "
                    "count differs between passes)."));
                writer.abort();
                return attempt;
            }
            if (!sourceIsUnchanged())
            {
                rejectChangedSource();
                writer.abort();
                return attempt;
            }
            if (cancelled.load(std::memory_order_acquire))
            {
                writer.abort();
                attempt.status = VideoProcessStatus::Cancelled;
                return attempt;
            }

            StageTimer finishTimer;
            const auto finishMark = StageTimer::now();
            const auto canPublish = [&]()
            {
                return !cancelled.load(std::memory_order_acquire) && sourceIsUnchanged();
            };
            const bool finished = writer.finish(canPublish);
            finishTimer.add(finishMark);
            if (!finished)
            {
                if (cancelled.load(std::memory_order_acquire))
                {
                    attempt.status = VideoProcessStatus::Cancelled;
                    return attempt;
                }
                attempt.encoderFailed = writer.encoderFailed();
                result.error = writer.errorString();
                return attempt;
            }

            spdlog::info("Pass 2 timing: decode {} ms, redact {} ms, encode-write {} ms, "
                         "finalize {} ms ({} frames)",
                readTimer.ms(),
                maskTimer.ms(),
                writeTimer.ms(),
                finishTimer.ms(),
                frameIndex);

            attempt.status = VideoProcessStatus::Completed;
            return attempt;
        };

        auto attempt = runEncodePass(options.hardwareEncoder);
        if (attempt.status == VideoProcessStatus::Failed && attempt.encoderFailed
            && attempt.usedHardwareEncoder && !cancelled.load(std::memory_order_acquire))
        {
            spdlog::warn("Hardware encoder {} failed while encoding ({}); retrying in software",
                result.encoderName.toStdString(),
                result.error.toStdString());
            result.error.clear();
            result.encoderName.clear();
            attempt = runEncodePass(false);
        }
        if (attempt.status != VideoProcessStatus::Completed)
        {
            result.status = attempt.status;
            return result;
        }

        result.status = VideoProcessStatus::Completed;
        return result;
    }
}

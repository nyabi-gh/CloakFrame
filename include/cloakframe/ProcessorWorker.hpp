#pragma once

#include "cloakframe/FileResult.hpp"
#include "cloakframe/ImageScanner.hpp"
#include "cloakframe/ModelCatalog.hpp"
#include "cloakframe/Mosaic.hpp"
#include "cloakframe/VideoIo.hpp"

#include <QByteArray>
#include <QObject>
#include <QPointer>
#include <QString>
#include <QStringList>

#include <atomic>
#include <condition_variable>
#include <cstdint>
#include <filesystem>
#include <memory>
#include <mutex>

namespace cloakframe
{
    class Detector;
    class PlateDetector;

    // Peak bytes one image can hold at once: decoded pixels, the oriented copy, the detection
    // view, mask work and the encoded buffer. Saturates instead of wrapping, so an oversized
    // header can never report a small requirement.
    [[nodiscard]] std::uint64_t estimatedImageMemoryBytes(
        std::uint64_t pixels, std::uint64_t encodedBytes);

    // The whole pool one image run may hold at once. An item whose estimate exceeds this is
    // rejected before it is decoded rather than admitted against a truncated reservation.
    [[nodiscard]] std::uint64_t imageMemoryBudget();

    struct ProcessingRequest
    {
        QString modelPath;
        QByteArray modelSha256;
        FaceModelKind faceModelKind = FaceModelKind::Scrfd;
        QStringList inputs;
        QString outputDirectory;
        QString plateModelPath;
        QByteArray plateModelSha256;
        QObject *reviewReceiver = nullptr;
        bool recursive = true;
        float scoreThreshold = 0.5F;
        float nmsThreshold = 0.4F;
        int mosaicBlockSize = 14;
        float paddingRatio = 0.18F;
        AnonymizationMethod method = AnonymizationMethod::Mosaic;
        cv::Mat customImage;
        MaskShape shape = MaskShape::Rectangle;
        bool softEdges = false;
        bool preserveMetadata = false;
        bool reviewEnabled = false;
        bool detectFaces = true;
        bool detectPlates = false;
        bool gpuAcceleration = false;
        int videoCrf = 18;
        VideoCodec videoCodec = VideoCodec::H264;
    };

    struct DetectorCache
    {
        std::shared_ptr<Detector> face;
        std::shared_ptr<PlateDetector> plate;
        std::shared_ptr<Detector> videoFace;
    };

    enum class RunOutcome
    {
        Completed,
        CompletedWithWarnings,
        Cancelled,
        Failed,
    };

    struct RunSummary
    {
        int total = 0;
        int redacted = 0;
        int copied = 0;
        int skipped = 0;
        int failed = 0;
        int unredacted = 0;
        qint64 omittedRegions = 0;
        qint64 trackingGapFrames = 0;
        qint64 droppedTracks = 0;
        int coverageWarningFiles = 0;
        int warningFiles = 0;
        qint64 unreadableInputs = 0;
    };

    class ProcessorWorker final : public QObject
    {
        Q_OBJECT

    public:
        explicit ProcessorWorker(ProcessingRequest request, DetectorCache cache = {});

        ~ProcessorWorker() override;

        [[nodiscard]] std::shared_ptr<Detector> takeDetector();

        [[nodiscard]] std::shared_ptr<PlateDetector> takePlateDetector();

        [[nodiscard]] std::shared_ptr<Detector> takeVideoDetector();

    public slots:
        void process();

        void cancel();

    signals:
        void progressChanged(int completed, int total);

        void stageChanged(int index, int total, const QString &stage, const QString &fileName);

        void logMessage(const QString &message);

        void summaryAvailable(cloakframe::RunSummary summary);

        void fileResultAvailable(cloakframe::FileResult result);

        void finished(cloakframe::RunOutcome outcome);

    private:
        struct ItemOutcome;

        ItemOutcome processItem(const ScanResult &item,
            const std::filesystem::path &safeRoot,
            int index,
            int total,
            bool allowReview);

        ItemOutcome processVideoItem(const ScanResult &item,
            const std::filesystem::path &safeRoot,
            const std::filesystem::path &destination,
            int index,
            int total);

        QString modelPath_;
        QByteArray modelSha256_;
        FaceModelKind faceModelKind_;
        QStringList inputs_;
        QString outputDirectory_;
        bool recursive_;
        float scoreThreshold_;
        float nmsThreshold_;
        int mosaicBlockSize_;
        float paddingRatio_;
        AnonymizationMethod method_;
        cv::Mat customImage_;
        MaskShape shape_;
        bool softEdges_;
        bool preserveMetadata_;
        bool reviewEnabled_;
        QPointer<QObject> reviewReceiver_;
        bool detectFaces_;
        bool detectPlates_;
        QString plateModelPath_;
        QByteArray plateModelSha256_;
        bool gpuAcceleration_;
        int videoCrf_;
        VideoCodec videoCodec_;
        std::atomic<bool> cancelled_{false};
        std::mutex imageMemoryMutex_;
        std::condition_variable imageMemoryCv_;
        // The whole pool one run may hold at once. `imageMemoryAvailable_` starts here, so an
        // item estimated above this can never be admitted and has to be rejected before decode
        // instead of waiting for a reservation that will never be granted.
        std::uint64_t imageMemoryBudget_ = 0;
        std::uint64_t imageMemoryAvailable_ = 0;
        std::shared_ptr<Detector> detector_;
        std::shared_ptr<PlateDetector> plateDetector_;
        std::shared_ptr<Detector> videoDetector_;
    };
}

Q_DECLARE_METATYPE(cloakframe::RunOutcome)
Q_DECLARE_METATYPE(cloakframe::RunSummary)

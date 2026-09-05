#include "cloakframe/DetectionGeometry.hpp"
#include "cloakframe/ImageIo.hpp"
#include "cloakframe/ImageScanner.hpp"
#include "cloakframe/ModelCatalog.hpp"
#include "cloakframe/Mosaic.hpp"
#include "cloakframe/OnnxGraphPatch.hpp"
#include "cloakframe/OrtAcceleration.hpp"
#include "cloakframe/OutputPlan.hpp"
#include "cloakframe/PathSafety.hpp"
#include "cloakframe/PlateDetector.hpp"
#include "cloakframe/ProcessorWorker.hpp"
#include "cloakframe/ReleaseNotes.hpp"
#include "cloakframe/ReviewTypes.hpp"
#include "cloakframe/ScrfdFaceDetector.hpp"
#include "cloakframe/Yolo5FaceDetector.hpp"
#include "cloakframe/YuNetFaceDetector.hpp"

#include <QCoreApplication>
#include <QCryptographicHash>
#include <QDir>
#include <QFile>
#include <QImage>
#include <QTemporaryDir>
#include <QThread>

#include <opencv2/core.hpp>
#include <opencv2/imgcodecs.hpp>

#include <algorithm>
#include <array>
#include <atomic>
#include <barrier>
#include <cassert>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <limits>
#include <memory>
#include <set>
#include <stdexcept>
#include <string_view>
#include <thread>
#include <vector>

#ifndef _WIN32
#include <sys/stat.h>
#endif

#ifdef CLOAKFRAME_HAVE_EXIV2
#include <exiv2/exiv2.hpp>
#endif

namespace
{
    class CopyOriginalReviewer final : public QObject
    {
        Q_OBJECT

    public slots:
        cloakframe::ReviewResult requestReview(
            const QImage &, const QString &, const QVector<QRectF> &, int, int, double)
        {
            cloakframe::ReviewResult result;
            result.decision = cloakframe::ReviewDecision::CopyOriginal;
            return result;
        }
    };

    class ReplacingCopyOriginalReviewer final : public QObject
    {
        Q_OBJECT

    public:
        ReplacingCopyOriginalReviewer(QString source, QString replacement)
            : source_(std::move(source))
            , replacement_(std::move(replacement))
        {
        }

    public slots:
        cloakframe::ReviewResult requestReview(
            const QImage &, const QString &, const QVector<QRectF> &, int, int, double)
        {
            assert(QFile::remove(source_));
            assert(QFile::rename(replacement_, source_));
            cloakframe::ReviewResult result;
            result.decision = cloakframe::ReviewDecision::CopyOriginal;
            return result;
        }

    private:
        QString source_;
        QString replacement_;
    };

    void testAccelerationBackendNames()
    {
        assert(
            std::string(cloakframe::ortAcceleratorName(cloakframe::OrtAccelerator::None)) == "CPU");
        assert(std::string(cloakframe::ortAcceleratorName(cloakframe::OrtAccelerator::CoreML))
               == "CoreML");
        assert(std::string(cloakframe::ortAcceleratorName(cloakframe::OrtAccelerator::DirectML))
               == "DirectML");
        assert(std::string(cloakframe::ortAcceleratorName(cloakframe::OrtAccelerator::CUDA))
               == "CUDA");
        assert(std::string(cloakframe::ortAcceleratorName(cloakframe::OrtAccelerator::MIGraphX))
               == "MIGraphX");
        assert(std::string(cloakframe::ortAcceleratorName(cloakframe::OrtAccelerator::ROCm))
               == "ROCm");
    }

    void testModelCacheTagIsContentDerived()
    {
        const std::vector<std::uint8_t> bytes = {'m', 'o', 'd', 'e', 'l'};
        const QByteArray digest =
            QCryptographicHash::hash(QByteArrayView("model", 5), QCryptographicHash::Sha256);

        // A caller-supplied digest wins; without one the tag is computed from the bytes, and
        // both spellings must agree so the same model never lands in two cache directories.
        assert(cloakframe::ortModelCacheTag(bytes, digest) == QString::fromLatin1(digest.toHex()));
        assert(cloakframe::ortModelCacheTag(bytes, {}) == QString::fromLatin1(digest.toHex()));

        const std::vector<std::uint8_t> otherBytes = {'o', 't', 'h', 'e', 'r'};
        assert(cloakframe::ortModelCacheTag(otherBytes, {})
               != cloakframe::ortModelCacheTag(bytes, {}));
    }

    void testBuiltinModelDigests()
    {
        assert(cloakframe::builtinModels()[0].faceKind == cloakframe::FaceModelKind::Yolo5Face);
        assert(cloakframe::builtinModels()[1].faceKind == cloakframe::FaceModelKind::YuNet);
        for (const auto &model : cloakframe::builtinModels())
        {
            const QByteArray digest = QByteArray::fromHex(model.sha256.toLatin1());
            assert(cloakframe::modelDigestMatches(model, digest));
            QByteArray changed = digest;
            changed[0] = static_cast<char>(changed[0] ^ 0x01);
            assert(!cloakframe::modelDigestMatches(model, changed));
        }
        const auto &plate = cloakframe::plateModel();
        assert(cloakframe::modelDigestMatches(plate, QByteArray::fromHex(plate.sha256.toLatin1())));
    }

    void writeBytes(const QString &path)
    {
        QFile file(path);
        assert(file.open(QIODevice::WriteOnly));
        assert(file.write("x") == 1);
    }

    void writeBytes(const std::filesystem::path &path, const std::vector<uchar> &bytes)
    {
        QFile file(QString::fromStdString(path.string()));
        assert(file.open(QIODevice::WriteOnly));
        assert(file.write(
                   reinterpret_cast<const char *>(bytes.data()), static_cast<qint64>(bytes.size()))
               == static_cast<qint64>(bytes.size()));
    }

    void writeJpegWithExifOrientation(
        const std::filesystem::path &path, const unsigned char orientation)
    {
        std::vector<uchar> jpeg;
        cv::Mat pixels(2, 3, CV_8UC3, cv::Scalar(20, 40, 60));
        assert(cv::imencode(".jpg", pixels, jpeg));
        assert(jpeg.size() >= 2 && jpeg[0] == 0xFF && jpeg[1] == 0xD8);

        const std::vector<uchar> exif = {
            0xFF,
            0xE1,
            0x00,
            0x22,
            'E',
            'x',
            'i',
            'f',
            0x00,
            0x00,
            'I',
            'I',
            0x2A,
            0x00,
            0x08,
            0x00,
            0x00,
            0x00,
            0x01,
            0x00,
            0x12,
            0x01,
            0x03,
            0x00,
            0x01,
            0x00,
            0x00,
            0x00,
            orientation,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
        };
        jpeg.insert(jpeg.begin() + 2, exif.begin(), exif.end());

        QFile file(QString::fromStdString(path.string()));
        assert(file.open(QIODevice::WriteOnly));
        assert(file.write(
                   reinterpret_cast<const char *>(jpeg.data()), static_cast<qint64>(jpeg.size()))
               == static_cast<qint64>(jpeg.size()));
    }

    void testSupportedImageExtensions()
    {
        assert(cloakframe::isSupportedImage("photo.jpg"));
        assert(cloakframe::isSupportedImage("photo.JPEG"));
        assert(cloakframe::isSupportedImage("photo.webp"));
        assert(!cloakframe::isSupportedImage("photo.txt"));
    }

    void testScanImagesRecursesAndDeduplicates()
    {
        QTemporaryDir temp;
        assert(temp.isValid());

        QDir root(temp.path());
        assert(root.mkpath("a/nested"));
        assert(root.mkpath("b"));

        const QString first = root.filePath("a/one.JPG");
        const QString second = root.filePath("a/nested/two.png");
        const QString ignored = root.filePath("b/notes.txt");
        writeBytes(first);
        writeBytes(second);
        writeBytes(ignored);

        const auto nonRecursive = cloakframe::scanImages({root.filePath("a")}, false);
        assert(nonRecursive.size() == 1);

        const auto recursive = cloakframe::scanImages({root.filePath("a"), first}, true);
        assert(recursive.size() == 2);

        std::set<std::string> relativePaths;
        for (const auto &item : recursive)
        {
            relativePaths.insert(item.relativePath.generic_string());
        }
        assert(relativePaths.contains("one.JPG"));
        assert(relativePaths.contains("nested/two.png"));
    }

    void testScanReportsInputsItCannotRead()
    {
        QTemporaryDir temp;
        assert(temp.isValid());

        QDir root(temp.path());
        assert(root.mkpath("a"));
        const QString present = root.filePath("a/one.JPG");
        writeBytes(present);

        const QString missingFile = root.filePath("a/gone.jpg");
        const QString missingDir = root.filePath("nowhere");

        std::vector<cloakframe::ScanIssue> issues;
        const auto found =
            cloakframe::scanImages({root.filePath("a"), missingFile, missingDir}, true, &issues);
        assert(found.size() == 1);
        assert(issues.size() == 2);
        for (const auto &issue : issues)
        {
            assert(issue.error);
        }

        std::set<std::string> reported;
        for (const auto &issue : issues)
        {
            reported.insert(issue.path.filename().generic_string());
        }
        assert(reported.contains("gone.jpg"));
        assert(reported.contains("nowhere"));

        // A readable tree still reports nothing, so the caller can treat a non-empty list as
        // the only reason to warn.
        std::vector<cloakframe::ScanIssue> clean;
        assert(cloakframe::scanImages({root.filePath("a")}, true, &clean).size() == 1);
        assert(clean.empty());
    }

    void testScanReportsDeniedDirectoriesAndContinues()
    {
#ifndef _WIN32
        QTemporaryDir temp;
        assert(temp.isValid());
        const auto root = std::filesystem::path(temp.path().toStdString());
        const auto input = root / "input";
        const auto denied = input / "denied";
        const auto readable = input / "readable";
        std::filesystem::create_directories(denied);
        std::filesystem::create_directories(readable);
        const cv::Mat image(24, 24, CV_8UC3, cv::Scalar(20, 40, 60));
        assert(cv::imwrite((denied / "private.png").string(), image));
        assert(cv::imwrite((readable / "visible.png").string(), image));
        assert(cv::imwrite((input / "top.png").string(), image));
        std::filesystem::create_directory_symlink(input, readable / "loop");
        std::filesystem::permissions(denied, std::filesystem::perms::none);
        std::error_code permissionError;
        const std::filesystem::directory_iterator probe(denied, permissionError);
        if (!permissionError)
        {
            std::filesystem::permissions(denied, std::filesystem::perms::owner_all);
            std::puts(
                "skipping denied-directory test: this account bypasses directory permissions");
            return;
        }

        std::vector<cloakframe::ScanIssue> recursiveIssues;
        const auto recursive = cloakframe::scanImages(
            {QString::fromStdString(input.string())}, true, &recursiveIssues);
        std::vector<cloakframe::ScanIssue> directIssues;
        const auto direct =
            cloakframe::scanImages({QString::fromStdString(denied.string())}, false, &directIssues);
        std::vector<cloakframe::ScanIssue> shallowIssues;
        const auto shallow =
            cloakframe::scanImages({QString::fromStdString(input.string())}, false, &shallowIssues);

        cloakframe::ProcessingRequest request;
        request.inputs = {QString::fromStdString(input.string())};
        request.outputDirectory = QString::fromStdString((root / "output").string());
        request.detectFaces = false;
        cloakframe::ProcessorWorker worker(std::move(request));
        cloakframe::RunSummary summary;
        cloakframe::RunOutcome outcome = cloakframe::RunOutcome::Completed;
        QObject::connect(&worker,
            &cloakframe::ProcessorWorker::summaryAvailable,
            [&](const cloakframe::RunSummary value)
            {
                summary = value;
            });
        QObject::connect(&worker,
            &cloakframe::ProcessorWorker::finished,
            [&](const cloakframe::RunOutcome value)
            {
                outcome = value;
            });
        worker.process();
        std::filesystem::permissions(denied, std::filesystem::perms::owner_all);

        assert(permissionError == std::errc::permission_denied);
        assert(recursive.size() == 2);
        assert(recursiveIssues.size() == 1);
        assert(recursiveIssues.front().path == denied);
        assert(recursiveIssues.front().error == std::errc::permission_denied);
        assert(direct.empty() && directIssues.size() == 1);
        assert(directIssues.front().path == denied);
        assert(shallow.size() == 1 && shallowIssues.empty());
        assert(summary.total == 2 && summary.unreadableInputs == 1);
        assert(summary.coverageWarningFiles == 0 && summary.warningFiles == 0);
        assert(outcome == cloakframe::RunOutcome::CompletedWithWarnings);
#else
        std::puts(
            "skipping denied-directory test: POSIX permission fixture is not supported on Windows");
#endif
    }

    void testOutputPlanRejectsExistingAndDuplicateDestinations()
    {
        QTemporaryDir temp;
        assert(temp.isValid());
        const std::filesystem::path root(temp.path().toStdString());
        const auto output = root / "out";
        assert(std::filesystem::create_directories(output / "nested"));

        const std::vector<cloakframe::ScanResult> unique = {
            {root / "input" / "one.jpg", "one.jpg"},
            {root / "input" / "two.mov", "nested/two.mov"},
        };
        assert(cloakframe::findOutputConflicts(unique, output).empty());
        assert(
            cloakframe::outputRelativePath(unique[1]) == std::filesystem::path("nested/two.mp4"));

        writeBytes(QString::fromStdString((output / "one.jpg").string()));
        const auto existing = cloakframe::findOutputConflicts(unique, output);
        assert(existing.size() == 1);
        assert(existing[0].kind == cloakframe::OutputConflict::Kind::ExistingDestination);

        const std::vector<cloakframe::ScanResult> duplicate = {
            {root / "a" / "same.jpg", "same.jpg"},
            {root / "b" / "same.jpg", "same.jpg"},
        };
        const auto collisions = cloakframe::findOutputConflicts(duplicate, output);
        assert(collisions.size() == 1);
        assert(collisions[0].kind == cloakframe::OutputConflict::Kind::DuplicateDestination);
    }

    void testWorkerReportsUnredactedOutputAsWarningAndPreservesIt()
    {
        QTemporaryDir temp;
        assert(temp.isValid());
        const auto root = std::filesystem::path(temp.path().toStdString());
        const auto source = root / "input.png";
        const auto output = root / "out";
        assert(cv::imwrite(source.string(), cv::Mat(24, 24, CV_8UC3, cv::Scalar(20, 40, 60))));

        auto makeWorker = [&]
        {
            cloakframe::ProcessingRequest request;
            request.inputs = {QString::fromStdString(source.string())};
            request.outputDirectory = QString::fromStdString(output.string());
            request.recursive = false;
            request.detectFaces = false;
            return std::make_unique<cloakframe::ProcessorWorker>(std::move(request));
        };

        cloakframe::RunOutcome firstOutcome = cloakframe::RunOutcome::Failed;
        cloakframe::RunSummary firstSummary;
        auto first = makeWorker();
        QObject::connect(first.get(),
            &cloakframe::ProcessorWorker::summaryAvailable,
            [&](const cloakframe::RunSummary summary)
            {
                firstSummary = summary;
            });
        QObject::connect(first.get(),
            &cloakframe::ProcessorWorker::finished,
            [&](const cloakframe::RunOutcome outcome)
            {
                firstOutcome = outcome;
            });
        first->process();
        assert(firstOutcome == cloakframe::RunOutcome::CompletedWithWarnings);
        assert(
            firstSummary.total == 1 && firstSummary.redacted == 0 && firstSummary.unredacted == 1);
        assert(std::filesystem::exists(output / "input.png"));

        const auto savedSize = std::filesystem::file_size(output / "input.png");
        cloakframe::RunOutcome secondOutcome = cloakframe::RunOutcome::Completed;
        auto second = makeWorker();
        QObject::connect(second.get(),
            &cloakframe::ProcessorWorker::finished,
            [&](const cloakframe::RunOutcome outcome)
            {
                secondOutcome = outcome;
            });
        second->process();
        assert(secondOutcome == cloakframe::RunOutcome::Failed);
        assert(std::filesystem::file_size(output / "input.png") == savedSize);
    }

    void testWorkerReportsCopiedOriginalAsWarning()
    {
        QTemporaryDir temp;
        assert(temp.isValid());
        const auto root = std::filesystem::path(temp.path().toStdString());
        const auto source = root / "input.png";
        const auto output = root / "out";
        assert(cv::imwrite(source.string(), cv::Mat(24, 24, CV_8UC3, cv::Scalar(20, 40, 60))));

        qRegisterMetaType<cloakframe::ReviewResult>("cloakframe::ReviewResult");
        QThread reviewThread;
        auto *reviewer = new CopyOriginalReviewer;
        reviewer->moveToThread(&reviewThread);
        QObject::connect(&reviewThread, &QThread::finished, reviewer, &QObject::deleteLater);
        reviewThread.start();

        cloakframe::ProcessingRequest request;
        request.inputs = {QString::fromStdString(source.string())};
        request.outputDirectory = QString::fromStdString(output.string());
        request.detectFaces = false;
        request.reviewEnabled = true;
        request.reviewReceiver = reviewer;

        cloakframe::RunOutcome result = cloakframe::RunOutcome::Failed;
        cloakframe::RunSummary summary;
        cloakframe::ProcessorWorker worker(std::move(request));
        QObject::connect(&worker,
            &cloakframe::ProcessorWorker::summaryAvailable,
            [&](const cloakframe::RunSummary value)
            {
                summary = value;
            });
        QObject::connect(&worker,
            &cloakframe::ProcessorWorker::finished,
            [&](const cloakframe::RunOutcome value)
            {
                result = value;
            });
        worker.process();

        reviewThread.quit();
        assert(reviewThread.wait());
        assert(result == cloakframe::RunOutcome::CompletedWithWarnings);
        assert(summary.total == 1 && summary.copied == 1);
        assert(std::filesystem::exists(output / "input.png"));
    }

    void testReviewConfirmsOnlyWhenDetectionsWereCleared()
    {
        // Nothing was found, so nothing was cleared. An ordinary photo with no face in it must
        // not raise the prompt, or the prompt stops meaning anything.
        assert(!cloakframe::reviewClearedEveryDetection(0, 0));
        // Found nothing, and the reader drew a region anyway.
        assert(!cloakframe::reviewClearedEveryDetection(0, 2));
        // Found regions and kept some or all of them.
        assert(!cloakframe::reviewClearedEveryDetection(3, 1));
        assert(!cloakframe::reviewClearedEveryDetection(3, 3));
        // Found regions, excluded them, and put manual regions in their place.
        assert(!cloakframe::reviewClearedEveryDetection(3, 2));
        // Found regions and excluded every one with nothing left behind.
        assert(cloakframe::reviewClearedEveryDetection(1, 0));
        assert(cloakframe::reviewClearedEveryDetection(64, 0));
    }

    void testWorkerFailsWhenTheReviewReceiverIsMissing()
    {
        QTemporaryDir temp;
        assert(temp.isValid());
        const auto root = std::filesystem::path(temp.path().toStdString());
        const auto source = root / "input.png";
        const auto output = root / "out";
        assert(cv::imwrite(source.string(), cv::Mat(24, 24, CV_8UC3, cv::Scalar(20, 40, 60))));

        cloakframe::ProcessingRequest request;
        request.inputs = {QString::fromStdString(source.string())};
        request.outputDirectory = QString::fromStdString(output.string());
        request.detectFaces = false;
        request.reviewEnabled = true;

        cloakframe::RunOutcome result = cloakframe::RunOutcome::Completed;
        cloakframe::ProcessorWorker worker(std::move(request));
        QObject::connect(&worker,
            &cloakframe::ProcessorWorker::finished,
            [&](const cloakframe::RunOutcome value)
            {
                result = value;
            });
        worker.process();

        assert(result == cloakframe::RunOutcome::Failed);
        assert(!std::filesystem::exists(output / "input.png"));
    }

    void testWorkerFailsWhenTheReviewReceiverDiesBeforeTheRun()
    {
        QTemporaryDir temp;
        assert(temp.isValid());
        const auto root = std::filesystem::path(temp.path().toStdString());
        const auto source = root / "input.png";
        const auto output = root / "out";
        assert(cv::imwrite(source.string(), cv::Mat(24, 24, CV_8UC3, cv::Scalar(20, 40, 60))));

        auto reviewer = std::make_unique<QObject>();
        cloakframe::ProcessingRequest request;
        request.inputs = {QString::fromStdString(source.string())};
        request.outputDirectory = QString::fromStdString(output.string());
        request.detectFaces = false;
        request.reviewEnabled = true;
        request.reviewReceiver = reviewer.get();

        cloakframe::RunOutcome result = cloakframe::RunOutcome::Completed;
        cloakframe::ProcessorWorker worker(std::move(request));
        QObject::connect(&worker,
            &cloakframe::ProcessorWorker::finished,
            [&](const cloakframe::RunOutcome value)
            {
                result = value;
            });
        reviewer.reset();
        worker.process();

        assert(result == cloakframe::RunOutcome::Failed);
        assert(!std::filesystem::exists(output / "input.png"));
    }

    void testWorkerSavesNothingWhenTheReviewSlotIsMissing()
    {
        QTemporaryDir temp;
        assert(temp.isValid());
        const auto root = std::filesystem::path(temp.path().toStdString());
        const auto source = root / "input.png";
        const auto output = root / "out";
        assert(cv::imwrite(source.string(), cv::Mat(24, 24, CV_8UC3, cv::Scalar(20, 40, 60))));

        // invokeMethod resolves the member before it honours the connection type, so a receiver
        // without the slot answers false here rather than blocking this thread on itself.
        QObject reviewer;
        cloakframe::ProcessingRequest request;
        request.inputs = {QString::fromStdString(source.string())};
        request.outputDirectory = QString::fromStdString(output.string());
        request.detectFaces = false;
        request.reviewEnabled = true;
        request.reviewReceiver = &reviewer;

        cloakframe::RunOutcome result = cloakframe::RunOutcome::Completed;
        cloakframe::RunSummary summary;
        cloakframe::ProcessorWorker worker(std::move(request));
        QObject::connect(&worker,
            &cloakframe::ProcessorWorker::summaryAvailable,
            [&](const cloakframe::RunSummary value)
            {
                summary = value;
            });
        QObject::connect(&worker,
            &cloakframe::ProcessorWorker::finished,
            [&](const cloakframe::RunOutcome value)
            {
                result = value;
            });
        worker.process();

        assert(result == cloakframe::RunOutcome::CompletedWithWarnings);
        assert(summary.total == 1 && summary.failed == 1);
        assert(summary.redacted == 0 && summary.unredacted == 0 && summary.copied == 0);
        assert(!std::filesystem::exists(output / "input.png"));
    }

    void testWorkerHonoursCancelRequestedBeforeProcess()
    {
        QTemporaryDir temp;
        assert(temp.isValid());
        const auto root = std::filesystem::path(temp.path().toStdString());
        const auto source = root / "input.png";
        const auto output = root / "out";
        assert(cv::imwrite(source.string(), cv::Mat(24, 24, CV_8UC3, cv::Scalar(20, 40, 60))));

        cloakframe::ProcessingRequest request;
        request.inputs = {QString::fromStdString(source.string())};
        request.outputDirectory = QString::fromStdString(output.string());
        request.detectFaces = false;

        cloakframe::RunOutcome result = cloakframe::RunOutcome::Completed;
        cloakframe::ProcessorWorker worker(std::move(request));
        QObject::connect(&worker,
            &cloakframe::ProcessorWorker::finished,
            [&](const cloakframe::RunOutcome value)
            {
                result = value;
            });
        worker.cancel();
        worker.process();

        assert(result == cloakframe::RunOutcome::Cancelled);
        assert(!std::filesystem::exists(output / "input.png"));
    }

    class OverflowingDetector final : public cloakframe::Detector
    {
    public:
        cloakframe::DetectionResult detect(const cv::Mat &, float, float) override
        {
            cloakframe::FaceDetections detections;
            detections.push_back({cv::Rect2f(4.0F, 4.0F, 8.0F, 8.0F), 0.9F});
            return {std::move(detections), 3};
        }
    };

    void testDroppedDetectionsKeepTheRunOutOfCleanCompletion()
    {
        QTemporaryDir temp;
        assert(temp.isValid());
        const auto root = std::filesystem::path(temp.path().toStdString());
        const auto source = root / "input.png";
        const auto output = root / "out";
        assert(cv::imwrite(source.string(), cv::Mat(32, 32, CV_8UC3, cv::Scalar(20, 40, 60))));

        cloakframe::ProcessingRequest request;
        request.inputs = {QString::fromStdString(source.string())};
        request.outputDirectory = QString::fromStdString(output.string());
        request.detectFaces = true;

        cloakframe::DetectorCache cache;
        cache.face = std::make_shared<OverflowingDetector>();

        cloakframe::RunOutcome result = cloakframe::RunOutcome::Failed;
        cloakframe::RunSummary summary;
        cloakframe::ProcessorWorker worker(std::move(request), std::move(cache));
        QObject::connect(&worker,
            &cloakframe::ProcessorWorker::summaryAvailable,
            [&](const cloakframe::RunSummary value)
            {
                summary = value;
            });
        QObject::connect(&worker,
            &cloakframe::ProcessorWorker::finished,
            [&](const cloakframe::RunOutcome value)
            {
                result = value;
            });
        worker.process();

        assert(summary.redacted == 1);
        assert(summary.omittedRegions == 3);
        assert(summary.trackingGapFrames == 0);
        assert(summary.droppedTracks == 0);
        assert(summary.coverageWarningFiles == 1);
        assert(result == cloakframe::RunOutcome::CompletedWithWarnings);
    }

    void testWorkerUsesStableImageSnapshotDuringReview()
    {
        QTemporaryDir temp;
        assert(temp.isValid());
        const auto root = std::filesystem::path(temp.path().toStdString());
        const auto source = root / "input.png";
        const auto replacement = root / "replacement.png";
        const auto output = root / "out";
        assert(cv::imwrite(source.string(), cv::Mat(32, 32, CV_8UC3, cv::Scalar(10, 20, 30))));
        assert(
            cv::imwrite(replacement.string(), cv::Mat(32, 32, CV_8UC3, cv::Scalar(200, 210, 220))));
        QFile originalFile(QString::fromStdString(source.string()));
        assert(originalFile.open(QIODevice::ReadOnly));
        const QByteArray originalBytes = originalFile.readAll();
        originalFile.close();

        QThread reviewThread;
        auto *reviewer = new ReplacingCopyOriginalReviewer(
            QString::fromStdString(source.string()), QString::fromStdString(replacement.string()));
        reviewer->moveToThread(&reviewThread);
        QObject::connect(&reviewThread, &QThread::finished, reviewer, &QObject::deleteLater);
        reviewThread.start();

        cloakframe::ProcessingRequest request;
        request.inputs = {QString::fromStdString(source.string())};
        request.outputDirectory = QString::fromStdString(output.string());
        request.detectFaces = false;
        request.reviewEnabled = true;
        request.reviewReceiver = reviewer;
        request.preserveMetadata = true;

        cloakframe::RunOutcome result = cloakframe::RunOutcome::Failed;
        cloakframe::ProcessorWorker worker(std::move(request));
        QObject::connect(&worker,
            &cloakframe::ProcessorWorker::finished,
            [&](const cloakframe::RunOutcome value)
            {
                result = value;
            });
        worker.process();

        reviewThread.quit();
        assert(reviewThread.wait());
        assert(result == cloakframe::RunOutcome::CompletedWithWarnings);
        QFile outputFile(QString::fromStdString((output / "input.png").string()));
        assert(outputFile.open(QIODevice::ReadOnly));
        assert(outputFile.readAll() == originalBytes);
    }

    void testWorkerAcceptsThirtyMegabyteJpeg()
    {
        QTemporaryDir temp;
        assert(temp.isValid());
        const auto root = std::filesystem::path(temp.path().toStdString());
        const auto source = root / "large.jpg";
        const auto output = root / "out";
        assert(cv::imwrite(source.string(), cv::Mat(64, 64, CV_8UC3, cv::Scalar(40, 80, 120))));
        std::filesystem::resize_file(source, 31ULL * 1024ULL * 1024ULL);

        cloakframe::ProcessingRequest request;
        request.inputs = {QString::fromStdString(source.string())};
        request.outputDirectory = QString::fromStdString(output.string());
        request.detectFaces = false;

        cloakframe::RunOutcome result = cloakframe::RunOutcome::Failed;
        cloakframe::ProcessorWorker worker(std::move(request));
        QObject::connect(&worker,
            &cloakframe::ProcessorWorker::finished,
            [&](const cloakframe::RunOutcome value)
            {
                result = value;
            });
        worker.process();
        assert(result == cloakframe::RunOutcome::CompletedWithWarnings);
        assert(std::filesystem::exists(output / "large.jpg"));
    }

    // CRC-32/ISO-HDLC over a PNG chunk's type and data, as the format requires.
    std::uint32_t pngChunkCrc(const std::vector<unsigned char> &bytes)
    {
        std::uint32_t crc = 0xFFFFFFFFU;
        for (const auto byte : bytes)
        {
            crc ^= byte;
            for (int bit = 0; bit < 8; ++bit)
            {
                crc = (crc & 1U) != 0U ? (crc >> 1U) ^ 0xEDB88320U : crc >> 1U;
            }
        }
        return crc ^ 0xFFFFFFFFU;
    }

    // A PNG carrying only its signature, IHDR and a stub IDAT. A reader takes the dimensions
    // from IHDR without decoding anything, which is exactly what the worker inspects before it
    // decides whether an image is small enough to decode.
    void writePngHeaderWithDeclaredSize(
        const std::filesystem::path &path, const std::uint32_t width, const std::uint32_t height)
    {
        std::vector<unsigned char> file{137, 80, 78, 71, 13, 10, 26, 10};
        const auto appendBigEndian =
            [](std::vector<unsigned char> &target, const std::uint32_t value)
        {
            target.push_back(static_cast<unsigned char>((value >> 24U) & 0xFFU));
            target.push_back(static_cast<unsigned char>((value >> 16U) & 0xFFU));
            target.push_back(static_cast<unsigned char>((value >> 8U) & 0xFFU));
            target.push_back(static_cast<unsigned char>(value & 0xFFU));
        };
        const auto appendChunk =
            [&](const std::string &type, const std::vector<unsigned char> &data)
        {
            appendBigEndian(file, static_cast<std::uint32_t>(data.size()));
            std::vector<unsigned char> checked(type.cbegin(), type.cend());
            checked.insert(checked.cend(), data.cbegin(), data.cend());
            file.insert(file.cend(), checked.cbegin(), checked.cend());
            appendBigEndian(file, pngChunkCrc(checked));
        };

        std::vector<unsigned char> header;
        appendBigEndian(header, width);
        appendBigEndian(header, height);
        header.push_back(8); // bit depth
        header.push_back(2); // truecolour
        header.push_back(0); // deflate
        header.push_back(0); // adaptive filtering
        header.push_back(0); // no interlace
        appendChunk("IHDR", header);
        appendChunk("IDAT", {0x78});
        appendChunk("IEND", {});

        std::ofstream stream(path, std::ios::binary);
        assert(stream.is_open());
        stream.write(
            reinterpret_cast<const char *>(file.data()), static_cast<std::streamsize>(file.size()));
        stream.close();
        assert(!stream.fail());
    }

#ifndef _WIN32
    class ForcedNonAtomicPublication
    {
    public:
        ForcedNonAtomicPublication()
        {
            cloakframe::setAtomicPublicationDisabledForTesting(true);
        }

        ~ForcedNonAtomicPublication()
        {
            cloakframe::setAtomicPublicationDisabledForTesting(false);
        }

        ForcedNonAtomicPublication(const ForcedNonAtomicPublication &) = delete;
        ForcedNonAtomicPublication &operator=(const ForcedNonAtomicPublication &) = delete;
    };

    void testPublicationWithoutAtomicPrimitivesLeavesNoCompleteLookingPartial()
    {
        QTemporaryDir temp;
        assert(temp.isValid());
        // The rooted publication path compares the canonical root against the absolute one, so
        // the symlinked temporary directory has to be resolved first.
        const auto root = std::filesystem::canonical(temp.path().toStdString());
        const cv::Mat image(64, 64, CV_8UC3, cv::Scalar(30, 60, 90));
        const auto noResidue = [&root]
        {
            for (const auto &entry : std::filesystem::directory_iterator(root))
            {
                if (entry.path().filename().string().find(".cloakframe-") != std::string::npos)
                {
                    return false;
                }
            }
            return true;
        };

        const ForcedNonAtomicPublication forced;

        assert(cloakframe::imwriteUnicodeNoReplaceAtRoot(root, "copy.png", image)
               == cloakframe::ImageWriteResult::Saved);
        assert(!cv::imread((root / "copy.png").string(), cv::IMREAD_UNCHANGED).empty());
        assert(noResidue());

        // An existing destination still wins: the fallback claims the name only after the copy
        // finishes, and only if nothing else took it.
        writeBytes(QString::fromStdString((root / "taken.png").string()));
        assert(cloakframe::imwriteUnicodeNoReplaceAtRoot(root, "taken.png", image)
               == cloakframe::ImageWriteResult::Failed);
        assert(std::filesystem::file_size(root / "taken.png") == 1);
        assert(noResidue());

        // A guard that refuses publication must leave neither an output nor a partial file.
        assert(cloakframe::imwriteUnicodeNoReplaceAtRoot(root,
                   "guarded.png",
                   image,
                   {},
                   {},
                   []
                   {
                       return false;
                   })
               == cloakframe::ImageWriteResult::Failed);
        assert(!std::filesystem::exists(root / "guarded.png"));
        assert(noResidue());
    }
#endif

    void testWorkerRejectsAnImageLargerThanTheMemoryBudget()
    {
        constexpr auto kUnsignedMaximum = std::numeric_limits<std::uint64_t>::max();
        assert(cloakframe::estimatedImageMemoryBytes(4, 100) == 164);
        assert(cloakframe::estimatedImageMemoryBytes(kUnsignedMaximum, 1) == kUnsignedMaximum);
        assert(cloakframe::estimatedImageMemoryBytes(kUnsignedMaximum, kUnsignedMaximum)
               == kUnsignedMaximum);

        const auto budget = cloakframe::imageMemoryBudget();
        const auto estimateForSide = [](const std::int32_t side)
        {
            const auto pixels = static_cast<std::uint64_t>(side) * static_cast<std::uint64_t>(side);
            return cloakframe::estimatedImageMemoryBytes(pixels, 0);
        };

        // The worker rejects images past a fixed pixel count before it looks at memory at all,
        // so on a host whose budget is larger than the biggest image that gate admits, this
        // rejection is unreachable through a real file.
        constexpr std::int32_t kLargestTestSide = 22000;
        std::int32_t side = 2048;
        while (side < kLargestTestSide && estimateForSide(side) <= budget)
        {
            side += 1024;
        }
        if (estimateForSide(side) <= budget)
        {
            std::puts("skipped over-budget image rejection: this host's image memory budget "
                      "exceeds the largest image the dimension gate admits");
            return;
        }

        QTemporaryDir temp;
        assert(temp.isValid());
        const auto root = std::filesystem::path(temp.path().toStdString());
        const auto source = root / "huge.png";
        const auto output = root / "out";
        writePngHeaderWithDeclaredSize(
            source, static_cast<std::uint32_t>(side), static_cast<std::uint32_t>(side));

        cloakframe::ProcessingRequest request;
        request.inputs = {QString::fromStdString(source.string())};
        request.outputDirectory = QString::fromStdString(output.string());
        request.detectFaces = false;

        cloakframe::RunOutcome result = cloakframe::RunOutcome::Failed;
        cloakframe::RunSummary summary;
        QStringList logs;
        cloakframe::ProcessorWorker worker(std::move(request));
        QObject::connect(&worker,
            &cloakframe::ProcessorWorker::logMessage,
            [&](const QString &message)
            {
                logs.push_back(message);
            });
        QObject::connect(&worker,
            &cloakframe::ProcessorWorker::summaryAvailable,
            [&](const cloakframe::RunSummary value)
            {
                summary = value;
            });
        QObject::connect(&worker,
            &cloakframe::ProcessorWorker::finished,
            [&](const cloakframe::RunOutcome value)
            {
                result = value;
            });
        worker.process();

        assert(summary.skipped == 1);
        assert(summary.redacted == 0);
        assert(result == cloakframe::RunOutcome::CompletedWithWarnings);
        assert(!std::filesystem::exists(output / "huge.png"));
        // The item has to be turned away for its memory requirement, not because the header
        // could not be read at all.
        assert(std::any_of(logs.cbegin(),
            logs.cend(),
            [](const QString &message)
            {
                return message.contains("MB limit");
            }));
    }

    void testReleaseNotesPickTheInterfaceLanguage()
    {
        const QString combined =
            QStringLiteral("<!-- notes:ko -->\n\n## 설치\n\n- 한국어 항목\n\n"
                           "<details><summary>English</summary>\n\n<!-- notes:en -->\n\n"
                           "## Install\n\n- English item\n\n</details>\n\n"
                           "<details><summary>日本語</summary>\n\n<!-- notes:ja -->\n\n"
                           "## インストール\n\n- 日本語の項目\n\n</details>\n");

        const QString korean = cloakframe::releaseNotesForLanguage(combined, "ko");
        assert(korean.startsWith("## 설치"));
        assert(korean.contains("한국어 항목"));
        assert(!korean.contains("English item"));
        assert(!korean.contains("日本語の項目"));

        const QString japanese = cloakframe::releaseNotesForLanguage(combined, "ja");
        assert(japanese.contains("日本語の項目"));
        assert(!japanese.contains("English item"));
        // The <details> wrapper is markup, and the update dialog renders plain text.
        assert(!japanese.contains("<details"));
        assert(!japanese.contains("</details>"));
        assert(!japanese.contains("<summary"));

        // A language the release does not carry falls back to English.
        const QString chinese = cloakframe::releaseNotesForLanguage(combined, "zh");
        assert(chinese.contains("English item"));

        // Notes without markers are shown as they are, so an older release still reads fine.
        const QString plain = QStringLiteral("  ## Only one language\n\n- item\n ");
        assert(cloakframe::releaseNotesForLanguage(plain, "ko")
               == QStringLiteral("## Only one language\n\n- item"));
        assert(cloakframe::releaseNotesForLanguage(QString(), "ko").isEmpty());
    }

    void testWorkerRejectsMultiFrameImages()
    {
        QTemporaryDir temp;
        assert(temp.isValid());
        const auto root = std::filesystem::path(temp.path().toStdString());
        const auto source = root / "pages.tiff";
        const auto output = root / "out";

        const std::vector<cv::Mat> pages = {
            cv::Mat(12, 16, CV_8UC3, cv::Scalar(20, 40, 60)),
            cv::Mat(12, 16, CV_8UC3, cv::Scalar(80, 100, 120)),
        };
        assert(cv::imwritemulti(source.string(), pages));
        assert(cloakframe::imageFrameCount(source) == 2);

        cloakframe::ProcessingRequest request;
        request.inputs = {QString::fromStdString(source.string())};
        request.outputDirectory = QString::fromStdString(output.string());
        request.detectFaces = false;

        cloakframe::RunOutcome result = cloakframe::RunOutcome::Failed;
        cloakframe::RunSummary summary;
        QStringList messages;
        cloakframe::ProcessorWorker worker(std::move(request));
        QObject::connect(&worker,
            &cloakframe::ProcessorWorker::summaryAvailable,
            [&](const cloakframe::RunSummary value)
            {
                summary = value;
            });
        QObject::connect(&worker,
            &cloakframe::ProcessorWorker::logMessage,
            [&](const QString &message)
            {
                messages.push_back(message);
            });
        QObject::connect(&worker,
            &cloakframe::ProcessorWorker::finished,
            [&](const cloakframe::RunOutcome value)
            {
                result = value;
            });
        worker.process();

        assert(result == cloakframe::RunOutcome::CompletedWithWarnings);
        assert(summary.total == 1 && summary.skipped == 1);
        assert(!std::filesystem::exists(output / "pages.tiff"));
        assert(std::ranges::any_of(messages,
            [](const QString &message)
            {
                return message.contains("multi-page", Qt::CaseInsensitive);
            }));
    }

    void testAnimatedImageContainersAreDetectedWithoutDecodingAllFrames()
    {
        QTemporaryDir temp;
        assert(temp.isValid());
        const auto root = std::filesystem::path(temp.path().toStdString());

        const auto apng = root / "animated.png";
        writeBytes(apng,
            {
                0x89,
                'P',
                'N',
                'G',
                0x0D,
                0x0A,
                0x1A,
                0x0A,
                0x00,
                0x00,
                0x00,
                0x08,
                'a',
                'c',
                'T',
                'L',
                0x00,
                0x00,
                0x00,
                0x03,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
            });
        assert(cloakframe::imageFrameCount(apng) > 1);

        const auto webp = root / "animated.webp";
        writeBytes(webp,
            {
                'R',
                'I',
                'F',
                'F',
                0x12,
                0x00,
                0x00,
                0x00,
                'W',
                'E',
                'B',
                'P',
                'V',
                'P',
                '8',
                'X',
                0x0A,
                0x00,
                0x00,
                0x00,
                0x02,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
            });
        assert(cloakframe::imageFrameCount(webp) > 1);

        const auto bigTiff = root / "pages-bigtiff.tiff";
        writeBytes(bigTiff,
            {
                'I',
                'I',
                0x2B,
                0x00,
                0x08,
                0x00,
                0x00,
                0x00,
                0x10,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x20,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
                0x00,
            });
        assert(cloakframe::imageFrameCount(bigTiff) > 1);
    }

    void testApplyMosaicTouchesOnlyDetectedRegion()
    {
        cv::Mat image(8, 8, CV_8UC3);
        for (int y = 0; y < image.rows; ++y)
        {
            for (int x = 0; x < image.cols; ++x)
            {
                image.at<cv::Vec3b>(y, x) = cv::Vec3b(static_cast<uchar>(x * 20),
                    static_cast<uchar>(y * 20),
                    static_cast<uchar>((x + y) * 10));
            }
        }

        const cv::Vec3b outsideBefore = image.at<cv::Vec3b>(0, 0);
        const cv::Vec3b insideBefore = image.at<cv::Vec3b>(3, 3);

        cloakframe::FaceDetections detections;
        detections.push_back({cv::Rect2f(2.0F, 2.0F, 4.0F, 4.0F), 1.0F});
        cloakframe::applyMosaic(image, detections, 4, 0.0F);

        assert(image.at<cv::Vec3b>(0, 0) == outsideBefore);
        assert(image.at<cv::Vec3b>(3, 3) != insideBefore);
    }

    void testSoftEdgesKeepDetectedRegionFullyCovered()
    {
        cv::Mat image(64, 64, CV_8UC3, cv::Scalar(100, 100, 100));
        const cv::Rect box(24, 24, 16, 16);

        cloakframe::FaceDetections detections;
        detections.push_back({cv::Rect2f(box), 1.0F});
        cloakframe::applyAnonymization(image,
            detections,
            cloakframe::AnonymizationMethod::Fill,
            4,
            0.0F,
            cloakframe::MaskShape::Rectangle,
            true);

        for (int y = box.y; y < box.y + box.height; ++y)
        {
            for (int x = box.x; x < box.x + box.width; ++x)
            {
                assert(image.at<cv::Vec3b>(y, x) == cv::Vec3b(0, 0, 0));
            }
        }

        assert(image.at<cv::Vec3b>(0, 0) == cv::Vec3b(100, 100, 100));

        const cv::Vec3b feathered =
            image.at<cv::Vec3b>(box.y + box.height / 2, box.x + box.width + 1);
        assert(feathered != cv::Vec3b(0, 0, 0));
        assert(feathered != cv::Vec3b(100, 100, 100));
    }

    void testFillIsOpaqueOnAlphaImages()
    {
        const cv::Rect box(24, 24, 16, 16);
        cloakframe::FaceDetections detections;
        detections.push_back({cv::Rect2f(box), 1.0F});

        cv::Mat image(64, 64, CV_8UC4, cv::Scalar(100, 110, 120, 200));
        cloakframe::applyAnonymization(image,
            detections,
            cloakframe::AnonymizationMethod::Fill,
            4,
            0.0F,
            cloakframe::MaskShape::Rectangle,
            false);

        for (int y = box.y; y < box.y + box.height; ++y)
        {
            for (int x = box.x; x < box.x + box.width; ++x)
            {
                assert(image.at<cv::Vec4b>(y, x) == cv::Vec4b(0, 0, 0, 255));
            }
        }
        assert(image.at<cv::Vec4b>(0, 0) == cv::Vec4b(100, 110, 120, 200));

        cv::Mat deep(64, 64, CV_16UC4, cv::Scalar(1000, 2000, 3000, 40000));
        cloakframe::applyAnonymization(deep,
            detections,
            cloakframe::AnonymizationMethod::Fill,
            4,
            0.0F,
            cloakframe::MaskShape::Rectangle,
            false);

        assert(deep.at<cv::Vec4w>(box.y + 1, box.x + 1) == cv::Vec4w(0, 0, 0, 65535));
        assert(deep.at<cv::Vec4w>(0, 0) == cv::Vec4w(1000, 2000, 3000, 40000));
    }

    void testSoftEdgesEllipseKeepsCoreCovered()
    {
        cv::Mat image(64, 64, CV_8UC3, cv::Scalar(100, 100, 100));
        const cv::Rect box(20, 20, 24, 24);

        cloakframe::FaceDetections detections;
        detections.push_back({cv::Rect2f(box), 1.0F});
        cloakframe::applyAnonymization(image,
            detections,
            cloakframe::AnonymizationMethod::Fill,
            4,
            0.0F,
            cloakframe::MaskShape::Ellipse,
            true);

        const int centerX = box.x + box.width / 2;
        const int centerY = box.y + box.height / 2;
        for (int dy = -box.height / 4; dy <= box.height / 4; ++dy)
        {
            for (int dx = -box.width / 4; dx <= box.width / 4; ++dx)
            {
                assert(image.at<cv::Vec3b>(centerY + dy, centerX + dx) == cv::Vec3b(0, 0, 0));
            }
        }

        assert(image.at<cv::Vec3b>(0, 0) == cv::Vec3b(100, 100, 100));
    }

    void testSoftEdgesAtImageBorderStayInBounds()
    {
        cv::Mat image(32, 32, CV_8UC3, cv::Scalar(100, 100, 100));

        cloakframe::FaceDetections detections;
        detections.push_back({cv::Rect2f(0.0F, 0.0F, 12.0F, 12.0F), 1.0F});
        detections.push_back({cv::Rect2f(24.0F, 24.0F, 8.0F, 8.0F), 1.0F});
        cloakframe::applyAnonymization(image,
            detections,
            cloakframe::AnonymizationMethod::Fill,
            4,
            0.0F,
            cloakframe::MaskShape::Rectangle,
            true);

        assert(image.at<cv::Vec3b>(0, 0) == cv::Vec3b(0, 0, 0));
        assert(image.at<cv::Vec3b>(31, 31) == cv::Vec3b(0, 0, 0));
    }

    void testSoftEdgesUsePaddingForAGradualTransition()
    {
        cv::Mat image(96, 96, CV_8UC3, cv::Scalar(100, 100, 100));
        const cv::Rect box(32, 32, 32, 32);

        cloakframe::FaceDetections detections;
        detections.push_back({cv::Rect2f(box), 1.0F});
        cloakframe::applyAnonymization(image,
            detections,
            cloakframe::AnonymizationMethod::Fill,
            4,
            0.25F,
            cloakframe::MaskShape::Rectangle,
            true);

        for (int y = box.y; y < box.y + box.height; ++y)
        {
            for (int x = box.x; x < box.x + box.width; ++x)
            {
                assert(image.at<cv::Vec3b>(y, x) == cv::Vec3b(0, 0, 0));
            }
        }

        const cv::Vec3b innerBlend = image.at<cv::Vec3b>(48, 25);
        assert(innerBlend != cv::Vec3b(0, 0, 0));
        assert(innerBlend != cv::Vec3b(100, 100, 100));

        const cv::Vec3b outerBlend = image.at<cv::Vec3b>(48, 23);
        assert(outerBlend != cv::Vec3b(0, 0, 0));
        assert(outerBlend != cv::Vec3b(100, 100, 100));
        assert(image.at<cv::Vec3b>(48, 16) == cv::Vec3b(100, 100, 100));
    }

    void testLargeSoftEdgeMaskUsesBoundedWorkingMemoryPath()
    {
        cv::Mat image(1500, 1500, CV_8UC3, cv::Scalar(100, 100, 100));
        cloakframe::FaceDetections detections;
        detections.push_back({cv::Rect2f(100.0F, 100.0F, 1300.0F, 1300.0F), 1.0F});

        cloakframe::applyAnonymization(image,
            detections,
            cloakframe::AnonymizationMethod::Fill,
            8,
            0.0F,
            cloakframe::MaskShape::Rectangle,
            true);

        assert(image.at<cv::Vec3b>(750, 750) == cv::Vec3b(0, 0, 0));
        assert(image.at<cv::Vec3b>(0, 0) == cv::Vec3b(100, 100, 100));
        const auto feathered = image.at<cv::Vec3b>(750, 30);
        assert(feathered != cv::Vec3b(0, 0, 0));
        assert(feathered != cv::Vec3b(100, 100, 100));
    }

    void testCustomImageCoversDetectedRegion()
    {
        cv::Mat image(96, 96, CV_8UC3, cv::Scalar(100, 100, 100));
        const cv::Rect box(32, 32, 32, 32);
        const cv::Mat customImage(4, 4, CV_8UC4, cv::Scalar(10, 20, 240, 255));

        cloakframe::FaceDetections detections;
        detections.push_back({cv::Rect2f(box), 1.0F});
        cloakframe::applyAnonymization(image,
            detections,
            cloakframe::AnonymizationMethod::CustomImage,
            4,
            0.0F,
            cloakframe::MaskShape::Ellipse,
            true,
            customImage);

        for (int y = box.y; y < box.y + box.height; ++y)
        {
            for (int x = box.x; x < box.x + box.width; ++x)
            {
                assert(image.at<cv::Vec3b>(y, x) == cv::Vec3b(10, 20, 240));
            }
        }
        assert(image.at<cv::Vec3b>(0, 0) == cv::Vec3b(100, 100, 100));
    }

    void testTransparentCustomImageFallsBackToSafeMosaic()
    {
        cv::Mat image(32, 32, CV_8UC3);
        for (int y = 0; y < image.rows; ++y)
        {
            for (int x = 0; x < image.cols; ++x)
            {
                image.at<cv::Vec3b>(y, x) = cv::Vec3b(static_cast<unsigned char>(x * 7),
                    static_cast<unsigned char>(y * 7),
                    static_cast<unsigned char>((x + y) * 3));
            }
        }
        const cv::Mat original = image.clone();
        cv::Mat expected = original.clone();
        const cv::Mat transparent(2, 2, CV_8UC4, cv::Scalar(250, 240, 230, 0));
        cloakframe::FaceDetections detections = {
            {cv::Rect2f(8.0F, 8.0F, 16.0F, 16.0F), 1.0F},
        };

        cloakframe::applyAnonymization(expected,
            detections,
            cloakframe::AnonymizationMethod::Mosaic,
            4,
            0.0F,
            cloakframe::MaskShape::Rectangle,
            false);
        cloakframe::applyAnonymization(image,
            detections,
            cloakframe::AnonymizationMethod::CustomImage,
            4,
            0.0F,
            cloakframe::MaskShape::Rectangle,
            false,
            transparent);

        assert(cv::norm(image, expected, cv::NORM_INF) == 0.0);
        assert(image.at<cv::Vec3b>(0, 0) == original.at<cv::Vec3b>(0, 0));
    }

    void testSemitransparentCustomImageBlendsWithOriginal()
    {
        cv::Mat image(16, 16, CV_8UC3, cv::Scalar(100, 120, 140));
        const cv::Mat semitransparent(2, 2, CV_8UC4, cv::Scalar(200, 40, 20, 128));
        cloakframe::FaceDetections detections = {
            {cv::Rect2f(4.0F, 4.0F, 8.0F, 8.0F), 1.0F},
        };

        cloakframe::applyAnonymization(image,
            detections,
            cloakframe::AnonymizationMethod::CustomImage,
            4,
            0.0F,
            cloakframe::MaskShape::Rectangle,
            false,
            semitransparent);

        assert(image.at<cv::Vec3b>(8, 8) == cv::Vec3b(150, 80, 80));
        assert(image.at<cv::Vec3b>(0, 0) == cv::Vec3b(100, 120, 140));
    }

    void testCustomImageSupports16BitOutput()
    {
        cv::Mat image(16, 16, CV_16UC3, cv::Scalar(1000, 2000, 3000));
        const cv::Mat customImage(2, 2, CV_8UC4, cv::Scalar(10, 20, 240, 255));
        cloakframe::FaceDetections detections = {
            {cv::Rect2f(4.0F, 4.0F, 8.0F, 8.0F), 1.0F},
        };

        cloakframe::applyAnonymization(image,
            detections,
            cloakframe::AnonymizationMethod::CustomImage,
            4,
            0.0F,
            cloakframe::MaskShape::Rectangle,
            false,
            customImage);

        assert(image.at<cv::Vec3w>(8, 8) == cv::Vec3w(2570, 5140, 61680));
        assert(image.at<cv::Vec3w>(0, 0) == cv::Vec3w(1000, 2000, 3000));
    }

    void testCustomImagePreservesAspectRatio()
    {
        cv::Mat image(40, 40, CV_8UC3, cv::Scalar(100, 100, 100));
        cv::Mat wideImage(20, 40, CV_8UC4, cv::Scalar(10, 20, 240, 255));
        wideImage(cv::Rect(10, 0, 20, 20)).setTo(cv::Scalar(10, 200, 20, 255));
        const cloakframe::FaceDetections detections = {
            {cv::Rect2f(8.0F, 8.0F, 24.0F, 24.0F), 1.0F},
        };

        cloakframe::applyAnonymization(image,
            detections,
            cloakframe::AnonymizationMethod::CustomImage,
            4,
            0.0F,
            cloakframe::MaskShape::Rectangle,
            false,
            wideImage);

        assert(image.at<cv::Vec3b>(20, 20) == cv::Vec3b(10, 200, 20));
        assert(image.at<cv::Vec3b>(9, 12) == cv::Vec3b(10, 200, 20));
        assert(image.at<cv::Vec3b>(30, 28) == cv::Vec3b(10, 200, 20));
        assert(image.at<cv::Vec3b>(0, 0) == cv::Vec3b(100, 100, 100));
    }

    void testCustomImageFollowsFaceRollWithoutDarkAlphaFringes()
    {
        const cv::Vec3b background(60, 70, 80);
        cv::Mat upright(64, 64, CV_8UC3, background);
        cv::Mat tilted = upright.clone();
        cv::Mat customImage(8, 16, CV_8UC4, cv::Scalar(0, 0, 0, 0));
        customImage(cv::Rect(2, 1, 12, 6)).setTo(cv::Scalar(10, 40, 240, 255));

        cloakframe::FaceDetection uprightFace{cv::Rect2f(16.0F, 16.0F, 32.0F, 32.0F), 1.0F};
        cloakframe::FaceDetection tiltedFace = uprightFace;
        tiltedFace.rollRadians = 0.45F;
        tiltedFace.hasPose = true;

        cloakframe::applyAnonymization(upright,
            {uprightFace},
            cloakframe::AnonymizationMethod::CustomImage,
            4,
            0.0F,
            cloakframe::MaskShape::Rectangle,
            false,
            customImage);
        cloakframe::applyAnonymization(tilted,
            {tiltedFace},
            cloakframe::AnonymizationMethod::CustomImage,
            4,
            0.0F,
            cloakframe::MaskShape::Rectangle,
            false,
            customImage);

        assert(cv::norm(upright, tilted, cv::NORM_L1) > 1000.0);
        bool foundBlendedEdge = false;
        for (int y = 16; y < 48; ++y)
        {
            for (int x = 16; x < 48; ++x)
            {
                const auto pixel = tilted.at<cv::Vec3b>(y, x);
                if (pixel != background)
                {
                    foundBlendedEdge = true;
                    assert(pixel[2] >= background[2]);
                }
            }
        }
        assert(foundBlendedEdge);
        assert(tilted.at<cv::Vec3b>(0, 0) == background);
    }

    void testOpaqueRotatedCustomImageStillCoversDetectedRegion()
    {
        const cv::Vec3b background(60, 70, 80);
        cv::Mat image(64, 64, CV_8UC3, background);
        const cv::Mat opaque(8, 12, CV_8UC4, cv::Scalar(10, 40, 240, 255));
        cloakframe::FaceDetection face{cv::Rect2f(16.0F, 16.0F, 32.0F, 32.0F), 1.0F};
        face.rollRadians = -0.6F;
        face.hasPose = true;

        cloakframe::applyAnonymization(image,
            {face},
            cloakframe::AnonymizationMethod::CustomImage,
            4,
            0.0F,
            cloakframe::MaskShape::Rectangle,
            false,
            opaque);

        for (int y = 16; y < 48; ++y)
        {
            for (int x = 16; x < 48; ++x)
            {
                assert(image.at<cv::Vec3b>(y, x) == cv::Vec3b(10, 40, 240));
            }
        }
        assert(image.at<cv::Vec3b>(0, 0) == background);
    }

    void testOrientationTransforms()
    {
        cv::Mat base(2, 3, CV_8UC1);
        for (int r = 0; r < base.rows; ++r)
        {
            for (int c = 0; c < base.cols; ++c)
            {
                base.at<uchar>(r, c) = static_cast<uchar>(r * 10 + c);
            }
        }

        cv::Mat identity = base.clone();
        cloakframe::applyOrientation(identity, 1);
        assert(cv::countNonZero(identity != base) == 0);

        cv::Mat rotated = base.clone();
        cloakframe::applyOrientation(rotated, 6);
        assert(rotated.rows == 3 && rotated.cols == 2);
        assert(rotated.at<uchar>(0, 0) == base.at<uchar>(base.rows - 1, 0));

        cv::Mat mirrored = base.clone();
        cloakframe::applyOrientation(mirrored, 2);
        assert(mirrored.rows == 2 && mirrored.cols == 3);
        assert(mirrored.at<uchar>(0, 0) == base.at<uchar>(0, base.cols - 1));
    }

    void testExifOrientationFallback()
    {
        QTemporaryDir temp;
        assert(temp.isValid());
        const auto root = std::filesystem::path(temp.path().toStdString());
        for (unsigned char orientation = 1; orientation <= 8; ++orientation)
        {
            const auto path = root / ("orientation-" + std::to_string(orientation) + ".jpg");
            writeJpegWithExifOrientation(path, orientation);
            assert(cloakframe::readExifOrientation(path) == orientation);
        }

        const auto path = root / "orientation-6.jpg";
        cv::Mat image = cloakframe::imreadUnicode(path, cv::IMREAD_UNCHANGED);
        assert(image.rows == 2 && image.cols == 3);
        cloakframe::applyOrientation(image, cloakframe::readExifOrientation(path));
        assert(image.rows == 3 && image.cols == 2);
    }

    void testEncodeParams()
    {
        const auto jpeg = cloakframe::encodeParamsForExtension(".JPG");
        assert(std::find(jpeg.begin(), jpeg.end(), cv::IMWRITE_JPEG_QUALITY) != jpeg.end());
        assert(std::find(jpeg.begin(), jpeg.end(), 100) != jpeg.end());

        const auto png = cloakframe::encodeParamsForExtension("png");
        assert(std::find(png.begin(), png.end(), cv::IMWRITE_PNG_COMPRESSION) != png.end());

        assert(cloakframe::encodeParamsForExtension(".bmp").empty());
    }

    void testImageWritePublishesWithoutReplacing()
    {
        QTemporaryDir temp;
        assert(temp.isValid());
        const auto root = std::filesystem::path(temp.path().toStdString());
        const auto destination = root / "result.png";

        writeBytes(QString::fromStdString(destination.string()));
        const cv::Mat first(128, 128, CV_8UC3, cv::Scalar(20, 40, 60));
        assert(!cloakframe::imwriteUnicodeNoReplace(destination, first));
        assert(std::filesystem::file_size(destination) == 1);
        assert(std::filesystem::remove(destination));

        std::barrier start(3);
        std::atomic<int> successes{0};
        const cv::Mat second(512, 512, CV_8UC3, cv::Scalar(80, 100, 120));
        const auto write = [&](const cv::Mat &image)
        {
            start.arrive_and_wait();
            if (cloakframe::imwriteUnicodeNoReplace(destination, image))
            {
                successes.fetch_add(1, std::memory_order_relaxed);
            }
        };
        std::thread firstWriter(write, std::cref(first));
        std::thread secondWriter(write, std::cref(second));
        start.arrive_and_wait();
        firstWriter.join();
        secondWriter.join();

        assert(successes.load(std::memory_order_relaxed) == 1);
        assert(!cv::imread(destination.string(), cv::IMREAD_UNCHANGED).empty());
        for (const auto &entry : std::filesystem::directory_iterator(root))
        {
            assert(entry.path().filename().string().find(".cloakframe-") == std::string::npos);
        }
    }

    void testRootedWritesRejectEscapesAndUsePrivateFiles()
    {
        QTemporaryDir temp;
        assert(temp.isValid());
        const auto base =
            std::filesystem::canonical(std::filesystem::path(temp.path().toStdString()));
        const auto root = base / "output";
        const auto outside = base / "outside";
        assert(std::filesystem::create_directories(root));
        assert(std::filesystem::create_directories(outside));

        const cv::Mat image(32, 32, CV_8UC3, cv::Scalar(30, 60, 90));
        assert(cloakframe::imwriteUnicodeNoReplaceAtRoot(root, "nested/result.png", image)
               == cloakframe::ImageWriteResult::Saved);
        assert(!cv::imread((root / "nested/result.png").string()).empty());
        assert(cloakframe::imwriteUnicodeNoReplaceAtRoot(root, "../outside/escaped.png", image)
               == cloakframe::ImageWriteResult::Failed);
        assert(!std::filesystem::exists(outside / "escaped.png"));

        const auto source = base / "original.bin";
        const std::vector<uchar> sourceBytes = {0, 1, 2, 3, 4, 5, 0xFE, 0xFF};
        writeBytes(source, sourceBytes);
#ifndef _WIN32
        assert(::chmod(source.c_str(), 0600) == 0);
#endif
        assert(cloakframe::copyFileNoReplaceAtRoot(source, root, "copies/original.bin"));
        std::ifstream copied(root / "copies/original.bin", std::ios::binary);
        const std::istreambuf_iterator<char> copiedBegin(copied);
        const std::istreambuf_iterator<char> copiedEnd;
        const std::vector<uchar> copiedBytes(copiedBegin, copiedEnd);
        assert(copiedBytes == sourceBytes);

        const auto moveSource = base / "move-source.bin";
        writeBytes(moveSource, sourceBytes);
        assert(cloakframe::moveFileNoReplaceAtRoot(moveSource, root, "moves/moved.bin")
               == cloakframe::FileMoveResult::Moved);
        assert(!std::filesystem::exists(moveSource));
        std::ifstream moved(root / "moves/moved.bin", std::ios::binary);
        const std::istreambuf_iterator<char> movedBegin(moved);
        const std::istreambuf_iterator<char> movedEnd;
        const std::vector<uchar> movedBytes(movedBegin, movedEnd);
        assert(movedBytes == sourceBytes);

        const auto blockedSource = base / "blocked-source.bin";
        const auto blockedDestination = root / "moves/existing.bin";
        const std::vector<uchar> existingBytes = {9, 8, 7, 6};
        writeBytes(blockedSource, sourceBytes);
        writeBytes(blockedDestination, existingBytes);
#ifndef _WIN32
        assert(::chmod(blockedSource.c_str(), 0640) == 0);
#endif
        assert(cloakframe::moveFileNoReplaceAtRoot(blockedSource, root, "moves/existing.bin")
               == cloakframe::FileMoveResult::Failed);
        assert(std::filesystem::exists(blockedSource));
        std::ifstream existing(blockedDestination, std::ios::binary);
        const std::istreambuf_iterator<char> existingBegin(existing);
        const std::istreambuf_iterator<char> existingEnd;
        const std::vector<uchar> preservedBytes(existingBegin, existingEnd);
        assert(preservedBytes == existingBytes);
#ifndef _WIN32
        struct stat blockedStatus{};
        assert(::stat(blockedSource.c_str(), &blockedStatus) == 0);
        assert((blockedStatus.st_mode & 0777) == 0640);
#endif

        const auto guardedSource = base / "guarded-source.bin";
        writeBytes(guardedSource, sourceBytes);
        assert(cloakframe::moveFileNoReplaceAtRoot(guardedSource,
                   root,
                   "moves/guarded.bin",
                   []
                   {
                       return false;
                   })
               == cloakframe::FileMoveResult::Failed);
        assert(std::filesystem::exists(guardedSource));
        assert(!std::filesystem::exists(root / "moves/guarded.bin"));
#ifndef _WIN32
        struct stat copiedStatus{};
        assert(::stat((root / "copies/original.bin").c_str(), &copiedStatus) == 0);
        assert((copiedStatus.st_mode & 0777) == 0600);

        std::error_code ec;
        std::filesystem::create_directory_symlink(outside, root / "linked", ec);
        assert(!ec);
        assert(cloakframe::imwriteUnicodeNoReplaceAtRoot(root, "linked/escaped.png", image)
               == cloakframe::ImageWriteResult::Failed);
        assert(!std::filesystem::exists(outside / "escaped.png"));
#endif

        for (const auto &entry : std::filesystem::recursive_directory_iterator(root))
        {
            assert(!entry.path().filename().string().starts_with(".cloakframe-"));
        }
    }

    void testMetadataFailurePublishesCleanImage()
    {
        QTemporaryDir temp;
        assert(temp.isValid());
        const auto root =
            std::filesystem::canonical(std::filesystem::path(temp.path().toStdString()));
        const cv::Mat image(24, 24, CV_8UC3, cv::Scalar(15, 45, 75));
        const auto result = cloakframe::imwriteUnicodeNoReplaceAtRoot(root,
            "result.jpg",
            image,
            cloakframe::encodeParamsForExtension("jpg"),
            root / "missing-metadata-source.jpg");
        assert(result == cloakframe::ImageWriteResult::SavedWithoutMetadata);
        assert(!cv::imread((root / "result.jpg").string()).empty());
    }

    void testIntersectionOverUnion()
    {
        const cv::Rect2f a(0.0F, 0.0F, 10.0F, 10.0F);
        assert(std::abs(cloakframe::intersectionOverUnion(a, a) - 1.0F) < 1e-5F);

        const cv::Rect2f disjoint(100.0F, 100.0F, 10.0F, 10.0F);
        assert(cloakframe::intersectionOverUnion(a, disjoint) == 0.0F);

        const cv::Rect2f empty(0.0F, 0.0F, 0.0F, 0.0F);
        assert(cloakframe::intersectionOverUnion(a, empty) == 0.0F);

        const cv::Rect2f halfShifted(5.0F, 0.0F, 10.0F, 10.0F);
        assert(
            std::abs(cloakframe::intersectionOverUnion(a, halfShifted) - (50.0F / 150.0F)) < 1e-5F);
    }

    void testNonMaxSuppression()
    {
        cloakframe::FaceDetections detections;
        detections.push_back({cv::Rect2f(0.0F, 0.0F, 10.0F, 10.0F), 0.9F});
        detections.push_back({cv::Rect2f(1.0F, 1.0F, 10.0F, 10.0F), 0.8F});
        detections.push_back({cv::Rect2f(100.0F, 100.0F, 10.0F, 10.0F), 0.7F});

        const auto kept = cloakframe::nonMaxSuppression(detections, 0.4F);
        assert(kept.size() == 2);
        assert(kept[0].score == 0.9F);
        assert(kept[1].box.x == 100.0F);
    }

    void testInvalidDetectionsAreIgnored()
    {
        const float nan = std::numeric_limits<float>::quiet_NaN();
        cloakframe::FaceDetections detections = {
            {cv::Rect2f(nan, 0.0F, 10.0F, 10.0F), 0.9F},
            {cv::Rect2f(0.0F, 0.0F, 10.0F, 10.0F), nan},
            {cv::Rect2f(2.0F, 2.0F, 8.0F, 8.0F), 0.8F},
        };
        const auto kept = cloakframe::nonMaxSuppression(detections, 0.4F);
        assert(kept.size() == 1);

        cv::Mat image(16, 16, CV_8UC3, cv::Scalar(30, 60, 90));
        const cv::Mat before = image.clone();
        cloakframe::applyAnonymization(image,
            {{cv::Rect2f(0.0F, nan, 10.0F, 10.0F), 0.9F}},
            cloakframe::AnonymizationMethod::Fill,
            8,
            0.0F);
        assert(cv::norm(image, before, cv::NORM_INF) == 0.0);
    }

    void testDestinationPathSafety()
    {
        QTemporaryDir temp;
        assert(temp.isValid());
        const std::filesystem::path root = std::filesystem::path(temp.path().toStdString()) / "out";
        assert(std::filesystem::create_directories(root));

        assert(cloakframe::destinationIsSafe(root / "a.jpg", root));
        assert(cloakframe::destinationIsSafe(root / "sub" / "b.png", root));
        assert(cloakframe::destinationIsSafe(root, root));

        assert(!cloakframe::destinationIsSafe(root / ".." / "escape.jpg", root));

        assert(cloakframe::isWithinRoot(root / "x.jpg", root));
        assert(!cloakframe::isWithinRoot(root.parent_path() / "x.jpg", root));
    }

#ifndef _WIN32
    void testDestinationRejectsSymlinkEscape()
    {
        QTemporaryDir temp;
        assert(temp.isValid());
        const std::filesystem::path base = std::filesystem::path(temp.path().toStdString());
        const std::filesystem::path root = base / "out";
        const std::filesystem::path outside = base / "outside";
        assert(std::filesystem::create_directories(root));
        assert(std::filesystem::create_directories(outside));

        std::error_code ec;
        const std::filesystem::path link = root / "evil";
        std::filesystem::create_directory_symlink(outside, link, ec);
        assert(!ec);

        assert(!cloakframe::destinationIsSafe(link / "leak.jpg", root));

        const std::filesystem::path danglingTarget = outside / "not-created.jpg";
        const std::filesystem::path danglingLink = root / "dangling.jpg";
        std::filesystem::create_symlink(danglingTarget, danglingLink, ec);
        assert(!ec);
        assert(!std::filesystem::exists(danglingLink));
        assert(std::filesystem::is_symlink(std::filesystem::symlink_status(danglingLink)));
        assert(!cloakframe::destinationIsSafe(danglingLink, root));

        const std::vector<cloakframe::ScanResult> planned = {
            {base / "input" / "dangling.jpg", "dangling.jpg"},
        };
        const auto conflicts = cloakframe::findOutputConflicts(planned, root);
        assert(conflicts.size() == 1);
        assert(conflicts.front().kind == cloakframe::OutputConflict::Kind::ExistingDestination);

        const auto source = base / "source.jpg";
        assert(cv::imwrite(source.string(), cv::Mat(8, 8, CV_8UC3, cv::Scalar(1, 2, 3))));
        assert(!cloakframe::copyFileNoReplace(source, danglingLink));
        assert(!cloakframe::imwriteUnicodeNoReplace(
            danglingLink, cv::Mat(8, 8, CV_8UC3, cv::Scalar(4, 5, 6))));
        assert(!std::filesystem::exists(danglingTarget));
    }
#endif

#ifdef CLOAKFRAME_HAVE_EXIV2
    void testMetadataCopyAndOrientationNormalize()
    {
        QTemporaryDir temp;
        assert(temp.isValid());
        const auto root =
            std::filesystem::canonical(std::filesystem::path(temp.path().toStdString()));
        const std::filesystem::path src = root / "src.jpg";
        const std::filesystem::path dst = root / "dst.jpg";

        cv::Mat img(16, 16, CV_8UC3, cv::Scalar(120, 120, 120));
        assert(cv::imwrite(src.string(), img));
        assert(cv::imwrite(dst.string(), img));

        {
            auto image = Exiv2::ImageFactory::open(src.string());
            image->readMetadata();
            image->exifData()["Exif.Image.Artist"] = "TestPhotographer";
            image->exifData()["Exif.Image.Orientation"] = static_cast<uint16_t>(6);
            image->exifData()["Exif.Photo.UserComment"] = "data:image/jpeg;base64,unsafe-payload";
            image->xmpData()["Xmp.tiff.Orientation"] = "6";
            image->xmpData()["Xmp.dc.description"] = "data:image/jpeg;base64,unsafe-payload";
            image->setComment("data:image/jpeg;base64,unsafe-payload");
            image->writeMetadata();
        }

        assert(cloakframe::readExifOrientation(src) == 6);
        assert(cloakframe::copyMetadata(src, dst, true));

        {
            auto image = Exiv2::ImageFactory::open(dst.string());
            image->readMetadata();
            const Exiv2::ExifData &exif = image->exifData();

            const auto artist = exif.findKey(Exiv2::ExifKey("Exif.Image.Artist"));
            assert(artist != exif.end());
            assert(artist->toString() == "TestPhotographer");

            const auto orientation = exif.findKey(Exiv2::ExifKey("Exif.Image.Orientation"));
            assert(orientation != exif.end());
#if EXIV2_TEST_VERSION(0, 28, 0)
            assert(orientation->toInt64() == 1);
#else
            assert(orientation->toLong() == 1);
#endif
            assert(image->xmpData().findKey(Exiv2::XmpKey("Xmp.tiff.Orientation"))
                   == image->xmpData().end());
            assert(image->xmpData().empty());
            assert(image->exifData().findKey(Exiv2::ExifKey("Exif.Photo.UserComment"))
                   == image->exifData().end());
            assert(image->comment().empty());
        }

        const auto published = root / "published.jpg";
        assert(
            cloakframe::imwriteUnicodeNoReplaceAtRoot(
                root, published.filename(), img, cloakframe::encodeParamsForExtension("jpg"), src)
            == cloakframe::ImageWriteResult::Saved);
        auto publishedMetadata = Exiv2::ImageFactory::open(published.string());
        publishedMetadata->readMetadata();
        const auto artist =
            publishedMetadata->exifData().findKey(Exiv2::ExifKey("Exif.Image.Artist"));
        assert(artist != publishedMetadata->exifData().end());
        assert(artist->toString() == "TestPhotographer");
        assert(publishedMetadata->xmpData().findKey(Exiv2::XmpKey("Xmp.tiff.Orientation"))
               == publishedMetadata->xmpData().end());
    }

    long exifThumbnailBytes(Exiv2::ExifData &exif)
    {
        Exiv2::ExifThumb thumb(exif);
#if EXIV2_TEST_VERSION(0, 28, 0)
        return static_cast<long>(thumb.copy().size());
#else
        return thumb.copy().size_;
#endif
    }

    void testMetadataCopyStripsEmbeddedThumbnail()
    {
        QTemporaryDir temp;
        assert(temp.isValid());
        const std::filesystem::path base = std::filesystem::path(temp.path().toStdString());
        const std::filesystem::path src = base / "src.jpg";
        const std::filesystem::path dst = base / "dst.jpg";
        const std::filesystem::path thumbFile = base / "thumb.jpg";

        cv::Mat img(16, 16, CV_8UC3, cv::Scalar(120, 120, 120));
        assert(cv::imwrite(src.string(), img));
        assert(cv::imwrite(dst.string(), img));
        assert(cv::imwrite(thumbFile.string(), img));

        {
            auto image = Exiv2::ImageFactory::open(src.string());
            image->readMetadata();
            image->exifData()["Exif.Image.Artist"] = "TestPhotographer";
            Exiv2::ExifThumb thumb(image->exifData());
            thumb.setJpegThumbnail(thumbFile.string());
            image->writeMetadata();
        }

        {
            auto image = Exiv2::ImageFactory::open(src.string());
            image->readMetadata();
            assert(exifThumbnailBytes(image->exifData()) > 0);
        }

        assert(cloakframe::copyMetadata(src, dst, true));

        {
            auto image = Exiv2::ImageFactory::open(dst.string());
            image->readMetadata();
            assert(exifThumbnailBytes(image->exifData()) == 0);

            const Exiv2::ExifData &exif = image->exifData();
            const auto artist = exif.findKey(Exiv2::ExifKey("Exif.Image.Artist"));
            assert(artist != exif.end());
            assert(artist->toString() == "TestPhotographer");
        }
    }
#endif
}

namespace
{
    void testDetectorsRejectUnexpectedModelHash()
    {
        QTemporaryDir temp;
        assert(temp.isValid());
        const QString modelPath = temp.filePath("model.onnx");
        QFile model(modelPath);
        assert(model.open(QIODevice::WriteOnly));
        assert(model.write("invalid-model") == 13);
        model.close();

        const QByteArray unexpectedHash(32, '\0');
        bool faceRejected = false;
        try
        {
            cloakframe::ScrfdFaceDetector detector(
                modelPath.toStdString(), 640, false, unexpectedHash);
        }
        catch (const std::runtime_error &error)
        {
            faceRejected = std::string(error.what()).find("changed") != std::string::npos;
        }
        assert(faceRejected);

        bool yoloRejected = false;
        try
        {
            cloakframe::Yolo5FaceDetector detector(modelPath.toStdString(), false, unexpectedHash);
        }
        catch (const std::runtime_error &error)
        {
            yoloRejected = std::string(error.what()).find("changed") != std::string::npos;
        }
        assert(yoloRejected);

        bool yuNetRejected = false;
        try
        {
            cloakframe::YuNetFaceDetector detector(modelPath.toStdString(), unexpectedHash);
        }
        catch (const std::runtime_error &error)
        {
            yuNetRejected = std::string(error.what()).find("changed") != std::string::npos;
        }
        assert(yuNetRejected);

        bool plateRejected = false;
        try
        {
            cloakframe::PlateDetector detector(modelPath.toStdString(), false, unexpectedHash);
        }
        catch (const std::runtime_error &error)
        {
            plateRejected = std::string(error.what()).find("changed") != std::string::npos;
        }
        assert(plateRejected);
    }

    void testOnnxPatchRejectsInvalidBytes()
    {
        assert(!cloakframe::makeOnnxSpatialDimsFixed({}, 320).has_value());
        const std::vector<std::uint8_t> garbage = {0xFF, 0xFF, 0xFF, 0x01, 0x02, 0x9C};
        assert(!cloakframe::makeOnnxSpatialDimsFixed(garbage, 320).has_value());
        assert(!cloakframe::makeOnnxSpatialDimsFixed(garbage, 0).has_value());
        assert(!cloakframe::makeOnnxSpatialDimsFixed(garbage, -320).has_value());
    }

    // Enough of a protobuf writer to hand-build the ONNX models the patch tests need. Field
    // numbers below are the ones from onnx.proto.
    void appendProtobufVarint(std::vector<std::uint8_t> &out, std::uint64_t value)
    {
        while (value >= 0x80U)
        {
            out.push_back(static_cast<std::uint8_t>((value & 0x7FU) | 0x80U));
            value >>= 7;
        }
        out.push_back(static_cast<std::uint8_t>(value));
    }

    void appendVarintField(
        std::vector<std::uint8_t> &out, std::uint32_t number, std::uint64_t value)
    {
        appendProtobufVarint(out, static_cast<std::uint64_t>(number) << 3);
        appendProtobufVarint(out, value);
    }

    void appendBytesField(std::vector<std::uint8_t> &out,
        std::uint32_t number,
        const std::vector<std::uint8_t> &bytes)
    {
        appendProtobufVarint(out, (static_cast<std::uint64_t>(number) << 3) | 2U);
        appendProtobufVarint(out, bytes.size());
        out.insert(out.end(), bytes.begin(), bytes.end());
    }

    void appendStringField(
        std::vector<std::uint8_t> &out, std::uint32_t number, std::string_view text)
    {
        appendBytesField(out, number, {text.begin(), text.end()});
    }

    std::vector<std::uint8_t> onnxFixedDim(std::int64_t value)
    {
        std::vector<std::uint8_t> dim;
        appendVarintField(dim, 1, static_cast<std::uint64_t>(value));
        return dim;
    }

    std::vector<std::uint8_t> onnxNamedDim(std::string_view name)
    {
        std::vector<std::uint8_t> dim;
        appendStringField(dim, 2, name);
        return dim;
    }

    std::vector<std::uint8_t> onnxFloatTensorValueInfo(
        std::string_view name, const std::vector<std::vector<std::uint8_t>> &dims)
    {
        std::vector<std::uint8_t> shape;
        for (const auto &dim : dims)
        {
            appendBytesField(shape, 1, dim);
        }
        std::vector<std::uint8_t> tensorType;
        appendVarintField(tensorType, 1, 1);
        appendBytesField(tensorType, 2, shape);
        std::vector<std::uint8_t> type;
        appendBytesField(type, 1, tensorType);
        std::vector<std::uint8_t> valueInfo;
        appendStringField(valueInfo, 1, name);
        appendBytesField(valueInfo, 2, type);
        return valueInfo;
    }

    std::vector<std::uint8_t> onnxInt64Initializer(
        std::string_view name, const std::vector<std::int64_t> &values)
    {
        std::vector<std::uint8_t> tensor;
        appendVarintField(tensor, 1, values.size());
        appendVarintField(tensor, 2, 7);
        appendStringField(tensor, 8, name);
        std::vector<std::uint8_t> raw(values.size() * sizeof(std::int64_t));
        std::memcpy(raw.data(), values.data(), raw.size());
        appendBytesField(tensor, 9, raw);
        return tensor;
    }

    std::vector<std::uint8_t> onnxStringAttribute(std::string_view name, std::string_view value)
    {
        std::vector<std::uint8_t> attribute;
        appendStringField(attribute, 1, name);
        appendStringField(attribute, 4, value);
        appendVarintField(attribute, 20, 3);
        return attribute;
    }

    std::vector<std::uint8_t> onnxNode(std::string_view opType,
        const std::vector<std::string_view> &inputs,
        std::string_view output,
        const std::vector<std::vector<std::uint8_t>> &attributes = {})
    {
        std::vector<std::uint8_t> node;
        for (const auto &input : inputs)
        {
            appendStringField(node, 1, input);
        }
        appendStringField(node, 2, output);
        appendStringField(node, 4, opType);
        for (const auto &attribute : attributes)
        {
            appendBytesField(node, 5, attribute);
        }
        return node;
    }

    std::vector<std::uint8_t> onnxModel(const std::vector<std::uint8_t> &graph)
    {
        std::vector<std::uint8_t> opset;
        appendVarintField(opset, 2, 13);

        std::vector<std::uint8_t> model;
        appendVarintField(model, 1, 8);
        appendStringField(model, 2, "cloakframe-test");
        appendBytesField(model, 7, graph);
        appendBytesField(model, 8, opset);
        return model;
    }

    // A graph that is one Resize, exported at `exportedSize` and told to produce exactly
    // `resizeSizes`. Real detector graphs bake the same kind of constant into their upsamples.
    std::vector<std::uint8_t> makeResizeModel(std::int64_t exportedSize,
        const std::vector<std::int64_t> &resizeSizes,
        bool dynamicInput = false,
        bool shareTheConstant = false)
    {
        std::vector<std::vector<std::uint8_t>> inputDims = {onnxFixedDim(1), onnxFixedDim(3)};
        for (int i = 0; i < 2; ++i)
        {
            inputDims.push_back(
                dynamicInput ? onnxNamedDim("dynamic") : onnxFixedDim(exportedSize));
        }

        std::vector<std::uint8_t> graph;
        appendBytesField(graph,
            1,
            onnxNode("Resize",
                {"images", "", "", "resize_sizes"},
                "resized",
                {onnxStringAttribute("mode", "nearest"),
                    onnxStringAttribute("coordinate_transformation_mode", "asymmetric")}));
        if (shareTheConstant)
        {
            appendBytesField(graph, 1, onnxNode("Reshape", {"resized", "resize_sizes"}, "y"));
        }
        appendStringField(graph, 2, "cloakframe_test_graph");
        appendBytesField(graph, 5, onnxInt64Initializer("resize_sizes", resizeSizes));
        appendBytesField(graph, 11, onnxFloatTensorValueInfo("images", inputDims));
        appendBytesField(graph,
            12,
            onnxFloatTensorValueInfo(shareTheConstant ? "y" : "resized",
                {onnxFixedDim(resizeSizes[0]),
                    onnxFixedDim(resizeSizes[1]),
                    onnxFixedDim(resizeSizes[2]),
                    onnxFixedDim(resizeSizes[3])}));
        return onnxModel(graph);
    }

    std::vector<std::int64_t> runPatchedResize(
        const std::vector<std::uint8_t> &model, int inputSize)
    {
        Ort::Env env(ORT_LOGGING_LEVEL_ERROR, "cloakframe-test");
        Ort::SessionOptions options;
        Ort::Session session(env, model.data(), model.size(), options);

        std::vector<float> pixels(static_cast<std::size_t>(3) * inputSize * inputSize, 0.0F);
        const std::array<std::int64_t, 4> inputShape = {1, 3, inputSize, inputSize};
        auto memoryInfo = Ort::MemoryInfo::CreateCpu(OrtArenaAllocator, OrtMemTypeDefault);
        auto input = Ort::Value::CreateTensor<float>(
            memoryInfo, pixels.data(), pixels.size(), inputShape.data(), inputShape.size());

        const std::array<const char *, 1> inputNames = {"images"};
        const std::array<const char *, 1> outputNames = {"resized"};
        auto outputs = session.Run(Ort::RunOptions{nullptr},
            inputNames.data(),
            &input,
            1,
            outputNames.data(),
            outputNames.size());
        return outputs.front().GetTensorTypeAndShapeInfo().GetShape();
    }

    void testOnnxPatchScalesResizeTargetsByTheirExportedRatio()
    {
        // A quarter-resolution Resize, not the doubling an upsample usually is: assuming 2x
        // here would ask the graph for 640x640 out of a 320x320 input.
        const auto quarter = makeResizeModel(640, {1, 3, 160, 160});
        const auto patchedQuarter = cloakframe::makeOnnxSpatialDimsFixed(quarter, 320);
        assert(patchedQuarter.has_value());
        const auto quarterShape = runPatchedResize(*patchedQuarter, 320);
        assert(quarterShape.size() == 4);
        assert(quarterShape[2] == 80 && quarterShape[3] == 80);

        // The doubling case the built-in models actually use still lands where it did.
        const auto doubled = makeResizeModel(640, {1, 3, 40, 40});
        const auto patchedDoubled = cloakframe::makeOnnxSpatialDimsFixed(doubled, 320);
        assert(patchedDoubled.has_value());
        const auto doubledShape = runPatchedResize(*patchedDoubled, 320);
        assert(doubledShape.size() == 4);
        assert(doubledShape[2] == 20 && doubledShape[3] == 20);

        // Patching to the size it was exported at leaves the target where the model put it.
        const auto unchanged = cloakframe::makeOnnxSpatialDimsFixed(quarter, 640);
        assert(unchanged.has_value());
        const auto unchangedShape = runPatchedResize(*unchanged, 640);
        assert(unchangedShape[2] == 160 && unchangedShape[3] == 160);
    }

    void testOnnxPatchRefusesResizeTargetsItCannotDerive()
    {
        // Nothing to measure the constant against.
        const auto dynamicInput = makeResizeModel(640, {1, 3, 160, 160}, true);
        assert(!cloakframe::makeOnnxSpatialDimsFixed(dynamicInput, 320).has_value());

        // The constant feeds a Reshape as well, so rewriting it would change that too.
        const auto shared = makeResizeModel(640, {1, 3, 160, 160}, false, true);
        assert(!cloakframe::makeOnnxSpatialDimsFixed(shared, 320).has_value());

        // 3/640 of the input is not a whole number of pixels at 320.
        const auto uneven = makeResizeModel(640, {1, 3, 3, 3});
        assert(!cloakframe::makeOnnxSpatialDimsFixed(uneven, 320).has_value());
    }

    void testFixedScrfdModelRunsAtRequestedSize()
    {
        const auto modelPath = std::filesystem::path(__FILE__).parent_path().parent_path()
                               / "models" / "2.5g_bnkps.onnx";
        if (!std::filesystem::exists(modelPath))
        {
            std::puts("skipping fixed-model patch test: models/2.5g_bnkps.onnx not present");
            return;
        }

        cloakframe::ScrfdFaceDetector nativeSize(modelPath.string(), 640);
        assert(nativeSize.inputSize() == 640);

        cloakframe::ScrfdFaceDetector patchedSize(modelPath.string(), 320);
        assert(patchedSize.inputSize() == 320);

        const cv::Mat blank(180, 320, CV_8UC3, cv::Scalar(30, 30, 30));
        assert(patchedSize.detect(blank, 0.5F, 0.4F).detections.empty());
    }

    void testDynamicScrfdModelRunsAtRequestedSize()
    {
        const auto modelPath = std::filesystem::path(__FILE__).parent_path().parent_path()
                               / "models" / "10g_bnkps.onnx";
        if (!std::filesystem::exists(modelPath))
        {
            std::puts("skipping dynamic-model patch test: models/10g_bnkps.onnx not present");
            return;
        }

        cloakframe::ScrfdFaceDetector patchedSize(modelPath.string(), 320);
        assert(patchedSize.inputSize() == 320);

        const cv::Mat blank(180, 320, CV_8UC3, cv::Scalar(30, 30, 30));
        assert(patchedSize.detect(blank, 0.5F, 0.4F).detections.empty());
    }

    void testRecommendedFaceModels()
    {
        const QString yoloPath = qEnvironmentVariable("CLOAKFRAME_TEST_YOLO5FACE_MODEL");
        const QString yuNetPath = qEnvironmentVariable("CLOAKFRAME_TEST_YUNET_MODEL");
        const QString faceImagePath = qEnvironmentVariable("CLOAKFRAME_TEST_FACE_IMAGE");
        const cv::Mat blank(360, 640, CV_8UC3, cv::Scalar(30, 30, 30));
        const cv::Mat faceImage =
            faceImagePath.isEmpty() ? cv::Mat{} : cv::imread(faceImagePath.toStdString());

        if (yoloPath.isEmpty())
        {
            std::puts("skipping YOLO5Face-n model test: environment path not set");
        }
        else
        {
            const auto &model = cloakframe::builtinModels()[0];
            cloakframe::Yolo5FaceDetector detector(
                yoloPath.toStdString(), false, QByteArray::fromHex(model.sha256.toLatin1()));
            assert(detector.inputSize() == 640);
            assert(detector.detect(blank, 0.99F, 0.4F).detections.empty());
            if (!faceImage.empty())
            {
                const auto result = detector.detect(faceImage, 0.25F, 0.4F);
                assert(!result.detections.empty());
                assert(result.omitted == 0);
                assert(std::ranges::any_of(result.detections, &cloakframe::FaceDetection::hasPose));
            }
        }

        if (yuNetPath.isEmpty())
        {
            std::puts("skipping YuNet model test: environment path not set");
        }
        else
        {
            const auto &model = cloakframe::builtinModels()[1];
            cloakframe::YuNetFaceDetector detector(
                yuNetPath.toStdString(), QByteArray::fromHex(model.sha256.toLatin1()));
            assert(detector.inputSize() == 640);
            assert(detector.detect(blank, 0.99F, 0.4F).detections.empty());
            if (!faceImage.empty())
            {
                const auto result = detector.detect(faceImage, 0.25F, 0.4F);
                assert(!result.detections.empty());
                assert(result.omitted == 0);
                assert(std::ranges::any_of(result.detections, &cloakframe::FaceDetection::hasPose));
            }
        }
    }

    void testPlateModelRunsWhenProvided()
    {
        const QString platePath = qEnvironmentVariable("CLOAKFRAME_TEST_PLATE_MODEL");
        if (platePath.isEmpty())
        {
            std::puts("skipping license plate model test: environment path not set");
            return;
        }

        const auto &model = cloakframe::plateModel();
        cloakframe::PlateDetector detector(
            platePath.toStdString(), false, QByteArray::fromHex(model.sha256.toLatin1()));
        const cv::Mat blank(360, 640, CV_8UC3, cv::Scalar(30, 30, 30));
        assert(detector.detect(blank, 0.99F, 0.4F).detections.empty());
        const cv::Mat tall(640, 360, CV_8UC3, cv::Scalar(200, 200, 200));
        assert(detector.detect(tall, 0.99F, 0.4F).detections.empty());
    }
}

int main(int argc, char **argv)
{
    QCoreApplication app(argc, argv);
    testAccelerationBackendNames();
    testModelCacheTagIsContentDerived();
    testBuiltinModelDigests();
    testSupportedImageExtensions();
    testScanImagesRecursesAndDeduplicates();
    testScanReportsInputsItCannotRead();
    testScanReportsDeniedDirectoriesAndContinues();
    testOutputPlanRejectsExistingAndDuplicateDestinations();
    testWorkerReportsUnredactedOutputAsWarningAndPreservesIt();
    testWorkerReportsCopiedOriginalAsWarning();
    testReviewConfirmsOnlyWhenDetectionsWereCleared();
    testWorkerFailsWhenTheReviewReceiverIsMissing();
    testWorkerFailsWhenTheReviewReceiverDiesBeforeTheRun();
    testWorkerSavesNothingWhenTheReviewSlotIsMissing();
    testWorkerHonoursCancelRequestedBeforeProcess();
    testDroppedDetectionsKeepTheRunOutOfCleanCompletion();
    testWorkerUsesStableImageSnapshotDuringReview();
    testWorkerAcceptsThirtyMegabyteJpeg();
    testWorkerRejectsAnImageLargerThanTheMemoryBudget();
    testReleaseNotesPickTheInterfaceLanguage();
    testWorkerRejectsMultiFrameImages();
    testAnimatedImageContainersAreDetectedWithoutDecodingAllFrames();
    testApplyMosaicTouchesOnlyDetectedRegion();
    testSoftEdgesKeepDetectedRegionFullyCovered();
    testFillIsOpaqueOnAlphaImages();
    testSoftEdgesEllipseKeepsCoreCovered();
    testSoftEdgesAtImageBorderStayInBounds();
    testSoftEdgesUsePaddingForAGradualTransition();
    testLargeSoftEdgeMaskUsesBoundedWorkingMemoryPath();
    testCustomImageCoversDetectedRegion();
    testTransparentCustomImageFallsBackToSafeMosaic();
    testSemitransparentCustomImageBlendsWithOriginal();
    testCustomImageSupports16BitOutput();
    testCustomImagePreservesAspectRatio();
    testCustomImageFollowsFaceRollWithoutDarkAlphaFringes();
    testOpaqueRotatedCustomImageStillCoversDetectedRegion();
    testOrientationTransforms();
    testExifOrientationFallback();
    testEncodeParams();
    testImageWritePublishesWithoutReplacing();
#ifndef _WIN32
    testPublicationWithoutAtomicPrimitivesLeavesNoCompleteLookingPartial();
#endif
    testRootedWritesRejectEscapesAndUsePrivateFiles();
    testMetadataFailurePublishesCleanImage();
    testIntersectionOverUnion();
    testNonMaxSuppression();
    testInvalidDetectionsAreIgnored();
    testDetectorsRejectUnexpectedModelHash();
    testOnnxPatchRejectsInvalidBytes();
    testOnnxPatchScalesResizeTargetsByTheirExportedRatio();
    testOnnxPatchRefusesResizeTargetsItCannotDerive();
    testFixedScrfdModelRunsAtRequestedSize();
    testDynamicScrfdModelRunsAtRequestedSize();
    testRecommendedFaceModels();
    testPlateModelRunsWhenProvided();
    testDestinationPathSafety();
#ifndef _WIN32
    testDestinationRejectsSymlinkEscape();
#endif
#ifdef CLOAKFRAME_HAVE_EXIV2
    testMetadataCopyAndOrientationNormalize();
    testMetadataCopyStripsEmbeddedThumbnail();
#endif
    return 0;
}

#include "test_core.moc"

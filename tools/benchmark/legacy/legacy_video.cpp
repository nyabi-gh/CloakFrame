// Runs CloakFrame 1.12.1's video analysis (pass 1, tracking and post-processing) on one clip and
// writes the tracks it would mask, stopping before pass 2 (docs/M5_BENCHMARK.md).
//
// Usage: cloakframe_bench_legacy_video <models dir> <ffmpeg> <ffprobe> <video> <out>
//            <yolo5face|yunet|off> <plates on|off> <score> <nms>

#include "cloakframe/PlateDetector.hpp"
#include "cloakframe/VideoProcessor.hpp"
#include "cloakframe/Yolo5FaceDetector.hpp"
#include "cloakframe/YuNetFaceDetector.hpp"

#include <QCoreApplication>
#include <QFile>

#include <atomic>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <filesystem>
#include <memory>
#include <string>

namespace fs = std::filesystem;

namespace
{
    QByteArray number(double v, int places)
    {
        const double scale = std::pow(10.0, places);
        return QByteArray::number(std::round(v * scale) / scale, 'g', 12);
    }
}

int main(int argc, char **argv)
{
    QCoreApplication app(argc, argv);
    if (argc != 10)
    {
        std::fprintf(stderr,
            "usage: %s <models dir> <ffmpeg> <ffprobe> <video> <out> <yolo5face|yunet|off> "
            "<plates on|off> <score> <nms>\n",
            argv[0]);
        return 2;
    }
    const fs::path models = argv[1];
    const cloakframe::FfmpegTools tools{QString::fromUtf8(argv[2]), QString::fromUtf8(argv[3]), true, {}};
    const QString video = QString::fromUtf8(argv[4]);
    const std::string faces = argv[6];
    const bool plates = std::string(argv[7]) == "on";
    const float score = std::strtof(argv[8], nullptr);
    const float nms = std::strtof(argv[9], nullptr);

    QString error;
    const auto info = cloakframe::probeVideo(tools, video, &error);
    if (!info)
    {
        std::fprintf(stderr, "probe: %s\n", error.toUtf8().constData());
        return 1;
    }

    // CPU sessions, as the photo benchmark: 1.12.1's CoreML results equal its CPU results.
    std::unique_ptr<cloakframe::Detector> face;
    if (faces == "yolo5face")
        face = std::make_unique<cloakframe::Yolo5FaceDetector>((models / "yolov5n_face.onnx").string(), false);
    else if (faces == "yunet")
        face = std::make_unique<cloakframe::YuNetFaceDetector>((models / "face_detection_yunet_2023mar.onnx").string());
    std::unique_ptr<cloakframe::Detector> plate;
    if (plates)
        plate = std::make_unique<cloakframe::PlateDetector>(
            (models / "yolo-v9-t-512-license-plates-end2end.onnx").string(), false);

    // ProcessorWorker::processVideoItem: every candidate down to the tracker's low threshold,
    // faces and plates in one list.
    const float detectionThreshold = std::min(cloakframe::TrackerConfig{}.lowScoreThreshold, score);
    const auto detect = [&](const cv::Mat &frame)
    {
        cloakframe::FaceDetections detections;
        if (face)
            detections = face->detect(frame, detectionThreshold, nms).detections;
        if (plate)
        {
            auto found = plate->detect(frame, detectionThreshold, nms).detections;
            detections.insert(detections.end(), found.begin(), found.end());
        }
        return detections;
    };

    cloakframe::VideoProcessOptions options;
    options.scoreThreshold = score;
    options.nmsThreshold = nms;

    QByteArray text;
    const auto started = std::chrono::steady_clock::now();
    // The review hook sees the tracks after post-processing in display pixels, as pass 2 would
    // mask them, and skipping there leaves pass 2 out.
    const auto review = [&](std::vector<cloakframe::Track> &tracks,
                            const std::vector<cloakframe::UncoveredSpan> &spans,
                            qint64 frameCount,
                            const QString &,
                            const cloakframe::VideoInfo &active)
    {
        const double seconds =
            std::chrono::duration<double>(std::chrono::steady_clock::now() - started).count();
        text += "{\"detector\":\"1.12.1\",\"frames\":" + QByteArray::number(frameCount)
                + ",\"width\":" + QByteArray::number(active.displayWidth())
                + ",\"height\":" + QByteArray::number(active.displayHeight())
                + ",\"seconds\":" + number(seconds, 2) + ",\"gaps\":[";
        for (std::size_t i = 0; i < spans.size(); ++i)
            text += (i ? "," : "") + QByteArray("[") + QByteArray::number(spans[i].firstFrame) + ","
                    + QByteArray::number(spans[i].lastFrame) + "]";
        text += "],\"tracks\":[\n";
        for (std::size_t i = 0; i < tracks.size(); ++i)
        {
            const auto &t = tracks[i];
            text += (i ? ",\n" : "") + QByteArray("{\"id\":") + QByteArray::number(t.id)
                    + ",\"low_confidence\":" + (t.lowConfidence ? "true" : "false") + ",\"boxes\":[";
            for (std::size_t j = 0; j < t.boxes.size(); ++j)
            {
                const auto &b = t.boxes[j];
                text += (j ? "," : "") + QByteArray("[") + QByteArray::number(b.frame) + ","
                        + number(b.box.x, 2) + "," + number(b.box.y, 2) + "," + number(b.box.width, 2)
                        + "," + number(b.box.height, 2) + "," + number(b.score, 4) + ","
                        + (b.interpolated ? "1" : "0") + "]";
            }
            text += "]}";
        }
        text += "\n]}\n";
        return cloakframe::VideoTrackReviewOutcome{cloakframe::VideoTrackReviewAction::Skip, {}};
    };

    std::atomic<bool> cancelled{false};
    const auto result = cloakframe::processVideo(
        tools, video, QString::fromUtf8(argv[5]) + ".unused", *info, options, detect, cancelled, {}, review);
    if (result.status != cloakframe::VideoProcessStatus::Skipped)
    {
        std::fprintf(stderr, "processVideo: %s\n", result.error.toUtf8().constData());
        return 1;
    }
    QFile out(QString::fromUtf8(argv[5]));
    if (!out.open(QIODevice::WriteOnly | QIODevice::Truncate))
    {
        std::fprintf(stderr, "cannot write %s\n", argv[5]);
        return 1;
    }
    out.write(text);
    return 0;
}

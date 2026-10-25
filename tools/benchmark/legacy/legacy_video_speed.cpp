// Runs CloakFrame 1.12.1's whole video pipeline on one clip (pass 1, tracking, pass 2 and the
// encode) the way ProcessorWorker does with YOLO5Face-n at its default thresholds, and prints how
// long pass 1 and everything took as one JSON line (docs/M5_VERIFY.md).
//
// Usage: cloakframe_bench_legacy_video_speed <models dir> <ffmpeg> <ffprobe> <video> <out.mp4>
//            <gpu on|off>

#include "cloakframe/VideoProcessor.hpp"
#include "cloakframe/Yolo5FaceDetector.hpp"

#include <QCoreApplication>

#include <atomic>
#include <chrono>
#include <cstdio>
#include <filesystem>
#include <string>

namespace fs = std::filesystem;

int main(int argc, char **argv)
{
    QCoreApplication app(argc, argv);
    if (argc != 7)
    {
        std::fprintf(stderr, "usage: %s <models dir> <ffmpeg> <ffprobe> <video> <out.mp4> <gpu on|off>\n", argv[0]);
        return 2;
    }
    const fs::path models = argv[1];
    const cloakframe::FfmpegTools tools{QString::fromUtf8(argv[2]), QString::fromUtf8(argv[3]), true, {}};
    const QString video = QString::fromUtf8(argv[4]);
    // 1.12.1's one switch: CoreML for the model and the hardware encoder (ProcessorWorker).
    const bool gpu = std::string(argv[6]) == "on";

    QString error;
    const auto info = cloakframe::probeVideo(tools, video, &error);
    if (!info)
    {
        std::fprintf(stderr, "probe: %s\n", error.toUtf8().constData());
        return 1;
    }
    cloakframe::Yolo5FaceDetector face((models / "yolov5n_face.onnx").string(), gpu);
    cloakframe::VideoProcessOptions options;
    options.hardwareEncoder = gpu;
    const float detectionThreshold = std::min(cloakframe::TrackerConfig{}.lowScoreThreshold, options.scoreThreshold);
    const auto detect = [&](const cv::Mat &frame)
    { return face.detect(frame, detectionThreshold, options.nmsThreshold).detections; };

    const auto started = std::chrono::steady_clock::now();
    const auto since = [&] { return std::chrono::duration<double>(std::chrono::steady_clock::now() - started).count(); };
    double firstPass = 0;
    std::size_t tracks = 0;
    // The review hook runs between tracking and pass 2.
    const auto review = [&](std::vector<cloakframe::Track> &found,
                            const std::vector<cloakframe::UncoveredSpan> &,
                            qint64,
                            const QString &,
                            const cloakframe::VideoInfo &)
    {
        firstPass = since();
        tracks = found.size();
        return cloakframe::VideoTrackReviewOutcome{cloakframe::VideoTrackReviewAction::Encode, {}};
    };
    std::atomic<bool> cancelled{false};
    const auto result = cloakframe::processVideo(
        tools, video, QString::fromUtf8(argv[5]), *info, options, detect, cancelled, {}, review);
    const double total = since();
    if (result.status != cloakframe::VideoProcessStatus::Completed)
    {
        std::fprintf(stderr, "processVideo: %s\n", result.error.toUtf8().constData());
        return 1;
    }
    std::printf("{\"frames\":%lld,\"tracks\":%zu,\"first_pass\":%.2f,\"seconds\":%.2f,\"encoder\":\"%s\"}\n",
        static_cast<long long>(result.frameCount), tracks, firstPass, total,
        result.encoderName.toUtf8().constData());
    return 0;
}

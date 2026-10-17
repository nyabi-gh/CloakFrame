// Times 1.12.1 processVideo end to end (detection returns nothing) on the M0 throughput clips:
// the video baseline for plan 9.3, run on the same machine as spikes/ffmpeg-pipe.
//
// Usage: cloakframe_baseline_video <work dir> <clip>...

#include "cloakframe/VideoIo.hpp"
#include "cloakframe/VideoProcessor.hpp"

#include <QCoreApplication>
#include <QJsonDocument>
#include <QJsonObject>

#include <atomic>
#include <chrono>
#include <cstdio>
#include <filesystem>

namespace fs = std::filesystem;

int main(int argc, char **argv)
{
    QCoreApplication app(argc, argv);
    if (argc < 3)
    {
        std::fprintf(stderr, "usage: %s <work dir> <clip>...\n", argv[0]);
        return 2;
    }
    const fs::path work = argv[1];
    fs::create_directories(work);
    QString error;
    const auto tools = cloakframe::locateFfmpegTools(&error);
    if (!tools)
    {
        std::fprintf(stderr, "%s\n", error.toUtf8().constData());
        return 1;
    }
    for (int i = 2; i < argc; ++i)
    {
        const fs::path clip = argv[i];
        const auto info = cloakframe::probeVideo(*tools, QString::fromStdString(clip.string()));
        for (const bool hardware : {false, true})
        {
            const fs::path out = work / (clip.stem().string() + (hardware ? "-hw.mp4" : "-sw.mp4"));
            fs::remove(out);
            cloakframe::VideoProcessOptions options;
            options.hardwareEncoder = hardware;
            std::atomic<bool> cancelled{false};
            const auto start = std::chrono::steady_clock::now();
            const auto result = cloakframe::processVideo(*tools,
                QString::fromStdString(clip.string()),
                QString::fromStdString(out.string()),
                *info,
                options,
                [](const cv::Mat &)
                {
                    return cloakframe::FaceDetections{};
                },
                cancelled);
            const double seconds = std::chrono::duration<double>(std::chrono::steady_clock::now() - start).count();
            const QJsonObject line{{"clip", QString::fromStdString(clip.filename().string())},
                {"hardware_encoder_requested", hardware},
                {"encoder", result.encoderName},
                {"status", result.status == cloakframe::VideoProcessStatus::Completed ? "completed" : "not completed"},
                {"frames", result.frameCount},
                {"wall_s", seconds},
                {"fps", result.frameCount / seconds},
                {"realtime_x", result.frameCount / seconds / info->fps()}};
            std::printf("%s\n", QJsonDocument(line).toJson(QJsonDocument::Compact).constData());
            std::fflush(stdout);
        }
    }
    return 0;
}
